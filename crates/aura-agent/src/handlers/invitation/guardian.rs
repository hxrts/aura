use super::vm_loop::{
    handle_invitation_vm_step, handle_invitation_vm_wait_status, map_invitation_vm_timeout,
};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
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

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                "Principal",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(
                to_vec(&request).map_err(|error| AgentError::internal(error.to_string()))?,
            );
            session.queue_send_bytes(
                to_vec(&confirm).map_err(|error| AgentError::internal(error.to_string()))?,
            );

            let budget = invitation_timeout_budget(
                effects.as_ref(),
                "guardian_invitation_principal_vm",
                INVITATION_VM_LOOP_TIMEOUT_MS,
            )
            .await?;

            let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
                loop {
                    let round = session
                        .advance_round_until_receive(
                            "Principal",
                            &peer_roles,
                            InvitationHandler::is_transport_no_message,
                        )
                        .await
                        .map_err(|error| AgentError::internal(error.to_string()))?;

                    if let Some(blocked) = round.blocked_receive {
                        session
                            .inject_blocked_receive(&blocked)
                            .map_err(|error| AgentError::internal(error.to_string()))?;
                        continue;
                    }

                    if handle_invitation_vm_wait_status(
                        round.host_wait_status,
                        true,
                        "guardian principal VM timed out while waiting for receive",
                        "guardian principal VM cancelled while waiting for receive",
                    )?
                    .is_some()
                    {
                        break Ok(());
                    }

                    if handle_invitation_vm_step(
                        round.step,
                        "guardian principal VM became stuck without a pending receive",
                    )? {
                        break Ok(());
                    }
                }
            })
            .await
            .map_err(|error| map_invitation_vm_timeout("guardian principal VM", &budget, error));

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }

    pub(super) async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let _ = (effects, invitation);
        Err(AgentError::internal(
            "guardian invitation acceptance requires signed guardian recovery key material; placeholder acceptances are disabled",
        ))
    }

    #[cfg(test)]
    #[allow(dead_code)]
    async fn execute_guardian_invitation_guardian_unsigned_for_tests(
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

        let result = async {
            let mut session = open_owned_manifest_vm_session_admitted(
                effects.clone(),
                session_id,
                roles,
                &manifest,
                "Guardian",
                &global_type,
                &local_types,
                crate::runtime::AuraVmSchedulerSignals::default(),
            )
            .await
            .map_err(|error| AgentError::internal(error.to_string()))?;
            session.queue_send_bytes(
                to_vec(&accept).map_err(|error| AgentError::internal(error.to_string()))?,
            );

            let budget = invitation_timeout_budget(
                effects.as_ref(),
                "guardian_invitation_guardian_vm",
                INVITATION_VM_LOOP_TIMEOUT_MS,
            )
            .await?;

            let loop_result = execute_with_timeout_budget(effects.as_ref(), &budget, || async {
                loop {
                    let round = session
                        .advance_round("Guardian", &peer_roles)
                        .await
                        .map_err(|error| AgentError::internal(error.to_string()))?;

                    if let Some(blocked) = round.blocked_receive {
                        session
                            .inject_blocked_receive(&blocked)
                            .map_err(|error| AgentError::internal(error.to_string()))?;
                        continue;
                    }

                    if handle_invitation_vm_wait_status(
                        round.host_wait_status,
                        false,
                        "guardian VM timed out while waiting for receive",
                        "guardian VM cancelled while waiting for receive",
                    )?
                    .is_some()
                    {
                        break Ok(());
                    }

                    if handle_invitation_vm_step(
                        round.step,
                        "guardian VM became stuck without a pending receive",
                    )? {
                        break Ok(());
                    }
                }
            })
            .await
            .map_err(|error| map_invitation_vm_timeout("guardian VM", &budget, error));

            let _ = session.close().await;
            loop_result
        }
        .await;
        result
    }
}
