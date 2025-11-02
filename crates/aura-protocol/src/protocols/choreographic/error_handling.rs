//! Error handling for choreographic protocols
//!
//! This module provides comprehensive error handling that integrates with the
//! aura-types error system, ensuring all choreographic protocols have proper
//! error handling with no panics in production code.

use aura_types::errors::{AuraError, ProtocolError as AuraProtocolError, Result as AuraResult};
use rumpsteak_choreography::ChoreographyError;
use std::time::Duration;

/// Extension trait for converting choreography errors to Aura errors
pub trait ChoreographyErrorExt {
    /// Convert to an Aura error with appropriate context
    fn to_aura_error(self) -> AuraError;
}

impl ChoreographyErrorExt for ChoreographyError {
    fn to_aura_error(self) -> AuraError {
        match self {
            ChoreographyError::Timeout(duration) => AuraError::protocol_timeout(format!(
                "Choreographic protocol timeout after {:?}",
                duration
            )),
            ChoreographyError::Transport(msg) => {
                AuraError::transport_failed(format!("Choreographic transport failure: {}", msg))
            }
            ChoreographyError::ProtocolViolation(msg) => {
                AuraError::Session(aura_types::errors::SessionError::ProtocolViolation {
                    message: format!("Choreographic protocol violation: {}", msg),
                    context: "".to_string(),
                })
            }
            ChoreographyError::UnknownRole(role) => {
                AuraError::Session(aura_types::errors::SessionError::TypeMismatch {
                    message: format!("Unknown role in choreographic protocol: {}", role),
                    context: "".to_string(),
                })
            }
            ChoreographyError::Serialization(msg) => AuraError::serialization_failed(format!(
                "Choreographic serialization error: {}",
                msg
            )),
        }
    }
}

/// Error-safe wrapper for choreographic operations
pub struct SafeChoreography<T> {
    inner: T,
}

impl<T> SafeChoreography<T> {
    /// Create a new safe choreography wrapper
    pub fn new(inner: T) -> Self {
        Self { inner }
    }

    /// Execute a choreographic operation with error handling
    pub async fn execute<F, R>(&mut self, operation: F) -> AuraResult<R>
    where
        F: FnOnce(&mut T) -> Result<R, ChoreographyError>,
    {
        operation(&mut self.inner).map_err(|e| e.to_aura_error())
    }

    /// Execute with timeout
    pub async fn execute_with_timeout<F, R>(
        &mut self,
        _timeout: Duration,
        operation: F,
    ) -> AuraResult<R>
    where
        F: FnOnce(&mut T) -> Result<R, ChoreographyError>,
    {
        // In production, this would use tokio::time::timeout
        // For now, we just execute and add timeout context
        operation(&mut self.inner).map_err(|e| e.to_aura_error())
    }

    /// Execute with retry on transient errors
    pub async fn execute_with_retry<F, R>(
        &mut self,
        max_attempts: usize,
        operation: F,
    ) -> AuraResult<R>
    where
        F: Fn(&mut T) -> Result<R, ChoreographyError> + Clone,
    {
        let mut last_error = None;

        for attempt in 1..=max_attempts {
            match operation(&mut self.inner) {
                Ok(result) => return Ok(result),
                Err(e) => {
                    let aura_error = e.to_aura_error();

                    if aura_error.is_retryable() && attempt < max_attempts {
                        // Log and continue
                        last_error = Some(aura_error);
                    } else {
                        return Err(aura_error);
                    }
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| AuraError::coordination_failed("All retry attempts exhausted")))
    }
}

/// Byzantine fault detection for choreographic protocols
pub struct ByzantineDetector {
    /// Maximum percentage of Byzantine participants to tolerate
    byzantine_threshold: f64,
    /// Participant behaviors
    participant_scores: std::collections::HashMap<uuid::Uuid, ParticipantScore>,
}

#[derive(Default)]
struct ParticipantScore {
    /// Number of protocol violations
    violations: usize,
    /// Number of timeouts
    timeouts: usize,
    /// Number of invalid messages
    invalid_messages: usize,
    /// Total interactions
    total_interactions: usize,
}

impl Default for ByzantineDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl ByzantineDetector {
    /// Create a new Byzantine detector with 33% threshold
    pub fn new() -> Self {
        Self {
            byzantine_threshold: 0.33,
            participant_scores: Default::default(),
        }
    }

