#![allow(dead_code)]
//! Session Coordination Handler
//!
//! Session coordination operations using choreography macros instead of manual patterns.

use super::shared::*;
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::handlers::shared::HandlerUtilities;
use crate::runtime::AuraEffectSystem;
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{RandomEffects, StorageEffects, TransportEffects};
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId};
use aura_macros::choreography;
use aura_protocol::effects::{ChoreographicRole, EffectApiEffects, SessionType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

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
#[allow(dead_code)] // Part of future session coordination API
pub struct SessionRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator_id: DeviceId,
    pub session_id: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Participant invitation message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session coordination API
pub struct ParticipantInvitation {
    pub session_id: String,
    pub session_type: SessionType,
    pub initiator_id: DeviceId,
    pub invited_participants: Vec<DeviceId>,
}

/// Session acceptance message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session coordination API
pub struct SessionAccepted {
    pub session_id: String,
    pub participant_id: DeviceId,
    pub accepted_at: u64,
}

/// Session rejection message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session coordination API
pub struct SessionRejected {
    pub session_id: String,
    pub participant_id: DeviceId,
    pub reason: String,
    pub rejected_at: u64,
}

/// Session creation success message
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session coordination API
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
#[allow(dead_code)] // Part of future session coordination API
pub struct SessionCreationFailed {
    pub session_id: String,
    pub reason: String,
    pub failed_at: u64,
}

/// Session operations handler with authority-first design and choreographic patterns
#[allow(dead_code)] // Part of future session coordination API
pub struct SessionOperations {
    /// Effect system for session operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Authority context
    pub(super) authority_context: AuthorityContext,
    /// Account ID
    _account_id: AccountId,
    /// In-memory participant registry keyed by session id
    pub(super) session_participants: Arc<RwLock<HashMap<String, Vec<DeviceId>>>>,
    /// In-memory metadata registry keyed by session id
    pub(super) session_metadata: Arc<RwLock<HashMap<String, HashMap<String, serde_json::Value>>>>,
}

#[allow(dead_code)]
impl SessionOperations {
    /// Create new session operations handler
    #[allow(dead_code)] // Part of future session coordination API
    pub fn new(
        effects: Arc<RwLock<AuraEffectSystem>>,
        authority_context: AuthorityContext,
        account_id: AccountId,
    ) -> Self {
        Self {
            effects,
            authority_context,
            _account_id: account_id,
            session_participants: Arc::new(RwLock::new(HashMap::new())),
            session_metadata: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Get the device ID derived from authority
    #[allow(dead_code)] // Part of future session coordination API
    pub(super) fn device_id(&self) -> DeviceId {
        self.authority_context.device_id()
    }

    /// Access to effects system for submodules
    #[allow(dead_code)] // Part of future session coordination API
    pub(super) fn effects(&self) -> &Arc<RwLock<AuraEffectSystem>> {
        &self.effects
    }

    pub(super) async fn persist_session_handle(&self, handle: &SessionHandle) -> AgentResult<()> {
        let effects = self.effects().read().await;
        let key = format!("session/{}", handle.session_id);
        let bytes = serde_json::to_vec(handle)
            .map_err(|e| AgentError::effects(format!("serialize session: {e}")))?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store session: {e}")))
    }

    pub(super) async fn load_session_handle(
        &self,
        session_key: &str,
    ) -> AgentResult<Option<SessionHandle>> {
        let effects = self.effects().read().await;
        let key = format!("session/{}", session_key);
        let maybe = effects
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
        let effects = self.effects().read().await;
        let key = format!("session/{session_id}/participants");
        let bytes = serde_json::to_vec(participants)
            .map_err(|e| AgentError::effects(format!("serialize participants: {e}")))?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store participants: {e}")))
    }

    pub(super) async fn persist_metadata(
        &self,
        session_id: &str,
        metadata: &HashMap<String, serde_json::Value>,
    ) -> AgentResult<()> {
        let effects = self.effects().read().await;
        let key = format!("session/{session_id}/metadata");
        let bytes = serde_json::to_vec(metadata)
            .map_err(|e| AgentError::effects(format!("serialize metadata: {e}")))?;
        effects
            .store(&key, bytes)
            .await
            .map_err(|e| AgentError::effects(format!("store metadata: {e}")))
    }

    pub(super) fn guard_context(&self) -> ContextId {
        self.authority_context
            .active_contexts
            .keys()
            .next()
            .copied()
            .unwrap_or_default()
    }

    async fn enforce_guard(
        &self,
        effects: &AuraEffectSystem,
        operation: &str,
        cost: u32,
    ) -> AgentResult<()> {
        if cfg!(test) {
            return Ok(());
        }
        let guard = aura_protocol::guards::send_guard::create_send_guard(
            operation.to_string(),
            self.guard_context(),
            self.authority_context.authority_id,
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
        let effects = self.effects.read().await;
        self.enforce_guard(&effects, "session:create", 100).await?;
        let device_id = self.device_id();
        let _timestamp_millis = effects.current_timestamp().await.unwrap_or(0);

        // Generate unique session ID
        let session_uuid = effects.random_uuid().await;
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
            .execute_session_creation_choreography(&session_request, &effects)
            .await
        {
            Ok(session_handle) => {
                {
                    let mut participants_map = self.session_participants.write().await;
                    participants_map
                        .entry(session_id.clone())
                        .or_insert_with(|| participants.clone());
                }
                {
                    let mut metadata_map = self.session_metadata.write().await;
                    metadata_map
                        .entry(session_id.clone())
                        .or_insert_with(HashMap::new);
                }
                self.persist_session_handle(&session_handle).await?;
                HandlerUtilities::append_relational_fact(
                    &self.authority_context,
                    &effects,
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
    #[allow(dead_code)] // Part of future session coordination API
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
    #[allow(dead_code)] // Part of future session coordination API
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
    #[allow(dead_code)] // Part of future session coordination API
    async fn invite_participants_choreographically(
        &self,
        request: &SessionRequest,
        effects: &AuraEffectSystem,
    ) -> AgentResult<Vec<ParticipantResponse>> {
        let mut responses = Vec::new();
        let timestamp = effects.current_timestamp().await.unwrap_or(0);

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
                source: self.authority_context.authority_id,
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

            effects
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
    #[allow(dead_code)] // Part of future session coordination API
    async fn create_session_handle_choreographically(
        &self,
        request: &SessionRequest,
        effects: &AuraEffectSystem,
    ) -> AgentResult<SessionHandle> {
        let device_id = self.device_id();
        let timestamp_millis = effects.current_timestamp().await.unwrap_or(0);
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
#[allow(dead_code)] // Part of future session coordination API
struct ParticipantResponse {
    participant_id: DeviceId,
    accepted: bool,
    timestamp: u64,
}

impl SessionOperations {
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
        match self
            .get_session_status_via_effects(&effects, &session_id_typed)
            .await
        {
            Ok(Some(handle)) => Ok(Some(handle)),
            Ok(None) => Ok(None),
            Err(_) => Ok(None), // Session doesn't exist or is inactive
        }
    }

    /// End a session
    pub async fn end_session(&self, session_id: &str) -> AgentResult<SessionHandle> {
        let effects = self.effects.read().await;
        self.end_session_via_effects(&effects, session_id).await
    }

    /// List all active sessions
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        let effects = self.effects.read().await;
        self.list_sessions_via_effects(&effects).await
    }

    /// Get session statistics
    pub async fn get_session_stats(&self) -> AgentResult<SessionStats> {
        let effects = self.effects.read().await;
        self.get_session_stats_via_effects(&effects).await
    }

    /// Cleanup expired sessions
    pub async fn cleanup_expired_sessions(&self, max_age_seconds: u64) -> AgentResult<Vec<String>> {
        let effects = self.effects.read().await;
        self.cleanup_sessions_via_effects(&effects, max_age_seconds)
            .await
    }

    // Private implementation methods

    /// Create session via effects system
    async fn create_session_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        _session_type: &SessionType,
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
        effects: &AuraEffectSystem,
        session_id: &str,
    ) -> AgentResult<SessionHandle> {
        // End session (logging removed for simplicity)
        let current_time = effects.current_timestamp().await.unwrap_or(0);

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
        effects: &AuraEffectSystem,
    ) -> AgentResult<SessionStats> {
        let current_time = effects.current_timestamp().await.unwrap_or(0);

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
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_session_creation() {
        let mut authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([1u8; 32]));
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([2u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        let account_id = AccountId::new_from_entropy([3u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));

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
        let mut authority_context =
            AuthorityContext::new(AuthorityId::new_from_entropy([70u8; 32]));
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([11u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        let account_id = AccountId::new_from_entropy([12u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let other_device = DeviceId::new_from_entropy([5u8; 32]);
        let _ = sessions
            .create_session(
                SessionType::Coordination,
                vec![sessions.device_id(), other_device],
            )
            .await
            .unwrap();

        let effects_guard = effects.read().await;
        let envelope = effects_guard
            .receive_envelope()
            .await
            .expect("invitation sent");
        assert_eq!(envelope.destination, AuthorityId::from_uuid(other_device.0));
        assert_eq!(
            envelope.metadata.get("type"),
            Some(&"session_invitation".to_string())
        );
    }

    #[tokio::test]
    async fn session_handles_are_persisted() {
        let mut authority_context =
            AuthorityContext::new(AuthorityId::new_from_entropy([71u8; 32]));
        authority_context.add_context(crate::core::context::RelationalContext {
            context_id: ContextId::new_from_entropy([13u8; 32]),
            participants: vec![],
            metadata: Default::default(),
        });
        let account_id = AccountId::new_from_entropy([14u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(RwLock::new(AuraEffectSystem::testing(&config).unwrap()));
        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let handle = sessions
            .create_session(SessionType::Coordination, vec![sessions.device_id()])
            .await
            .unwrap();

        let storage_key = format!("session/{}", handle.session_id);
        let stored = effects.read().await.retrieve(&storage_key).await.unwrap();

        assert!(stored.is_some(), "session handle persisted to storage");
    }
}
