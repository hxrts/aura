//! Canonical host-side ingress for owned Telltale protocol-machine sessions.

#![allow(clippy::result_large_err, clippy::incompatible_msrv)]

use std::collections::BTreeMap;
use std::sync::Arc;

use aura_mpst::{
    upstream::types::{GlobalType, LocalTypeR},
    CompositionManifest,
};
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole};
use thiserror::Error;
use uuid::Uuid;

use super::subsystems::choreography::{
    RuntimeChoreographySessionId, SessionOwnerCapability, SessionOwnerCapabilityScope,
    SessionOwnershipError,
};
use super::vm_host_bridge::{
    advance_host_bridged_vm_round, advance_host_bridged_vm_round_until_receive,
    close_and_reap_vm_session, handle_standard_vm_round, open_manifest_vm_session_admitted,
    AuraQueuedVmBridgeHandler, AuraVmBridgeRound, AuraVmHostWaitStatus, AuraVmRoundDisposition,
    BlockedVmReceive,
};
use super::{AuraChoreoEngine, AuraEffectSystem, AuraVmSchedulerSignals};
use super::{AuraLinkBoundary, RuntimeBoundaryError, RuntimeSessionEvent};
use aura_core::OwnershipCategory;
use telltale_machine::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionOwner {
    pub session_id: RuntimeChoreographySessionId,
    pub owner_label: String,
    pub capability: SessionOwnerCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStartFailureReason {
    AlreadyExists,
    TaskAlreadyBound,
    OwnerClaimRejected,
    VmSessionOpenFailed,
    Other,
}

/// Required close failures retain the concrete producing service error.
#[derive(Debug, Error)]
pub enum SessionCloseFailure {
    #[error("VM close failed: {0}")]
    Engine(#[source] super::choreo_engine::AuraChoreoEngineError),
    #[error("runtime choreography close failed: {0}")]
    Choreography(#[source] aura_protocol::effects::ChoreographyError),
}

/// A forced drop may fail in both close and exact owner retirement. Its standard
/// source is the first failure; any secondary owner failure remains typed here.
#[derive(Debug, Error)]
pub enum ForcedVmRetirementFailure {
    #[error("VM close failed: {source}; secondary owner failure: {ownership:?}")]
    Vm {
        #[source]
        source: super::choreo_engine::AuraChoreoEngineError,
        ownership: Option<SessionOwnershipError>,
    },
    #[error("owner retirement failed: {0}")]
    Ownership(#[source] SessionOwnershipError),
}
impl ForcedVmRetirementFailure {
    pub fn ownership_failure(&self) -> Option<&SessionOwnershipError> {
        match self {
            Self::Vm { ownership, .. } => ownership.as_ref(),
            Self::Ownership(error) => Some(error),
        }
    }
}

/// Keeps the original native error allocation after acknowledged resource
/// retirement. Ordinary close still reports the required validation failure.
#[derive(Debug, Clone)]
pub struct RetainedSessionCloseFailure {
    original: Arc<SessionIngressError>,
}
/// Sealed original resource acknowledgment with its failed close validation.
/// It carries the producer's claim witness, rather than using an error or a
/// missing binding as evidence that disposal completed.
pub(in crate::runtime) struct AcknowledgedVmResourceRetirement {
    _retirement: Arc<super::effects::SessionEndResourceRetirement>,
    original: Arc<SessionIngressError>,
}
impl AcknowledgedVmResourceRetirement {
    pub(in crate::runtime) fn native_failure(&self) -> Arc<SessionIngressError> {
        Arc::clone(&self.original)
    }
}
impl std::fmt::Display for RetainedSessionCloseFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.original.as_ref(), formatter)
    }
}
impl std::error::Error for RetainedSessionCloseFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.original.as_ref())
    }
}

#[derive(Debug, Error)]
pub enum SessionIngressError {
    #[error("{0}")]
    RetainedClose(#[source] RetainedSessionCloseFailure),
    #[error("runtime session {session_id} has no owner record")]
    MissingOwner {
        session_id: RuntimeChoreographySessionId,
    },
    #[error("runtime session {session_id} is not owned by expected owner {expected_owner}")]
    StaleOwner {
        session_id: RuntimeChoreographySessionId,
        expected_owner: String,
    },
    #[error("runtime session {session_id} rejected ingress for owner {owner_label}: {details}")]
    InvalidIngressRouting {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        details: RuntimeBoundaryError,
    },
    #[error(
        "failed to start owned runtime session {session_id} for {owner_label} ({reason:?}): {message}"
    )]
    SessionStart {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        reason: SessionStartFailureReason,
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
    #[error("failed to advance owned runtime session {session_id} for {owner_label}: {message}")]
    Round {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        message: String,
    },
    #[error("failed to bridge owned runtime session {session_id} for {owner_label}: {source}")]
    BridgeRound {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        #[source]
        source: super::vm_host_bridge::AuraVmBridgeRoundError,
    },
    #[error("failed to close owned runtime session {session_id} for {owner_label}: {source}")]
    SessionClose {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        #[source]
        source: SessionCloseFailure,
    },
    #[error("forced retirement of runtime session {session_id} failed: {source}")]
    ForcedRetirement {
        session_id: RuntimeChoreographySessionId,
        #[source]
        source: ForcedVmRetirementFailure,
    },
    #[error(
        "failed to transfer owned runtime session {session_id} from {from_owner_label} to {to_owner_label}: {message}"
    )]
    OwnerTransfer {
        session_id: RuntimeChoreographySessionId,
        from_owner_label: String,
        to_owner_label: String,
        message: String,
    },
}

impl SessionIngressError {
    fn error_kind(&self) -> &'static str {
        match self {
            Self::MissingOwner { .. } => "missing_owner",
            Self::StaleOwner { .. } => "stale_owner",
            Self::InvalidIngressRouting { .. } => "invalid_ingress_routing",
            Self::SessionStart { .. } => "session_start",
            Self::Round { .. } => "round",
            Self::BridgeRound { .. } => "bridge_round",
            Self::SessionClose { .. } | Self::RetainedClose(_) => "session_close",
            Self::ForcedRetirement { .. } => "forced_retirement",
            Self::OwnerTransfer { .. } => "owner_transfer",
        }
    }
}

fn classify_session_start_error(
    error: &aura_protocol::effects::ChoreographyError,
) -> SessionStartFailureReason {
    match error {
        aura_protocol::effects::ChoreographyError::SessionAlreadyExists { .. } => {
            SessionStartFailureReason::AlreadyExists
        }
        aura_protocol::effects::ChoreographyError::InternalError { message }
            if message.starts_with("task already bound to active choreography session") =>
        {
            SessionStartFailureReason::TaskAlreadyBound
        }
        _ => SessionStartFailureReason::Other,
    }
}

impl RuntimeSessionOwner {
    pub const OWNERSHIP_CATEGORY: OwnershipCategory = OwnershipCategory::MoveOwned;
}

