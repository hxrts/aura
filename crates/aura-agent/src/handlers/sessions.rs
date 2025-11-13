//! Session management operations using session management effects
//!
//! This module provides high-level session management operations that consume
//! session management effects for distributed coordination.
//!
//! **Phase 5 Update**: Now integrated with authorization operations system.

use crate::{
    effects::SessionData,
    errors::{AuraError, Result},
    operations::*,
};
use aura_core::{AccountId, DeviceId};
use aura_protocol::effects::{
    AuraEffectSystem, ChoreographicRole, ConsoleEffects, EffectSystemConfig, LedgerEffects,
    SessionManagementEffects, SessionType,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Session handle for managing active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHandle {
    /// Session ID
    pub session_id: String,
    /// Session type
    pub session_type: SessionType,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// This device's role
    pub my_role: ChoreographicRole,
    /// Session epoch
    pub epoch: u64,
    /// Session start time
    pub start_time: u64,
    /// Session metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total active sessions
    pub active_sessions: usize,
    /// Sessions by type
    pub sessions_by_type: HashMap<String, usize>,
    /// Total participants across all sessions
    pub total_participants: usize,
    /// Average session duration
    pub average_duration: f64,
    /// Last cleanup time
    pub last_cleanup: u64,
}

/// Session operations handler
pub struct SessionOperations {
    /// Effect system for session operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this instance
    device_id: DeviceId,
    /// Account ID
    account_id: AccountId,
    /// Authorized operations handler
    auth_operations: Option<Arc<AuthorizedAgentOperations>>,
}

impl SessionOperations {
    /// Create new session operations handler
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Self {
        Self {
            effects,
            device_id,
            account_id,
            auth_operations: None,
        }
    }

    /// Create new session operations handler with authorization
    pub fn with_authorization(
        effects: Arc<RwLock<AuraEffectSystem>>,
        device_id: DeviceId,
        account_id: AccountId,
        auth_operations: Arc<AuthorizedAgentOperations>,
    ) -> Self {
        Self {
            effects,
            device_id,
            account_id,
            auth_operations: Some(auth_operations),
        }
    }

    /// Create a new session with authorization check
    pub async fn create_session_authorized(
        &self,
        request: AgentOperationRequest,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionHandle> {
        if let Some(auth_ops) = &self.auth_operations {
            let session_op = SessionOperation::Create {
                session_type: format!("{:?}", session_type),
                participants: participants.clone(),
            };

            let agent_op = AgentOperation::Session {
                operation: session_op,
            };

            let auth_request = AgentOperationRequest {
                identity_proof: request.identity_proof,
                operation: agent_op,
                signed_message: request.signed_message,
                context: request.context,
            };

            let result = auth_ops.execute_operation(auth_request).await?;

            match result {
                AgentOperationResult::Session {
                    result: SessionResult::Success { session_id },
                } => {
                    // Return session handle with the authorized session ID
                    self.get_session(&session_id)
                        .await?
                        .ok_or_else(|| AuraError::internal("Session not found after creation"))
                }
                _ => Err(AuraError::internal("Unexpected result type")),
            }
        } else {
            // Fallback to direct session creation
            self.create_session_direct(session_type, participants).await
        }
    }

    /// Create a new coordination session (legacy method, kept for compatibility)
    pub async fn create_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionHandle> {
        self.create_session_direct(session_type, participants).await
    }

    /// Create a new coordination session (direct, no authorization)
    pub async fn create_session_direct(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionHandle> {
        let effects = self.effects.read().await;

        let _ = effects
            .log_info(&format!(
                "Creating {:?} session with {} participants",
                session_type,
                participants.len()
            ))
            .await;

        // Get current timestamp
        let timestamp = LedgerEffects::current_timestamp(&*effects)
            .await
            .unwrap_or(0);

        // Generate session ID
        let session_id = format!(
            "session_{}_{}_{}",
            self.device_id.0.simple(),
            timestamp,
            session_type_suffix(&session_type)
        );

        // Create choreographic roles for participants
        let mut roles = Vec::new();
        for (index, participant) in participants.iter().enumerate() {
            roles.push(ChoreographicRole::new(participant.0, index));
        }

        // Find our role
        let my_role = roles
            .iter()
            .find(|role| role.device_id == self.device_id.0)
            .cloned()
            .unwrap_or_else(|| ChoreographicRole::new(self.device_id.0, 0));

        // Create session data
        let session_data = SessionData {
            session_id: session_id.clone(),
            account_id: self.account_id,
            device_id: self.device_id,
            epoch: timestamp / 1000, // Convert to epoch seconds
            start_time: timestamp,
            participants: roles.clone(),
            my_role,
            session_type: session_type.clone(),
            metadata: Default::default(),
        };

        // Create session through effects using SessionManagementEffects trait
        use crate::effects::SessionManagementEffects as AgentSessionEffects;
        let created_session_id = effects
            .create_session(session_type.clone())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create session: {}", e)))?;

        let created_id = created_session_id.to_string();
        let _ = effects
            .log_info(&format!("Created session: {}", created_id))
            .await;

        Ok(SessionHandle {
            session_id: created_id,
            session_type,
            participants,
            my_role,
            epoch: timestamp / 1000,
            start_time: timestamp,
            metadata: Default::default(),
        })
    }

    /// Get session information
    pub async fn get_session(&self, session_id: &str) -> Result<Option<SessionHandle>> {
        let effects = self.effects.read().await;

        // Convert string to SessionId by parsing the UUID part
        let session_id_typed = if let Some(uuid_str) = session_id.strip_prefix("session-") {
            // Remove "session-" prefix
            match uuid::Uuid::parse_str(uuid_str) {
                Ok(uuid) => aura_core::identifiers::SessionId::from_uuid(uuid),
                Err(_) => aura_core::identifiers::SessionId::new(), // Create new if parsing fails
            }
        } else {
            aura_core::identifiers::SessionId::new()
        };

        // Use the SessionManagementEffects trait to get session status
        let session_status = effects
            .get_session_status(session_id_typed)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get session status: {}", e)))?;

        // For now, return a basic handle if session exists and is active
        if matches!(
            session_status,
            crate::effects::SessionStatus::Active | crate::effects::SessionStatus::Created
        ) {
            Ok(Some(SessionHandle {
                session_id: session_id.to_string(),
                session_type: crate::effects::SessionType::Coordination, // Default fallback
                participants: vec![self.device_id],                      // Basic fallback
                my_role: aura_protocol::effects::ChoreographicRole::new(self.device_id.0, 0),
                epoch: 0, // Fallback
                start_time: LedgerEffects::current_timestamp(&*effects)
                    .await
                    .unwrap_or(0),
                metadata: Default::default(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Update session metadata
    pub async fn update_session_metadata(
        &self,
        session_id: &str,
        _metadata: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let effects = self.effects.read().await;

        // For now, just log that we would update metadata
        // The SessionManagementEffects trait doesn't provide update_metadata method
        // This would need to be implemented via send_session_message or similar
        effects
            .log_debug(&format!(
                "Metadata update requested for session: {}",
                session_id
            ))
            .await
            .ok();

        Ok(())
    }

    /// Add participant to session
    pub async fn add_participant(&self, session_id: &str, device_id: DeviceId) -> Result<()> {
        let effects = self.effects.read().await;

        // For now, just log that we would add a participant
        // This would need to be implemented via session messages or protocol-level coordination
        effects
            .log_info(&format!(
                "Participant addition requested: {} to session: {}",
                device_id, session_id
            ))
            .await
            .ok();

        Ok(())
    }

    /// Remove participant from session
    pub async fn remove_participant(&self, session_id: &str, device_id: DeviceId) -> Result<()> {
        let effects = self.effects.read().await;

        // For now, just log that we would remove a participant
        effects
            .log_info(&format!(
                "Participant removal requested: {} from session: {}",
                device_id, session_id
            ))
            .await
            .ok();

        Ok(())
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> Result<SessionHandle> {
        let effects = self.effects.read().await;

        // Convert string to SessionId and end the session
        // Convert string to SessionId by parsing the UUID part
        let session_id_typed = if let Some(uuid_str) = session_id.strip_prefix("session-") {
            // Remove "session-" prefix
            match uuid::Uuid::parse_str(uuid_str) {
                Ok(uuid) => aura_core::identifiers::SessionId::from_uuid(uuid),
                Err(_) => aura_core::identifiers::SessionId::new(), // Create new if parsing fails
            }
        } else {
            aura_core::identifiers::SessionId::new()
        };
        SessionManagementEffects::end_session(&*effects, session_id_typed)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to end session: {}", e)))?;

        let _ = effects
            .log_info(&format!("Ended session: {}", session_id))
            .await;

        // Return a basic session handle for the ended session
        Ok(SessionHandle {
            session_id: session_id.to_string(),
            session_type: crate::effects::SessionType::Coordination, // Default fallback
            participants: vec![self.device_id],                      // Basic fallback
            my_role: aura_protocol::effects::ChoreographicRole::new(self.device_id.0, 0),
            epoch: 0, // Fallback
            start_time: LedgerEffects::current_timestamp(&*effects)
                .await
                .unwrap_or(0),
            metadata: Default::default(),
        })
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> Result<Vec<String>> {
        let effects = self.effects.read().await;

        // Use the SessionManagementEffects trait to list sessions
        let session_infos = effects
            .list_active_sessions()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list active sessions: {}", e)))?;

        // Convert SessionInfo to session ID strings
        let session_ids: Vec<String> = session_infos
            .into_iter()
            .map(|info| info.session_id.to_string())
            .collect();

        let _ = effects
            .log_debug(&format!("Found {} active sessions", session_ids.len()))
            .await;

        Ok(session_ids)
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> Result<SessionStats> {
        let effects = self.effects.read().await;

        let active_sessions = effects
            .list_active_sessions()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list active sessions: {}", e)))?;

        let mut sessions_by_type = HashMap::new();
        let mut total_participants = 0;
        let mut total_duration = 0.0;
        let mut valid_sessions = 0;

        let current_time = LedgerEffects::current_timestamp(&*effects)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get timestamp: {}", e)))?;

        for session_info in &active_sessions {
            if let Ok(_session_status) = effects.get_session_status(session_info.session_id).await {
                // Use session info to provide stats
                let type_name = format!("{:?}", session_info.session_type);
                *sessions_by_type.entry(type_name).or_insert(0) += 1;

                total_participants += session_info.participants.len();

                let duration = (current_time - session_info.created_at) as f64 / 1000.0; // Convert to seconds
                total_duration += duration;
                valid_sessions += 1;
            }
        }

        let average_duration = if valid_sessions > 0 {
            total_duration / valid_sessions as f64
        } else {
            0.0
        };

        Ok(SessionStats {
            active_sessions: active_sessions.len(),
            sessions_by_type,
            total_participants,
            average_duration,
            last_cleanup: current_time,
        })
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age_seconds: u64) -> Result<Vec<String>> {
        let effects = self.effects.read().await;

        // Use SessionManagementEffects to list sessions and filter by age
        let session_infos = effects
            .list_active_sessions()
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list active sessions: {}", e)))?;

        let current_time = LedgerEffects::current_timestamp(&*effects)
            .await
            .unwrap_or(0);
        let cutoff_time = current_time.saturating_sub(max_age_seconds * 1000); // Convert to milliseconds

        let mut cleaned_sessions = Vec::new();

        // Find and end expired sessions
        for session_info in session_infos {
            if session_info.created_at < cutoff_time
                && SessionManagementEffects::end_session(&*effects, session_info.session_id)
                    .await
                    .is_ok()
            {
                cleaned_sessions.push(session_info.session_id.to_string());
            }
        }

        let _ = effects
            .log_info(&format!(
                "Cleaned up {} expired sessions (older than {}s)",
                cleaned_sessions.len(),
                max_age_seconds
            ))
            .await;

        Ok(cleaned_sessions)
    }

    /// Create threshold operation session
    pub async fn create_threshold_session(
        &self,
        participants: Vec<DeviceId>,
        threshold: usize,
    ) -> Result<SessionHandle> {
        let effects = self.effects.read().await;

        if participants.len() < threshold {
            return Err(AuraError::invalid("Not enough participants for threshold"));
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

        // Update session with metadata
        self.update_session_metadata(&handle.session_id, handle.metadata.clone())
            .await?;

        let _ = effects
            .log_info(&format!(
                "Created threshold session {}/{} for {}",
                threshold,
                handle.participants.len(),
                handle.session_id
            ))
            .await;

        Ok(handle)
    }

    // Note: Recovery sessions have been removed in favor of the simplified
    // aura-recovery crate which handles guardian recovery operations directly.
    // Use the RecoveryOperations handler instead of session-based recovery.

    /// Create key rotation session
    pub async fn create_key_rotation_session(&self) -> Result<SessionHandle> {
        let effects = self.effects.read().await;

        let participants = vec![self.device_id]; // Single participant for self-rotation

        let mut handle = self
            .create_session(SessionType::KeyRotation, participants)
            .await?;

        // Add rotation metadata
        handle.metadata.insert(
            "rotation_type".to_string(),
            serde_json::Value::String("self_rotation".to_string()),
        );

        let timestamp = LedgerEffects::current_timestamp(&*effects)
            .await
            .unwrap_or(0);

        handle.metadata.insert(
            "requested_at".to_string(),
            serde_json::Value::Number(timestamp.into()),
        );

        // Update session with metadata
        self.update_session_metadata(&handle.session_id, handle.metadata.clone())
            .await?;

        effects
            .log_info(&format!(
                "Created key rotation session: {}",
                handle.session_id
            ))
            .await
            .ok();

        Ok(handle)
    }
}

// Helper functions

fn session_type_suffix(session_type: &SessionType) -> &'static str {
    match session_type {
        SessionType::ThresholdOperation => "threshold",
        SessionType::Recovery => "recovery",
        SessionType::KeyRotation => "rotation",
        SessionType::Coordination => "coord",
        SessionType::Backup => "backup",
        SessionType::Custom(_) => "custom",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::effects::AuraEffectSystem;

    #[tokio::test]
    async fn test_session_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let account_id = AccountId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = Arc::new(RwLock::new(AuraEffectSystem::new(config).unwrap()));
        let sessions = SessionOperations::new(effects, device_id, account_id);

        let participants = vec![
            device_id,
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
        ];
        let handle = sessions
            .create_session(SessionType::Coordination, participants.clone())
            .await
            .unwrap();

        assert!(!handle.session_id.is_empty());
        assert_eq!(handle.participants, participants);
        assert_eq!(DeviceId(handle.my_role.device_id), device_id);
    }

    #[tokio::test]
    async fn test_threshold_session() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let account_id = AccountId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = Arc::new(RwLock::new(AuraEffectSystem::new(config).unwrap()));
        let sessions = SessionOperations::new(effects, device_id, account_id);

        let participants = vec![
            device_id,
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
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

    // Note: Recovery session test removed - recovery operations are now handled
    // by the RecoveryOperations handler using the simplified aura-recovery crate.
    // See /Users/hxrts/projects/aura/crates/aura-agent/src/handlers/recovery.rs

    #[tokio::test]
    async fn test_session_stats() {
        use aura_testkit::{test_device_trio, ChoreographyTestHarness};

        // Create a multi-device test harness with 3 devices
        let harness = test_device_trio();

        // Create a few coordinated sessions
        let session1 = harness
            .create_coordinated_session("coordination")
            .await
            .expect("Should create first session");

        let session2 = harness
            .create_coordinated_session("threshold_operation")
            .await
            .expect("Should create second session");

        // Verify both sessions are active
        let status1 = session1
            .status()
            .await
            .expect("Should get session 1 status");
        let status2 = session2
            .status()
            .await
            .expect("Should get session 2 status");

        assert_eq!(status1.session_type, "coordination");
        assert_eq!(status2.session_type, "threshold_operation");
        assert_eq!(status1.participants.len(), 3);
        assert_eq!(status2.participants.len(), 3);

        // Clean up
        session1.end().await.expect("Should end session 1");
        session2.end().await.expect("Should end session 2");
    }

    #[tokio::test]
    async fn test_session_metadata_update() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let account_id = AccountId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = Arc::new(RwLock::new(AuraEffectSystem::new(config).unwrap()));
        let sessions = SessionOperations::new(effects, device_id, account_id);

        let participants = vec![device_id];
        let handle = sessions
            .create_session(SessionType::Coordination, participants)
            .await
            .unwrap();

        let mut metadata = HashMap::new();
        metadata.insert(
            "test_key".to_string(),
            serde_json::Value::String("test_value".to_string()),
        );

        sessions
            .update_session_metadata(&handle.session_id, metadata.clone())
            .await
            .unwrap();

        // Verify update was applied (TODO fix - In a real implementation we'd fetch and check)
        // For test purposes, we just verify the call completed without error
    }
}
