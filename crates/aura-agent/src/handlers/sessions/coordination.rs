//! Session Coordination Handler
//!
//! Main session coordination operations using choreographic programming patterns.

use super::shared::*;
use crate::core::{AgentResult, AgentError, AuthorityContext};
use crate::runtime::{AuraEffectSystem};
use aura_core::identifiers::{AccountId, DeviceId};
use aura_protocol::effects::{SessionType, ChoreographicRole};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Session operations handler with authority-first design
pub struct SessionOperations {
    /// Effect system for session operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Authority context
    authority_context: AuthorityContext,
    /// Account ID
    _account_id: AccountId,
}

impl SessionOperations {
    /// Create new session operations handler
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        authority_context: AuthorityContext,
        account_id: AccountId,
    ) -> Self {
        Self {
            effects,
            authority_context,
            _account_id: account_id,
        }
    }

    /// Get the device ID derived from authority
    pub(super) fn device_id(&self) -> DeviceId {
        self.authority_context.device_id()
    }
    
    /// Access to effects system for submodules
    pub(super) fn effects(&self) -> &Arc<RwLock<AuraEffectSystem>> {
        &self.effects
    }

    /// Create a new coordination session
    pub async fn create_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        self.create_session_choreography(session_type, participants).await
    }

    /// Create session using choreographic protocol
    pub async fn create_session_choreography(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        let effects = self.effects.read().await;

        // Create choreographic roles
        let device_id = self.device_id();
        let _initiator_role = SessionManagementRole::Initiator(device_id);
        let _coordinator_role = SessionManagementRole::Coordinator(device_id);

        // Create participant roles
        let mut participant_roles = Vec::new();
        for (idx, participant) in participants.iter().enumerate() {
            if *participant != device_id {
                participant_roles.push(SessionManagementRole::Participant(*participant, idx as u32));
            }
        }

        // Execute session creation using choreographic protocol
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Create choreographic role for this device
        let my_role = ChoreographicRole::new(device_id.0, 0);

        // Create session through effects
        let session_id = self.create_session_via_effects(&*effects, &session_type).await?;

        let result = SessionHandle {
            session_id,
            session_type,
            participants,
            my_role,
            epoch: timestamp / 1000,
            start_time: timestamp,
            metadata: Default::default(),
        };

        Ok(result)
    }

    /// Get session information
    pub async fn get_session(&self, session_id: &str) -> AgentResult<Option<SessionHandle>> {
        let effects = self.effects.read().await;

        // Convert string to SessionId by parsing the UUID part
        let session_id_typed = if let Some(uuid_str) = session_id.strip_prefix("session-") {
            match uuid::Uuid::parse_str(uuid_str) {
                Ok(uuid) => aura_core::identifiers::SessionId::from_uuid(uuid),
                Err(_) => aura_core::identifiers::SessionId::new(),
            }
        } else {
            aura_core::identifiers::SessionId::new()
        };

        // Implement session status lookup via effects system
        match self.get_session_status_via_effects(&*effects, &session_id_typed).await {
            Ok(Some(handle)) => Ok(Some(handle)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None), // Session doesn't exist or is inactive
        }
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> AgentResult<SessionHandle> {
        let effects = self.effects.read().await;
        self.end_session_via_effects(&*effects, session_id).await
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        let effects = self.effects.read().await;
        self.list_sessions_via_effects(&*effects).await
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> AgentResult<SessionStats> {
        let effects = self.effects.read().await;
        self.get_session_stats_via_effects(&*effects).await
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age_seconds: u64) -> AgentResult<Vec<String>> {
        let effects = self.effects.read().await;
        self.cleanup_sessions_via_effects(&*effects, max_age_seconds).await
    }

    // Private implementation methods

    /// Create session via effects system
    async fn create_session_via_effects(
        &self,
        effects: &AuraEffectSystem,
        session_type: &SessionType,
    ) -> AgentResult<String> {
        use aura_core::identifiers::SessionId;

        // Generate session ID through effects system
        let session_id = SessionId::new();
        let session_id_string = format!("session-{}", session_id.uuid().simple());

        // Session created successfully (logging removed for simplicity)

        Ok(session_id_string)
    }

    /// Get session status via effects system
    async fn get_session_status_via_effects(
        &self,
        effects: &AuraEffectSystem,
        session_id: &aura_core::identifiers::SessionId,
    ) -> AgentResult<Option<SessionHandle>> {
        // Lookup session status (logging removed for simplicity)

        // For now, simulate that no sessions are found (no persistent storage yet)
        Ok(None)
    }

    /// End session via effects system
    async fn end_session_via_effects(
        &self,
        effects: &AuraEffectSystem,
        session_id: &str,
    ) -> AgentResult<SessionHandle> {
        // End session (logging removed for simplicity)
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let device_id = self.device_id();
        Ok(SessionHandle {
            session_id: session_id.to_string(),
            session_type: SessionType::Coordination,
            participants: vec![device_id],
            my_role: ChoreographicRole::new(device_id.0, 0),
            epoch: 0,
            start_time: current_time,
            metadata: {
                let mut metadata = HashMap::new();
                metadata.insert("status".to_string(), serde_json::Value::String("ended".to_string()));
                metadata.insert("ended_at".to_string(), serde_json::Value::Number(current_time.into()));
                metadata
            },
        })
    }

    /// List sessions via effects system
    async fn list_sessions_via_effects(&self, effects: &AuraEffectSystem) -> AgentResult<Vec<String>> {
        // List sessions (logging removed for simplicity)
        // Return empty list (no persistent storage yet)
        Ok(Vec::new())
    }

    /// Get session statistics via effects system
    async fn get_session_stats_via_effects(&self, effects: &AuraEffectSystem) -> AgentResult<SessionStats> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Return empty stats (no persistent storage yet)
        Ok(SessionStats {
            active_sessions: 0,
            sessions_by_type: HashMap::new(),
            total_participants: 0,
            average_duration: 0.0,
            last_cleanup: current_time,
        })
    }

    /// Cleanup sessions via effects system
    async fn cleanup_sessions_via_effects(
        &self,
        effects: &AuraEffectSystem,
        max_age_seconds: u64,
    ) -> AgentResult<Vec<String>> {
        // Cleanup sessions (logging removed for simplicity)

        // Return empty list (no persistent storage yet)
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::{AccountId, AuthorityId};

    #[tokio::test]
    async fn test_session_creation() {
        use crate::runtime::effects::AuraEffectSystem;
        use crate::core::AgentConfig;
        
        let authority_id = AuthorityId::new();
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new();
        
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config);
        let effects = Arc::new(RwLock::new(effect_system));
        
        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let device_id = sessions.device_id();
        let participants = vec![device_id];
        
        let handle = sessions.create_session(SessionType::Coordination, participants.clone())
            .await.unwrap();

        assert!(!handle.session_id.is_empty());
        assert_eq!(handle.participants, participants);
        assert_eq!(DeviceId(handle.my_role.device_id), device_id);
    }
}