impl From<SessionOwnershipError> for SessionIngressError {
    fn from(value: SessionOwnershipError) -> Self {
        match value {
            SessionOwnershipError::MissingOwner { session_id } => {
                SessionIngressError::MissingOwner { session_id }
            }
            SessionOwnershipError::OwnerMismatch {
                session_id,
                expected_owner,
            } => SessionIngressError::StaleOwner {
                session_id,
                expected_owner,
            },
            SessionOwnershipError::CapabilityMismatch {
                session_id,
                expected_owner,
                current_generation,
            } => SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: expected_owner,
                details: RuntimeBoundaryError::CapabilityRejected {
                    details: format!(
                        "session capability no longer valid; current generation is {current_generation}"
                    ),
                },
            },
            SessionOwnershipError::CapabilitySessionMismatch {
                session_id,
                expected_owner,
                capability_session_id,
            } => SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: expected_owner,
                details: RuntimeBoundaryError::CapabilityRejected {
                    details: format!(
                        "session capability was issued for {capability_session_id}, not {session_id}"
                    ),
                },
            },
            SessionOwnershipError::OwnerConflict {
                session_id,
                existing_owner,
                requested_owner,
            } => SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: requested_owner,
                details: RuntimeBoundaryError::CapabilityRejected {
                    details: format!("session already owned by {existing_owner}"),
                },
            },
        }
    }
}

pub struct OwnedVmSession {
    effects: Arc<AuraEffectSystem>,
    owner: RuntimeSessionOwner,
    routing_boundary: AuraLinkBoundary,
    engine: AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    handler: Arc<AuraQueuedVmBridgeHandler>,
    vm_session_id: SessionId,
    retirement_armed: bool,
    acknowledged_close_failure: Option<(
        Arc<super::effects::SessionEndResourceRetirement>,
        Arc<SessionIngressError>,
    )>,
}

/// Owns the claimed choreography capability until VM open transfers it to the
/// completed session. Cancellation at any later await retires it synchronously.
struct OpeningVmSessionOwner {
    effects: Arc<AuraEffectSystem>,
    owner: Option<RuntimeSessionOwner>,
}

impl Drop for OpeningVmSessionOwner {
    fn drop(&mut self) {
        let Some(owner) = self.owner.take() else {
            return;
        };
        if let Err(source) = self
            .effects
            .retire_unfinished_vm_open_owner(&owner.capability)
        {
            let source = SessionIngressError::ForcedRetirement {
                session_id: owner.session_id,
                source: ForcedVmRetirementFailure::Ownership(source),
            };
            let retained = crate::task_registry::retain_current_task_cleanup_failure(
                aura_core::AuraError::Internal {
                    message: source.to_string(),
                    source: Some(Arc::new(source)),
                },
            );
            if !retained {
                tracing::error!(session_id=%owner.session_id,
                    "unfinished VM open retirement failure has no registered task health owner");
            }
        }
    }
}

impl Drop for OwnedVmSession {
    fn drop(&mut self) {
        if !self.retirement_armed {
            return;
        }
        self.retirement_armed = false;
        // The backend's delivery lease preserves unknown-delivery custody;
        // retirement never retries or acknowledges an in-flight frame.
        let vm_close = close_and_reap_vm_session(&mut self.engine, self.vm_session_id).err();
        let ownership = self
            .effects
            .retire_dropped_vm_owner(&self.owner.capability)
            .err();
        let failure = match (vm_close, ownership) {
            (Some(source), ownership) => Some(ForcedVmRetirementFailure::Vm { source, ownership }),
            (None, Some(source)) => Some(ForcedVmRetirementFailure::Ownership(source)),
            (None, None) => None,
        };
        if let Some(source) = failure {
            let source = SessionIngressError::ForcedRetirement {
                session_id: self.owner.session_id,
                source,
            };
            let retained = crate::task_registry::retain_current_task_cleanup_failure(
                aura_core::AuraError::Internal {
                    message: source.to_string(),
                    source: Some(Arc::new(source)),
                },
            );
            if !retained {
                tracing::error!(session_id=%self.owner.session_id,
                    "VM retirement failure has no registered task health owner");
            }
        }
    }
}

#[derive(Debug)]
struct OwnedVmSessionOwnerTransfer {
    next_owner: RuntimeSessionOwner,
    next_boundary: AuraLinkBoundary,
}

fn log_session_owner_assigned(
    owner: &RuntimeSessionOwner,
    protocol_id: Option<&str>,
    context: &'static str,
) {
    tracing::debug!(
        event = RuntimeSessionEvent::OwnerAssigned.as_event_name(),
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
        capability_generation = owner.capability.generation(),
        protocol_id,
        context,
        "Assigned runtime session owner"
    );
}

fn log_session_owner_rejected(
    session_id: RuntimeChoreographySessionId,
    owner_label: &str,
    protocol_id: Option<&str>,
    reason: &'static str,
    error: &str,
) {
    tracing::warn!(
        event = RuntimeSessionEvent::OwnerRejected.as_event_name(),
        session_id = %session_id,
        owner_label,
        protocol_id,
        reason,
        error,
        "Rejected runtime session owner"
    );
}

fn log_session_owner_transferred(
    previous_owner: &RuntimeSessionOwner,
    next_owner: &RuntimeSessionOwner,
    protocol_id: Option<&str>,
    context: &'static str,
) {
    tracing::info!(
        event = RuntimeSessionEvent::OwnerTransferred.as_event_name(),
        session_id = %previous_owner.session_id,
        from_owner_label = %previous_owner.owner_label,
        from_generation = previous_owner.capability.generation(),
        to_owner_label = %next_owner.owner_label,
        to_generation = next_owner.capability.generation(),
        protocol_id,
        context,
        "Transferred runtime session owner"
    );
}

fn log_session_owner_transfer_rejected(
    previous_owner: &RuntimeSessionOwner,
    next_owner_label: &str,
    protocol_id: Option<&str>,
    error: &SessionIngressError,
    context: &'static str,
) {
    tracing::warn!(
        event = RuntimeSessionEvent::OwnerTransferRejected.as_event_name(),
        session_id = %previous_owner.session_id,
        from_owner_label = %previous_owner.owner_label,
        from_generation = previous_owner.capability.generation(),
        to_owner_label = next_owner_label,
        protocol_id,
        context,
        error_kind = error.error_kind(),
        error = %error,
        "Rejected runtime session owner transfer"
    );
}

fn log_session_ingress_received(
    owner: &RuntimeSessionOwner,
    ingress_kind: &'static str,
    active_role: Option<&str>,
    from_role: Option<&str>,
    to_role: Option<&str>,
    payload_bytes: usize,
) {
    tracing::debug!(
        event = RuntimeSessionEvent::IngressReceived.as_event_name(),
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
        capability_generation = owner.capability.generation(),
        ingress_kind,
        active_role,
        from_role,
        to_role,
        payload_bytes,
        "Accepted owned session ingress"
    );
}

fn log_session_ingress_dropped(
    owner: &RuntimeSessionOwner,
    ingress_kind: &'static str,
    error: &SessionIngressError,
    active_role: Option<&str>,
) {
    tracing::warn!(
        event = RuntimeSessionEvent::IngressDropped.as_event_name(),
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
        capability_generation = owner.capability.generation(),
        ingress_kind,
        active_role,
        error_kind = error.error_kind(),
        error = %error,
        "Dropped owned session ingress"
    );
}

impl OwnedVmSession {
    fn prepare_owner_transfer(
        &self,
        next_owner_label: impl Into<String>,
        next_boundary: AuraLinkBoundary,
    ) -> Result<OwnedVmSessionOwnerTransfer, SessionIngressError> {
        let next_scope = next_boundary.capability_scope.clone();
        let next_owner = self.effects.transfer_owned_choreography_session_owner(
            self.owner.clone(),
            next_owner_label,
            next_scope,
        )?;
        Ok(OwnedVmSessionOwnerTransfer {
            next_owner,
            next_boundary,
        })
    }

    pub fn owner(&self) -> &RuntimeSessionOwner {
        &self.owner
    }

