use super::*;
use crate::runtime::vm_host_bridge::{
    advance_host_bridged_vm_round, advance_host_bridged_vm_round_until_receive,
    close_and_reap_vm_session, inject_vm_receive, open_manifest_vm_session_admitted,
    AuraVmHostWaitStatus,
};
use std::collections::BTreeMap;

pub(super) struct InvitationGuardianHandler<'a> {
    handler: &'a InvitationHandler,
}

impl<'a> InvitationGuardianHandler<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    fn role(authority_id: AuthorityId) -> ChoreographicRole {
        ChoreographicRole::for_authority(authority_id, RoleIndex::new(0).expect("role index"))
    }

    pub(super) async fn execute_guardian_invitation_principal(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let authority_id = self.handler.context.authority.authority_id();
        let role_description = invitation
            .message
            .clone()
            .unwrap_or_else(|| "guardian invitation".to_string());
        let request = GuardianInvitationRequest(GuardianRequest {
            invitation_id: invitation.invitation_id.clone(),
            principal: authority_id,
            role_description,
            recovery_capabilities: Vec::new(),
            expires_at_ms: invitation.expires_at,
        });
        let invitation_id = invitation.invitation_id.clone();
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let roles = vec![Self::role(authority_id), Self::role(invitation.receiver_id)];
        let peer_roles =
            BTreeMap::from([("Guardian".to_string(), Self::role(invitation.receiver_id))]);
        let manifest = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::local_types();
        let confirm = GuardianInvitationConfirm(GuardianConfirm {
            invitation_id: invitation_id.clone(),
            established: true,
            relationship_id: None,
        });

        effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("guardian invite VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                effects.as_ref(),
                &manifest,
                "Principal",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(to_vec(&request).map_err(|error| {
                AgentError::internal(format!("guardian request encode failed: {error}"))
            })?);
            handler.push_send_bytes(to_vec(&confirm).map_err(|error| {
                AgentError::internal(format!("guardian confirm encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round_until_receive(
                    effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "Principal",
                    &peer_roles,
                    InvitationHandler::is_transport_no_message,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Deferred => break Ok(()),
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian principal VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian principal VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian principal VM became stuck without a pending receive"
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

    pub(super) async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let authority_id = self.handler.context.authority.authority_id();
        let accept = GuardianInvitationAccept(GuardianAccept {
            invitation_id: invitation.invitation_id.clone(),
            signature: Vec::new(),
            recovery_public_key: Vec::new(),
        });
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let roles = vec![Self::role(invitation.sender_id), Self::role(authority_id)];
        let peer_roles =
            BTreeMap::from([("Principal".to_string(), Self::role(invitation.sender_id))]);
        let manifest = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::local_types();

        effects
            .start_session(session_id, roles)
            .await
            .map_err(|error| {
                AgentError::internal(format!("guardian invite VM start failed: {error}"))
            })?;

        let result = async {
            let (mut engine, handler, vm_sid) = open_manifest_vm_session_admitted(
                effects.as_ref(),
                &manifest,
                "Guardian",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(AgentError::internal)?;
            handler.push_send_bytes(to_vec(&accept).map_err(|error| {
                AgentError::internal(format!("guardian accept encode failed: {error}"))
            })?);

            let loop_result = loop {
                let round = advance_host_bridged_vm_round(
                    effects.as_ref(),
                    &mut engine,
                    handler.as_ref(),
                    vm_sid,
                    "Guardian",
                    &peer_roles,
                )
                .await
                .map_err(AgentError::internal)?;

                if let Some(blocked) = round.blocked_receive {
                    inject_vm_receive(&mut engine, vm_sid, &blocked)
                        .map_err(AgentError::internal)?;
                    continue;
                }

                match round.host_wait_status {
                    AuraVmHostWaitStatus::Idle => {}
                    AuraVmHostWaitStatus::TimedOut => {
                        break Err(AgentError::internal(
                            "guardian VM timed out while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Cancelled => {
                        break Err(AgentError::internal(
                            "guardian VM cancelled while waiting for receive".to_string(),
                        ));
                    }
                    AuraVmHostWaitStatus::Deferred | AuraVmHostWaitStatus::Delivered => {}
                }

                match round.step {
                    StepResult::AllDone => break Ok(()),
                    StepResult::Continue => {}
                    StepResult::Stuck => {
                        break Err(AgentError::internal(
                            "guardian VM became stuck without a pending receive".to_string(),
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
}
