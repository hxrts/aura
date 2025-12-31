//! Threshold Session Operations
//!
//! Specialized handlers for threshold operation sessions.

use super::{coordination::SessionOperations, shared::*};
use crate::core::{AgentError, AgentResult};
use aura_core::effects::SessionType;
use aura_core::identifiers::DeviceId;
use aura_protocol::effects::EffectApiEffects;

impl SessionOperations {
    /// Create threshold operation session
    pub async fn create_threshold_session(
        &self,
        participants: Vec<DeviceId>,
        threshold: usize,
    ) -> AgentResult<SessionHandle> {
        let _effects = self.effects().clone();

        if participants.len() < threshold {
            return Err(AgentError::config("Not enough participants for threshold"));
        }

        let mut handle = self
            .create_session(SessionType::ThresholdOperation, participants)
            .await?;

        // Add threshold metadata
        handle.metadata.insert(
            "threshold".to_string(),
            serde_json::Value::Number(threshold.into()),
        );
        handle.metadata.insert(
            "total_participants".to_string(),
            serde_json::Value::Number(handle.participants.len().into()),
        );

        // Update session with metadata (this would be coordinated in a full implementation)
        // Session created successfully

        Ok(handle)
    }

    /// Create key rotation session
    pub async fn create_key_rotation_session(&self) -> AgentResult<SessionHandle> {
        let device_id = self.device_id();

        let participants = vec![device_id]; // Single participant for self-rotation

        let mut handle = self
            .create_session(SessionType::KeyRotation, participants)
            .await?;

        // Add rotation metadata
        handle.metadata.insert(
            "rotation_type".to_string(),
            serde_json::Value::String("self_rotation".to_string()),
        );

        let timestamp = self.effects().current_timestamp().await.unwrap_or(0);

        handle.metadata.insert(
            "requested_at".to_string(),
            serde_json::Value::Number(timestamp.into()),
        );

        // Key rotation session created successfully

        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AuthorityContext;
    use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_threshold_session() {
        use crate::core::AgentConfig;
        use crate::runtime::effects::AuraEffectSystem;

        let authority_id = AuthorityId::new_from_entropy([79u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([30u8; 32]);

        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let participants = vec![
            sessions.device_id(),
            DeviceId::from_bytes([1; 32]), // This will not be divisible by 5 when converted to u128
            DeviceId::from_bytes([2; 32]), // This will not be divisible by 5 when converted to u128
        ];

        let handle = sessions
            .create_threshold_session(participants, 2)
            .await
            .unwrap();

        assert!(handle.metadata.contains_key("threshold"));
        assert_eq!(
            handle.metadata["threshold"],
            serde_json::Value::Number(2.into())
        );
    }

    #[tokio::test]
    async fn test_key_rotation_session() {
        use crate::core::AgentConfig;
        use crate::runtime::effects::AuraEffectSystem;

        let authority_id = AuthorityId::new_from_entropy([80u8; 32]);
        let authority_context = AuthorityContext::new(authority_id);
        let account_id = AccountId::new_from_entropy([31u8; 32]);

        let config = AgentConfig::default();
        let effect_system = AuraEffectSystem::testing(&config).unwrap();
        let effects = Arc::new(effect_system);

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let handle = sessions.create_key_rotation_session().await.unwrap();

        assert!(handle.metadata.contains_key("rotation_type"));
        assert_eq!(
            handle.metadata["rotation_type"],
            serde_json::Value::String("self_rotation".to_string())
        );
    }
}
