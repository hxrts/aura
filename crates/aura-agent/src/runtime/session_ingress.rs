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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSessionOwner {
    pub session_id: RuntimeChoreographySessionId,
    pub owner_label: String,
    pub capability: SessionOwnerCapability,
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
                details: format!(
                    "session capability no longer valid; current generation is {current_generation}"
                ),
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
        event = "runtime.session.owner.rejected",
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
        event = "runtime.session.owner.transferred",
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
        event = "runtime.session.owner.transfer_rejected",
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
        event = "runtime.session.ingress.received",
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
        event = "runtime.session.ingress.dropped",
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

    pub fn transfer_owner(
        mut self,
        next_owner_label: impl Into<String>,
        next_scope: SessionOwnerCapabilityScope,
    ) -> Result<Self, SessionIngressError> {
        self.owner = self.effects.transfer_owned_choreography_session_owner(
            self.owner,
            next_owner_label,
            next_scope,
        )?;
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
                    details: "no choreography session bound to current task".to_string(),
                })?;

        if current_session_id != session_id {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id,
                owner_label: owner_label.to_string(),
                details: format!("current task bound to session {current_session_id}"),
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
        self.assert_runtime_choreography_session_binding(owner.session_id, &owner.owner_label)?;

        if !owner.capability.allows_full_session() {
            return Err(SessionIngressError::InvalidIngressRouting {
                session_id: owner.session_id,
                owner_label: owner.owner_label.clone(),
                details: "current capability does not authorize full-session ingress".to_string(),
            });
        }

        self.ensure_runtime_choreography_session_owner_capability(
            owner.session_id,
            &owner.capability,
        )
        .map_err(|error| SessionIngressError::InvalidIngressRouting {
            session_id: owner.session_id,
            owner_label: owner.owner_label.clone(),
            details: error,
        })
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
            details: error,
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
    use aura_protocol::effects::{ChoreographicRole, RoleIndex};

    fn test_authority(byte: u8) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([byte; 16]))
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
}
