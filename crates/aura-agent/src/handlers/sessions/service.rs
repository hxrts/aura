//! Session Service - Public API for Session Management
//!
//! Provides a clean public interface for session management operations.
//! Wraps `SessionOperations` with ergonomic methods and proper error handling.

use super::coordination::SessionOperations;
use super::shared::{SessionHandle, SessionStats};
use crate::core::{AgentResult, AuthorityContext};
use crate::runtime::AuraEffectSystem;
use aura_core::identifiers::{AccountId, DeviceId};
use aura_protocol::effects::SessionType;
use std::sync::Arc;

/// Session management service
///
/// Provides session creation, management, and lifecycle operations
/// through a clean public API.
pub struct SessionService {
    operations: SessionOperations,
}

impl SessionService {
    /// Create a new session service
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        account_id: AccountId,
    ) -> Self {
        Self {
            operations: SessionOperations::new(effects, authority_context, account_id),
        }
    }

    /// Create a new coordination session
    ///
    /// Creates a session for coordinating operations between multiple devices.
    ///
    /// # Arguments
    /// * `participants` - Device IDs of all participants including self
    ///
    /// # Returns
    /// A `SessionHandle` for the newly created session
    pub async fn create_coordination_session(
        &self,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        self.operations
            .create_session(SessionType::Coordination, participants)
            .await
    }

    /// Create a threshold operation session
    ///
    /// Creates a session for threshold cryptographic operations.
    ///
    /// # Arguments
    /// * `participants` - Device IDs of threshold participants
    /// * `threshold` - Minimum number of participants required
    ///
    /// # Returns
    /// A `SessionHandle` for the threshold session
    pub async fn create_threshold_session(
        &self,
        participants: Vec<DeviceId>,
        threshold: usize,
    ) -> AgentResult<SessionHandle> {
        self.operations
            .create_threshold_session(participants, threshold)
            .await
    }

    /// Create a key rotation session
    ///
    /// Creates a session for rotating cryptographic keys.
    ///
    /// # Returns
    /// A `SessionHandle` for the rotation session
    pub async fn create_key_rotation_session(&self) -> AgentResult<SessionHandle> {
        self.operations.create_key_rotation_session().await
    }

    /// Get session by ID
    ///
    /// # Arguments
    /// * `session_id` - The session identifier
    ///
    /// # Returns
    /// The `SessionHandle` if found, or `None`
    pub async fn get_session(&self, session_id: &str) -> AgentResult<Option<SessionHandle>> {
        self.operations.get_session(session_id).await
    }

    /// End a session
    ///
    /// Terminates an active session and cleans up resources.
    ///
    /// # Arguments
    /// * `session_id` - The session to end
    ///
    /// # Returns
    /// The final `SessionHandle` with end status
    pub async fn end_session(&self, session_id: &str) -> AgentResult<SessionHandle> {
        self.operations.end_session(session_id).await
    }

    /// List all active sessions
    ///
    /// # Returns
    /// Vector of active session IDs
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        self.operations.list_active_sessions().await
    }

    /// Get session statistics
    ///
    /// # Returns
    /// Aggregate statistics about sessions
    pub async fn get_stats(&self) -> AgentResult<SessionStats> {
        self.operations.get_session_stats().await
    }

    /// Cleanup expired sessions
    ///
    /// Removes sessions that have exceeded the maximum age.
    ///
    /// # Arguments
    /// * `max_age_seconds` - Maximum session age in seconds
    ///
    /// # Returns
    /// Vector of session IDs that were cleaned up
    pub async fn cleanup_expired(&self, max_age_seconds: u64) -> AgentResult<Vec<String>> {
        self.operations
            .cleanup_expired_sessions(max_age_seconds)
            .await
    }

    /// Get the device ID for this service
    pub fn device_id(&self) -> DeviceId {
        self.operations.device_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::identifiers::AuthorityId;

    #[tokio::test]
    async fn test_session_service_creation() {
        let authority_id = AuthorityId::new_from_entropy([42u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([43u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

        let service = SessionService::new(effects, authority_context, account_id);

        // Test creating a coordination session
        let participants = vec![service.device_id()];
        let handle = service
            .create_coordination_session(participants.clone())
            .await
            .unwrap();

        assert!(!handle.session_id.is_empty());
        assert_eq!(handle.participants, participants);
    }

    #[tokio::test]
    async fn test_session_service_stats() {
        let authority_id = AuthorityId::new_from_entropy([44u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([45u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

        let service = SessionService::new(effects, authority_context, account_id);

        let stats = service.get_stats().await.unwrap();
        assert_eq!(stats.active_sessions, 0);
    }
}
