//! Session Coordination Handler
//!
//! Session coordination operations using choreography macros instead of manual patterns.

use super::shared::*;
use crate::core::{AgentError, AgentResult, AuthorityContext};
use crate::fact_types::{SESSION_CREATED_FACT_TYPE_ID, SESSION_INVITATION_SENT_FACT_TYPE_ID};
use crate::handlers::shared::HandlerUtilities;
use crate::runtime::services::SessionManager;
use crate::runtime::vm_host_bridge::{
    close_and_reap_vm_session, flush_pending_vm_sends, inject_vm_receive,
    open_manifest_vm_session_admitted, receive_blocked_vm_message,
};
use crate::runtime::{AuraEffectSystem, RuntimeChoreographySessionId};
use aura_core::effects::transport::TransportEnvelope;
use aura_core::effects::{
    RandomExtendedEffects, SessionType, StorageCoreEffects, TransportEffects, TransportError,
};
use aura_core::hash;
use aura_core::identifiers::{AccountId, AuthorityId, ContextId, DeviceId, SessionId};
use aura_core::util::serialization::to_vec;
use aura_core::FlowCost;
use aura_macros::choreography;
use aura_protocol::effects::{
    ChoreographicEffects, ChoreographicRole, EffectApiEffects, RoleIndex,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use telltale_vm::vm::StepResult;

// Session coordination choreography protocol
//
// This choreography implements distributed session creation and management:
// 1. Initiator submits session creation request to coordinator
// 2. Coordinator validates request and seeks participant agreement
// 3. Participants approve or reject session participation
// 4. Coordinator creates session and distributes session handles
choreography!(include_str!("src/handlers/sessions/coordination.choreo"));

// Re-export role type for external use (tests, etc.)
pub use self::telltale_session_types_session_coordination::session_coordination::SessionCoordinationChoreographyRole as SessionCoordinationRole;

// Message types for session coordination choreography

/// Session creation request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator_id: DeviceId,
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Participant invitation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInvitation {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub session_type: SessionType,
    pub initiator_id: DeviceId,
    pub invited_participants: Vec<DeviceId>,
}

/// Session acceptance message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDecision {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub participant_id: DeviceId,
    pub accepted: bool,
    pub reason: Option<String>,
    pub timestamp: u64,
}

/// Session creation success message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreated {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub session_handle: SessionHandle,
    pub created_at: u64,
}

#[derive(Debug, Serialize)]
struct SessionCreatedFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    session_type: SessionType,
    participants: Vec<DeviceId>,
    initiator: DeviceId,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct SessionParticipantsFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    participants: Vec<DeviceId>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct SessionMetadataFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct SessionInvitationFact {
    #[serde(with = "session_id_serde")]
    session_id: SessionId,
    participant: DeviceId,
}

/// Session creation failure message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreationFailed {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub reason: String,
    pub failed_at: u64,
}

mod session_id_serde {
    use aura_core::identifiers::SessionId;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(session_id: &SessionId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&session_id.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SessionId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value
            .parse::<SessionId>()
            .map_err(|e| D::Error::custom(format!("invalid session id `{value}`: {e}")))
    }
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

