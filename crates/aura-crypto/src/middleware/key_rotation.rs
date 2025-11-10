//! Key rotation middleware for coordinated key updates

use super::{CryptoContext, CryptoHandler, CryptoMiddleware, SecurityLevel};
use crate::middleware::CryptoOperation;
use crate::{CryptoError, Result};
use aura_core::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Key rotation middleware that manages coordinated key updates
pub struct KeyRotationMiddleware {
    /// Rotation session tracker
    tracker: Arc<RwLock<RotationTracker>>,

    /// Configuration
    config: RotationConfig,
}

impl KeyRotationMiddleware {
    /// Create new key rotation middleware
    pub fn new(config: RotationConfig) -> Self {
        Self {
            tracker: Arc::new(RwLock::new(RotationTracker::new())),
            config,
        }
    }

    /// Get key rotation statistics
    pub fn stats(&self) -> RotationStats {
        let tracker = self.tracker.read().unwrap();
        tracker.stats()
    }

    /// Get active rotation sessions
    pub fn active_rotations(&self) -> Result<Vec<RotationSession>> {
        let tracker = self.tracker.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on rotation tracker")
        })?;

        Ok(tracker.get_active_sessions())
    }

    /// Cancel a rotation session
    pub fn cancel_rotation(&self, session_id: &str) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on rotation tracker")
        })?;

        tracker.cancel_rotation(session_id)
    }
}

impl CryptoMiddleware for KeyRotationMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        match operation {
            CryptoOperation::RotateKeys {
                old_threshold,
                new_threshold,
                new_participants,
            } => {
                // Validate rotation request
                self.validate_rotation_request(
                    old_threshold,
                    new_threshold,
                    &new_participants,
                    context,
                )?;

                // Check if device can initiate rotation
                self.check_rotation_permissions(&context.device_id)?;

                // Check for existing rotation sessions
                self.check_existing_rotations(&context.account_id.to_string())?;

                // Create rotation session
                let session_id = self.create_rotation_session(
                    &context.account_id.to_string(),
                    &context.device_id,
                    old_threshold,
                    new_threshold,
                    new_participants.clone(),
                )?;

                // Process through next handler
                let operation_clone = CryptoOperation::RotateKeys {
                    old_threshold,
                    new_threshold,
                    new_participants: new_participants.clone(),
                };
                let result = next.handle(operation_clone, context);

                // Update session based on result
                match &result {
                    Ok(response) => {
                        self.complete_rotation_session(&session_id, true)?;

                        // Add session metadata to response
                        let mut enriched_response = response.clone();
                        if let Some(obj) = enriched_response.as_object_mut() {
                            obj.insert(
                                "rotation_session_id".to_string(),
                                serde_json::Value::String(session_id),
                            );
                            obj.insert(
                                "rotation_phase".to_string(),
                                serde_json::Value::String("completed".to_string()),
                            );
                            obj.insert(
                                "threshold_change".to_string(),
                                serde_json::json!({
                                    "old": old_threshold,
                                    "new": new_threshold
                                }),
                            );
                            obj.insert(
                                "participant_count".to_string(),
                                serde_json::Value::Number(new_participants.len().into()),
                            );
                        }
                        Ok(enriched_response)
                    }
                    Err(_) => {
                        self.complete_rotation_session(&session_id, false)?;
                        result
                    }
                }
            }

            _ => {
                // Pass through other operations
                next.handle(operation, context)
            }
        }
    }

    fn name(&self) -> &str {
        "key_rotation"
    }
}

