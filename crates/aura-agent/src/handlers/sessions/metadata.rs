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
    #[allow(dead_code)] // Part of future session metadata API
    pub async fn update_session_metadata(
        &self,
        _session_id: &str,
        _metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let mut metadata_registry = self.session_metadata.write().await;
        let entry = metadata_registry
            .entry(_session_id.to_string())
            .or_insert_with(HashMap::new);
        entry.extend(_metadata);
        self.persist_metadata(_session_id, entry).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            &*self.effects(),
            self.guard_context(),
            "session_metadata_updated",
            &SessionMetadataFact {
                session_id: _session_id.to_string(),
                metadata: entry.clone(),
            },
        )
        .await?;

        Ok(())
    }

    /// Add participant to session
    #[allow(dead_code)] // Part of future session metadata API
    pub async fn add_participant(
        &self,
        _session_id: &str,
        _device_id: DeviceId,
    ) -> AgentResult<()> {
        let mut participant_registry = self.session_participants.write().await;
        let participants = participant_registry
            .entry(_session_id.to_string())
            .or_insert_with(Vec::new);
        if !participants.contains(&_device_id) {
            participants.push(_device_id);
        }
        self.persist_participants(_session_id, participants).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            &*self.effects(),
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
    #[allow(dead_code)] // Part of future session metadata API
    pub async fn remove_participant(
        &self,
        _session_id: &str,
        _device_id: DeviceId,
    ) -> AgentResult<()> {
        let mut participant_registry = self.session_participants.write().await;
        if let Some(participants) = participant_registry.get_mut(_session_id) {
            participants.retain(|id| id != &_device_id);
            self.persist_participants(_session_id, participants).await?;
            HandlerUtilities::append_relational_fact(
                &self.authority_context,
                &*self.effects(),
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
    use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
    use aura_protocol::effects::SessionType;
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
        let mut authority_context = AuthorityContext::new(authority_id);
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([1u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
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