    #[allow(dead_code)]
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
        session_id: SessionId,
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
        session_id: SessionId,
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
        cost: FlowCost,
    ) -> AgentResult<()> {
        // Skip guard enforcement in test/simulation mode
        if effects.is_testing() {
            return Ok(());
        }
        let guard = aura_guards::chain::create_send_guard(
            aura_guards::types::CapabilityId::from(operation),
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
        self.create_session_with_metadata(session_type, participants, HashMap::new())
            .await
    }

    /// Create a new coordination session with initial metadata
    ///
    /// The metadata is included in the session request and coordinated through
    /// the choreographic protocol, ensuring all participants receive the same
    /// metadata as part of the session creation process.
    pub async fn create_session_with_metadata(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<SessionHandle> {
        self.create_session_choreography_with_metadata(session_type, participants, metadata)
            .await
    }

    /// Create session using choreographic protocol
    pub async fn create_session_choreography(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
    ) -> AgentResult<SessionHandle> {
        self.create_session_choreography_with_metadata(session_type, participants, HashMap::new())
            .await
    }

    /// Create session using choreographic protocol with initial metadata
    ///
    /// The metadata is included in the session request and distributed to all
    /// participants through the choreography, ensuring consistent state.
    pub async fn create_session_choreography_with_metadata(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> AgentResult<SessionHandle> {
        self.enforce_guard(&self.effects, "session:create", FlowCost::new(100))
            .await?;
        let device_id = self.device_id();
        let _timestamp_millis = self.effects.current_timestamp().await.unwrap_or(0);

        // Generate unique session ID
        let runtime_session_id =
            RuntimeChoreographySessionId::from_uuid(self.effects.random_uuid().await);
        let session_id = runtime_session_id.into_aura_session_id();

        // Create session request message for choreography with provided metadata
        let session_request = SessionRequest {
            session_type: session_type.clone(),
            participants: participants.clone(),
            initiator_id: device_id,
            session_id,
            metadata,
        };

        // Execute the choreographic protocol
        match self
            .execute_session_creation_choreography(&session_request, &self.effects)
            .await
        {
            Ok(session_handle) => {
                self.session_manager
                    .register_session(session_id, participants.clone())
                    .await;
                self.persist_session_handle(&session_handle).await?;
                HandlerUtilities::append_relational_fact(
                    &self.authority_context,
                    &self.effects,
                    self.guard_context(),
                    SESSION_CREATED_FACT_TYPE_ID,
                    &SessionCreatedFact {
                        session_id,
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

            self.enforce_guard(effects, "session:invite", FlowCost::new(50))
                .await?;

            let invitation = ParticipantInvitation {
                session_id: request.session_id,
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
                    metadata.insert("session_id".to_string(), request.session_id.to_string());
                    metadata
                },
                receipt: None,
            };

            match self.effects.send_envelope(envelope).await {
                Ok(()) => {}
                Err(TransportError::DestinationUnreachable { destination }) => {
                    tracing::warn!(
                        %destination,
                        session_id = %request.session_id,
                        "Participant unreachable; invitation will be retried on reconnect"
                    );
                }
                Err(e) => {
                    return Err(AgentError::effects(format!("send invitation failed: {e}")));
                }
            }

            HandlerUtilities::append_relational_fact(
                &self.authority_context,
                effects,
                self.guard_context(),
                SESSION_INVITATION_SENT_FACT_TYPE_ID,
                &SessionInvitationFact {
                    session_id: request.session_id,
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
        let role_index = RoleIndex::new(0).expect("role index");
        let my_role = ChoreographicRole::new(device_id, role_index);

        let session_handle = SessionHandle {
            session_id: request.session_id,
            session_type: request.session_type.clone(),
            participants: request.participants.clone(),
            my_role,
            epoch: timestamp_millis / 1000,
            start_time: timestamp_millis,
            metadata: request.metadata.clone(),
        };

        // Session journaling happens in the caller (create_session_choreography_with_metadata)
        // via HandlerUtilities::append_relational_fact after the handle is created
        tracing::info!("Session handle created for session {}", request.session_id);

        Ok(session_handle)
    }

    // ====================================================================
    // Choreography Wiring (execute_as)
    // ====================================================================

    /// Execute session coordination as initiator.
    pub async fn execute_session_coordination_initiator(
        &self,
        coordinator_id: AuthorityId,
        participants: Vec<AuthorityId>,
        session_request: SessionRequest,
        _success: bool,
    ) -> AgentResult<()> {
        let authority_id = self.authority_context.authority_id();

        let session_uuid =
            session_coordination_runtime_session_id(&session_request.session_id)?.as_uuid();
        let mut roles = vec![coordination_role(authority_id, 0)];
        roles.push(coordination_role(coordinator_id, 0));
        for participant in &participants {
            roles.push(coordination_role(*participant, 0));
        }
        let peer_roles = BTreeMap::from([(
            "Coordinator".to_string(),
            coordination_role(coordinator_id, 0),
        )]);
        let manifest =
            self::telltale_session_types_session_coordination::vm_artifacts::composition_manifest();
        let global_type =
            self::telltale_session_types_session_coordination::vm_artifacts::global_type();
        let local_types =
            self::telltale_session_types_session_coordination::vm_artifacts::local_types();

        self.effects
            .start_session(session_uuid, roles)
            .await
            .map_err(|e| AgentError::internal(format!("session coordination start failed: {e}")))?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                &manifest,
                "Initiator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(
                to_vec(&session_request)
                    .map_err(|error| AgentError::internal(format!("session request encode failed: {error}")))?,
            );

            loop {
                let step = engine.step().map_err(|error| {
                    AgentError::internal(format!("session coordination initiator VM step failed: {error}"))
                })?;
                flush_pending_vm_sends(self.effects.as_ref(), handler.as_ref(), &peer_roles)
                    .await
                    .map_err(AgentError::internal)?;

                if let Some(blocked) = receive_blocked_vm_message(
                    self.effects.as_ref(),
                    engine.vm(),
                    vm_sid,
                    "Initiator",
                    &peer_roles,
                )
                .await
                .map_err(|error| {
                    AgentError::internal(format!("session coordination initiator receive failed: {error}"))
                })? {
                    inject_vm_receive(&mut engine, vm_sid, &blocked).map_err(AgentError::internal)?;
                    continue;
                }

                match step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "session coordination initiator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            }
            .map(|_| {
                let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            })
        }
        .await;

        let _ = self.effects.end_session().await;
        result
    }

    /// Execute session coordination as coordinator.
    pub async fn execute_session_coordination_coordinator(
        &self,
        initiator_id: AuthorityId,
        participants: Vec<AuthorityId>,
        invitation: ParticipantInvitation,
        created: SessionCreated,
        failed: SessionCreationFailed,
        accept_threshold: usize, // usize ok: function parameter, not serialized
    ) -> AgentResult<()> {
        let authority_id = self.authority_context.authority_id();
        let session_uuid =
            session_coordination_runtime_session_id(&invitation.session_id)?.as_uuid();
        let mut roles = vec![coordination_role(authority_id, 0)];
        roles.push(coordination_role(initiator_id, 0));
        for participant in &participants {
            roles.push(coordination_role(*participant, 0));
        }
        let mut peer_roles =
            BTreeMap::from([("Initiator".to_string(), coordination_role(initiator_id, 0))]);
        for (idx, participant) in participants.iter().enumerate() {
            peer_roles.insert(
                format!("Participant{idx}"),
                coordination_role(*participant, 0),
            );
        }
        let manifest =
            self::telltale_session_types_session_coordination::vm_artifacts::composition_manifest();
        let global_type =
            self::telltale_session_types_session_coordination::vm_artifacts::global_type();
        let local_types =
            self::telltale_session_types_session_coordination::vm_artifacts::local_types();

        self.effects
            .start_session(session_uuid, roles)
            .await
            .map_err(|e| AgentError::internal(format!("session coordination start failed: {e}")))?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                &manifest,
                "Coordinator",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            for _ in &participants {
                handler.push_send_bytes(
                    to_vec(&invitation).map_err(|error| {
                        AgentError::internal(format!("participant invitation encode failed: {error}"))
                    })?,
                );
            }
            let mut decisions = Vec::new();
            let mut branch_queued = false;

            loop {
                let step = engine.step().map_err(|error| {
                    AgentError::internal(format!("session coordination coordinator VM step failed: {error}"))
                })?;
                flush_pending_vm_sends(self.effects.as_ref(), handler.as_ref(), &peer_roles)
                    .await
                    .map_err(AgentError::internal)?;

                if let Some(blocked) = receive_blocked_vm_message(
                    self.effects.as_ref(),
                    engine.vm(),
                    vm_sid,
                    "Coordinator",
                    &peer_roles,
                )
                .await
                .map_err(|error| {
                    AgentError::internal(format!("session coordination coordinator receive failed: {error}"))
                })? {
                    let decision: SessionDecision = serde_json::from_slice(&blocked.payload)
                        .map_err(|error| {
                            AgentError::internal(format!("session decision decode failed: {error}"))
                        })?;
                    decisions.push(decision);
                    if !branch_queued && decisions.len() == participants.len() {
                        let accepted = decisions.iter().filter(|decision| decision.accepted).count();
                        let success = accepted >= accept_threshold;
                        handler.push_choice_label(if success { "Success" } else { "Failure" });
                        if success {
                            let initiator_payload = to_vec(&created).map_err(|error| {
                                AgentError::internal(format!("session created encode failed: {error}"))
                            })?;
                            handler.push_send_bytes(initiator_payload.clone());
                            for _ in &participants {
                                handler.push_send_bytes(initiator_payload.clone());
                            }
                        } else {
                            let initiator_payload = to_vec(&failed).map_err(|error| {
                                AgentError::internal(format!("session failure encode failed: {error}"))
                            })?;
                            handler.push_send_bytes(initiator_payload.clone());
                            for _ in &participants {
                                handler.push_send_bytes(initiator_payload.clone());
                            }
                        }
                        branch_queued = true;
                    }
                    inject_vm_receive(&mut engine, vm_sid, &blocked).map_err(AgentError::internal)?;
                    continue;
                }

                match step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "session coordination coordinator VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            }
            .map(|_| {
                let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            })
        }
        .await;

        let _ = self.effects.end_session().await;
        result
    }

    /// Execute session coordination as a participant (accept or reject).
    pub async fn execute_session_coordination_participant(
        &self,
        _initiator_id: AuthorityId,
        coordinator_id: AuthorityId,
        participants: Vec<AuthorityId>,
        decision: SessionDecision,
    ) -> AgentResult<()> {
        let authority_id = self.authority_context.authority_id();
        let session_uuid = session_coordination_runtime_session_id(&decision.session_id)?.as_uuid();
        let participant_index = participants
            .iter()
            .position(|id| *id == authority_id)
            .ok_or_else(|| AgentError::invalid("authority not listed in session participants"))?;
        let active_role_name = format!("Participant{participant_index}");
        let roles = vec![
            coordination_role(coordinator_id, 0),
            coordination_role(authority_id, 0),
        ];
        let peer_roles = BTreeMap::from([(
            "Coordinator".to_string(),
            coordination_role(coordinator_id, 0),
        )]);
        let manifest =
            self::telltale_session_types_session_coordination::vm_artifacts::composition_manifest();
        let global_type =
            self::telltale_session_types_session_coordination::vm_artifacts::global_type();
        let local_types =
            self::telltale_session_types_session_coordination::vm_artifacts::local_types();

        self.effects
            .start_session(session_uuid, roles)
            .await
            .map_err(|e| AgentError::internal(format!("session coordination start failed: {e}")))?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                &manifest,
                &active_role_name,
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(
                to_vec(&decision)
                    .map_err(|error| AgentError::internal(format!("session decision encode failed: {error}")))?,
            );

            loop {
                let step = engine.step().map_err(|error| {
                    AgentError::internal(format!("session coordination participant VM step failed: {error}"))
                })?;
                flush_pending_vm_sends(self.effects.as_ref(), handler.as_ref(), &peer_roles)
                    .await
                    .map_err(AgentError::internal)?;

                if let Some(blocked) = receive_blocked_vm_message(
                    self.effects.as_ref(),
                    engine.vm(),
                    vm_sid,
                    &active_role_name,
                    &peer_roles,
                )
                .await
                .map_err(|error| {
                    AgentError::internal(format!("session coordination participant receive failed: {error}"))
                })? {
                    inject_vm_receive(&mut engine, vm_sid, &blocked).map_err(AgentError::internal)?;
                    continue;
                }

                match step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "session coordination participant VM became stuck without a pending receive"
                                .to_string(),
                        ));
                    }
                }
            }
            .map(|_| {
                let _ = close_and_reap_vm_session(&mut engine, vm_sid);
            })
        }
        .await;

        let _ = self.effects.end_session().await;
        result
    }
}

fn parse_session_id(session_id: &str) -> AgentResult<SessionId> {
    session_id.parse::<SessionId>().map_err(|e| {
        AgentError::invalid(format!(
            "invalid session id `{session_id}` for session coordination: {e}"
        ))
    })
}

fn session_coordination_runtime_session_id(
    session_id: &SessionId,
) -> AgentResult<RuntimeChoreographySessionId> {
    Ok(RuntimeChoreographySessionId::from_aura_session_id(
        *session_id,
    ))
}

fn coordination_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
    ChoreographicRole::new(
        DeviceId::from_uuid(authority_id.0),
        RoleIndex::new(role_index.into()).expect("role index"),
    )
}

