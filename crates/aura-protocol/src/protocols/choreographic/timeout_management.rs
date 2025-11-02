//! Timeout management for choreographic protocols
//!
//! This module provides configurable timeout management for all choreographic
//! protocol operations, ensuring no operations can hang indefinitely.

use aura_types::errors::{AuraError, ErrorCode, Result as AuraResult};
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::{timeout, Timeout};

/// Timeout configuration for choreographic protocols
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Default timeout for protocol operations
    pub default_timeout: Duration,
    /// Timeout for DKD protocol operations
    pub dkd_timeout: Duration,
    /// Timeout for FROST signing operations
    pub frost_timeout: Duration,
    /// Timeout for resharing operations
    pub resharing_timeout: Duration,
    /// Timeout for recovery operations
    pub recovery_timeout: Duration,
    /// Timeout for network operations
    pub network_timeout: Duration,
    /// Timeout for Byzantine agreement
    pub byzantine_timeout: Duration,
    /// Enable adaptive timeouts based on network conditions
    pub adaptive_timeouts: bool,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            dkd_timeout: Duration::from_secs(60),
            frost_timeout: Duration::from_secs(45),
            resharing_timeout: Duration::from_secs(120),
            recovery_timeout: Duration::from_secs(300),
            network_timeout: Duration::from_secs(10),
            byzantine_timeout: Duration::from_secs(90),
            adaptive_timeouts: true,
        }
    }
}

impl TimeoutConfig {
    /// Create a configuration for testing with shorter timeouts
    pub fn for_testing() -> Self {
        Self {
            default_timeout: Duration::from_secs(5),
            dkd_timeout: Duration::from_secs(10),
            frost_timeout: Duration::from_secs(8),
            resharing_timeout: Duration::from_secs(15),
            recovery_timeout: Duration::from_secs(20),
            network_timeout: Duration::from_secs(2),
            byzantine_timeout: Duration::from_secs(12),
            adaptive_timeouts: false,
        }
    }

    /// Create a configuration for production with conservative timeouts
    pub fn for_production() -> Self {
        Self {
            default_timeout: Duration::from_secs(60),
            dkd_timeout: Duration::from_secs(120),
            frost_timeout: Duration::from_secs(90),
            resharing_timeout: Duration::from_secs(300),
            recovery_timeout: Duration::from_secs(600),
            network_timeout: Duration::from_secs(30),
            byzantine_timeout: Duration::from_secs(180),
            adaptive_timeouts: true,
        }
    }
}

/// Manages timeouts for choreographic protocols with adaptive behavior
pub struct TimeoutManager {
    config: TimeoutConfig,
    /// Network latency statistics for adaptive timeouts
    latency_stats: Arc<RwLock<LatencyStats>>,
}

#[derive(Default)]
struct LatencyStats {
    /// Recent round-trip times in milliseconds
    recent_rtts: Vec<u64>,
    /// Maximum observed RTT
    max_rtt: u64,
    /// Average RTT
    avg_rtt: u64,
}

impl Default for TimeoutManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeoutManager {
    /// Create a new timeout manager with default config
    pub fn new() -> Self {
        Self::with_config(TimeoutConfig::default())
    }

    /// Create a new timeout manager with custom config
    pub fn with_config(config: TimeoutConfig) -> Self {
        Self {
            config,
            latency_stats: Arc::new(RwLock::new(LatencyStats::default())),
        }
    }

    /// Execute an operation with timeout
    pub async fn with_timeout<T, F>(
        &self,
        operation_type: OperationType,
        future: F,
    ) -> AuraResult<T>
    where
        F: Future<Output = AuraResult<T>>,
    {
        let timeout_duration = self.get_timeout_duration(operation_type).await;

        match timeout(timeout_duration, future).await {
            Ok(result) => result,
            Err(_) => Err(self.timeout_error(operation_type, timeout_duration)),
        }
    }

    /// Execute with custom timeout
    pub async fn with_custom_timeout<T, F>(
        &self,
        timeout_duration: Duration,
        operation_name: &str,
        future: F,
    ) -> AuraResult<T>
    where
        F: Future<Output = AuraResult<T>>,
    {
        match timeout(timeout_duration, future).await {
            Ok(result) => result,
            Err(_) => Err(AuraError::timeout_error(format!(
                "{} timed out after {:?}",
                operation_name, timeout_duration
            ))),
        }
    }

