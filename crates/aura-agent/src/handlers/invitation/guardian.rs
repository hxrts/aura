use super::vm_loop::{
    handle_invitation_vm_step, handle_invitation_vm_wait_status, invitation_invalid_error,
    map_invitation_vm_timeout,
};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use aura_core::effects::CryptoCoreEffects;
use aura_signature::{sign_ed25519_transcript, verify_ed25519_transcript};
use std::collections::BTreeMap;

/// How long the principal waits for the guardian to accept. Acceptance is a
/// human decision on another device, so this is far longer than a VM round.
const GUARDIAN_PRINCIPAL_ACCEPT_WINDOW_MS: u64 = 600_000;

#[derive(Debug, Clone, serde::Serialize)]
pub(super) struct GuardianInvitationAcceptancePayload {
    invitation_id: InvitationId,
    principal: AuthorityId,
    guardian: AuthorityId,
    recovery_public_key: Vec<u8>,
    invitation_sender_proof_key: Vec<u8>,
    expires_at: Option<u64>,
    decision: &'static str,
}

/// Transcript a guardian signs with its recovery key when accepting a
/// guardian invitation. Binds the key to this principal and invitation.
pub(super) struct GuardianInvitationAcceptanceTranscript<'a> {
    pub(super) invitation: &'a Invitation,
    pub(super) guardian: AuthorityId,
    pub(super) recovery_public_key: &'a [u8],
    pub(super) invitation_sender_proof_key: &'a [u8],
}