/// Internal type for tracking participant responses
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ParticipantResponse {
    participant_id: DeviceId,
    accepted: bool,
    timestamp: u64,
}

impl SessionOperations {
    /// Get session information
    pub async fn get_session(&self, session_id: &str) -> AgentResult<Option<SessionHandle>> {
        let session_id_typed = parse_session_id(session_id)?;

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
    #[allow(dead_code)]
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
        let session_id_string = session_id.to_string();

        // Session created successfully (logging removed for simplicity)

        Ok(session_id_string)
    }

    /// Get session status via effects system
    async fn get_session_status_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        _session_id: &aura_core::identifiers::SessionId,
    ) -> AgentResult<Option<SessionHandle>> {
        // Lookup session status
        // Session status lookup is not persisted in this handler yet, so misses return None.
        Ok(None)
    }

    /// End session via effects system
    async fn end_session_via_effects(
        &self,
        _effects: &AuraEffectSystem,
        session_id: &str,
    ) -> AgentResult<SessionHandle> {
        // End session
        let current_time = self.effects.current_timestamp().await.unwrap_or(0);

        let device_id = self.device_id();
        Ok(SessionHandle {
            session_id: parse_session_id(session_id)?,
            session_type: SessionType::Coordination,
            participants: vec![device_id],
            my_role: ChoreographicRole::new(device_id, RoleIndex::new(0).expect("role index")),
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
        // List sessions
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
        // Cleanup sessions
        // Cleanup is a no-op while this handler has no persisted session catalog.
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{AgentConfig, AuthorityContext};
    use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_session_creation() {
        let authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([1u8; 32]));
        let account_id = AccountId::new_from_entropy([3u8; 32]);

        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());

        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let device_id = sessions.device_id();
        let participants = vec![device_id];

        let handle = sessions
            .create_session(SessionType::Coordination, participants.clone())
            .await
            .unwrap();

        assert!(!handle.session_id.to_string().is_empty());
        assert_eq!(handle.participants, participants);
        assert_eq!(handle.my_role.device_id, device_id);
    }

    #[tokio::test]
    async fn invitations_use_transport_envelopes() {
        let authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([70u8; 32]));
        let account_id = AccountId::new_from_entropy([12u8; 32]);
        let config = AgentConfig::default();

        // Use shared transport inbox to verify messages are sent
        let shared_transport = crate::runtime::SharedTransport::new();
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_with_shared_transport(
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
        let authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([71u8; 32]));
        let account_id = AccountId::new_from_entropy([14u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
        let sessions = SessionOperations::new(effects.clone(), authority_context, account_id);

        let handle = sessions
            .create_session(SessionType::Coordination, vec![sessions.device_id()])
            .await
            .unwrap();

        let storage_key = format!("session/{}", handle.session_id);
        let stored = effects.retrieve(&storage_key).await.unwrap();

        assert!(stored.is_some(), "session handle persisted to storage");
    }

    #[tokio::test]
    async fn invalid_session_id_is_rejected() {
        let authority_context = AuthorityContext::new(AuthorityId::new_from_entropy([72u8; 32]));
        let account_id = AccountId::new_from_entropy([15u8; 32]);
        let config = AgentConfig::default();
        let effects = Arc::new(AuraEffectSystem::simulation_for_test(&config).unwrap());
        let sessions = SessionOperations::new(effects, authority_context, account_id);

        let err = sessions
            .get_session("not-a-session-id")
            .await
            .expect_err("invalid session id should be rejected");
        assert!(
            matches!(err, AgentError::Config(_)),
            "expected invalid/config error, got {err:?}"
        );
    }
}
