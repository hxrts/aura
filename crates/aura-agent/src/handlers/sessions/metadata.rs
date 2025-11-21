//! Session Metadata Management
//!
//! Handlers for session metadata operations and participant management.

use super::{shared::*, coordination::SessionOperations};
use crate::core::AgentResult;
use aura_core::identifiers::DeviceId;
use std::collections::HashMap;

impl SessionOperations {
    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let effects = self.effects().read().await;

        // For now, just record that we would update metadata
        // In a full implementation, this would use choreographic coordination
        // to sync metadata across all session participants

        Ok(())
    }

    /// Add participant to session
    pub async fn add_participant(
        &self, 
        session_id: &str, 
        device_id: DeviceId
    ) -> AgentResult<()> {
        let effects = self.effects().read().await;

        // For now, just record that we would add a participant
        // In a full implementation, this would use choreographic coordination
        // to update participant lists across all session participants

        Ok(())
    }

    /// Remove participant from session
    pub async fn remove_participant(
        &self, 
        session_id: &str, 
        device_id: DeviceId
    ) -> AgentResult<()> {
        let effects = self.effects().read().await;

        // For now, just record that we would remove a participant
        // In a full implementation, this would use choreographic coordination
        // to update participant lists across all session participants

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AuthorityContext;
    use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
    use aura_protocol::effects::SessionType;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_session_metadata_update() {
        use crate::runtime::effects::AuraEffectSystem;
        use crate::core::AgentConfig;
        
        let authority_id = AuthorityId::new();
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new();
        
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config);
        let effects = Arc::new(RwLock::new(effect_system));

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let participants = vec![sessions.device_id()];
        let handle = sessions.create_session(SessionType::Coordination, participants)
            .await.unwrap();

        let mut metadata = HashMap::new();
        metadata.insert(
            "test_key".to_string(),
            serde_json::Value::String("test_value".to_string()),
        );

        // Should complete without error
        sessions.update_session_metadata(&handle.session_id, metadata)
            .await.unwrap();
    }

    #[tokio::test]
    async fn test_participant_management() {
        use crate::runtime::effects::AuraEffectSystem;
        use crate::core::AgentConfig;
        
        let authority_id = AuthorityId::new();
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new();
        
        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config);
        let effects = Arc::new(RwLock::new(effect_system));

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let participants = vec![sessions.device_id()];
        let handle = sessions.create_session(SessionType::Coordination, participants)
            .await.unwrap();

        let new_device = DeviceId::new();
        
        // Should complete without error
        sessions.add_participant(&handle.session_id, new_device)
            .await.unwrap();
            
        sessions.remove_participant(&handle.session_id, new_device)
            .await.unwrap();
    }
}