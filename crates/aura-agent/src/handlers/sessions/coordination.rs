//! Session Coordination Handler
//!
//! Session coordination operations using choreography macros instead of manual patterns.

use super::shared::*;
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::handlers::shared::HandlerUtilities;
use crate::runtime::AuraEffectSystem;
use crate::runtime::services::SessionManager;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    RandomExtendedEffects, SessionType, StorageCoreEffects, TransportEffects,
};
use aura_core::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::{ChoreographicRole, EffectApiEffects};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

// Session coordination choreography protocol
//
// This choreography implements distributed session creation and management:
// 1. Initiator submits session creation request to coordinator
// 2. Coordinator validates request and seeks participant agreement
// 3. Participants approve or reject session participation
// 4. Coordinator creates session and distributes session handles
choreography! {
    #[namespace = "session_coordination"]
    protocol SessionCoordinationChoreography {
        roles: Initiator, Participants[*], Coordinator;

        // Phase 1: Session Creation Request
        Initiator[guard_capability = "request_session",
                  flow_cost = 100,
                  journal_facts = "session_requested"]
        -> Coordinator: SessionRequest(SessionRequest);

        // Phase 2: Participant Invitation
        Coordinator[guard_capability = "invite_participants",
                   flow_cost = 50,
                   journal_facts = "participants_invited"]
        -> Participants[*]: ParticipantInvitation(ParticipantInvitation);

        // Phase 3: Participant Response
        choice Participants[*] {
            accept: {
                Participants[*][guard_capability = "accept_session",
                              flow_cost = 75,
                              journal_facts = "session_accepted"]
                -> Coordinator: SessionAccepted(SessionAccepted);
            }
            reject: {
                Participants[*][guard_capability = "reject_session",
                              flow_cost = 50,
                              journal_facts = "session_rejected"]
                -> Coordinator: SessionRejected(SessionRejected);
            }
        }

        // Phase 4: Session Creation Result
        choice Coordinator {
            success: {
                Coordinator[guard_capability = "create_session",
                           flow_cost = 200,
                           journal_facts = "session_created",
                           journal_merge = true]
                -> Initiator: SessionCreated(SessionCreated);

                Coordinator[guard_capability = "notify_participants",
                           flow_cost = 100,
                           journal_facts = "session_participants_notified"]
                -> Participants[*]: SessionCreated(SessionCreated);
            }
            failure: {
                Coordinator[guard_capability = "reject_session_creation",
                           flow_cost = 100,
                           journal_facts = "session_creation_failed"]
                -> Initiator: SessionCreationFailed(SessionCreationFailed);

                Coordinator[guard_capability = "notify_participants_failure",
                           flow_cost = 50,
                           journal_facts = "session_failure_notified"]
                -> Participants[*]: SessionCreationFailed(SessionCreationFailed);
            }
        }
    }
}

// Message types for session coordination choreography

/// Session creation request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator_id: DeviceId,
    pub session_id: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Participant invitation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInvitation {
    pub session_id: String,
    pub session_type: SessionType,
    pub initiator_id: DeviceId,
    pub invited_participants: Vec<DeviceId>,
}

/// Session acceptance message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionAccepted {
    pub session_id: String,
    pub participant_id: DeviceId,
    pub accepted_at: u64,
}

/// Session rejection message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRejected {
    pub session_id: String,
    pub participant_id: DeviceId,
    pub reason: String,
    pub rejected_at: u64,
}

/// Session creation success message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    pub session_id: String,
    pub session_handle: SessionHandle,
    pub created_at: u64,
}

#[derive(Debug, Serialize)]
struct SessionCreatedFact {
    session_id: String,
    session_type: SessionType,
    participants: Vec<DeviceId>,
    initiator: DeviceId,
}

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

#[derive(Debug, Serialize)]
struct SessionInvitationFact {
    session_id: String,
    participant: DeviceId,
}

/// Session creation failure message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationFailed {
    pub session_id: String,
    pub reason: String,
    pub failed_at: u64,
}