impl KeyRotationMiddleware {
    fn validate_rotation_request(
        &self,
        old_threshold: u32,
        new_threshold: u32,
        new_participants: &[DeviceId],
        context: &CryptoContext,
    ) -> Result<()> {
        // Must be critical security level for key rotation
        if context.security_level < SecurityLevel::Critical {
            return Err(CryptoError::insufficient_security_level(format!(
                "Required: Critical, Provided: {:?}",
                context.security_level
            )));
        }

        // Validate thresholds
        if old_threshold == 0 || new_threshold == 0 {
            return Err(CryptoError::invalid_input("Threshold cannot be zero"));
        }

        if new_threshold > new_participants.len() as u32 {
            return Err(CryptoError::invalid_input(format!(
                "New threshold {} cannot exceed participant count {}",
                new_threshold,
                new_participants.len()
            )));
        }

        // Validate participant count
        if new_participants.is_empty() {
            return Err(CryptoError::invalid_input(
                "Participants list cannot be empty",
            ));
        }

        if new_participants.len() > self.config.max_participants {
            return Err(CryptoError::invalid_input(format!(
                "Too many participants: {} > {}",
                new_participants.len(),
                self.config.max_participants
            )));
        }

        if new_participants.len() < self.config.min_participants {
            return Err(CryptoError::invalid_input(format!(
                "Too few participants: {} < {}",
                new_participants.len(),
                self.config.min_participants
            )));
        }

        // Check for duplicate participants
        let mut unique_participants = std::collections::HashSet::new();
        for participant in new_participants {
            if !unique_participants.insert(participant.to_string()) {
                return Err(CryptoError::invalid_input(
                    "Duplicate participants not allowed",
                ));
            }
        }

        // Validate threshold change bounds
        let threshold_increase = new_threshold as i32 - old_threshold as i32;
        if threshold_increase.abs() > self.config.max_threshold_change as i32 {
            return Err(CryptoError::invalid_input(format!(
                "Threshold change too large: {} (max: {})",
                threshold_increase.abs(),
                self.config.max_threshold_change
            )));
        }

        Ok(())
    }

    fn check_rotation_permissions(&self, _device_id: &DeviceId) -> Result<()> {
        // TODO fix - Simplified permission check - in real implementation would check:
        // - Device authorization level
        // - Multi-party approval requirements
        // - Governance policies
        // - Account-level permissions

        if !self.config.allow_device_initiated_rotation {
            return Err(CryptoError::permission_denied(
                "Device not authorized to initiate key rotation",
            ));
        }

        Ok(())
    }

    fn check_existing_rotations(&self, account_id: &str) -> Result<()> {
        let tracker = self.tracker.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on rotation tracker")
        })?;

        // Check for existing active rotations for this account
        if tracker.has_active_rotation(account_id) {
            return Err(CryptoError::invalid_operation(
                "Another key rotation is already in progress for this account",
            ));
        }

        // Check rate limiting for rotation initiation
        if self.config.enable_rate_limiting {
            #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in rotation rate limiting
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            if let Some(&last_rotation) = tracker.last_rotation_times.get(account_id) {
                let time_since_last = now - last_rotation;
                if time_since_last < self.config.min_rotation_interval.as_secs() {
                    return Err(CryptoError::rate_limited(format!(
                        "Key rotation rate limited. {} seconds remaining",
                        self.config.min_rotation_interval.as_secs() - time_since_last
                    )));
                }
            }
        }

        Ok(())
    }

    fn create_rotation_session(
        &self,
        account_id: &str,
        initiator: &DeviceId,
        old_threshold: u32,
        new_threshold: u32,
        new_participants: Vec<DeviceId>,
    ) -> Result<String> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on rotation tracker")
        })?;

        #[allow(clippy::disallowed_methods)]
        // [VERIFIED] Acceptable in rotation session ID generation
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = RotationSession {
            session_id: session_id.clone(),
            account_id: account_id.to_string(),
            initiator: initiator.clone(),
            old_threshold,
            new_threshold,
            new_participants,
            status: RotationStatus::InProgress,
            #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in rotation session timestamp
            started_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            completed_at: None,
            success: None,
            error_message: None,
        };

        tracker.add_session(session_id.clone(), session);
        tracker.total_rotations += 1;

        Ok(session_id)
    }

    fn complete_rotation_session(&self, session_id: &str, success: bool) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on rotation tracker")
        })?;

        if let Some(session) = tracker.sessions.get_mut(session_id) {
            let account_id = session.account_id.clone();
            let started_at = session.started_at;

            #[allow(clippy::disallowed_methods)]
            // [VERIFIED] Acceptable in rotation completion timestamp
            let completed_at = Some(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
            session.completed_at = completed_at;
            session.success = Some(success);
            session.status = if success {
                RotationStatus::Completed
            } else {
                RotationStatus::Failed
            };

            // Update statistics
            if success {
                tracker.successful_rotations += 1;
                // Update last rotation time for rate limiting
                tracker.last_rotation_times.insert(account_id, started_at);
            } else {
                tracker.failed_rotations += 1;
            }
        }

        Ok(())
    }
}