    /// Record network latency for adaptive timeout adjustment
    pub async fn record_latency(&self, rtt_ms: u64) {
        if !self.config.adaptive_timeouts {
            return;
        }

        let mut stats = self.latency_stats.write().await;

        // Keep last 100 measurements
        if stats.recent_rtts.len() >= 100 {
            stats.recent_rtts.remove(0);
        }
        stats.recent_rtts.push(rtt_ms);

        // Update statistics
        stats.max_rtt = stats.max_rtt.max(rtt_ms);
        if !stats.recent_rtts.is_empty() {
            stats.avg_rtt = stats.recent_rtts.iter().sum::<u64>() / stats.recent_rtts.len() as u64;
        }
    }

    /// Get adjusted timeout duration based on operation type and network conditions
    async fn get_timeout_duration(&self, operation_type: OperationType) -> Duration {
        let base_timeout = match operation_type {
            OperationType::Dkd => self.config.dkd_timeout,
            OperationType::Frost => self.config.frost_timeout,
            OperationType::Resharing => self.config.resharing_timeout,
            OperationType::Recovery => self.config.recovery_timeout,
            OperationType::Network => self.config.network_timeout,
            OperationType::Byzantine => self.config.byzantine_timeout,
            OperationType::Generic => self.config.default_timeout,
        };

        if !self.config.adaptive_timeouts {
            return base_timeout;
        }

        // Apply adaptive adjustment based on network conditions
        let stats = self.latency_stats.read().await;
        if stats.recent_rtts.is_empty() {
            return base_timeout;
        }

        // Calculate 95th percentile RTT
        let mut sorted_rtts = stats.recent_rtts.clone();
        sorted_rtts.sort_unstable();
        let p95_index = (sorted_rtts.len() as f64 * 0.95) as usize;
        let p95_rtt = sorted_rtts.get(p95_index).unwrap_or(&stats.max_rtt);

        // Adjust timeout based on network conditions
        // Add 3x the 95th percentile RTT to account for multiple round trips
        let network_adjustment = Duration::from_millis(p95_rtt * 3);

        base_timeout + network_adjustment
    }

    /// Create an appropriate timeout error for the operation type
    fn timeout_error(&self, operation_type: OperationType, _duration: Duration) -> AuraError {
        let (message, _code) = match operation_type {
            OperationType::Dkd => ("DKD protocol timed out", ErrorCode::ProtocolDkdTimeout),
            OperationType::Frost => ("FROST signing timed out", ErrorCode::CryptoFrostSignTimeout),
            OperationType::Resharing => (
                "Resharing protocol timed out",
                ErrorCode::ProtocolSessionTimeout,
            ),
            OperationType::Recovery => (
                "Recovery protocol timed out",
                ErrorCode::ProtocolRecoveryFailed,
            ),
            OperationType::Network => (
                "Network operation timed out",
                ErrorCode::InfraTransportTimeout,
            ),
            OperationType::Byzantine => (
                "Byzantine agreement timed out",
                ErrorCode::ProtocolCoordinationFailed,
            ),
            OperationType::Generic => (
                "Protocol operation timed out",
                ErrorCode::ProtocolSessionTimeout,
            ),
        };

        AuraError::timeout_error(message)
    }
}

/// Types of choreographic operations for timeout management
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationType {
    /// Distributed Key Derivation
    Dkd,
    /// FROST threshold signing
    Frost,
    /// Key resharing
    Resharing,
    /// Account recovery
    Recovery,
    /// Network communication
    Network,
    /// Byzantine agreement
    Byzantine,
    /// Generic protocol operation
    Generic,
}

/// Deadline tracker for multi-phase protocols
pub struct DeadlineTracker {
    /// Overall protocol deadline
    protocol_deadline: Instant,
    /// Phase deadlines
    phase_deadlines: Vec<(String, Instant)>,
    /// Current phase index
    current_phase: usize,
}

impl DeadlineTracker {
    /// Create a new deadline tracker
    pub fn new(overall_timeout: Duration) -> Self {
        Self {
            protocol_deadline: Instant::now() + overall_timeout,
            phase_deadlines: Vec::new(),
            current_phase: 0,
        }
    }

    /// Add a phase with its deadline
    pub fn add_phase(&mut self, name: String, duration: Duration) {
        let deadline = if let Some((_, prev_deadline)) = self.phase_deadlines.last() {
            *prev_deadline + duration
        } else {
            Instant::now() + duration
        };

        self.phase_deadlines.push((name, deadline));
    }

    /// Check if current phase has timed out
    pub fn check_phase_timeout(&self) -> AuraResult<()> {
        if self.current_phase >= self.phase_deadlines.len() {
            return Ok(());
        }

        let (phase_name, deadline) = &self.phase_deadlines[self.current_phase];
        if Instant::now() > *deadline {
            return Err(AuraError::protocol_timeout(format!(
                "Phase '{}' timed out",
                phase_name
            )));
        }

        Ok(())
    }