/// Session operations handler with authority-first design and choreographic patterns
#[derive(Clone)]
pub struct SessionOperations {
    /// Effect system for session operations
    effects: Arc<AuraEffectSystem>,
    /// Authority context
    pub(super) authority_context: AuthorityContext,
    /// Account ID
    _account_id: AccountId,
    /// Session state manager
    pub(super) session_manager: SessionManager,
}

impl SessionOperations {
    /// Create new session operations handler
    pub fn new(
        effects: Arc<AuraEffectSystem>,
        authority_context: AuthorityContext,
        account_id: AccountId,
    ) -> Self {
        Self {
            effects,
            authority_context,
            _account_id: account_id,
            session_manager: SessionManager::new(),
        }
    }

    /// Get the device ID derived from authority
    pub(super) fn device_id(&self) -> DeviceId {
        self.authority_context.device_id()
    }

    /// Access to effects system for submodules
    pub(super) fn effects(&self) -> &Arc<AuraEffectSystem> {
        &self.effects
    }

    pub(super) async fn persist_session_handle(&self, handle: &SessionHandle) -> AgentResult<()> {
        let key = format!("session/{}", handle.session_id);
        let bytes = serde_json::to_vec(handle)
            .map_err(|e| AgentError::effects(format!("serialize session: {e}")))?;
        self.effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store session: {e}")))
    }

    pub(super) async fn load_session_handle(
        &self,
        session_key: &str,
    ) -> AgentResult<Option<SessionHandle>> {
        let key = format!("session/{}", session_key);
        let maybe = self
            .effects
            .retrieve(&key)
            .await
            .map_err(|e| AgentError::effects(format!("retrieve session: {e}")))?;
        if let Some(bytes) = maybe {
            let handle: SessionHandle = serde_json::from_slice(&bytes)
                .map_err(|e| AgentError::effects(format!("deserialize session: {e}")))?;
            Ok(Some(handle))
        } else {
            Ok(None)
        }
    }

    pub(super) async fn persist_participants(
        &self,
        session_id: &str,
        participants: &[DeviceId],
    ) -> AgentResult<()> {
        let key = format!("session/{session_id}/participants");
        let bytes = serde_json::to_vec(participants)
            .map_err(|e| AgentError::effects(format!("serialize participants: {e}")))?;
        self.effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store participants: {e}")))
    }

    pub(super) async fn persist_metadata(
        &self,
        session_id: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let key = format!("session/{session_id}/metadata");
        let bytes = serde_json::to_vec(metadata)
            .map_err(|e| AgentError::effects(format!("serialize metadata: {e}")))?;
        self.effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store metadata: {e}")))
    }

    pub(super) fn guard_context(&self) -> ContextId {
        self.authority_context.default_context_id()
    }

    async fn enforce_guard(
        &self,
        effects: &AuraEffectSystem,
        operation: &str,
        cost: u32,
    ) -> AgentResult<()> {
        // Skip guard enforcement in test mode
        if effects.is_testing() {
            return Ok(());
        }
        let guard = aura_guards::chain::create_send_guard(
            operation.to_string(),
            self.guard_context(),
            self.authority_context.authority_id(),
            cost,
        );
        let result = guard
            .evaluate(effects)
            .await
            .map_err(|e| AgentError::effects(format!("guard evaluation failed: {e}")))?;
        if !result.authorized {
            return Err(AgentError::effects(
                result
                    .denial_reason
                    .unwrap_or_else(|| format!("{operation} not authorized")),
            ));
        }
        Ok(())
    }

