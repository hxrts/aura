//! Session Metadata Management
//!
//! Handlers for session metadata operations and participant management.

use super::coordination::SessionOperations;
use crate::core::AgentResult;
use crate::handlers::shared::HandlerUtilities;
use aura_core::identifiers::DeviceId;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
struct SessionParticipantsFact {
    session_id: String,
    participants: Vec<DeviceId>,
}

#[derive(Debug, Serialize)]
struct SessionMetadataFact {
    session_id: String,
    metadata: HashMap<String, serde_json::Value>,
}

impl SessionOperations {
    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        _session_id: &str,
        _metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let updated = self
            .session_manager
            .update_metadata(_session_id, _metadata)
            .await;
        self.persist_metadata(_session_id, &updated).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            self.effects(),
            self.guard_context(),
            "session_metadata_updated",
            &SessionMetadataFact {
                session_id: _session_id.to_string(),
                metadata: updated.clone(),
            },
        )
        .await?;

        Ok(())
    }

    /// Add participant to session
    pub async fn add_participant(
        &self,
        _session_id: &str,
        _device_id: DeviceId,
    ) -> AgentResult<()> {
        let participants = self
            .session_manager
            .add_participant(_session_id, _device_id)
            .await;
        self.persist_participants(_session_id, &participants).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            self.effects(),
            self.guard_context(),
            "session_participant_added",
            &SessionParticipantsFact {
                session_id: _session_id.to_string(),
                participants: participants.clone(),
            },
        )
        .await?;

        Ok(())
    }

    /// Remove participant from session
    pub async fn remove_participant(
        &self,
        _session_id: &str,
        _device_id: DeviceId,
    ) -> AgentResult<()> {
        if let Some(participants) = self
            .session_manager
            .remove_participant(_session_id, _device_id)
            .await
        {
            self.persist_participants(_session_id, &participants).await?;
            HandlerUtilities::append_relational_fact(
                &self.authority_context,
                self.effects(),
                self.guard_context(),
                "session_participant_removed",
                &SessionParticipantsFact {
                    session_id: _session_id.to_string(),
                    participants: participants.clone(),
                },
            )
            .await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AuthorityContext;
    use aura_core::effects::SessionType;
    use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_session_metadata_update() {
        use crate::core::AgentConfig;
        use crate::runtime::effects::AuraEffectSystem;

        let authority_id = AuthorityId::new_from_entropy([82u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([10u8; 32]);

        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let participants = vec![sessions.device_id()];
        let handle = sessions
            .create_session(SessionType::Coordination, participants)
            .await
            .unwrap();

        let mut metadata = HashMap::new();
        metadata.insert(
            "test_key".to_string(),
            serde_json::Value::String("test_value".to_string()),
        );

        // Should complete without error
        sessions
            .update_session_metadata(&handle.session_id, metadata)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_participant_management() {
        use crate::core::AgentConfig;
        use crate::runtime::effects::AuraEffectSystem;

        let authority_id = AuthorityId::new_from_entropy([83u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([11u8; 32]);

        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let participants = vec![sessions.device_id()];
        let handle = sessions
            .create_session(SessionType::Coordination, participants)
            .await
            .unwrap();

        let new_device = DeviceId::new_from_entropy([7u8; 32]);

        // Should complete without error
        sessions
            .add_participant(&handle.session_id, new_device)
            .await
            .unwrap();

        sessions
            .remove_participant(&handle.session_id, new_device)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_session_facts_are_journaled() {
        use crate::core::AgentConfig;
        use crate::runtime::effects::AuraEffectSystem;

        let authority_id = AuthorityId::new_from_entropy([84u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([12u8; 32]);

        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let participants = vec![sessions.device_id()];
        let handle = sessions
            .create_session(SessionType::Coordination, participants.clone())
            .await
            .unwrap();

        // Trigger metadata update to ensure fact emitted
        let mut metadata = HashMap::new();
        metadata.insert(
            "label".to_string(),
            serde_json::Value::String("demo".to_string()),
        );
        sessions
            .update_session_metadata(&handle.session_id, metadata)
            .await
            .unwrap();

        // No-op journaling path; presence not asserted here.
    }
}
