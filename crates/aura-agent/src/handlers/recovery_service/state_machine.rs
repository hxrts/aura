use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use crate::runtime::vm_host_bridge::{AuraVmHostWaitStatus, AuraVmRoundDisposition};
use aura_core::util::serialization::to_vec;
use aura_protocol::effects::{ChoreographicRole, RoleIndex};
use std::collections::BTreeMap;
use telltale_vm::vm::StepResult;

fn recovery_role(authority_id: AuthorityId, role_index: u16) -> ChoreographicRole {
    ChoreographicRole::for_authority(
        authority_id,
        RoleIndex::new(role_index.into()).expect("role index"),
    )
}

pub(super) async fn execute_recovery_protocol_account(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let roles = vec![
        recovery_role(authority_id, 0),
        recovery_role(authority_id, 1),
        recovery_role(guardian_id, 0),
    ];
    let peer_roles = BTreeMap::from([("Coordinator".to_string(), recovery_role(authority_id, 1))]);
    let manifest =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
    let global_type =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::global_type();
    let local_types =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::local_types();

    let result = async {
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Account",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);

        let loop_result = loop {
            let round = session
                .advance_round("Account", &peer_roles)
                .await
                .map_err(|error| AgentError::internal(error.to_string()))?;

            match crate::runtime::handle_owned_vm_round(&mut session, round, "recovery account VM")
                .map_err(|error| AgentError::internal(error.to_string()))?
            {
                AuraVmRoundDisposition::Continue => {}
                AuraVmRoundDisposition::Complete => break Ok(()),
            }
        };

        let _ = session.close().await;
        loop_result
    }
    .await;
    result
}

pub(super) async fn execute_recovery_protocol_coordinator(
    effects: Arc<AuraEffectSystem>,
    authority_id: AuthorityId,
    guardian_id: AuthorityId,
    request: ProtocolRecoveryRequest,
) -> AgentResult<()> {
    let session_id = recovery_session_id(&request.recovery_id, &guardian_id);
    let roles = vec![
        recovery_role(authority_id, 0),
        recovery_role(authority_id, 1),
        recovery_role(guardian_id, 0),
    ];
    let peer_roles = BTreeMap::from([
        ("Account".to_string(), recovery_role(authority_id, 0)),
        ("Guardian".to_string(), recovery_role(guardian_id, 0)),
    ]);
    let manifest =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::composition_manifest();
    let global_type =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::global_type();
    let local_types =
        aura_recovery::recovery_protocol::telltale_session_types_recovery_protocol::vm_artifacts::local_types();

    let result = async {
        let mut session = open_owned_manifest_vm_session_admitted(
            effects.clone(),
            session_id,
            roles,
            &manifest,
            "Coordinator",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(|error| AgentError::internal(error.to_string()))?;
        session.queue_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);
        let mut approvals = Vec::new();

        let loop_result = loop {
            let round = session
                .advance_round("Coordinator", &peer_roles)
                .await
                .map_err(|error| AgentError::internal(error.to_string()))?;

            if let Some(blocked) = round.blocked_receive {
                let approval: ProtocolGuardianApproval =
                    from_slice(&blocked.payload).map_err(|error| {
                        AgentError::internal(format!("guardian approval decode failed: {error}"))
                    })?;
                approvals.push(approval.clone());
                session.queue_send_bytes(
                    to_vec(&RecoveryOutcome {
                        success: true,
                        recovery_grant: None,
                        error: None,
                        approvals: approvals.clone(),
                    })
                    .map_err(|error| {
                        AgentError::internal(format!("recovery outcome encode failed: {error}"))
                    })?,
                );
                session
                    .inject_blocked_receive(&blocked)
                    .map_err(|error| AgentError::internal(error.to_string()))?;
                continue;
            }

            match round.host_wait_status {
                AuraVmHostWaitStatus::Idle => {}
                AuraVmHostWaitStatus::TimedOut => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM timed out while waiting for receive".to_string(),
                    ));
                }
                AuraVmHostWaitStatus::Cancelled => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM cancelled while waiting for receive".to_string(),
                    ));
                }
                AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
            }

            match round.step {
                StepResult::AllDone => break Ok(()),
                StepResult::Continue => {}
                StepResult::Stuck => {
                    break Err(AgentError::internal(
                        "recovery coordinator VM became stuck without a pending receive"
                            .to_string(),
                    ));
                }
            }
        };

        let _ = session.close().await;
        loop_result
    }
    .await;
    result
}

pub(super) fn recovery_session_id(recovery_id: &RecoveryId, guardian_id: &AuthorityId) -> Uuid {
    let mut material = Vec::new();
    material.extend_from_slice(recovery_id.as_str().as_bytes());
    material.extend_from_slice(&guardian_id.to_bytes());
    let digest = hash(&material);
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub(super) fn guardian_setup_session_id(setup_id: &str) -> Uuid {
    let digest = hash(setup_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}

pub(super) fn membership_session_id(change_id: &str) -> Uuid {
    let digest = hash(change_id.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::from_bytes(bytes)
}