    pub fn routing_boundary(&self) -> &AuraLinkBoundary {
        &self.routing_boundary
    }

    pub fn queue_send_bytes(&self, payload: Vec<u8>) {
        self.handler.push_send_bytes(payload);
    }

    pub fn queue_choice_label(&self, label: impl Into<String>) {
        self.handler.push_choice_label(label.into());
    }

    pub fn engine_mut(&mut self) -> &mut AuraChoreoEngine<AuraQueuedVmBridgeHandler> {
        &mut self.engine
    }

    pub fn vm_session_id(&self) -> SessionId {
        self.vm_session_id
    }

    pub async fn advance_round(
        &mut self,
        active_role: &str,
        peer_roles: &BTreeMap<String, ChoreographicRole>,
    ) -> Result<AuraVmBridgeRound, SessionIngressError> {
        log_session_ingress_received(
            &self.owner,
            "advance_round",
            Some(active_role),
            None,
            None,
            0,
        );
        if let Err(error) = self
            .effects
            .assert_owned_choreography_boundary(&self.owner, &self.routing_boundary)
        {
            log_session_ingress_dropped(&self.owner, "advance_round", &error, Some(active_role));
            return Err(error);
        }
        let result = advance_host_bridged_vm_round(
            self.effects.as_ref(),
            &mut self.engine,
            self.handler.as_ref(),
            self.vm_session_id,
            active_role,
            peer_roles,
        )
        .await
        .map_err(|source| SessionIngressError::BridgeRound {
            session_id: self.owner.session_id,
            owner_label: self.owner.owner_label.clone(),
            source,
        });
        if let Err(error) = &result {
            log_session_ingress_dropped(&self.owner, "advance_round", error, Some(active_role));
        }
        result
    }

    pub async fn advance_round_until_receive<F>(
        &mut self,
        active_role: &str,
        peer_roles: &BTreeMap<String, ChoreographicRole>,
        stop_on_receive_error: F,
    ) -> Result<AuraVmBridgeRound, SessionIngressError>
    where
        F: Fn(&aura_protocol::effects::ChoreographyError) -> bool,
    {
        log_session_ingress_received(
            &self.owner,
            "advance_round_until_receive",
            Some(active_role),
            None,
            None,
            0,
        );
        if let Err(error) = self
            .effects
            .assert_owned_choreography_boundary(&self.owner, &self.routing_boundary)
        {
            log_session_ingress_dropped(
                &self.owner,
                "advance_round_until_receive",
                &error,
                Some(active_role),
            );
            return Err(error);
        }
        let result = advance_host_bridged_vm_round_until_receive(
            self.effects.as_ref(),
            &mut self.engine,
            self.handler.as_ref(),
            self.vm_session_id,
            active_role,
            peer_roles,
            stop_on_receive_error,
        )
        .await
        .map_err(|source| SessionIngressError::BridgeRound {
            session_id: self.owner.session_id,
            owner_label: self.owner.owner_label.clone(),
            source,
        });
        if let Err(error) = &result {
            log_session_ingress_dropped(
                &self.owner,
                "advance_round_until_receive",
                error,
                Some(active_role),
            );
        }
        result
    }

    pub fn inject_blocked_receive(
        &mut self,
        receive: BlockedVmReceive,
    ) -> Result<(), SessionIngressError> {
        log_session_ingress_received(
            &self.owner,
            "blocked_receive",
            Some(receive.to_role.as_str()),
            Some(receive.from_role.as_str()),
            Some(receive.to_role.as_str()),
            receive.payload.len(),
        );
        if let Err(error) = self
            .effects
            .assert_owned_choreography_boundary(&self.owner, &self.routing_boundary)
        {
            log_session_ingress_dropped(&self.owner, "blocked_receive", &error, None);
            return Err(error);
        }
        let result =
            super::vm_host_bridge::inject_vm_receive(&mut self.engine, self.vm_session_id, receive)
                .map_err(|message| SessionIngressError::Round {
                    session_id: self.owner.session_id,
                    owner_label: self.owner.owner_label.clone(),
                    message,
                });
        if let Err(error) = &result {
            log_session_ingress_dropped(&self.owner, "blocked_receive", error, None);
        }
        result
    }

    pub async fn close(mut self) -> Result<(), SessionIngressError> {
        self.close_retained().await
    }

    /// Close through the actual retained owner. An interrupted close leaves the
    /// same session capability available to its required task custodian.
    pub(crate) async fn close_retained(&mut self) -> Result<(), SessionIngressError> {
        if let Some((_, original)) = &self.acknowledged_close_failure {
            return Err(SessionIngressError::RetainedClose(
                RetainedSessionCloseFailure {
                    original: Arc::clone(original),
                },
            ));
        }
        self.effects
            .assert_owned_choreography_session(&self.owner)?;
        // Scheduler activity is observational. Both backends acknowledge
        // targeted disposal, including an idempotent summary after prior reap;
        // only that actual producer can settle the VM resource before the
        // physical runtime binding and fragments are released.
        close_and_reap_vm_session(&mut self.engine, self.vm_session_id).map_err(|source| {
            SessionIngressError::SessionClose {
                session_id: self.owner.session_id,
                owner_label: self.owner.owner_label.clone(),
                source: SessionCloseFailure::Engine(source),
            }
        })?;
        let result = self
            .effects
            .end_owned_choreography_session(&self.owner)
            .await;
        match result {
            Ok(()) => {
                self.retirement_armed = false;
                Ok(())
            }
            Err(error) => {
                let retirement = match &error {
                    SessionIngressError::SessionClose {
                        source:
                            SessionCloseFailure::Choreography(
                                aura_protocol::effects::ChoreographyError::RequiredTime {
                                    source,
                                    ..
                                },
                            ),
                        ..
                    } => source
                        .downcast_ref::<super::effects::SessionEndTimeFailure>()
                        .and_then(|failure| failure.original_retirement(&self.owner)),
                    _ => None,
                };
                if let Some(retirement) = retirement {
                    let original = Arc::new(error);
                    self.acknowledged_close_failure = Some((retirement, Arc::clone(&original)));
                    self.retirement_armed = false;
                    return Err(SessionIngressError::RetainedClose(
                        RetainedSessionCloseFailure { original },
                    ));
                }
                Err(error)
            }
        }
    }

    /// Read-only resource evidence, separate from ordinary close validation.
    /// Missing binding or fragments cannot construct this acknowledgment.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OwnedVmSession", receiver_type = OwnedVmSession, family = "runtime_helper")]
    pub(in crate::runtime) fn acknowledged_close_resource_failure(
        &self,
    ) -> Option<AcknowledgedVmResourceRetirement> {
        self.acknowledged_close_failure
            .as_ref()
            .filter(|(retirement, _)| retirement.matches_owner(&self.owner))
            .map(|(retirement, original)| AcknowledgedVmResourceRetirement {
                _retirement: Arc::clone(retirement),
                original: Arc::clone(original),
            })
    }

    pub fn transfer_owner_in_place(
        &mut self,
        next_owner_label: impl Into<String>,
        next_boundary: AuraLinkBoundary,
    ) -> Result<(), SessionIngressError> {
        let transfer = self.prepare_owner_transfer(next_owner_label, next_boundary)?;
        self.routing_boundary = transfer.next_boundary;
        let _previous = std::mem::replace(&mut self.owner, transfer.next_owner);
        Ok(())
    }