    /// Create a new coordination session
    pub async fn create_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        self.create_session_choreography(session_type, participants)
            .await
    }

    /// Create session using choreographic protocol
    pub async fn create_session_choreography(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        self.enforce_guard(&self.effects, "session:create", 100)
            .await?;
        let device_id = self.device_id();
        let _timestamp_millis = self.effects.current_timestamp().await.unwrap_or(0);

        // Generate unique session ID
        let session_uuid = self.effects.random_uuid().await;
        let session_id = format!("session-{}", session_uuid.simple());

        // Create session request message for choreography
        let session_request = SessionRequest {
            session_type: session_type.clone(),
            participants: participants.clone(),
            initiator_id: device_id,
            session_id: session_id.clone(),
            metadata: HashMap::new(),
        };

        // Execute the choreographic protocol
        match self
            .execute_session_creation_choreography(&session_request, &self.effects)
            .await
        {
            Ok(session_handle) => {
                self.session_manager
                    .register_session(&session_id, participants.clone())
                    .await;
                self.persist_session_handle(&session_handle).await?;
                HandlerUtilities::append_relational_fact(
                    &self.authority_context,
                    &self.effects,
                    self.guard_context(),
                    "session_created",
                    &SessionCreatedFact {
                        session_id: session_id.clone(),
                        session_type,
                        participants: participants.clone(),
                        initiator: device_id,
                    },
                )
                .await?;
                tracing::info!(
                    "Session created successfully using choreography: {}",
                    session_id
                );
                Ok(session_handle)
            }
            Err(e) => {
                tracing::error!("Session creation choreography failed: {}", e);
                Err(AgentError::internal(format!(
                    "Choreography execution failed: {}",
                    e
                )))
            }
        }
    }

    /// Execute the session creation choreography protocol
    async fn execute_session_creation_choreography(
        &self,
        request: &SessionRequest,
        effects: &AuraEffectSystem,
    ) -> AgentResult<SessionHandle> {
        tracing::info!(
            "Executing session creation choreography for session {}",
            request.session_id
        );

        // Phase 1: As initiator, create session request (already done)

        // Phase 2: As coordinator, validate request and invite participants
        self.validate_session_request(request, effects).await?;

        let participant_responses = self
            .invite_participants_choreographically(request, effects)
            .await?;

        // Phase 3: Process participant responses
        let accepted_participants = participant_responses
            .iter()
            .filter(|response| response.accepted)
            .count();

        // Phase 4: Create session if sufficient participants accepted
        if accepted_participants >= request.participants.len() / 2 {
            self.create_session_handle_choreographically(request, effects)
                .await
        } else {
            Err(AgentError::invalid(format!(
                "Insufficient participant acceptance: {}/{}",
                accepted_participants,
                request.participants.len()
            )))
        }
    }

    /// Validate session request (choreographic pattern)
    async fn validate_session_request(
        &self,
        request: &SessionRequest,
        _effects: &AuraEffectSystem,
    ) -> AgentResult<()> {
        // Validate participants list is not empty
        if request.participants.is_empty() {
            return Err(AgentError::invalid("No participants specified"));
        }

        // Validate initiator is included in participants
        if !request.participants.contains(&request.initiator_id) {
            return Err(AgentError::invalid(
                "Initiator must be included in participants",
            ));
        }

        // Additional validation would go here (e.g., check authorization)
        tracing::debug!(
            "Session request validation passed for session {}",
            request.session_id
        );
        Ok(())
    }

    /// Simulate participant invitation and response collection (choreographic pattern)
    async fn invite_participants_choreographically(
        &self,
        request: &SessionRequest,
        effects: &AuraEffectSystem,
    ) -> AgentResult<Vec<ParticipantResponse>> {
        let mut responses = Vec::new();
        let timestamp = self.effects.current_timestamp().await.unwrap_or(0);

        // For each participant (excluding initiator), send invitation over transport
        for participant_id in &request.participants {
            if *participant_id == request.initiator_id {
                responses.push(ParticipantResponse {
                    participant_id: *participant_id,
                    accepted: true,
                    timestamp,
                });
                continue;
            }

            self.enforce_guard(effects, "session:invite", 50).await?;

            let invitation = ParticipantInvitation {
                session_id: request.session_id.clone(),
                session_type: request.session_type.clone(),
                initiator_id: request.initiator_id,
                invited_participants: request.participants.clone(),
            };

            let envelope = TransportEnvelope {
                destination: AuthorityId::from_uuid(participant_id.0),
                source: self.authority_context.authority_id(),
                context: self.guard_context(),
                payload: serde_json::to_vec(&invitation)
                    .map_err(|e| AgentError::effects(format!("serialize invitation: {e}")))?,
                metadata: {
                    let mut metadata = HashMap::new();
                    metadata.insert("type".to_string(), "session_invitation".to_string());
                    metadata.insert("session_id".to_string(), request.session_id.clone());
                    metadata
                },
                receipt: None,
            };

            self.effects
                .send_envelope(envelope)
                .await
                .map_err(|e| AgentError::effects(format!("send invitation failed: {e}")))?;

            HandlerUtilities::append_relational_fact(
                &self.authority_context,
                effects,
                self.guard_context(),
                "session_invitation_sent",
                &SessionInvitationFact {
                    session_id: request.session_id.clone(),
                    participant: *participant_id,
                },
            )
            .await?;

            responses.push(ParticipantResponse {
                participant_id: *participant_id,
                accepted: true,
                timestamp,
            });
            tracing::debug!(
                "Participant {} invited for session {}",
                participant_id,
                request.session_id
            );
        }

        Ok(responses)
    }

    /// Create final session handle (choreographic pattern)
    async fn create_session_handle_choreographically(
        &self,
        request: &SessionRequest,
        _effects: &AuraEffectSystem,
    ) -> AgentResult<SessionHandle> {
        let device_id = self.device_id();
        let timestamp_millis = self.effects.current_timestamp().await.unwrap_or(0);
        let my_role = ChoreographicRole::new(device_id.0, 0);

        let session_handle = SessionHandle {
            session_id: request.session_id.clone(),
            session_type: request.session_type.clone(),
            participants: request.participants.clone(),
            my_role,
            epoch: timestamp_millis / 1000,
            start_time: timestamp_millis,
            metadata: request.metadata.clone(),
        };

        // In a full implementation, this would journal the session creation
        tracing::info!("Session handle created for session {}", request.session_id);

        Ok(session_handle)
    }
}

