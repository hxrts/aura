//! Canonical host-side ingress for owned Telltale VM sessions.

#![allow(dead_code)] // Phase 1A migrates session callers onto the ingress incrementally.
#![allow(clippy::result_large_err, clippy::incompatible_msrv)]

use std::collections::BTreeMap;
use std::sync::Arc;

use aura_mpst::{
    telltale_types::{GlobalType, LocalTypeR},
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

#[derive(Debug, Error)]
pub enum SessionIngressError {
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
    },
    #[error("failed to advance owned runtime session {session_id} for {owner_label}: {message}")]
    Round {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        message: String,
    },
    #[error("failed to close owned runtime session {session_id} for {owner_label}: {message}")]
    SessionClose {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
        message: String,
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
            Self::SessionClose { .. } => "session_close",
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
    vm_session_id: telltale_vm::SessionId,
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
        capability_generation = owner.capability.generation,
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
        from_generation = previous_owner.capability.generation,
        to_owner_label = %next_owner.owner_label,
        to_generation = next_owner.capability.generation,
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
        from_generation = previous_owner.capability.generation,
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
        capability_generation = owner.capability.generation,
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
        capability_generation = owner.capability.generation,
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

    pub fn vm_session_id(&self) -> telltale_vm::SessionId {
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
        .map_err(|message| SessionIngressError::Round {
            session_id: self.owner.session_id,
            owner_label: self.owner.owner_label.clone(),
            message,
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
        .map_err(|message| SessionIngressError::Round {
            session_id: self.owner.session_id,
            owner_label: self.owner.owner_label.clone(),
            message,
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
        receive: &BlockedVmReceive,
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
        self.effects
            .assert_owned_choreography_session(&self.owner)?;
        close_and_reap_vm_session(&mut self.engine, self.vm_session_id).map_err(|message| {
            SessionIngressError::SessionClose {
                session_id: self.owner.session_id,
                owner_label: self.owner.owner_label.clone(),
                message,
            }
        })?;
        self.effects
            .end_owned_choreography_session(&self.owner)
            .await
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
            owner,
            routing_boundary,
            engine,
            handler,
            vm_session_id,
        }),
        Err(error) => {
            log_session_owner_rejected(
                owner.session_id,
                &owner.owner_label,
                Some(manifest.protocol_id.as_str()),
                "vm_session_open_failed",
                &error.to_string(),
            );
            let _ = effects.end_owned_choreography_session(&owner).await;
            Err(SessionIngressError::SessionStart {
                session_id: owner.session_id,
                owner_label: owner.owner_label,
                reason: SessionStartFailureReason::VmSessionOpenFailed,
                message: error.to_string(),
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
        session.inject_blocked_receive(&blocked)?;
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

        if !boundary.is_allowed_by(&owner.capability.scope) {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: RuntimeBoundaryError::BoundaryScopeRejected {
                    boundary: boundary.clone(),
                    capability_scope: owner.capability.scope.clone(),
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

    pub async fn end_owned_choreography_session(
        &self,
        owner: &RuntimeSessionOwner,
    ) -> Result<(), SessionIngressError> {
        self.assert_owned_choreography_session(owner)?;
        ChoreographicEffects::end_session(self)
            .await
            .map_err(|error| SessionIngressError::SessionClose {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                message: error.to_string(),
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
        let original = effects
            .start_owned_choreography_session("owner-a", session_uuid, roles.clone())
            .await
            .expect("start first session");

        let duplicate_effects = Arc::clone(&effects);
        let duplicate = tokio::spawn(async move {
            duplicate_effects
                .start_owned_choreography_session("owner-b", session_uuid, roles)
                .await
        })
        .await
        .expect("duplicate task joined")
        .expect_err("duplicate session start must fail");

        assert!(matches!(
            duplicate,
            SessionIngressError::SessionStart {
                reason: SessionStartFailureReason::AlreadyExists,
                ..
            }
        ));

        effects
            .end_owned_choreography_session(&original)
            .await
            .expect("close original session");
    }
}