/// Configuration for key rotation middleware
#[derive(Debug, Clone)]
pub struct RotationConfig {
    /// Maximum number of participants allowed
    pub max_participants: usize,

    /// Minimum number of participants required
    pub min_participants: usize,

    /// Maximum threshold change in a single rotation
    pub max_threshold_change: u32,

    /// Whether devices can initiate rotation directly
    pub allow_device_initiated_rotation: bool,

    /// Whether to enable rate limiting
    pub enable_rate_limiting: bool,

    /// Minimum interval between rotations
    pub min_rotation_interval: Duration,

    /// Session timeout
    pub session_timeout: Duration,

    /// Whether to require multi-party approval
    pub require_multi_party_approval: bool,

    /// Minimum approval threshold for rotation
    pub approval_threshold: u32,
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            max_participants: 20,
            min_participants: 2,
            max_threshold_change: 5,
            allow_device_initiated_rotation: true,
            enable_rate_limiting: true,
            min_rotation_interval: Duration::from_secs(24 * 60 * 60), // 24 hours
            session_timeout: Duration::from_secs(30 * 60),            // 30 minutes
            require_multi_party_approval: false,
            approval_threshold: 2,
        }
    }
}

/// Key rotation session status
#[derive(Debug, Clone, PartialEq)]
pub enum RotationStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Expired,
}

/// Key rotation session
#[derive(Debug, Clone)]
pub struct RotationSession {
    pub session_id: String,
    pub account_id: String,
    pub initiator: DeviceId,
    pub old_threshold: u32,
    pub new_threshold: u32,
    pub new_participants: Vec<DeviceId>,
    pub status: RotationStatus,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub success: Option<bool>,
    pub error_message: Option<String>,
}

/// Rotation tracker for managing sessions
struct RotationTracker {
    sessions: HashMap<String, RotationSession>,
    last_rotation_times: HashMap<String, u64>, // account_id -> timestamp
    total_rotations: u64,
    successful_rotations: u64,
    failed_rotations: u64,
    cancelled_rotations: u64,
}

impl RotationTracker {
    fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            last_rotation_times: HashMap::new(),
            total_rotations: 0,
            successful_rotations: 0,
            failed_rotations: 0,
            cancelled_rotations: 0,
        }
    }

    fn add_session(&mut self, session_id: String, session: RotationSession) {
        self.sessions.insert(session_id, session);
    }

    fn has_active_rotation(&self, account_id: &str) -> bool {
        self.sessions.values().any(|session| {
            session.account_id == account_id && matches!(session.status, RotationStatus::InProgress)
        })
    }

    fn get_active_sessions(&self) -> Vec<RotationSession> {
        self.sessions
            .values()
            .filter(|session| matches!(session.status, RotationStatus::InProgress))
            .cloned()
            .collect()
    }

    // [VERIFIED] Uses SystemTime::now() for rotation cancellation timestamp
    #[allow(clippy::disallowed_methods)]
    fn cancel_rotation(&mut self, session_id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get_mut(session_id) {
            if session.status == RotationStatus::InProgress {
                session.status = RotationStatus::Cancelled;
                session.completed_at = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                );
                self.cancelled_rotations += 1;
                Ok(())
            } else {
                Err(CryptoError::invalid_operation(
                    "Cannot cancel rotation that is not in progress",
                ))
            }
        } else {
            Err(CryptoError::not_found("Rotation session not found"))
        }
    }

    fn stats(&self) -> RotationStats {
        let active_sessions = self
            .sessions
            .values()
            .filter(|session| matches!(session.status, RotationStatus::InProgress))
            .count();

        RotationStats {
            active_sessions,
            total_rotations: self.total_rotations,
            successful_rotations: self.successful_rotations,
            failed_rotations: self.failed_rotations,
            cancelled_rotations: self.cancelled_rotations,
        }
    }
}