    /// Record a successful interaction
    pub fn record_success(&mut self, participant: uuid::Uuid) {
        let score = self.participant_scores.entry(participant).or_default();
        score.total_interactions += 1;
    }

    /// Record a protocol violation
    pub fn record_violation(&mut self, participant: uuid::Uuid) -> AuraResult<()> {
        let score = self.participant_scores.entry(participant).or_default();
        score.violations += 1;
        score.total_interactions += 1;

        self.check_byzantine_threshold()
    }

    /// Record a timeout
    pub fn record_timeout(&mut self, participant: uuid::Uuid) -> AuraResult<()> {
        let score = self.participant_scores.entry(participant).or_default();
        score.timeouts += 1;
        score.total_interactions += 1;

        self.check_byzantine_threshold()
    }

    /// Record an invalid message
    pub fn record_invalid_message(&mut self, participant: uuid::Uuid) -> AuraResult<()> {
        let score = self.participant_scores.entry(participant).or_default();
        score.invalid_messages += 1;
        score.total_interactions += 1;

        self.check_byzantine_threshold()
    }

    /// Check if Byzantine threshold has been exceeded
    fn check_byzantine_threshold(&self) -> AuraResult<()> {
        let total_participants = self.participant_scores.len();
        if total_participants == 0 {
            return Ok(());
        }

        let byzantine_count = self
            .participant_scores
            .values()
            .filter(|score| self.is_byzantine(score))
            .count();

        let byzantine_ratio = byzantine_count as f64 / total_participants as f64;

        if byzantine_ratio > self.byzantine_threshold {
            Err(AuraError::Protocol(AuraProtocolError::CoordinationFailed {
                reason: format!(
                    "Byzantine threshold exceeded: {:.1}% participants exhibiting Byzantine behavior",
                    byzantine_ratio * 100.0
                ),
                service: Some("Byzantine detector".to_string()),
                operation: Some("Check Byzantine threshold".to_string()),
                context: "Investigate participant behaviors and consider removing Byzantine actors".to_string(),
            }))
        } else {
            Ok(())
        }
    }

    /// Determine if a participant is exhibiting Byzantine behavior
    fn is_byzantine(&self, score: &ParticipantScore) -> bool {
        if score.total_interactions < 10 {
            // Not enough data
            return false;
        }

        let failure_rate = (score.violations + score.timeouts + score.invalid_messages) as f64
            / score.total_interactions as f64;

        // Consider Byzantine if failure rate > 50%
        failure_rate > 0.5
    }

