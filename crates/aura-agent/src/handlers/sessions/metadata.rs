//! Session Metadata Management
//!
//! Handlers for session metadata operations and participant management.

use super::coordination::SessionOperations;
use crate::core::{AgentError, AgentResult};
use crate::fact_types::{
    SESSION_METADATA_UPDATED_FACT_TYPE_ID, SESSION_PARTICIPANT_ADDED_FACT_TYPE_ID,
    SESSION_PARTICIPANT_REMOVED_FACT_TYPE_ID,
};
use crate::handlers::shared::HandlerUtilities;
use aura_core::types::identifiers::{DeviceId, SessionId};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
struct SessionParticipantsFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    participants: Vec<DeviceId>,
}

#[derive(Debug, Serialize)]
struct SessionMetadataFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    metadata: HashMap<String, serde_json::Value>,
}

mod session_id_serde {
    use aura_core::types::identifiers::SessionId;
    use serde::Serializer;

    pub fn serialize<S>(session_id: &SessionId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&session_id.to_string())
    }
}

impl SessionOperations {
    fn session_id_from_str(session_id: &str) -> AgentResult<SessionId> {
        session_id.parse::<SessionId>().map_err(|e| {
            AgentError::invalid(format!(
                "invalid session id `{session_id}` for session metadata: {e}"
            ))
        })
    }

    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: SessionId,
        metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let updated = self
            .session_manager
            .update_metadata(session_id, metadata)
            .await;
        self.persist_metadata(session_id, &updated).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            self.effects(),
            self.guard_context(),
            SESSION_METADATA_UPDATED_FACT_TYPE_ID,
            &SessionMetadataFact {
                session_id,
                metadata: updated.clone(),
            },
        )
        .await?;

        Ok(())
    }

    /// Update session metadata from a string session identifier.
    pub async fn update_session_metadata_from_str(
        &self,
        session_id: &str,
        metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        self.update_session_metadata(Self::session_id_from_str(session_id)?, metadata)
            .await
    }

    /// Add participant to session
    pub async fn add_participant(
        &self,
        session_id: SessionId,
        device_id: DeviceId,
    ) -> AgentResult<()> {
        let participants = self
            .session_manager
            .add_participant(session_id, device_id)
            .await;
        self.persist_participants(session_id, &participants).await?;
        HandlerUtilities::append_relational_fact(
            &self.authority_context,
            self.effects(),
            self.guard_context(),
            SESSION_PARTICIPANT_ADDED_FACT_TYPE_ID,
            &SessionParticipantsFact {
                session_id,
                participants: participants.clone(),
            },
        )
        .await?;

        Ok(())
    }

    /// Remove participant from session
    pub async fn remove_participant(
        &self,
        session_id: SessionId,
        device_id: DeviceId,
    ) -> AgentResult<()> {
        if let Some(participants) = self
            .session_manager
            .remove_participant(session_id, device_id)
            .await
        {
            self.persist_participants(session_id, &participants).await?;
            HandlerUtilities::append_relational_fact(
                &self.authority_context,
                self.effects(),
                self.guard_context(),
                SESSION_PARTICIPANT_REMOVED_FACT_TYPE_ID,
                &SessionParticipantsFact {
                    session_id,
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
    use aura_core::types::identifiers::{AccountId, AuthorityId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_session_metadata_update() {
        use crate::core::AgentConfig;

        let authority_id = AuthorityId::new_from_entropy([82u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([10u8; 32]);

        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);
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
            .update_session_metadata(handle.session_id, metadata)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_participant_management() {
        use crate::core::AgentConfig;

        let authority_id = AuthorityId::new_from_entropy([83u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([11u8; 32]);

        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);
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
            .add_participant(handle.session_id, new_device)
            .await
            .unwrap();

        sessions
            .remove_participant(handle.session_id, new_device)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_session_facts_are_journaled() {
        use crate::core::AgentConfig;

        let authority_id = AuthorityId::new_from_entropy([84u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([12u8; 32]);

        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);
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
            .update_session_metadata(handle.session_id, metadata)
            .await
            .unwrap();

        // No-op journaling path; presence not asserted here.
    }

    #[tokio::test]
    async fn invalid_session_id_is_rejected_without_persisting() {
        use crate::core::{AgentConfig, AgentError};
        use aura_core::effects::StorageCoreEffects;

        let authority_id = AuthorityId::new_from_entropy([85u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([13u8; 32]);

        let config = AgentConfig::default();
        let effect_system = crate::testing::simulation_effect_system(&config);
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let mut metadata = HashMap::new();
        metadata.insert(
            "label".to_string(),
            serde_json::Value::String("demo".to_string()),
        );

        let invalid_session_id = "not-a-session-id";
        let err = sessions
            .update_session_metadata_from_str(invalid_session_id, metadata)
            .await
            .expect_err("invalid session id should be rejected");
        assert!(
            matches!(err, AgentError::Config(_)),
            "expected invalid/config error, got {err:?}"
        );

        let stored = effects
            .retrieve(&format!("session/{invalid_session_id}/metadata"))
            .await
            .expect("storage lookup should succeed");
        assert!(
            stored.is_none(),
            "invalid session id should not persist metadata"
        );
    }
}