    pub fn transfer_owner(
        mut self,
        next_owner_label: impl Into<String>,
        next_scope: SessionOwnerCapabilityScope,
    ) -> Result<Self, SessionIngressError> {
        self.transfer_owner_in_place(next_owner_label, AuraLinkBoundary::for_scope(next_scope))?;
        Ok(self)
    }
}

#[track_caller]
pub fn caller_session_owner_label() -> String {
    let caller = std::panic::Location::caller();
    format!("{}:{}:{}", caller.file(), caller.line(), caller.column())
}

pub async fn open_owned_manifest_vm_session_admitted(
    effects: Arc<AuraEffectSystem>,
    session_uuid: Uuid,
    roles: Vec<ChoreographicRole>,
    manifest: &CompositionManifest,
    active_role: &str,
    global_type: &GlobalType,
    local_types: &BTreeMap<String, LocalTypeR>,
    scheduler_signals: AuraVmSchedulerSignals,
) -> Result<OwnedVmSession, SessionIngressError> {
    let owner_label = caller_session_owner_label();
    let owner = effects
        .start_owned_choreography_session(owner_label, session_uuid, roles)
        .await?;
    let mut opening = OpeningVmSessionOwner {
        effects: effects.clone(),
        owner: Some(owner),
    };
    let owner = opening.owner.as_ref().expect("opening owner retained");
    effects
        .set_current_runtime_choreography_protocol_id(manifest.protocol_id.clone())
        .map_err(|message| SessionIngressError::SessionStart {
            session_id: owner.session_id,
            owner_label: owner.owner_label.clone(),
            reason: SessionStartFailureReason::VmSessionOpenFailed,
            message,
            source: None,
        })?;
    let routing_boundary = AuraLinkBoundary::for_manifest(manifest);

    match open_manifest_vm_session_admitted(
        effects.as_ref(),
        manifest,
        active_role,
        global_type,
        local_types,
        scheduler_signals,
    )
    .await
    {
        Ok((engine, handler, vm_session_id)) => Ok(OwnedVmSession {
            effects,
            owner: opening.owner.take().expect("opening owner transfers once"),
            routing_boundary,
            engine,
            handler,
            vm_session_id,
            retirement_armed: true,
            acknowledged_close_failure: None,
        }),
        Err(error) => {
            log_session_owner_rejected(
                owner.session_id,
                &owner.owner_label,
                Some(manifest.protocol_id.as_str()),
                "vm_session_open_failed",
                &error.to_string(),
            );
            let message = error.to_string();
            // Retirement remains synchronous and preserves the admitted packet.
            // The guard retains any concrete cleanup failure through task health.
            Err(SessionIngressError::SessionStart {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                reason: SessionStartFailureReason::VmSessionOpenFailed,
                message,
                source: Some(Box::new(error)),
            })
        }
    }
}

pub fn handle_owned_vm_round(
    session: &mut OwnedVmSession,
    round: AuraVmBridgeRound,
    context_label: &str,
) -> Result<AuraVmRoundDisposition, SessionIngressError> {
    if let Some(blocked) = round.blocked_receive {
        session.inject_blocked_receive(blocked)?;
        return Ok(AuraVmRoundDisposition::Continue);
    }

    match round.host_wait_status {
        AuraVmHostWaitStatus::Idle | AuraVmHostWaitStatus::Delivered => {}
        AuraVmHostWaitStatus::TimedOut => {
            return Err(SessionIngressError::Round {
                session_id: session.owner.session_id,
                owner_label: session.owner.owner_label.clone(),
                message: format!("{context_label} timed out while waiting for receive"),
            });
        }
        AuraVmHostWaitStatus::Cancelled => {
            return Err(SessionIngressError::Round {
                session_id: session.owner.session_id,
                owner_label: session.owner.owner_label.clone(),
                message: format!("{context_label} cancelled while waiting for receive"),
            });
        }
        AuraVmHostWaitStatus::Deferred => {}
    }

    let vm_session_id = session.vm_session_id();
    handle_standard_vm_round(
        session.engine_mut(),
        vm_session_id,
        AuraVmBridgeRound {
            step: round.step,
            blocked_receive: None,
            host_wait_status: round.host_wait_status,
        },
        context_label,
    )
    .map_err(|message| SessionIngressError::Round {
        session_id: session.owner.session_id,
        owner_label: session.owner.owner_label.clone(),
        message,
    })
}

