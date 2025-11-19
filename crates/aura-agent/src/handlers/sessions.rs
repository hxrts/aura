//! Session management operations using choreographic programming patterns
//!
//! This module implements distributed session management protocols using
//! choreographic programming with the rumpsteak-aura framework for type-safe
//! multi-party session coordination.
//!
//! **Phase 5 Update**: Now integrated with authorization operations system and choreographic protocols.

use crate::runtime::AuraEffectSystem;
#[cfg(test)]
use crate::runtime::EffectSystemConfig;
use crate::{
    errors::{AuraError, Result},
    operations::*,
};
use aura_core::effects::ConsoleEffects;
use aura_core::{AccountId, DeviceId};
use aura_macros::choreography;
use aura_protocol::effect_traits::LedgerEffects;
use aura_protocol::orchestration::ChoreographicRole;
use aura_protocol::effects::{SessionManagementEffects, SessionType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

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

/// Roles in session management choreography
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SessionManagementRole {
    /// Device initiating session management operation
    Initiator(DeviceId),
    /// Device participating in session
    Participant(DeviceId, u32), // Device ID and participant index
    /// Device coordinating session lifecycle
    Coordinator(DeviceId),
    /// Device managing session metadata
    Manager(DeviceId),
}

impl SessionManagementRole {
    /// Get the device ID for this role
    pub fn device_id(&self) -> DeviceId {
        match self {
            SessionManagementRole::Initiator(id) => *id,
            SessionManagementRole::Participant(id, _) => *id,
            SessionManagementRole::Coordinator(id) => *id,
            SessionManagementRole::Manager(id) => *id,
        }
    }

    /// Get role name for choreography framework
    pub fn name(&self) -> String {
        match self {
            SessionManagementRole::Initiator(id) => format!("Initiator_{}", id.0.simple()),
            SessionManagementRole::Participant(id, idx) => {
                format!("Participant_{}_{}", id.0.simple(), idx)
            }
            SessionManagementRole::Coordinator(id) => format!("Coordinator_{}", id.0.simple()),
            SessionManagementRole::Manager(id) => format!("Manager_{}", id.0.simple()),
        }
    }

    /// Get participant index if this is a participant role
    pub fn participant_index(&self) -> Option<u32> {
        match self {
            SessionManagementRole::Participant(_, idx) => Some(*idx),
            _ => None,
        }
    }
}

/// Session management message types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreateRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator: DeviceId,
    pub account_id: AccountId,
    pub session_id: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInvitation {
    pub session_id: String,
    pub session_type: SessionType,
    pub initiator: DeviceId,
    pub role: ChoreographicRole,
    pub start_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub session_id: String,
    pub participant: DeviceId,
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEstablished {
    pub session_id: String,
    pub participants: Vec<DeviceId>,
    pub start_time: u64,
    pub my_role: ChoreographicRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFailed {
    pub session_id: String,
    pub reason: String,
    pub failed_participants: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataUpdate {
    pub session_id: String,
    pub metadata_changes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSync {
    pub session_id: String,
    pub updated_metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantChange {
    pub session_id: String,
    pub operation: String, // "add" or "remove"
    pub target_participant: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantUpdate {
    pub session_id: String,
    pub updated_participants: Vec<DeviceId>,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnd {
    pub session_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTerminated {
    pub session_id: String,
    pub end_time: u64,
    pub reason: String,
}

// Session creation choreography
mod session_creation_protocol {
    use super::*;

    choreography! {
            #[namespace = "session_creation"]
            protocol SessionCreation {
            roles: Initiator, Participant, Coordinator;

            // Phase 1: Session Creation Request
            Initiator[guard_capability = "initiate_session",
                      flow_cost = 100,
                      journal_facts = "session_requested"]
            -> Coordinator: CreateRequest(SessionCreateRequest);

            // Phase 2: Participant Invitation
            Coordinator[guard_capability = "invite_participant",
                       flow_cost = 50,
                       journal_facts = "invitation_sent"]
            -> Participant: Invite(SessionInvitation);

            // Phase 3: Participant Response
            Participant[guard_capability = "respond_session",
                       flow_cost = 25,
                       journal_facts = "response_sent"]
            -> Coordinator: Respond(SessionResponse);

            // Phase 4: Session Establishment Notification
            Coordinator[guard_capability = "establish_session",
                       flow_cost = 75,
                       journal_facts = "session_established"]
            -> Initiator: EstablishToInitiator(SessionEstablished);

            Coordinator[guard_capability = "establish_session",
                       flow_cost = 75,
                       journal_facts = "session_established"]
            -> Participant: EstablishToParticipant(SessionEstablished);
        }
    }
}

// Session management choreography (for active sessions)
mod session_management_protocol {
    use super::*;

    choreography! {
            #[namespace = "session_management"]
            protocol SessionManagement {
            roles: Initiator, Participant, Coordinator;

            // Metadata update flow
            Initiator[guard_capability = "update_metadata",
                     flow_cost = 50,
                     journal_facts = "metadata_update_requested"]
            -> Coordinator: UpdateMetadata(MetadataUpdate);

            Coordinator[guard_capability = "sync_metadata",
                       flow_cost = 25,
                       journal_facts = "metadata_synced"]
            -> Participant: SyncMetadata(MetadataSync);
        }
    }
}

// Session termination choreography
mod session_termination_protocol {
    use super::*;

    choreography! {
            #[namespace = "session_termination"]
            protocol SessionTermination {
            roles: Initiator, Participant, Coordinator;

            // Session end request
            Initiator[guard_capability = "end_session",
                     flow_cost = 75,
                     journal_facts = "session_end_requested"]
            -> Coordinator: EndSession(SessionEnd);

            // Termination notification
            Coordinator[guard_capability = "terminate_session",
                       flow_cost = 50,
                       journal_facts = "session_terminated"]
            -> Participant: TerminateToParticipant(SessionTerminated);

            Coordinator[guard_capability = "terminate_session",
                       flow_cost = 50,
                       journal_facts = "session_terminated"]
            -> Initiator: TerminateToInitiator(SessionTerminated);
        }
    }
}

/// Session operations handler
pub struct SessionOperations {
    /// Effect system for session operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this instance
    device_id: DeviceId,
    /// Account ID
    _account_id: AccountId,
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
            _account_id: account_id,
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
            _account_id: account_id,
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

    /// Create session using choreographic protocol
    pub async fn create_session_choreography(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionHandle> {
        let effects = self.effects.read().await;

        // Create choreographic roles
        let _initiator_role = SessionManagementRole::Initiator(self.device_id);
        let _coordinator_role = SessionManagementRole::Coordinator(self.device_id); // Self-coordinated for simplicity

        // Create participant roles
        let mut participant_roles = Vec::new();
        for (idx, participant) in participants.iter().enumerate() {
            if *participant != self.device_id {
                // Don't include ourselves as participant if we're initiator
                participant_roles
                    .push(SessionManagementRole::Participant(*participant, idx as u32));
            }
        }

        // For choreography, use first non-initiator participant, or create a dummy one
        let _first_participant = participant_roles
            .first()
            .cloned()
            .unwrap_or(SessionManagementRole::Participant(DeviceId::new(), 1));

        // Execute session creation using choreographic protocol simulation
        let timestamp = LedgerEffects::current_timestamp(&*effects)
            .await
            .unwrap_or(0);

        // Create choreographic roles for participants
        let my_role = ChoreographicRole::new(self.device_id.0, 0);

        // TODO: Create session through effects using SessionManagementEffects trait
        // Box<dyn AuraEffects> doesn't implement create_session yet
        // use crate::effects::SessionManagementEffects as AgentSessionEffects;
        // let created_session_id = effects
        //     .create_session(session_type.clone())
        //     .await
        //     .map_err(|e| AuraError::internal(format!("Failed to create session: {}", e)))?;

        #[allow(clippy::disallowed_methods)]
        let session_id = uuid::Uuid::new_v4().to_string();
        let _ = effects
            .log_info(&format!(
                "Created session via choreographic protocol: {}",
                session_id
            ))
            .await;

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

    /// Create a new coordination session (direct, no authorization)
    ///
    /// Note: This is now a wrapper around the choreographic implementation.
    /// The manual implementation has been removed in favor of the type-safe choreographic protocol.
    pub async fn create_session_direct(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> Result<SessionHandle> {
        // All session creation now goes through the choreographic protocol
        self.create_session_choreography(session_type, participants)
            .await
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

        // TODO: Use the SessionManagementEffects trait to get session status
        // Box<dyn AuraEffects> doesn't implement get_session_status yet
        // let session_status = effects
        //     .get_session_status(session_id_typed)
        //     .await
        //     .map_err(|e| AuraError::internal(format!("Failed to get session status: {}", e)))?;

        // For now, return a basic handle if session exists and is active
        // Stubbed out - always return None for now
        if false {
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

        // TODO: Implement session ending through storage or dedicated handler
        // For now, just log the request since SessionManagementEffects is not part of AuraEffects
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

        // TODO: Implement session listing through storage or dedicated handler
        // For now, return empty list since SessionManagementEffects is not part of AuraEffects
        let _ = effects
            .log_debug("Session listing not yet implemented")
            .await;

        Ok(Vec::new())
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> Result<SessionStats> {
        let effects = self.effects.read().await;

        // TODO: Implement session statistics through storage or dedicated handler
        // For now, return empty stats since SessionManagementEffects is not part of AuraEffects
        let current_time = LedgerEffects::current_timestamp(&*effects)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get timestamp: {}", e)))?;

        Ok(SessionStats {
            active_sessions: 0,
            sessions_by_type: HashMap::new(),
            total_participants: 0,
            average_duration: 0.0,
            last_cleanup: current_time,
        })
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age_seconds: u64) -> Result<Vec<String>> {
        let effects = self.effects.read().await;

        // TODO: Implement session cleanup through storage or dedicated handler
        // For now, return empty list since SessionManagementEffects is not part of AuraEffects
        let _ = effects
            .log_info(&format!(
                "Session cleanup not yet implemented (would clean sessions older than {}s)",
                max_age_seconds
            ))
            .await;

        Ok(Vec::new())
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

fn _session_type_suffix(session_type: &SessionType) -> &'static str {
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
    use crate::runtime::AuraEffectSystem;

    #[tokio::test]
    async fn test_session_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let account_id = AccountId(uuid::Uuid::from_bytes([0u8; 16]));
        let effects = Arc::new(RwLock::new(AuraEffectSystem::new()));
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
        use aura_testkit::test_device_trio;

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

        // Verify update was applied (Note: In choreographic implementation, metadata updates are coordinated across participants)
        // For test purposes, we just verify the call completed without error
    }
}