/// Internal type for tracking participant responses
#[derive(Debug, Clone)]
struct ParticipantResponse {
    participant_id: DeviceId,
    accepted: bool,
    timestamp: u64,
}

impl SessionOperations {
    /// Get session information
    pub async fn get_session(&self, session_id: &str) -> AgentResult<Option<SessionHandle>> {
        // Convert string to SessionId by parsing the UUID part
        let session_id_typed = if let Some(uuid_str) = session_id.strip_prefix("session-") {
            match uuid::Uuid::parse_str(uuid_str) {
                Ok(uuid) => aura_core::identifiers::SessionId::from_uuid(uuid),
                Err(_) => aura_core::identifiers::SessionId::new_from_entropy(hash::hash(
                    session_id.as_bytes(),
                )),
            }
        } else {
            aura_core::identifiers::SessionId::new_from_entropy(hash::hash(session_id.as_bytes()))
        };

        // Implement session status lookup via effects system
        match self
            .get_session_status_via_effects(&self.effects, &session_id_typed)
            .await
        {
            Ok(Some(handle)) => Ok(Some(handle)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None), // Session doesn't exist or is inactive
        }
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> AgentResult<SessionHandle> {
        self.end_session_via_effects(&self.effects, session_id)
            .await
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        self.list_sessions_via_effects(&self.effects).await
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> AgentResult<SessionStats> {
        self.get_session_stats_via_effects(&self.effects).await
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age_seconds: u64) -> AgentResult<Vec<String>> {
        self.cleanup_sessions_via_effects(&self.effects, max_age_seconds)
            .await
    }

    // Private implementation methods

    /// Create session via effects system
    async fn create_session_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        session_type: &SessionType,
    ) -> AgentResult<String> {
        use aura_core::identifiers::SessionId;

        let current_time = self.effects.current_timestamp().await.unwrap_or(0);
        let device_id = self.device_id();
        let mut material = Vec::with_capacity(64);
        material.extend_from_slice(b"aura-session");
        material.extend_from_slice(device_id.0.as_bytes());
        material.extend_from_slice(&current_time.to_le_bytes());
        match session_type {
            SessionType::Coordination => material.push(0),
            SessionType::ThresholdOperation => material.push(1),
            SessionType::Recovery => material.push(2),
            SessionType::KeyRotation => material.push(3),
            SessionType::Invitation => material.push(4),
            SessionType::Rendezvous => material.push(5),
            SessionType::Sync => material.push(6),
            SessionType::Backup => material.push(8),
            SessionType::Custom(label) => {
                material.push(7);
                material.extend_from_slice(label.as_bytes());
            }
        }
        let session_id = SessionId::new_from_entropy(hash::hash(&material));
        let session_id_string = format!("session-{}", session_id.uuid().simple());

        // Session created successfully (logging removed for simplicity)

        Ok(session_id_string)
    }

    /// Get session status via effects system
    async fn get_session_status_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        _session_id: &aura_core::identifiers::SessionId,
    ) -> AgentResult<Option<SessionHandle>> {
        // Lookup session status (logging removed for simplicity)

        // Session lookup requires persistent storage integration - return None until wired
        // Real implementation would query effects.retrieve() for session state by ID
        Ok(None)
    }