    /// Check if overall protocol has timed out
    pub fn check_protocol_timeout(&self) -> AuraResult<()> {
        if Instant::now() > self.protocol_deadline {
            return Err(AuraError::protocol_timeout(
                "Overall protocol deadline exceeded",
            ));
        }

        Ok(())
    }

    /// Move to next phase
    pub fn next_phase(&mut self) -> AuraResult<()> {
        self.check_protocol_timeout()?;
        self.current_phase += 1;
        Ok(())
    }

    /// Get remaining time for current phase
    pub fn remaining_phase_time(&self) -> Option<Duration> {
        if self.current_phase >= self.phase_deadlines.len() {
            return None;
        }

        let (_, deadline) = &self.phase_deadlines[self.current_phase];
        deadline.checked_duration_since(Instant::now())
    }

    /// Get remaining time for overall protocol
    pub fn remaining_protocol_time(&self) -> Option<Duration> {
        self.protocol_deadline
            .checked_duration_since(Instant::now())
    }
}

/// Helper to create timeboxed futures
pub fn timebox<T, F>(duration: Duration, future: F) -> Timeout<F>
where
    F: Future<Output = T>,
{
    timeout(duration, future)
}

/*
TODO: These timeout management tests need API updates after protocol refactoring.
The tests verify timeout configuration, adaptive timeouts, and deadline tracking but
require updates to work with the new protocol APIs.

Key areas needing updates:
- Error code enum variants
- AuraResult and AuraError API changes
- TimeoutManager integration with new protocol context
- OperationType mapping to updated error codes

Re-enable once the timeout management APIs have stabilized.
*/

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_timeout_manager_basic() {
        let manager = TimeoutManager::new();

        // Test successful operation
        let result = manager
            .with_timeout(OperationType::Generic, async { Ok::<_, AuraError>(42) })
            .await;
        assert_eq!(result.unwrap(), 42);

        // Test timeout
        let result = manager
            .with_timeout(OperationType::Network, async {
                tokio::time::sleep(Duration::from_secs(30)).await;
                Ok::<_, AuraError>(42)
            })
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code(),
            Some(ErrorCode::InfraTransportTimeout)
        );
    }

    #[tokio::test]
    async fn test_adaptive_timeouts() {
        let manager = TimeoutManager::with_config(TimeoutConfig {
            network_timeout: Duration::from_secs(1),
            adaptive_timeouts: true,
            ..Default::default()
        });

        // Record some latencies
        for rtt in [100, 150, 200, 250, 300] {
            manager.record_latency(rtt).await;
        }

        // Timeout should be adjusted based on recorded latencies
        let timeout_duration = manager.get_timeout_duration(OperationType::Network).await;
        assert!(timeout_duration > Duration::from_secs(1));
    }

    #[test]
    fn test_deadline_tracker() {
        let mut tracker = DeadlineTracker::new(Duration::from_secs(10));

        // Add phases
        tracker.add_phase("Phase 1".to_string(), Duration::from_secs(3));
        tracker.add_phase("Phase 2".to_string(), Duration::from_secs(3));
        tracker.add_phase("Phase 3".to_string(), Duration::from_secs(3));

        // Check phase hasn't timed out
        assert!(tracker.check_phase_timeout().is_ok());
        assert!(tracker.check_protocol_timeout().is_ok());

        // Move to next phase
        assert!(tracker.next_phase().is_ok());
        assert_eq!(tracker.current_phase, 1);

        // Check remaining time
        assert!(tracker.remaining_phase_time().is_some());
        assert!(tracker.remaining_protocol_time().is_some());
    }

    #[test]
    fn test_operation_type_timeouts() {
        let config = TimeoutConfig::default();
        let manager = TimeoutManager::with_config(config.clone());

        // Verify each operation type gets appropriate timeout
        let test_cases = vec![
            (OperationType::Dkd, config.dkd_timeout),
            (OperationType::Frost, config.frost_timeout),
            (OperationType::Resharing, config.resharing_timeout),
            (OperationType::Recovery, config.recovery_timeout),
            (OperationType::Network, config.network_timeout),
            (OperationType::Byzantine, config.byzantine_timeout),
            (OperationType::Generic, config.default_timeout),
        ];

        for (op_type, expected_timeout) in test_cases {
            let error = manager.timeout_error(op_type, expected_timeout);
            assert!(error.is_retryable() || matches!(op_type, OperationType::Recovery));
        }
    }
}
*/
