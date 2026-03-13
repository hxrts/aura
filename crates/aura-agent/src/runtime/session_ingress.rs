//! Canonical host-side ingress for owned Telltale VM sessions.

#![allow(dead_code)] // Phase 1A migrates session callers onto the ingress incrementally.

use std::collections::BTreeMap;
use std::sync::Arc;

use aura_mpst::{
    telltale_types::{GlobalType, LocalTypeR},
    CompositionManifest,
};
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole};
use thiserror::Error;
use uuid::Uuid;

use super::subsystems::choreography::{RuntimeChoreographySessionId, SessionOwnershipError};
use super::vm_host_bridge::{
    advance_host_bridged_vm_round, advance_host_bridged_vm_round_until_receive,
    close_and_reap_vm_session, handle_standard_vm_round, open_manifest_vm_session_admitted,
    AuraQueuedVmBridgeHandler, AuraVmBridgeRound, AuraVmHostWaitStatus, AuraVmRoundDisposition,
    BlockedVmReceive,
};
use super::{AuraChoreoEngine, AuraEffectSystem, AuraVmSchedulerSignals};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionOwner {
    pub session_id: RuntimeChoreographySessionId,
    pub owner_label: String,
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
        details: String,
    },
    #[error("failed to start owned runtime session {session_id} for {owner_label}: {message}")]
    SessionStart {
        session_id: RuntimeChoreographySessionId,
        owner_label: String,
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
        }
    }
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
            SessionOwnershipError::OwnerConflict {
                session_id,
                existing_owner,
                requested_owner,
            } => SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: requested_owner,
                details: format!("session already owned by {existing_owner}"),
            },
        }
    }
}

pub struct OwnedVmSession {
    effects: Arc<AuraEffectSystem>,
    owner: RuntimeSessionOwner,
    engine: AuraChoreoEngine<AuraQueuedVmBridgeHandler>,
    handler: Arc<AuraQueuedVmBridgeHandler>,
    vm_session_id: telltale_vm::SessionId,
}

fn log_session_owner_assigned(
    owner: &RuntimeSessionOwner,
    protocol_id: Option<&str>,
    context: &'static str,
) {
    tracing::debug!(
        event = "runtime.session.owner.assigned",
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
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
        event = "runtime.session.owner.rejected",
        session_id = %session_id,
        owner_label,
        protocol_id,
        reason,
        error,
        "Rejected runtime session owner"
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
        event = "runtime.session.ingress.received",
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
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
        event = "runtime.session.ingress.dropped",
        session_id = %owner.session_id,
        owner_label = %owner.owner_label,
        ingress_kind,
        active_role,
        error_kind = error.error_kind(),
        error = %error,
        "Dropped owned session ingress"
    );
}

impl OwnedVmSession {
    pub fn owner(&self) -> &RuntimeSessionOwner {
        &self.owner
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
        if let Err(error) = self.effects.assert_owned_choreography_session(&self.owner) {
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
        if let Err(error) = self.effects.assert_owned_choreography_session(&self.owner) {
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
        if let Err(error) = self.effects.assert_owned_choreography_session(&self.owner) {
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
        if let Err(error) =
            self.claim_runtime_choreography_session_owner(session_id, owner_label.clone())
        {
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
                message: error,
            });
        }

        let owner = RuntimeSessionOwner {
            session_id,
            owner_label,
        };
        log_session_owner_assigned(&owner, None, "start_owned_choreography_session");
        Ok(owner)
    }

    pub fn assert_owned_choreography_session(
        &self,
        owner: &RuntimeSessionOwner,
    ) -> Result<(), SessionIngressError> {
        let current_session_id =
            self.current_runtime_choreography_session_id()
                .ok_or_else(|| SessionIngressError::InvalidIngressRouting {
                    session_id: owner.session_id,
                    owner_label: owner.owner_label.clone(),
                    details: "no choreography session bound to current task".to_string(),
                })?;

        if current_session_id != owner.session_id {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: format!("current task bound to session {current_session_id}"),
            });
        }

        self.ensure_runtime_choreography_session_owner(owner.session_id, &owner.owner_label)
            .map_err(|error| SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: error,
            })
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