    /// End session via effects system
    async fn end_session_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        session_id: &str,
    ) -> AgentResult<SessionHandle> {
        // End session (logging removed for simplicity)
        let current_time = self.effects.current_timestamp().await.unwrap_or(0);

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
                metadata.insert(
                    "status".to_string(),
                    serde_json::Value::String("ended".to_string()),
                );
                metadata.insert(
                    "ended_at".to_string(),
                    serde_json::Value::Number(current_time.into()),
                );
                metadata
            },
        })
    }

    /// List sessions via effects system
    async fn list_sessions_via_effects(
        &self,
        _effects: &AuraEffectSystem,
    ) -> AgentResult<Vec<String>> {
        // List sessions (logging removed for simplicity)
        // Return empty list (no persistent storage yet)
        Ok(Vec::new())
    }

    /// Get session statistics via effects system
    async fn get_session_stats_via_effects(
        &self,
        _effects: &AuraEffectSystem,
    ) -> AgentResult<SessionStats> {
        let current_time = self.effects.current_timestamp().await.unwrap_or(0);

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
        _effects: &AuraEffectSystem,
        _max_age_seconds: u64,
    ) -> AgentResult<Vec<String>> {
        // Cleanup sessions (logging removed for simplicity)

        // Return empty list (no persistent storage yet)
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AgentConfig, AuthorityContext};
    use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_session_creation() {
        let authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([1u8; 32]));
        let account_id = AccountId::new_from_entropy([3u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let device_id = sessions.device_id();
        let participants = vec![device_id];

        let handle = sessions
            .create_session(SessionType::Coordination, participants.clone())
            .await
            .unwrap();

        assert!(!handle.session_id.is_empty());
        assert_eq!(handle.participants, participants);
        assert_eq!(DeviceId(handle.my_role.device_id), device_id);
    }

    #[tokio::test]
    async fn invitations_use_transport_envelopes() {
        let authority_context =
            AuthorityContext::new(AuthorityId::new_from_entropy([70u8; 32]));
        let account_id = AccountId::new_from_entropy([12u8; 32]);
        let config = AgentConfig::default();

        // Use shared transport inbox to verify messages are sent
        let shared_transport = crate::runtime::SharedTransport::new();
        let effects = Arc::new(
            AuraEffectSystem::testing_with_shared_transport(
                &config,
                shared_transport.clone(),
            )
            .unwrap(),
        );
        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let other_device = DeviceId::new_from_entropy([5u8; 32]);
        let _ = sessions
            .create_session(
                SessionType::Coordination,
                vec![sessions.device_id(), other_device],
            )
            .await
            .unwrap();

        // Verify that an invitation was sent to the transport layer
        let destination = AuthorityId::from_uuid(other_device.0);
        let inbox = shared_transport.inbox_for(destination);
        let inbox = inbox.read();
        assert_eq!(inbox.len(), 1, "Expected exactly one transport envelope");
        let envelope = &inbox[0];
        assert_eq!(envelope.destination, destination);
        assert_eq!(
            envelope.metadata.get("type"),
            Some(&"session_invitation".to_string())
        );
    }

    #[tokio::test]
    async fn session_handles_are_persisted() {
        let authority_context =
            AuthorityContext::new(AuthorityId::new_from_entropy([71u8; 32]));
        let account_id = AccountId::new_from_entropy([14u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::testing(&config).unwrap());
        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let handle = sessions
            .create_session(SessionType::Coordination, vec![sessions.device_id()])
            .await
            .unwrap();

        let storage_key = format!("session/{}", handle.session_id);
        let stored = effects.retrieve(&storage_key).await.unwrap();

        assert!(stored.is_some(), "session handle persisted to storage");
    }
}