impl AuraEffectSystem {
    fn assert_runtime_choreography_session_binding(
        &self,
        session_id: RuntimeChoreographySessionId,
        owner_label: &str,
    ) -> Result<(), SessionIngressError> {
        let current_session_id =
            self.current_runtime_choreography_session_id()
                .ok_or_else(|| SessionIngressError::InvalidIngressRouting {
                    session_id,
                    owner_label: owner_label.to_string(),
                    details: RuntimeBoundaryError::MissingTaskBinding,
                })?;

        if current_session_id != session_id {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: owner_label.to_string(),
                details: RuntimeBoundaryError::SessionBindingMismatch {
                    expected_session_id: session_id,
                    bound_session_id: current_session_id,
                },
            });
        }

        Ok(())
    }

    pub async fn start_owned_choreography_session(
        &self,
        owner_label: impl Into<String>,
        session_uuid: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<RuntimeSessionOwner, SessionIngressError> {
        let owner_label = owner_label.into();
        ChoreographicEffects::start_session(self, session_uuid, roles)
            .await
            .map_err(|error| SessionIngressError::SessionStart {
                session_id: RuntimeChoreographySessionId::from_uuid(session_uuid),
                owner_label: owner_label.clone(),
                reason: classify_session_start_error(&error),
                message: error.to_string(),
                source: Some(Box::new(error)),
            })
            .inspect_err(|error| {
                log_session_owner_rejected(
                    RuntimeChoreographySessionId::from_uuid(session_uuid),
                    &owner_label,
                    None,
                    "runtime_session_start_failed",
                    &error.to_string(),
                );
            })?;

        let session_id = RuntimeChoreographySessionId::from_uuid(session_uuid);
        let capability =
            match self.claim_runtime_choreography_session_owner(session_id, owner_label.clone()) {
                Ok(capability) => capability,
                Err(error) => {
                    let _ = ChoreographicEffects::end_session(self).await;
                    log_session_owner_rejected(
                        session_id,
                        &owner_label,
                        None,
                        "owner_claim_rejected",
                        &error,
                    );
                    return Err(SessionIngressError::SessionStart {
                        session_id,
                        owner_label,
                        reason: SessionStartFailureReason::OwnerClaimRejected,
                        message: error,
                        source: None,
                    });
                }
            };

        let owner = RuntimeSessionOwner {
            session_id,
            owner_label,
            capability,
        };
        log_session_owner_assigned(&owner, None, "start_owned_choreography_session");
        Ok(owner)
    }

    pub fn assert_owned_choreography_session(
        &self,
        owner: &RuntimeSessionOwner,
    ) -> Result<(), SessionIngressError> {
        self.assert_owned_choreography_boundary(
            owner,
            &AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Session),
        )?;

        if !owner.capability.allows_full_session() {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: RuntimeBoundaryError::FullSessionCapabilityRequired,
            });
        }

        Ok(())
    }

    pub fn assert_owned_choreography_boundary(
        &self,
        owner: &RuntimeSessionOwner,
        boundary: &AuraLinkBoundary,
    ) -> Result<(), SessionIngressError> {
        self.assert_runtime_choreography_session_binding(owner.session_id, &owner.owner_label)?;
        self.ensure_runtime_choreography_session_owner_capability(
            owner.session_id,
            &owner.capability,
        )
        .map_err(|error| SessionIngressError::InvalidIngressRouting {
            session_id: owner.session_id,
            owner_label: owner.owner_label.clone(),
            details: RuntimeBoundaryError::CapabilityRejected { details: error },
        })?;

        if !boundary.is_allowed_by(owner.capability.scope()) {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: RuntimeBoundaryError::BoundaryScopeRejected {
                    boundary: boundary.clone(),
                    capability_scope: owner.capability.scope().clone(),
                },
            });
        }

        Ok(())
    }

    pub fn transfer_owned_choreography_session_owner(
        &self,
        owner: RuntimeSessionOwner,
        next_owner_label: impl Into<String>,
        next_scope: SessionOwnerCapabilityScope,
    ) -> Result<RuntimeSessionOwner, SessionIngressError> {
        self.assert_runtime_choreography_session_binding(owner.session_id, &owner.owner_label)?;
        self.ensure_runtime_choreography_session_owner_capability(
            owner.session_id,
            &owner.capability,
        )
        .map_err(|error| SessionIngressError::InvalidIngressRouting {
            session_id: owner.session_id,
            owner_label: owner.owner_label.clone(),
            details: RuntimeBoundaryError::CapabilityRejected { details: error },
        })?;

        let next_owner_label = next_owner_label.into();
        let next_capability = self
            .transfer_runtime_choreography_session_owner(
                owner.session_id,
                &owner.capability,
                next_owner_label.clone(),
                next_scope,
            )
            .map_err(|message| SessionIngressError::OwnerTransfer {
                session_id: owner.session_id,
                from_owner_label: owner.owner_label.clone(),
                to_owner_label: next_owner_label.clone(),
                message,
            });

        match next_capability {
            Ok(next_capability) => {
                let next_owner = RuntimeSessionOwner {
                    session_id: owner.session_id,
                    owner_label: next_owner_label,
                    capability: next_capability,
                };
                log_session_owner_transferred(
                    &owner,
                    &next_owner,
                    None,
                    "transfer_owned_choreography_session_owner",
                );
                Ok(next_owner)
            }
            Err(error) => {
                log_session_owner_transfer_rejected(
                    &owner,
                    &next_owner_label,
                    None,
                    &error,
                    "transfer_owned_choreography_session_owner",
                );
                Err(error)
            }
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn original_session_end_gate_for_test(
        &self,
        owner: &RuntimeSessionOwner,
    ) -> Option<futures::future::BoxFuture<'_, ()>> {
        let gate = {
            let mut gate = self.original_session_end_gate.lock();
            if gate
                .as_ref()
                .is_some_and(|(session, _, _)| *session == owner.session_id.as_uuid())
            {
                gate.take()
            } else {
                None
            }
        };
        gate.map(|(_, entered, release)| {
            let owner = owner.clone();
            let original: futures::future::BoxFuture<'_, ()> = Box::pin(async move {
                self.assert_owned_choreography_session(&owner).expect(
                    "actual close task retains its original session binding before acknowledgment",
                );
                entered
                    .send(owner)
                    .unwrap_or_else(|_| panic!("original session-end observer remains held"));
                if let Ok(clock) = release.await {
                    let selected: Arc<dyn aura_core::effects::PhysicalTimeEffects> = clock.clone();
                    assert!(
                        self.time_effects().selected_provider_is(&selected),
                        "session-end injection retains the actual selected physical provider"
                    );
                    clock
                        .fail_next_current_task_observation(
                            aura_core::effects::TimeError::OperationFailed {
                                reason: "original issuer signed-notice close physical read".into(),
                            },
                        )
                        .await
                        .expect("same original task arms its actual close read");
                }
            });
            original
        })
    }

    pub async fn end_owned_choreography_session(
        &self,
        owner: &RuntimeSessionOwner,
    ) -> Result<(), SessionIngressError> {
        self.assert_owned_choreography_session(owner)?;
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if let Some(original) = self.original_session_end_gate_for_test(owner) {
            original.await;
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        {
            let mut fault = self.original_session_end_fault.lock();
            if fault.as_ref() == Some(&owner.session_id) {
                fault.take();
                return Err(SessionIngressError::SessionClose {
                    session_id: owner.session_id,
                    owner_label: owner.owner_label.clone(),
                    source: SessionCloseFailure::Choreography(
                        aura_protocol::effects::ChoreographyError::InternalError {
                            message: "selected original session-end fault before binding mutation"
                                .into(),
                        },
                    ),
                });
            }
        }
        ChoreographicEffects::end_session(self)
            .await
            .map_err(|error| SessionIngressError::SessionClose {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                source: SessionCloseFailure::Choreography(error),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::AuthorityId;
    use aura_mpst::{CompositionLinkSpec, CompositionManifest};
    use aura_protocol::effects::{ChoreographicRole, RoleIndex};
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use aura_core::effects::transport::TransportEnvelope;

    #[tokio::test]
    async fn owned_start_retains_selected_physical_read_fault_source() {
        use std::error::Error;

        let authority = test_authority(0xe3);
        let selected = aura_testkit::time::ManualPhysicalClock::new(5_000);
        selected
            .fail_next_observation(aura_core::effects::TimeError::OperationFailed {
                reason: "selected owned start read fault".into(),
            })
            .await;
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("effect system")
                .with_physical_time_provider(Arc::new(selected));
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role"),
        )];
        let error = effects
            .start_owned_choreography_session("selected-start", Uuid::from_bytes([0xe4; 16]), roles)
            .await
            .expect_err("actual selected clock fault must refuse owner claim");
        assert!(matches!(
            &error,
            SessionIngressError::SessionStart {
                reason: SessionStartFailureReason::Other,
                ..
            }
        ));
        let mut cursor: &(dyn Error + 'static) = &error;
        let mut choreography = false;
        let mut physical = false;
        loop {
            choreography |= cursor.is::<aura_protocol::effects::ChoreographyError>();
            physical |= matches!(
                cursor.downcast_ref::<aura_core::effects::TimeError>(),
                Some(aura_core::effects::TimeError::OperationFailed { reason })
                    if reason == "selected owned start read fault"
            );
            let Some(source) = cursor.source() else { break };
            cursor = source;
        }
        assert!(
            choreography && physical,
            "both native sources remain downcastable"
        );
        assert!(effects.vm_fragment_snapshot().is_empty());
    }

    #[tokio::test]
    async fn owned_vm_open_refusal_retains_native_source_and_releases_owner() {
        use std::error::Error;

        let authority = test_authority(0xe5);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("effect system"),
        );
        let session_uuid = Uuid::from_bytes([0xe6; 16]);
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role"),
        )];
        let manifest = linked_manifest("aura.test.vm-refusal", "vm-refusal-fragment");
        let error = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_uuid,
            roles.clone(),
            &manifest,
            "Role",
            &GlobalType::End,
            &BTreeMap::new(),
            AuraVmSchedulerSignals::default(),
        )
        .await
        .err()
        .expect("missing active-role local type must refuse VM open");
        assert!(matches!(
            &error,
            SessionIngressError::SessionStart {
                reason: SessionStartFailureReason::VmSessionOpenFailed,
                ..
            }
        ));
        let mut cursor: &(dyn Error + 'static) = &error;
        let mut native = false;
        while let Some(source) = cursor.source() {
            if let Some(refusal) =
                source.downcast_ref::<crate::runtime::vm_host_bridge::AuraVmSessionOpenError>()
            {
                native = true;
                assert!(
                    matches!(
                        refusal,
                        crate::runtime::vm_host_bridge::AuraVmSessionOpenError::RoleScopedImage { .. }
                    ),
                    "actual VM refusal: {refusal:?}"
                );
            }
            cursor = source;
        }
        assert!(
            native,
            "VM refusal keeps its native source rather than display text"
        );
        assert!(effects.vm_fragment_snapshot().is_empty());
        let replacement = effects
            .start_owned_choreography_session("replacement", session_uuid, roles)
            .await
            .expect("refused VM open releases exact runtime owner");
        effects
            .end_owned_choreography_session(&replacement)
            .await
            .expect("replacement closes");
    }

    #[tokio::test]
    async fn cancelled_after_owner_claim_retires_fragments_and_preserves_original_inbox() {
        let authority = test_authority(0xd5);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("effect system"),
        );
        let session_uuid = Uuid::from_bytes([0xd6; 16]);
        let (claimed_tx, claimed_rx) = tokio::sync::oneshot::channel();
        let (_release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
        crate::runtime::vm_host_bridge::opening_vm_test_holds()
            .lock()
            .expect("opening test holds lock")
            .insert(
                RuntimeChoreographySessionId::from_uuid(session_uuid),
                (claimed_tx, release_rx),
            );
        let context = aura_core::ContextId::new_from_entropy([0xd7; 32]);
        effects.queue_runtime_envelope(TransportEnvelope {
            destination: authority,
            source: test_authority(0xd8),
            context,
            payload: vec![0x51],
            metadata: std::collections::HashMap::from([
                (
                    "content-type".to_string(),
                    "application/aura-choreography".to_string(),
                ),
                ("session-id".to_string(), session_uuid.to_string()),
            ]),
            receipt: None,
        });
        let task_effects = effects.clone();
        let mut task = tokio::spawn(async move {
            let roles = vec![ChoreographicRole::for_authority(
                authority,
                RoleIndex::new(0).expect("role"),
            )];
            let manifest = linked_manifest("aura.test.opening", "opening-fragment");
            let intake = task_effects
                .take_unclaimed_choreography_envelope_retained(|envelope| {
                    envelope.payload == [0x51]
                })
                .expect("original session packet is retained during VM open");
            let opened = open_owned_manifest_vm_session_admitted(
                task_effects.clone(),
                session_uuid,
                roles,
                &manifest,
                "Role",
                &GlobalType::End,
                &BTreeMap::from([("Role".to_string(), LocalTypeR::End)]),
                AuraVmSchedulerSignals::default(),
            )
            .await;
            drop(intake);
            opened
        });
        tokio::select! {
            claimed = claimed_rx => {
                claimed.expect("real VM open claimed owner before cancellation");
            }
            _ = &mut task => {
                crate::runtime::vm_host_bridge::opening_vm_test_holds()
                    .lock()
                    .expect("opening test holds lock")
                    .remove(&RuntimeChoreographySessionId::from_uuid(session_uuid));
                panic!("real VM open completed before the after-fragment-claim hold");
            }
        }
        assert_eq!(effects.vm_fragment_snapshot().len(), 1);
        task.abort();
        let cancelled = match task.await {
            Ok(_) => panic!("VM open unexpectedly completed after abort"),
            Err(error) => error,
        };
        assert!(cancelled.is_cancelled());
        let queued = effects.peek_queued_choreography_envelopes();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].payload, vec![0x51]);
        assert!(effects.vm_fragment_snapshot().is_empty());
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role"),
        )];
        let next = effects
            .start_owned_choreography_session("replacement-owner", session_uuid, roles)
            .await
            .expect("cancelled opening owner releases session admission");
        effects
            .end_owned_choreography_session(&next)
            .await
            .expect("replacement owner closes");
    }

    fn test_authority(byte: u8) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([byte; 16]))
    }

    fn linked_manifest(protocol_id: &str, bundle_id: &str) -> CompositionManifest {
        CompositionManifest {
            protocol_name: protocol_id.to_string(),
            protocol_namespace: None,
            protocol_qualified_name: protocol_id.to_string(),
            protocol_id: protocol_id.to_string(),
            role_names: vec!["Role".to_string()],
            required_capabilities: Vec::new(),
            theorem_packs: Vec::new(),
            required_theorem_packs: Vec::new(),
            required_theorem_pack_capabilities: Vec::new(),
            guard_capabilities: Vec::new(),
            determinism_policy_ref: None,
            delegation_constraints: Vec::new(),
            link_specs: vec![CompositionLinkSpec {
                role: "Role".to_string(),
                bundle_id: bundle_id.to_string(),
                imports: Vec::new(),
                exports: Vec::new(),
            }],
        }
    }

    #[tokio::test]
    async fn transferring_runtime_session_owner_invalidates_stale_capability() {
        let authority = test_authority(0x11);
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system");
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_uuid = Uuid::from_bytes([0x22; 16]);
        let original = effects
            .start_owned_choreography_session("owner-a", session_uuid, roles)
            .await
            .expect("start owned session");
        let stale = original.clone();

        let transferred = effects
            .transfer_owned_choreography_session_owner(
                original,
                "owner-b",
                SessionOwnerCapabilityScope::Session,
            )
            .expect("transfer owner");

        assert_eq!(transferred.owner_label, "owner-b");
        assert!(
            effects
                .assert_owned_choreography_session(&transferred)
                .is_ok(),
            "new owner must be accepted"
        );
        assert!(
            matches!(
                effects.assert_owned_choreography_session(&stale),
                Err(SessionIngressError::StaleOwner { .. })
                    | Err(SessionIngressError::InvalidIngressRouting { .. })
            ),
            "stale owner handle must be rejected after transfer"
        );

        effects
            .end_owned_choreography_session(&transferred)
            .await
            .expect("close transferred session");
    }

    #[tokio::test]
    async fn transferring_runtime_session_owner_moves_fragment_ownership_together() {
        let authority = test_authority(0x33);
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system");
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_uuid = Uuid::from_bytes([0x44; 16]);
        let original = effects
            .start_owned_choreography_session("owner-a", session_uuid, roles)
            .await
            .expect("start owned session");
        let manifest = linked_manifest("aura.test.protocol", "bundle-a");
        effects
            .claim_vm_fragments_for_manifest("owner-a", &manifest)
            .expect("claim fragment ownership");

        let transferred = effects
            .transfer_owned_choreography_session_owner(
                original,
                "owner-b",
                SessionOwnerCapabilityScope::Session,
            )
            .expect("transfer owner");

        let snapshot = effects.vm_fragment_snapshot();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].1.owner_label, "owner-b");
        assert!(matches!(
            snapshot[0].1.bundle_id.as_deref(),
            Some("bundle-a")
        ));

        effects
            .end_owned_choreography_session(&transferred)
            .await
            .expect("close transferred session");
    }

    #[tokio::test]
    async fn fragment_scoped_owner_accepts_matching_boundary_and_rejects_wrong_boundary() {
        let authority = test_authority(0x55);
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system");
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_uuid = Uuid::from_bytes([0x66; 16]);
        let original = effects
            .start_owned_choreography_session("owner-a", session_uuid, roles)
            .await
            .expect("start owned session");

        let transferred = effects
            .transfer_owned_choreography_session_owner(
                original,
                "owner-b",
                SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                    "bundle:bundle-a".to_string()
                ])),
            )
            .expect("transfer owner");

        let allowed_boundary =
            AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                "bundle:bundle-a".to_string(),
            ])));
        effects
            .assert_owned_choreography_boundary(&transferred, &allowed_boundary)
            .expect("matching boundary should be accepted");

        let rejected_boundary =
            AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                "bundle:bundle-b".to_string(),
            ])));
        let rejected = effects
            .assert_owned_choreography_boundary(&transferred, &rejected_boundary)
            .expect_err("wrong boundary should be rejected");
        assert!(matches!(
            rejected,
            SessionIngressError::InvalidIngressRouting { .. }
        ));
    }

    #[tokio::test]
    async fn attenuating_owner_capability_rejects_stale_generation_even_for_same_owner_label() {
        let authority = test_authority(0x77);
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system");
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_uuid = Uuid::from_bytes([0x88; 16]);
        let original = effects
            .start_owned_choreography_session("owner-a", session_uuid, roles)
            .await
            .expect("start owned session");
        let stale_full_capability = original.clone();

        let attenuated = effects
            .transfer_owned_choreography_session_owner(
                original,
                "owner-a",
                SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                    "bundle:bundle-a".to_string()
                ])),
            )
            .expect("attenuate owner capability");
        let attenuated_boundary =
            AuraLinkBoundary::for_scope(SessionOwnerCapabilityScope::Fragments(BTreeSet::from([
                "bundle:bundle-a".to_string(),
            ])));

        effects
            .assert_owned_choreography_boundary(&attenuated, &attenuated_boundary)
            .expect("attenuated capability should authorize its delegated boundary");
        assert!(
            matches!(
                effects.assert_owned_choreography_boundary(
                    &stale_full_capability,
                    &attenuated_boundary,
                ),
                Err(SessionIngressError::InvalidIngressRouting { .. })
                    | Err(SessionIngressError::StaleOwner { .. })
            ),
            "stale capability generation must be rejected even if the owner label is unchanged"
        );
    }

    #[tokio::test]
    async fn forged_cross_session_owner_capability_is_rejected() {
        let authority = test_authority(0x79);
        let effects =
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system");
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_a = Uuid::from_bytes([0x8a; 16]);
        let session_b = Uuid::from_bytes([0x8b; 16]);
        let owner_a = effects
            .start_owned_choreography_session("owner-a", session_a, roles.clone())
            .await
            .expect("start session a");
        effects
            .end_owned_choreography_session(&owner_a)
            .await
            .expect("close session a");

        let owner_b = effects
            .start_owned_choreography_session("owner-a", session_b, roles)
            .await
            .expect("start session b");
        let forged = RuntimeSessionOwner {
            session_id: owner_b.session_id,
            owner_label: owner_b.owner_label.clone(),
            capability: owner_a.capability.clone(),
        };

        assert!(
            matches!(
                effects.assert_owned_choreography_session(&forged),
                Err(SessionIngressError::InvalidIngressRouting { .. })
            ),
            "cross-session forged authority artifacts must fail closed"
        );
        effects
            .assert_owned_choreography_session(&owner_b)
            .expect("canonical capability should still be accepted");

        effects
            .end_owned_choreography_session(&owner_b)
            .await
            .expect("close session b");
    }

    #[tokio::test]
    async fn duplicate_session_start_reports_typed_already_exists_reason() {
        let authority = test_authority(0x99);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), authority)
                .expect("test effect system"),
        );
        let roles = vec![ChoreographicRole::for_authority(
            authority,
            RoleIndex::new(0).expect("role index"),
        )];
        let session_uuid = Uuid::from_bytes([0xaa; 16]);
        let (first, second) = futures::join!(
            effects.start_owned_choreography_session("owner-a", session_uuid, roles.clone()),
            effects.start_owned_choreography_session("owner-b", session_uuid, roles)
        );

        let (original, duplicate) = match (first, second) {
            (Ok(original), Err(duplicate)) => (original, duplicate),
            (Err(duplicate), Ok(original)) => (original, duplicate),
            (Ok(_), Ok(_)) => panic!("duplicate session start unexpectedly succeeded twice"),
            (Err(first_error), Err(second_error)) => {
                panic!(
                    "duplicate session start unexpectedly failed twice: {first_error:?} / {second_error:?}"
                )
            }
        };

        assert!(matches!(
            duplicate,
            SessionIngressError::SessionStart {
                reason: SessionStartFailureReason::AlreadyExists
                    | SessionStartFailureReason::TaskAlreadyBound
                    | SessionStartFailureReason::OwnerClaimRejected,
                ..
            }
        ));

        effects
            .end_owned_choreography_session(&original)
            .await
            .expect("close original session");
    }
    async fn live_drop_fixture(effects: Arc<AuraEffectSystem>, id: u8) -> OwnedVmSession {
        let manifest = linked_manifest("aura.test.owned-drop", "drop-fragment");
        let roles = vec![ChoreographicRole::for_authority(
            test_authority(id),
            RoleIndex::new(0).expect("actual role index"),
        )];
        let owner = effects
            .start_owned_choreography_session(
                "actual-drop-owner",
                Uuid::from_bytes([id; 16]),
                roles,
            )
            .await
            .expect("actual runtime admission");
        effects
            .claim_vm_fragments_for_manifest(&owner.owner_label, &manifest)
            .expect("actual registered fragment custody");
        let (engine, handler, vm_session_id) =
            crate::runtime::vm_host_bridge::open_drop_fixture_vm_session()
                .expect("real VM engine allocation");
        OwnedVmSession {
            effects,
            owner,
            routing_boundary: AuraLinkBoundary::for_manifest(&manifest),
            engine,
            handler,
            vm_session_id,
            retirement_armed: true,
            acknowledged_close_failure: None,
        }
    }

    #[tokio::test]
    async fn dropping_live_vm_retires_exact_runtime_binding_and_fragment_custody() {
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc1),
            )
            .expect("owned-drop effect system"),
        );
        let session = live_drop_fixture(effects.clone(), 0xc1).await;
        assert!(session
            .engine
            .active_sessions()
            .contains(&session.vm_session_id));
        assert!(!effects.vm_fragment_snapshot().is_empty());
        drop(session);
        assert!(effects.current_runtime_choreography_session_id().is_none());
        assert!(effects.vm_fragment_snapshot().is_empty());
        let next = live_drop_fixture(effects.clone(), 0xc1).await;
        next.close()
            .await
            .expect("same physical task admits new owner after retirement");
    }

    #[tokio::test]
    async fn original_session_end_fault_retains_same_vm_owner_for_retry() {
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc2),
            )
            .expect("one selected original effect owner"),
        );
        let mut session = live_drop_fixture(effects.clone(), 0xc2).await;
        let original_id = session.owner().session_id;
        *effects.original_session_end_fault.lock() = Some(original_id);
        let first = session
            .close_retained()
            .await
            .expect_err("named original end fault refuses close before binding mutation");
        assert!(matches!(
            first,
            SessionIngressError::SessionClose {
                session_id,
                source: SessionCloseFailure::Choreography(
                    aura_protocol::effects::ChoreographyError::InternalError { ref message }
                ),
                ..
            } if session_id == original_id
                && message == "selected original session-end fault before binding mutation"
        ));
        assert_eq!(
            effects.current_runtime_choreography_session_id(),
            Some(original_id),
            "failed close retains the exact original binding"
        );
        assert_eq!(session.owner().session_id, original_id);
        assert!(
            session.acknowledged_close_resource_failure().is_none(),
            "a refused close before binding mutation has no retirement receipt"
        );
        session
            .close_retained()
            .await
            .expect("same original owner retries the one-shot close");
        assert!(effects.current_runtime_choreography_session_id().is_none());
    }

    #[tokio::test]
    async fn original_vm_close_clock_failure_retains_actual_retirement_and_original_cause() {
        use aura_core::effects::{PhysicalTimeEffects, TimeError};
        let clock = aura_testkit::time::ManualPhysicalClock::new(100);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc3),
            )
            .expect("original runtime effects")
            .with_physical_time_provider(Arc::new(clock.clone())),
        );
        let mut session = live_drop_fixture(Arc::clone(&effects), 0xc3).await;
        let original_owner = session.owner().clone();
        clock
            .fail_next_observation(TimeError::OperationFailed {
                reason: "original VM close selected clock failure".into(),
            })
            .await;
        let first = session
            .close_retained()
            .await
            .expect_err("clock failure remains a close failure");
        let acknowledgment = session
            .acknowledged_close_resource_failure()
            .expect("actual producer retained binding and fragment retirement");
        let original = acknowledgment.native_failure();
        let retirement = Arc::clone(&acknowledgment._retirement);
        assert!(retirement.matches_owner(&original_owner));
        assert!(effects.current_runtime_choreography_session_id().is_none());
        assert!(effects.vm_fragment_snapshot().is_empty());
        fn retains_original(
            error: &(dyn std::error::Error + 'static),
            original: &SessionIngressError,
        ) -> bool {
            let mut current = Some(error);
            while let Some(cause) = current {
                if cause
                    .downcast_ref::<SessionIngressError>()
                    .is_some_and(|actual| std::ptr::eq(actual, original))
                {
                    return true;
                }
                current = cause.source();
            }
            false
        }
        assert!(retains_original(&first, original.as_ref()));
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(original.as_ref());
        let mut selected_failure = false;
        while let Some(cause) = current {
            selected_failure |= matches!(cause.downcast_ref::<TimeError>(),
                Some(TimeError::OperationFailed { reason }) if reason == "original VM close selected clock failure");
            current = cause.source();
        }
        assert!(
            selected_failure,
            "original native selected-provider cause survives"
        );
        clock
            .fail_next_observation(TimeError::ServiceUnavailable)
            .await;
        let repeated = session
            .close_retained()
            .await
            .expect_err("retirement never turns ordinary close into success");
        assert!(retains_original(&repeated, original.as_ref()));
        assert!(
            matches!(
                effects.physical_time().await,
                Err(TimeError::ServiceUnavailable)
            ),
            "retry must not replace original validation with a fresh clock read"
        );
        drop(session);
        let next = live_drop_fixture(Arc::clone(&effects), 0xc3).await;
        assert_eq!(next.owner().session_id, original_owner.session_id);
        assert!(
            !retirement.matches_owner(next.owner()),
            "same id and label cannot replace the original physical owner claim"
        );
        next.close()
            .await
            .expect("new independent owner closes normally");
    }

    #[tokio::test]
    async fn guardian_terminal_requires_actual_vm_close_and_preserves_both_native_failures() {
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc8),
            )
            .expect("one original actual runtime custody"),
        );
        for fail_primary in [false, true] {
            let mut session = live_drop_fixture(effects.clone(), 0xc8).await;
            effects
                .end_owned_choreography_session(session.owner())
                .await
                .expect("actual runtime owner retirement before terminal close");
            let primary = if fail_primary {
                let native = session
                    .advance_round("actual-drop-owner", &BTreeMap::new())
                    .await
                    .expect_err("retired actual owner cannot advance VM");
                Err(crate::core::AgentError::Aura(
                    aura_core::AuraError::Internal {
                        message: "actual retired VM operation".into(),
                        source: Some(Arc::new(native)),
                    },
                ))
            } else {
                Ok(())
            };
            let error = crate::handlers::invitation::finish_guardian_vm_operation(primary, session)
                .await
                .expect_err("successful primary cannot conceal failed actual close");
            let mut cursor: Option<&(dyn std::error::Error + 'static)> = Some(&error);
            let mut native_found = false;
            let mut dual_found = false;
            while let Some(source) = cursor {
                native_found |= source.is::<SessionIngressError>();
                if let Some(dual) =
                    source.downcast_ref::<crate::handlers::invitation::GuardianVmTerminalFailure>()
                {
                    dual_found = true;
                    assert!(std::error::Error::source(&dual.primary).is_some());
                    assert!(matches!(
                        dual.cleanup,
                        SessionIngressError::InvalidIngressRouting {
                            details: RuntimeBoundaryError::MissingTaskBinding,
                            ..
                        }
                    ));
                }
                cursor = source.source();
            }
            assert!(native_found, "actual runtime failure remains native");
            assert_eq!(dual_found, fail_primary);
            assert!(effects.current_runtime_choreography_session_id().is_none());
            assert!(effects.vm_fragment_snapshot().is_empty());
        }
    }

    #[tokio::test]
    async fn stale_dropped_vm_cannot_retire_transferred_runtime_owner() {
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc2),
            )
            .expect("transferred-drop effect system"),
        );
        let mut session = live_drop_fixture(effects.clone(), 0xc2).await;
        let stale = session.owner.clone();
        session
            .transfer_owner_in_place("new-drop-owner", session.routing_boundary.clone())
            .expect("actual transfer advances capability generation");
        let error = effects
            .retire_dropped_vm_owner(&stale.capability)
            .expect_err("old generation cannot cancel new custody");
        assert!(matches!(
            error,
            crate::runtime::subsystems::choreography::SessionOwnershipError::OwnerMismatch { .. }
        ));
        effects
            .assert_owned_choreography_boundary(&session.owner, &session.routing_boundary)
            .expect("new owner remains authoritative");
        assert!(!effects.vm_fragment_snapshot().is_empty());
        drop(session);
        assert!(effects.current_runtime_choreography_session_id().is_none());
        assert!(effects.vm_fragment_snapshot().is_empty());
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn actual_supervisor_abort_retires_live_vm_before_idle_publication() {
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                test_authority(0xc3),
            )
            .expect("forced-abort effect system"),
        );
        let supervisor = crate::task_registry::TaskSupervisor::new();
        let group = supervisor.group("actual-vm-abort");
        let (ready_tx, ready) = futures::channel::oneshot::channel();
        let owned_effects = effects.clone();
        let _handle = group.spawn_try_named("live-vm", async move {
            let session = live_drop_fixture(owned_effects, 0xc3).await;
            ready_tx
                .send(session.owner.clone())
                .expect("parent sees actual live allocation");
            futures::future::pending::<()>().await;
            drop(session);
            Ok(())
        });
        let owner = ready
            .await
            .expect("actual registered VM task reaches live state");
        assert!(!effects.vm_fragment_snapshot().is_empty());
        assert!(matches!(
            group.force_abort_remaining(),
            Err(crate::task_registry::TaskSupervisionError::ForcedAbort { .. })
        ));
        let drained = group.wait_for_idle(std::time::Duration::from_secs(2)).await;
        drained.expect("cancelled actual task retires and reaches quiescence");
        assert!(
            effects.vm_fragment_snapshot().is_empty(),
            "idle cannot publish before owned VM destructor retires fragments"
        );
        assert!(
            effects
                .ensure_runtime_choreography_session_owner_capability(
                    owner.session_id,
                    &owner.capability
                )
                .is_err(),
            "forced drop removes exact runtime owner as well as VM resources"
        );
    }
}