/// Key rotation statistics
#[derive(Debug, Clone)]
pub struct RotationStats {
    /// Number of active rotation sessions
    pub active_sessions: usize,

    /// Total rotations initiated
    pub total_rotations: u64,

    /// Successful rotations
    pub successful_rotations: u64,

    /// Failed rotations
    pub failed_rotations: u64,

    /// Cancelled rotations
    pub cancelled_rotations: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::Effects;
    use aura_core::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_key_rotation_middleware() {
        let effects = Effects::test();
        let account_id = aura_core::AccountId::new_with_effects(&effects);
        let device_id = aura_core::DeviceId::new_with_effects(&effects);
        let participant1 = aura_core::DeviceId::new_with_effects(&effects);
        let participant2 = aura_core::DeviceId::new_with_effects(&effects);
        let participant3 = aura_core::DeviceId::new_with_effects(&effects);

        let middleware = KeyRotationMiddleware::new(RotationConfig::default());
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );
        let operation = CryptoOperation::RotateKeys {
            old_threshold: 2,
            new_threshold: 3,
            new_participants: vec![participant1, participant2, participant3],
        };

        let result = middleware.process(operation, &context, &handler);
        if let Err(ref e) = result {
            println!("Key rotation failed with error: {:?}", e);
        }
        assert!(result.is_ok());

        let stats = middleware.stats();
        assert_eq!(stats.total_rotations, 1);
    }

    #[test]
    fn test_rotation_validation() {
        let effects = Effects::test();
        let account_id = aura_core::AccountId::new_with_effects(&effects);
        let device_id = aura_core::DeviceId::new_with_effects(&effects);

        let middleware = KeyRotationMiddleware::new(RotationConfig::default());
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );

        let participant1 = aura_core::DeviceId::new_with_effects(&effects);
        let participant2 = aura_core::DeviceId::new_with_effects(&effects);
        let participants = vec![device_id.clone(), participant1, participant2];

        // Valid rotation (2 out of 3 participants)
        assert!(middleware
            .validate_rotation_request(2, 2, &participants, &context)
            .is_ok());

        // Invalid zero threshold
        assert!(middleware
            .validate_rotation_request(0, 2, &participants, &context)
            .is_err());

        // Invalid threshold exceeds participants
        assert!(middleware
            .validate_rotation_request(2, 5, &participants, &context)
            .is_err());

        // Invalid empty participants
        assert!(middleware
            .validate_rotation_request(2, 3, &[], &context)
            .is_err());
    }

    #[test]
    fn test_rotation_session_management() {
        let middleware = KeyRotationMiddleware::new(RotationConfig::default());

        // Should start with no active sessions
        let active = middleware.active_rotations().unwrap();
        assert_eq!(active.len(), 0);

        let stats = middleware.stats();
        assert_eq!(stats.active_sessions, 0);
        assert_eq!(stats.total_rotations, 0);
    }

    #[test]
    fn test_insufficient_security_level() {
        let effects = Effects::test();
        let account_id = aura_core::AccountId::new_with_effects(&effects);
        let device_id = aura_core::DeviceId::new_with_effects(&effects);

        let middleware = KeyRotationMiddleware::new(RotationConfig::default());
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard, // Not critical
        );

        // Should fail with insufficient security level
        assert!(middleware
            .validate_rotation_request(2, 3, &[device_id], &context)
            .is_err());
    }
}
