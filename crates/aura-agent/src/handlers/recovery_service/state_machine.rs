use super::*;
use crate::runtime::vm_host_bridge::{
    advance_host_bridged_vm_round, close_and_reap_vm_session, handle_standard_vm_round,
    inject_vm_receive, open_manifest_vm_session_admitted, AuraVmHostWaitStatus,
    AuraVmRoundDisposition,
};
use aura_core::util::serialization::to_vec;
use aura_protocol::effects::{ChoreographicEffects, ChoreographicRole, RoleIndex};
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

    effects
        .start_session(session_id, roles)
        .await
        .map_err(|error| {
            AgentError::internal(format!("recovery account VM start failed: {error}"))
        })?;

    let result = async {
        let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
            effects.as_ref(),
            &manifest,
            "Account",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(AgentError::internal)?;
        handler.push_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);

        let loop_result = loop {
            let round = advance_host_bridged_vm_round(
                effects.as_ref(),
                &mut engine,
                handler.as_ref(),
                vm_sid,
                "Account",
                &peer_roles,
            )
            .await
            .map_err(AgentError::internal)?;

            match handle_standard_vm_round(&mut engine, vm_sid, round, "recovery account VM")
                .map_err(AgentError::internal)?
            {
                AuraVmRoundDisposition::Continue => {}
                AuraVmRoundDisposition::Complete => break Ok(()),
            }
        };

        let _ = close_and_reap_vm_session(&mut engine, vm_sid);
        loop_result
    }
    .await;

    let _ = effects.end_session().await;
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

    effects
        .start_session(session_id, roles)
        .await
        .map_err(|error| {
            AgentError::internal(format!("recovery coordinator VM start failed: {error}"))
        })?;

    let result = async {
        let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
            effects.as_ref(),
            &manifest,
            "Coordinator",
            &global_type,
            &local_types,
            crate::runtime::AuraVmSchedulerSignals::default(),
        )
        .await
        .map_err(AgentError::internal)?;
        handler.push_send_bytes(to_vec(&request).map_err(|error| {
            AgentError::internal(format!("recovery request encode failed: {error}"))
        })?);
        let mut approvals = Vec::new();

        let loop_result = loop {
            let round = advance_host_bridged_vm_round(
                effects.as_ref(),
                &mut engine,
                handler.as_ref(),
                vm_sid,
                "Coordinator",
                &peer_roles,
            )
            .await
            .map_err(AgentError::internal)?;

            if let Some(blocked) = round.blocked_receive {
                let approval: ProtocolGuardianApproval =
                    from_slice(&blocked.payload).map_err(|error| {
                        AgentError::internal(format!("guardian approval decode failed: {error}"))
                    })?;
                approvals.push(approval.clone());
                handler.push_send_bytes(
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
                inject_vm_receive(&mut engine, vm_sid, &blocked).map_err(AgentError::internal)?;
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

        let _ = close_and_reap_vm_session(&mut engine, vm_sid);
        loop_result
    }
    .await;

    let _ = effects.end_session().await;
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