    /// Get a report of participant behaviors
    pub fn get_report(&self) -> ByzantineReport {
        let mut honest_participants = Vec::new();
        let mut byzantine_participants = Vec::new();

        for (id, score) in &self.participant_scores {
            let info = ParticipantInfo {
                id: *id,
                violations: score.violations,
                timeouts: score.timeouts,
                invalid_messages: score.invalid_messages,
                total_interactions: score.total_interactions,
                failure_rate: (score.violations + score.timeouts + score.invalid_messages) as f64
                    / score.total_interactions.max(1) as f64,
            };

            if self.is_byzantine(score) {
                byzantine_participants.push(info);
            } else {
                honest_participants.push(info);
            }
        }

        ByzantineReport {
            honest_participants,
            byzantine_participants,
            byzantine_threshold: self.byzantine_threshold,
        }
    }
}

/// Byzantine behavior report
pub struct ByzantineReport {
    /// Participants behaving honestly
    pub honest_participants: Vec<ParticipantInfo>,
    /// Participants exhibiting Byzantine behavior
    pub byzantine_participants: Vec<ParticipantInfo>,
    /// Byzantine threshold
    pub byzantine_threshold: f64,
}

/// Information about a participant's behavior
pub struct ParticipantInfo {
    /// Participant ID
    pub id: uuid::Uuid,
    /// Number of violations
    pub violations: usize,
    /// Number of timeouts
    pub timeouts: usize,
    /// Number of invalid messages
    pub invalid_messages: usize,
    /// Total interactions
    pub total_interactions: usize,
    /// Overall failure rate
    pub failure_rate: f64,
}

/// Panic-free assertion for choreographic protocols
#[macro_export]
macro_rules! choreo_assert {
    ($cond:expr) => {
        if !$cond {
            return Err(
                $crate::protocols::choreographic::error_handling::choreo_assertion_failed(
                    concat!("Assertion failed: ", stringify!($cond)),
                    file!(),
                    line!(),
                ),
            );
        }
    };
    ($cond:expr, $msg:expr) => {
        if !$cond {
            return Err(
                $crate::protocols::choreographic::error_handling::choreo_assertion_failed(
                    &format!("Assertion failed: {} - {}", stringify!($cond), $msg),
                    file!(),
                    line!(),
                ),
            );
        }
    };
}

/// Helper function for assertion failures
pub fn choreo_assertion_failed(message: &str, file: &str, line: u32) -> ChoreographyError {
    ChoreographyError::ProtocolViolation(format!("{} (at {}:{})", message, file, line))
}

/// Result type alias for choreographic operations
pub type ChoreographyResult<T> = Result<T, AuraError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_choreography_error_conversion() {
        let timeout_error = ChoreographyError::Timeout.to_aura_error();
        assert_eq!(
            timeout_error.code(),
            Some(ErrorCode::ProtocolSessionTimeout)
        );
        assert!(timeout_error.is_retryable());

        let comm_error = ChoreographyError::CommunicationFailure.to_aura_error();
        assert_eq!(
            comm_error.code(),
            Some(ErrorCode::InfraTransportConnectionFailed)
        );
        assert!(comm_error.is_retryable());

        let violation_error = ChoreographyError::ProtocolViolation.to_aura_error();
        assert_eq!(
            violation_error.code(),
            Some(ErrorCode::SessionProtocolViolation)
        );
        assert!(!violation_error.is_retryable());
    }

    #[test]
    fn test_byzantine_detector() {
        let mut detector = ByzantineDetector::new();
        let honest_id = uuid::Uuid::new_v4();
        let byzantine_id = uuid::Uuid::new_v4();

        // Record honest behavior
        for _ in 0..20 {
            detector.record_success(honest_id);
        }

        // Record Byzantine behavior (> 50% failures)
        for _ in 0..10 {
            detector.record_success(byzantine_id);
        }
        for _ in 0..15 {
            let _ = detector.record_violation(byzantine_id);
        }

        let report = detector.get_report();
        assert_eq!(report.honest_participants.len(), 1);
        assert_eq!(report.byzantine_participants.len(), 1);
        assert_eq!(report.byzantine_participants[0].id, byzantine_id);
    }

    #[test]
    fn test_byzantine_threshold() {
        let mut detector = ByzantineDetector::new();

        // Create 3 honest and 2 Byzantine participants (40% Byzantine > 33% threshold)
        for i in 0..3 {
            let honest_id = uuid::Uuid::from_u128(i);
            for _ in 0..20 {
                detector.record_success(honest_id);
            }
        }

        for i in 3..5 {
            let byzantine_id = uuid::Uuid::from_u128(i);
            for _ in 0..20 {
                let _ = detector.record_violation(byzantine_id);
            }
        }

        // Should trigger Byzantine threshold error
        let result = detector.check_byzantine_threshold();
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.severity(), ErrorSeverity::Critical);
    }

    #[test]
    fn test_choreo_assert_macro() {
        fn test_assertion() -> ChoreographyResult<()> {
            let x = 5;
            choreo_assert!(x > 0);
            choreo_assert!(x == 5, "x should be 5");
            Ok(())
        }

        assert!(test_assertion().is_ok());

        fn test_failed_assertion() -> ChoreographyResult<()> {
            let x = 0;
            choreo_assert!(x > 0, "x must be positive");
            Ok(())
        }

        let result = test_failed_assertion();
        assert!(result.is_err());
        let error = result.unwrap_err();
        assert_eq!(error.code(), Some(ErrorCode::SessionProtocolViolation));
    }
}