impl SecurityTranscript for GuardianInvitationAcceptanceTranscript<'_> {
    type Payload = GuardianInvitationAcceptancePayload;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.guardian-acceptance";

    fn transcript_payload(&self) -> Self::Payload {
        GuardianInvitationAcceptancePayload {
            invitation_id: self.invitation.invitation_id.clone(),
            principal: self.invitation.sender_id,
            guardian: self.guardian,
            recovery_public_key: self.recovery_public_key.to_vec(),
            invitation_sender_proof_key: self.invitation_sender_proof_key.to_vec(),
            expires_at: self.invitation.expires_at,
            decision: "accepted",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
struct GuardianConfirmationPayload {
    invitation_id: InvitationId,
    principal: AuthorityId,
    guardian: AuthorityId,
    expires_at: Option<u64>,
    established: bool,
}

impl SecurityTranscript for GuardianConfirmationPayload {
    type Payload = Self;

    const DOMAIN_SEPARATOR: &'static str = "aura.invitation.guardian-confirmation";

    fn transcript_payload(&self) -> Self::Payload {
        self.clone()
    }
}

fn guardian_confirmation_payload(invitation: &Invitation) -> GuardianConfirmationPayload {
    GuardianConfirmationPayload {
        invitation_id: invitation.invitation_id.clone(),
        principal: invitation.sender_id,
        guardian: invitation.receiver_id,
        expires_at: invitation.expires_at,
        established: true,
    }
}

/// Load this authority's guardian recovery keypair, creating it on first use.
async fn guardian_recovery_keypair(
    effects: &AuraEffectSystem,
    guardian: AuthorityId,
) -> AgentResult<(Vec<u8>, Vec<u8>)> {
    let private_key_key =
        crate::handlers::recovery::recovery_guardian_private_key_storage_key(guardian);
    let public_key_key =
        crate::handlers::recovery::recovery_guardian_public_key_storage_key(guardian);
    let stored_private = effects
        .retrieve(&private_key_key)
        .await
        .map_err(|error| AgentError::effects(error.to_string()))?;
    let stored_public = effects
        .retrieve(&public_key_key)
        .await
        .map_err(|error| AgentError::effects(error.to_string()))?;
    if let (Some(private_key), Some(public_key)) = (stored_private, stored_public) {
        return Ok((private_key, public_key));
    }
    let (private_key, public_key) = effects
        .ed25519_generate_keypair()
        .await
        .map_err(|error| AgentError::effects(error.to_string()))?;
    effects
        .store(&private_key_key, private_key.clone())
        .await
        .map_err(|error| AgentError::effects(error.to_string()))?;
    effects
        .store(&public_key_key, public_key.clone())
        .await
        .map_err(|error| AgentError::effects(error.to_string()))?;
    Ok((private_key, public_key))
}

/// Verify a guardian's signed acceptance and record its recovery key so the
/// guardian can later take part in guardian setup and recovery.
pub(super) async fn verify_and_record_guardian_acceptance(
    effects: &AuraEffectSystem,
    invitation: &Invitation,
    accept: &GuardianAccept,
) -> AgentResult<()> {
    if accept.invitation_id != invitation.invitation_id {
        return Err(AgentError::invalid(
            "guardian acceptance does not match this invitation".to_string(),
        ));
    }
    if accept.recovery_public_key.len() != 32 || accept.signature.is_empty() {
        return Err(AgentError::invalid(
            "guardian acceptance is missing recovery key material".to_string(),
        ));
    }
    if accept.invitation_sender_proof_key.len() != 32 {
        return Err(AgentError::invalid(
            "guardian acceptance lacks the invitation sender proof key",
        ));
    }
    // First binding: there is no prior trusted key for this guardian. The
    // signature proves possession and binds the key to this invitation; the key
    // is trusted only after it is recorded for the guardian below.
    let self_certified_sender_key = &accept.recovery_public_key;
    let transcript = GuardianInvitationAcceptanceTranscript {
        invitation,
        guardian: invitation.receiver_id,
        recovery_public_key: self_certified_sender_key,
        invitation_sender_proof_key: &accept.invitation_sender_proof_key,
    };
    let verified = aura_signature::verify_ed25519_transcript(
        effects,
        &transcript,
        &accept.signature,
        self_certified_sender_key,
    )
    .await
    .map_err(|error| AgentError::invalid(error.to_string()))?;
    if !verified {
        return Err(AgentError::invalid(
            "guardian acceptance signature verification failed".to_string(),
        ));
    }
    effects
        .store(
            &crate::handlers::recovery::recovery_guardian_public_key_storage_key(
                invitation.receiver_id,
            ),
            accept.recovery_public_key.clone(),
        )
        .await
        .map_err(|error| AgentError::effects(error.to_string()))
}

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
            let mut confirmation_queued = false;

            let budget = invitation_timeout_budget(
                effects.as_ref(),
                "guardian_invitation_principal_vm",
                GUARDIAN_PRINCIPAL_ACCEPT_WINDOW_MS,
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
                        // The guardian's reply carries its signed recovery key.
                        let accept: GuardianInvitationAccept = from_slice(&blocked.payload)
                            .map_err(|error| {
                                invitation_invalid_error("malformed guardian acceptance", error)
                            })?;
                        if !confirmation_queued {
                            let Some((private_key, _)) = crate::handlers::rendezvous_identity::retrieve_identity_keys_matching_public(
                                    effects.as_ref(),
                                    &authority_id,
                                    &accept.0.invitation_sender_proof_key,
                                )
                                .await
                            else {
                                return Err(AgentError::invalid(
                                    "guardian confirmation requires the retained invitation sender signing key",
                                ));
                            };
                            verify_and_record_guardian_acceptance(
                                effects.as_ref(),
                                invitation,
                                &accept.0,
                            )
                            .await?;
                            let signature = sign_ed25519_transcript(
                                effects.as_ref(),
                                &guardian_confirmation_payload(invitation),
                                &private_key,
                            )
                            .await
                            .map_err(|error| AgentError::effects(error.to_string()))?;
                            let confirm = GuardianInvitationConfirm(GuardianConfirm {
                                invitation_id: invitation_id.clone(),
                                established: true,
                                relationship_id: None,
                                signature,
                            });
                            session.queue_send_bytes(
                                to_vec(&confirm)
                                    .map_err(|error| AgentError::internal(error.to_string()))?,
                            );
                            confirmation_queued = true;
                        }
                        session
                            .inject_blocked_receive(&blocked)
                            .map_err(|error| AgentError::internal(error.to_string()))?;
                        continue;
                    }

                    // No message yet means the guardian has not accepted. The VM
                    // reports itself stuck on that receive, so wait again within
                    // the acceptance window instead of judging the step.
                    if matches!(round.host_wait_status, AuraVmHostWaitStatus::Deferred) {
                        continue;
                    }
                    if handle_invitation_vm_wait_status(
                        round.host_wait_status,
                        false,
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
        let authority_id = self.handler.context.authority.authority_id();
        let imported = InvitationHandler::load_imported_invitation(
            effects.as_ref(),
            authority_id,
            &invitation.invitation_id,
            None,
        )
        .await
        .ok_or_else(|| {
            AgentError::invalid("guardian confirmation requires imported invitation evidence")
        })?;
        let sender_proof_key = imported
            .sender_proof_key
            .ok_or_else(|| AgentError::invalid("guardian invitation lacks sender proof key"))?;
        let (private_key, recovery_public_key) =
            guardian_recovery_keypair(effects.as_ref(), authority_id).await?;
        let transcript = GuardianInvitationAcceptanceTranscript {
            invitation,
            guardian: authority_id,
            recovery_public_key: &recovery_public_key,
            invitation_sender_proof_key: &sender_proof_key,
        };
        let signature =
            aura_signature::sign_ed25519_transcript(effects.as_ref(), &transcript, &private_key)
                .await
                .map_err(|error| AgentError::effects(error.to_string()))?;
        let accept = GuardianInvitationAccept(GuardianAccept {
            invitation_id: invitation.invitation_id.clone(),
            signature,
            recovery_public_key,
            invitation_sender_proof_key: sender_proof_key.clone(),
        });
        let session_id = InvitationHandler::invitation_session_id(&invitation.invitation_id);
        let roles = vec![Self::role(invitation.sender_id), Self::role(authority_id)];
        let peer_roles =
            BTreeMap::from([("Principal".to_string(), Self::role(invitation.sender_id))]);
        let manifest = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::composition_manifest();
        let global_type = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::global_type();
        let local_types = aura_invitation::protocol::guardian::telltale_session_types_invitation_guardian::vm_artifacts::local_types();

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
        let mut request_received = false;
        let mut confirmation_verified = false;

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
                    if !request_received {
                        let request: GuardianInvitationRequest = from_slice(&blocked.payload)
                            .map_err(|error| {
                                invitation_invalid_error("malformed guardian request", error)
                            })?;
                        if request.0.invitation_id != invitation.invitation_id
                            || request.0.principal != invitation.sender_id
                        {
                            return Err(AgentError::invalid(
                                "guardian request does not match imported invitation",
                            ));
                        }
                        request_received = true;
                    } else {
                        let confirm: GuardianInvitationConfirm = from_slice(&blocked.payload)
                            .map_err(|error| {
                                invitation_invalid_error("malformed guardian confirmation", error)
                            })?;
                        if confirm.0.invitation_id != invitation.invitation_id
                            || !confirm.0.established
                        {
                            return Err(AgentError::invalid(
                                "guardian confirmation does not match imported invitation",
                            ));
                        }
                        // This self-certified code key proves continuity with
                        // the invitation the guardian imported, not device identity.
                        let self_certified_sender_key = &sender_proof_key;
                        let verified = verify_ed25519_transcript(
                            effects.as_ref(),
                            &guardian_confirmation_payload(invitation),
                            &confirm.0.signature,
                            self_certified_sender_key,
                        )
                        .await
                        .map_err(|error| AgentError::invalid(error.to_string()))?;
                        if !verified {
                            return Err(AgentError::invalid(
                                "guardian confirmation signature is invalid",
                            ));
                        }
                        let now_ms = PhysicalTimeEffects::physical_time(effects.as_ref())
                            .await
                            .map_err(|error| AgentError::effects(error.to_string()))?
                            .ts_ms;
                        if invitation.is_expired(now_ms) {
                            return Err(AgentError::invalid(
                                "guardian confirmation arrived after invitation expiry",
                            ));
                        }
                        let key = guardian_confirmation_storage_key(&invitation.invitation_id);
                        let encoded = to_vec(&confirm.0)
                            .map_err(|error| AgentError::internal(error.to_string()))?;
                        match effects
                            .retrieve(&key)
                            .await
                            .map_err(|error| AgentError::effects(error.to_string()))?
                        {
                            Some(existing) if existing != encoded => {
                                return Err(AgentError::invalid(
                                    "conflicting guardian confirmation replay",
                                ));
                            }
                            Some(_) => {}
                            None => effects
                                .store(&key, encoded)
                                .await
                                .map_err(|error| AgentError::effects(error.to_string()))?,
                        }
                        confirmation_verified = true;
                    }
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
        loop_result?;
        if confirmation_verified {
            Ok(())
        } else {
            Err(AgentError::invalid(
                "guardian choreography ended without verified confirmation",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn principal_confirmation_signature_binds_invitation_and_participants() {
        let effects =
            crate::testing::simulation_effect_system_arc(&crate::core::AgentConfig::default());
        let (private, public) = effects.ed25519_generate_keypair().await.unwrap();
        let payload = GuardianConfirmationPayload {
            invitation_id: InvitationId::new("guardian-proof"),
            principal: AuthorityId::new_from_entropy([1; 32]),
            guardian: AuthorityId::new_from_entropy([2; 32]),
            expires_at: Some(1_700_000_000_000),
            established: true,
        };
        let signature = sign_ed25519_transcript(effects.as_ref(), &payload, &private)
            .await
            .unwrap();
        assert!(
            verify_ed25519_transcript(effects.as_ref(), &payload, &signature, &public)
                .await
                .unwrap()
        );
        let forged = GuardianConfirmationPayload {
            invitation_id: InvitationId::new("different-invitation"),
            ..payload
        };
        assert!(
            !verify_ed25519_transcript(effects.as_ref(), &forged, &signature, &public)
                .await
                .unwrap()
        );
    }
}
