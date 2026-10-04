use super::vm_loop::{
    handle_invitation_vm_step, handle_invitation_vm_wait_status, invitation_invalid_error,
    map_invitation_vm_timeout,
};
use super::*;
use crate::runtime::open_owned_manifest_vm_session_admitted;
use aura_core::effects::CryptoCoreEffects;
#[cfg(test)]
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

/// Local proof that the retained private half matches the original public half.
/// This grants no invitation acceptance or replacement-key authority.
struct GuardianRecoveryKeyContinuityTranscript<'a> {
    guardian: AuthorityId,
    public: &'a [u8],
}

impl SecurityTranscript for GuardianRecoveryKeyContinuityTranscript<'_> {
    type Payload = (AuthorityId, Vec<u8>);

    const DOMAIN_SEPARATOR: &'static str = "aura.guardian.recovery-keypair";

    fn transcript_payload(&self) -> Self::Payload {
        (self.guardian, self.public.to_vec())
    }
}

#[derive(Debug, thiserror::Error)]
enum GuardianRecoveryKeyError {
    #[error("Guardian recovery key belongs to another runtime authority")]
    ForeignAuthority,
    #[error("original Guardian private recovery key is missing")]
    MissingPrivate,
    #[error("original Guardian public recovery key is missing")]
    MissingPublic,
    #[error("original Guardian recovery keys do not match")]
    Mismatch,
}

fn guardian_key_failure(source: GuardianRecoveryKeyError) -> AgentError {
    let message = "required original Guardian recovery identity".into();
    let error = match source {
        GuardianRecoveryKeyError::ForeignAuthority => aura_core::AuraError::PermissionDenied {
            message,
            source: Some(Arc::new(source)),
        },
        GuardianRecoveryKeyError::Mismatch => aura_core::AuraError::Crypto {
            message,
            source: Some(Arc::new(source)),
        },
        GuardianRecoveryKeyError::MissingPrivate | GuardianRecoveryKeyError::MissingPublic => {
            aura_core::AuraError::Storage {
                message,
                source: Some(Arc::new(source)),
            }
        }
    };
    AgentError::Aura(error)
}
fn guardian_crypto_failure(stage: &'static str, source: aura_core::AuraError) -> AgentError {
    AgentError::Aura(aura_core::AuraError::Crypto {
        message: stage.into(),
        source: Some(Arc::new(source)),
    })
}
fn guardian_codec_failure(source: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::Aura(aura_core::AuraError::Serialization {
        message: "required Guardian protocol encoding".into(),
        source: Some(Arc::new(source)),
    })
}
fn guardian_vm_failure(source: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::Aura(aura_core::AuraError::Internal {
        message: "required Guardian VM operation".into(),
        source: Some(Arc::new(source)),
    })
}
fn guardian_effect_failure(source: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::Aura(aura_core::AuraError::Internal {
        message: "required Guardian effect operation".into(),
        source: Some(Arc::new(source)),
    })
}
fn guardian_storage_failure(source: impl std::error::Error + Send + Sync + 'static) -> AgentError {
    AgentError::Aura(aura_core::AuraError::Storage {
        message: "required Guardian confirmation storage".into(),
        source: Some(Arc::new(source)),
    })
}

/// Local failure evidence retains both operation and acknowledged-close causes.
/// Its standard source follows the primary failure; cleanup stays typed here.
#[derive(Debug, thiserror::Error)]
#[error("Guardian operation failed: {primary}; required disposal also failed: {cleanup}")]
pub(crate) struct GuardianVmTerminalFailure {
    #[source]
    pub(crate) primary: AgentError,
    pub(crate) cleanup: crate::runtime::session_ingress::SessionIngressError,
}

pub(crate) async fn finish_guardian_vm_operation(
    primary: AgentResult<()>,
    session: crate::runtime::session_ingress::OwnedVmSession,
) -> AgentResult<()> {
    match (primary, session.close().await) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(primary), Ok(())) => Err(primary),
        (Ok(()), Err(cleanup)) => Err(guardian_vm_failure(cleanup)),
        (Err(primary), Err(cleanup)) => {
            let is_timeout = primary.is_timeout();
            let source = aura_core::AuraError::Internal {
                message: "Guardian operation and required VM disposal failed".into(),
                source: Some(Arc::new(GuardianVmTerminalFailure { primary, cleanup })),
            };
            if is_timeout {
                Err(AgentError::TimeoutWithSource {
                    message: "Guardian operation timed out and disposal failed".into(),
                    source,
                })
            } else {
                Err(AgentError::Aura(source))
            }
        }
    }
}
fn guardian_transcript<T: SecurityTranscript + ?Sized>(transcript: &T) -> AgentResult<Vec<u8>> {
    transcript.required_transcript_bytes().map_err(|source| {
        AgentError::Aura(aura_core::AuraError::Serialization {
            message: "encode required Guardian transcript".into(),
            source: Some(Arc::new(source)),
        })
    })
}
async fn sign_guardian_transcript<T: SecurityTranscript + ?Sized>(
    effects: &AuraEffectSystem,
    transcript: &T,
    private: &[u8],
) -> AgentResult<Vec<u8>> {
    let bytes = guardian_transcript(transcript)?;
    effects
        .ed25519_sign(&bytes, private)
        .await
        .map_err(|source| guardian_crypto_failure("sign required Guardian transcript", source))
}
// Primitive provider-fault fixtures carry no terminal publication authority.
#[cfg(test)]
async fn verify_guardian_transcript<T: SecurityTranscript + ?Sized>(
    effects: &AuraEffectSystem,
    transcript: &T,
    signature: &[u8],
    public: &[u8],
) -> AgentResult<bool> {
    let bytes = guardian_transcript(transcript)?;
    effects
        .ed25519_verify(&bytes, signature, public)
        .await
        .map_err(|source| guardian_crypto_failure("verify required Guardian transcript", source))
}

/// Birth and required reads share the actual runtime's exclusive key owner.
/// A missing half cannot authorize replacement of the retained original half.
struct GuardianRecoveryIdentityCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    private: zeroize::Zeroizing<Vec<u8>>,
    public: Vec<u8>,
}

/// Required original pair reads retain the actual exclusive runtime lease
/// until their continuity proof completes. This is local integrity evidence,
/// distinct from invitation continuity and first-binding possession.
struct RequiredGuardianPairVerificationCapability<'runtime> {
    lease: crate::runtime::effects::GuardianRecoveryKeypairLeaseCapability<'runtime>,
    private: zeroize::Zeroizing<Vec<u8>>,
    public: Vec<u8>,
}
impl RequiredGuardianPairVerificationCapability<'_> {
    fn original_pair_key<'owner>(
        &'owner self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<&'owner [u8]> {
        if !std::ptr::eq(self.lease.effects(), effects) {
            return Err(guardian_key_failure(
                GuardianRecoveryKeyError::ForeignAuthority,
            ));
        }
        Ok(self.public.as_slice())
    }
}
async fn verify_guardian_pair_required(
    effects: &AuraEffectSystem,
    original: &RequiredGuardianPairVerificationCapability<'_>,
    signature: &[u8],
) -> AgentResult<bool> {
    let public = original.original_pair_key(effects)?;
    let transcript = GuardianRecoveryKeyContinuityTranscript {
        guardian: effects.runtime_authority_id(),
        public,
    };
    let bytes = guardian_transcript(&transcript)?;
    effects
        .ed25519_verify(&bytes, signature, original.original_pair_key(effects)?)
        .await
        .map_err(|source| guardian_crypto_failure("verify required Guardian pair", source))
}

/// Original imported-code continuity is neither device membership nor a
/// first-binding recovery-key possession proof.
struct RequiredGuardianConfirmationVerificationCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    stored: StoredImportedInvitation,
    invitation: Invitation,
    lease: crate::runtime::effects::ImportedInvitationDecisionLeaseCapability<'runtime>,
}
impl RequiredGuardianConfirmationVerificationCapability<'_> {
    fn original_sender_key<'owner>(
        &'owner self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<&'owner [u8]> {
        self.lease
            .require_runtime_owner(effects)
            .map_err(AgentError::Aura)?;
        if !std::ptr::eq(self.effects, effects)
            || self.invitation.receiver_id != effects.runtime_authority_id()
        {
            return Err(AgentError::invalid(
                "Guardian confirmation has a foreign runtime owner",
            ));
        }
        self.stored
            .sender_proof_key
            .as_deref()
            .ok_or_else(|| AgentError::invalid("Guardian original import lacks sender proof key"))
    }
}
async fn verify_guardian_confirmation_required(
    effects: &AuraEffectSystem,
    original: &RequiredGuardianConfirmationVerificationCapability<'_>,
    signature: &[u8],
) -> AgentResult<bool> {
    let bytes = guardian_transcript(&guardian_confirmation_payload(&original.invitation))?;
    effects
        .ed25519_verify(&bytes, signature, original.original_sender_key(effects)?)
        .await
        .map_err(|source| guardian_crypto_failure("verify original Guardian confirmation", source))
}
#[aura_macros::capability_boundary(category = "capability_gated",
    capability = "guardian_recovery_identity", capability_type = GuardianRecoveryIdentityCapability,
    family = "proof_issuer")]
#[aura_macros::authoritative_source(kind = "proof_issuer")]
async fn guardian_recovery_keypair(
    effects: &AuraEffectSystem,
    guardian: AuthorityId,
) -> AgentResult<GuardianRecoveryIdentityCapability<'_>> {
    let lease = effects.acquire_guardian_recovery_keypair().await;
    let effects = lease.effects();
    if guardian != effects.runtime_authority_id() {
        return Err(guardian_key_failure(
            GuardianRecoveryKeyError::ForeignAuthority,
        ));
    }
    let private_key_key =
        crate::handlers::recovery::recovery_guardian_private_key_storage_key(guardian);
    let public_key_key =
        crate::handlers::recovery::recovery_guardian_public_key_storage_key(guardian);
    let stored_private = effects
        .retrieve(&private_key_key)
        .await
        .map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Storage {
                message: "read original Guardian private recovery key".into(),
                source: Some(Arc::new(source)),
            })
        })?
        .map(zeroize::Zeroizing::new);
    let stored_public = effects.retrieve(&public_key_key).await.map_err(|source| {
        AgentError::Aura(aura_core::AuraError::Storage {
            message: "read original Guardian public recovery key".into(),
            source: Some(Arc::new(source)),
        })
    })?;
    match (stored_private, stored_public) {
        (Some(private), Some(public)) => {
            let original = RequiredGuardianPairVerificationCapability {
                lease,
                private,
                public,
            };
            let transcript = GuardianRecoveryKeyContinuityTranscript {
                guardian,
                public: &original.public,
            };
            let proof =
                sign_guardian_transcript(effects, &transcript, original.private.as_ref()).await?;
            if !verify_guardian_pair_required(effects, &original, &proof).await? {
                return Err(guardian_key_failure(GuardianRecoveryKeyError::Mismatch));
            }
            Ok(GuardianRecoveryIdentityCapability {
                effects,
                private: original.private,
                public: original.public,
            })
        }
        (Some(_), None) => Err(guardian_key_failure(
            GuardianRecoveryKeyError::MissingPublic,
        )),
        (None, Some(_)) => Err(guardian_key_failure(
            GuardianRecoveryKeyError::MissingPrivate,
        )),
        (None, None) => {
            let (private, public) = effects.ed25519_generate_keypair().await.map_err(|source| {
                guardian_crypto_failure("birth original Guardian recovery key", source)
            })?;
            let private = zeroize::Zeroizing::new(private);
            effects
                .store(&private_key_key, private.to_vec())
                .await
                .map_err(|source| {
                    AgentError::Aura(aura_core::AuraError::Storage {
                        message: "acknowledge original Guardian private recovery key".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
            effects
                .store(&public_key_key, public.clone())
                .await
                .map_err(|source| {
                    AgentError::Aura(aura_core::AuraError::Storage {
                        message: "acknowledge original Guardian public recovery key".into(),
                        source: Some(Arc::new(source)),
                    })
                })?;
            Ok(GuardianRecoveryIdentityCapability {
                effects,
                private,
                public,
            })
        }
    }
}

/// Verify a guardian's signed acceptance and record its recovery key so the
/// guardian can later take part in guardian setup and recovery.
struct RequiredGuardianPossessionVerificationCapability<'owner, 'runtime> {
    issued: &'owner super::issued_identity::IssuedInvitationIdentityCapability<'runtime>,
    accept: &'owner GuardianAccept,
}
impl RequiredGuardianPossessionVerificationCapability<'_, '_> {
    fn original_possession_key<'owner>(
        &'owner self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<&'owner [u8]> {
        self.issued.require_runtime_owner(effects)?;
        if self.accept.invitation_id != self.issued.invitation().invitation_id
            || self.accept.invitation_sender_proof_key.as_slice()
                != self.issued.public_key().as_slice()
        {
            return Err(AgentError::invalid(
                "Guardian possession has another original issuer",
            ));
        }
        Ok(self.accept.recovery_public_key.as_slice())
    }
}
async fn verify_guardian_possession_required(
    effects: &AuraEffectSystem,
    original: &RequiredGuardianPossessionVerificationCapability<'_, '_>,
) -> AgentResult<bool> {
    let invitation = original.issued.invitation();
    let transcript = GuardianInvitationAcceptanceTranscript {
        invitation,
        guardian: invitation.receiver_id,
        recovery_public_key: original.original_possession_key(effects)?,
        invitation_sender_proof_key: &original.accept.invitation_sender_proof_key,
    };
    let bytes = guardian_transcript(&transcript)?;
    effects
        .ed25519_verify(
            &bytes,
            &original.accept.signature,
            original.original_possession_key(effects)?,
        )
        .await
        .map_err(|source| guardian_crypto_failure("verify bound Guardian possession", source))
}

pub(super) async fn verify_and_record_guardian_acceptance(
    effects: &AuraEffectSystem,
    issued: &super::issued_identity::IssuedInvitationIdentityCapability<'_>,
    accept: &GuardianAccept,
) -> AgentResult<()> {
    issued.require_runtime_owner(effects)?;
    let invitation = issued.invitation();
    if !matches!(invitation.invitation_type,
        InvitationType::Guardian { subject_authority }
            if subject_authority == effects.runtime_authority_id())
        || invitation.sender_id != effects.runtime_authority_id()
        || accept.invitation_sender_proof_key.as_slice() != issued.public_key().as_slice()
    {
        return Err(AgentError::invalid(
            "guardian acceptance lacks its original issued invitation binding",
        ));
    }
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
    let original = RequiredGuardianPossessionVerificationCapability { issued, accept };
    let verified = verify_guardian_possession_required(effects, &original).await?;
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
        .map_err(|source| {
            AgentError::Aura(aura_core::AuraError::Storage {
                message: "record required Guardian acceptance key".into(),
                source: Some(Arc::new(source)),
            })
        })
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
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "guardian_invitation_principal_operation",
            GUARDIAN_PRINCIPAL_ACCEPT_WINDOW_MS,
        )
        .await?;
        execute_with_timeout_budget(effects.as_ref(), &budget, || {
            self.execute_guardian_principal_with_original_budget(
                effects.clone(),
                invitation,
                &budget,
            )
        })
        .await
        .map_err(|error| map_invitation_vm_timeout("guardian principal operation", &budget, error))
    }

    async fn execute_guardian_principal_with_original_budget(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        budget: &TimeoutBudget,
    ) -> AgentResult<()> {
        let record = self
            .handler
            .created_invitation_required(effects.clone(), &invitation.invitation_id)
            .await?;
        let issued = super::issued_identity::load_original_identity(record).await?;
        let candidate =
            serde_json::to_vec(&ShareableInvitation::from(invitation)).map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode guardian invocation binding".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        let canonical = serde_json::to_vec(&ShareableInvitation::from(issued.invitation()))
            .map_err(|source| {
                AgentError::Aura(aura_core::AuraError::Serialization {
                    message: "encode original guardian invocation".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
        if candidate != canonical {
            return Err(AgentError::invalid(
                "guardian invocation differs from required sender record",
            ));
        }
        let invitation = issued.invitation();
        let selected_identity =
            crate::handlers::rendezvous_identity::require_issued_identity_signing_context(
                effects.as_ref(),
                &issued,
            )
            .await
            .map_err(AgentError::EnrollmentManifest)?;
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
            .map_err(guardian_vm_failure)?;
            session.queue_send_bytes(to_vec(&request).map_err(guardian_codec_failure)?);
            let mut confirmation_queued = false;

            let loop_result = execute_with_timeout_budget(effects.as_ref(), budget, || async {
                loop {
                    let round = session
                        .advance_round_until_receive(
                            "Principal",
                            &peer_roles,
                            InvitationHandler::is_transport_no_message,
                        )
                        .await
                        .map_err(guardian_vm_failure)?;

                    if let Some(blocked) = round.blocked_receive {
                        // The guardian's reply carries its signed recovery key.
                        let accept: GuardianInvitationAccept = from_slice(&blocked.payload)
                            .map_err(|error| {
                                invitation_invalid_error("malformed guardian acceptance", error)
                            })?;
                        if !confirmation_queued {
                            if accept.0.invitation_sender_proof_key.as_slice()
                                != issued.public_key().as_slice()
                            {
                                return Err(AgentError::invalid(
                                    "guardian acceptance names another original issuer key",
                                ));
                            }
                            let (private_key, _) =
                                crate::handlers::rendezvous_identity::require_identity_keys(
                                    &selected_identity,
                                )
                                .await
                                .map_err(AgentError::EnrollmentManifest)?;
                            verify_and_record_guardian_acceptance(
                                effects.as_ref(),
                                &issued,
                                &accept.0,
                            )
                            .await?;
                            let signature = sign_guardian_transcript(
                                effects.as_ref(),
                                &guardian_confirmation_payload(invitation),
                                &private_key,
                            )
                            .await?;
                            let confirm = GuardianInvitationConfirm(GuardianConfirm {
                                invitation_id: invitation_id.clone(),
                                established: true,
                                relationship_id: None,
                                signature,
                            });
                            session.queue_send_bytes(
                                to_vec(&confirm).map_err(guardian_codec_failure)?,
                            );
                            confirmation_queued = true;
                        }
                        session
                            .inject_blocked_receive(&blocked)
                            .map_err(guardian_vm_failure)?;
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
            .map_err(|error| map_invitation_vm_timeout("guardian principal VM", budget, error));

            finish_guardian_vm_operation(loop_result, session).await
        }
        .await;
        result
    }

    pub(super) async fn execute_guardian_invitation_guardian(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
    ) -> AgentResult<()> {
        let budget = invitation_timeout_budget(
            effects.as_ref(),
            "guardian_invitation_guardian_operation",
            INVITATION_VM_LOOP_TIMEOUT_MS,
        )
        .await?;
        execute_with_timeout_budget(effects.as_ref(), &budget, || {
            self.execute_guardian_with_original_budget(effects.clone(), invitation, &budget)
        })
        .await
        .map_err(|error| map_invitation_vm_timeout("guardian operation", &budget, error))
    }

    async fn execute_guardian_with_original_budget(
        &self,
        effects: Arc<AuraEffectSystem>,
        invitation: &Invitation,
        budget: &TimeoutBudget,
    ) -> AgentResult<()> {
        let authority_id = self.handler.context.authority.authority_id();
        let imported_lease = effects.acquire_imported_invitation_decision().await;
        let imported = InvitationCacheHandler::load_imported_regular_required(
            effects.as_ref(),
            authority_id,
            &invitation.invitation_id,
            &imported_lease,
        )
        .await?
        .ok_or_else(|| {
            AgentError::invalid("guardian confirmation requires imported invitation evidence")
        })?;
        // Guardian materialization assigns the receiver's local context. The
        // retained code's context/version remain original import evidence;
        // they cannot be regenerated from that receiver-local projection.
        if !matches!(
            imported.shareable.invitation_type,
            InvitationType::Guardian { .. }
        ) || invitation.receiver_id != authority_id
            || imported.shareable.invitation_id != invitation.invitation_id
            || imported.shareable.sender_id != invitation.sender_id
            || imported.shareable.invitation_type != invitation.invitation_type
            || imported.shareable.expires_at != invitation.expires_at
            || imported.shareable.message != invitation.message
        {
            return Err(AgentError::invalid(
                "Guardian invocation differs from required imported invitation",
            ));
        }
        let original = RequiredGuardianConfirmationVerificationCapability {
            effects: effects.as_ref(),
            stored: imported,
            invitation: invitation.clone(),
            lease: imported_lease,
        };
        let sender_proof_key = original.original_sender_key(effects.as_ref())?.to_vec();
        let identity = guardian_recovery_keypair(effects.as_ref(), authority_id).await?;
        let recovery_public_key = identity.public.clone();
        let transcript = GuardianInvitationAcceptanceTranscript {
            invitation,
            guardian: authority_id,
            recovery_public_key: &recovery_public_key,
            invitation_sender_proof_key: &sender_proof_key,
        };
        let signature =
            sign_guardian_transcript(identity.effects, &transcript, identity.private.as_ref())
                .await?;
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
        .map_err(guardian_vm_failure)?;
        session.queue_send_bytes(to_vec(&accept).map_err(guardian_codec_failure)?);
        let mut request_received = false;
        let mut confirmation_verified = false;

        let loop_result = execute_with_timeout_budget(effects.as_ref(), budget, || async {
            loop {
                let round = session
                    .advance_round("Guardian", &peer_roles)
                    .await
                    .map_err(guardian_vm_failure)?;

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
                        let verified = verify_guardian_confirmation_required(
                            effects.as_ref(),
                            &original,
                            &confirm.0.signature,
                        )
                        .await?;
                        if !verified {
                            return Err(AgentError::invalid(
                                "guardian confirmation signature is invalid",
                            ));
                        }
                        let now_ms = PhysicalTimeEffects::physical_time(effects.as_ref())
                            .await
                            .map_err(guardian_effect_failure)?
                            .ts_ms;
                        if invitation.is_expired(now_ms) {
                            return Err(AgentError::invalid(
                                "guardian confirmation arrived after invitation expiry",
                            ));
                        }
                        let key = guardian_confirmation_storage_key(&invitation.invitation_id);
                        let encoded = to_vec(&confirm.0).map_err(guardian_codec_failure)?;
                        match effects
                            .retrieve(&key)
                            .await
                            .map_err(guardian_storage_failure)?
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
                                .map_err(guardian_storage_failure)?,
                        }
                        confirmation_verified = true;
                    }
                    session
                        .inject_blocked_receive(&blocked)
                        .map_err(guardian_vm_failure)?;
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
        .map_err(|error| map_invitation_vm_timeout("guardian VM", budget, error));

        let primary = loop_result.and_then(|()| {
            if confirmation_verified {
                Ok(())
            } else {
                Err(AgentError::invalid(
                    "guardian choreography ended without verified confirmation",
                ))
            }
        });
        finish_guardian_vm_operation(primary, session).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn required_guardian_pair_verifier_rejects_foreign_runtime_owner() {
        let directory = tempfile::tempdir().expect("isolated profiles");
        let authority = AuthorityId::new_from_entropy([239; 32]);
        let effects = owned_effects(&directory.path().join("original"), authority);
        let foreign = owned_effects(&directory.path().join("foreign"), authority);
        let identity = guardian_recovery_keypair(&effects, authority)
            .await
            .expect("actual acknowledged original pair");
        let original = RequiredGuardianPairVerificationCapability {
            lease: effects.acquire_guardian_recovery_keypair().await,
            private: identity.private,
            public: identity.public,
        };
        let transcript = GuardianRecoveryKeyContinuityTranscript {
            guardian: authority,
            public: &original.public,
        };
        let signature = sign_guardian_transcript(&effects, &transcript, original.private.as_ref())
            .await
            .expect("original pair signs continuity");
        assert!(
            verify_guardian_pair_required(&effects, &original, &signature)
                .await
                .expect("same runtime verifies original pair")
        );
        let error = verify_guardian_pair_required(&foreign, &original, &signature)
            .await
            .expect_err("matching authority cannot substitute another runtime");
        assert!(has_source::<GuardianRecoveryKeyError>(&error));
    }

    #[test]
    fn required_guardian_key_continuity_preserves_original_transcript_encoding() {
        let guardian = AuthorityId::new_from_entropy([238; 32]);
        let public = vec![0x7c; 32];
        let transcript = GuardianRecoveryKeyContinuityTranscript {
            guardian,
            public: &public,
        };
        let original = aura_signature::encode_transcript_required(
            "aura.guardian.recovery-keypair",
            1,
            &(guardian, &public),
        )
        .unwrap();
        assert_eq!(guardian_transcript(&transcript).unwrap(), original);
    }

    #[tokio::test]
    async fn required_guardian_configured_ed25519_outages_preserve_native_cause_and_original_keys()
    {
        use aura_testkit::stateful_effects::custom_provider::{
            CustomCryptoProbe, CustomEd25519Fault, CustomProviderProbe,
        };
        let profile = tempfile::tempdir().unwrap();
        let authority = AuthorityId::new_from_entropy([240; 32]);
        let crypto = Arc::new(CustomCryptoProbe::default());
        let provider = Arc::new(CustomProviderProbe::default());
        let agent = crate::AgentBuilder::custom()
            .with_crypto(crypto.clone())
            .with_storage(provider.clone())
            .with_time(Arc::new(aura_effects::time::PhysicalTimeHandler::new()))
            .with_random(provider.clone())
            .with_console(provider.clone())
            .authority(authority)
            .testing_mode()
            .with_config(crate::AgentConfig {
                storage: crate::core::config::StorageConfig {
                    base_path: profile.path().to_path_buf(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .build()
            .await
            .unwrap();
        let effects = agent.runtime().effects();
        crypto.set_ed25519_fault(Some(CustomEd25519Fault::Generate));
        let generation = guardian_recovery_keypair(&effects, authority)
            .await
            .err()
            .expect("configured generation failure cannot issue identity");
        assert!(has_source::<CustomEd25519Fault>(&generation));
        for key in [
            crate::handlers::recovery::recovery_guardian_private_key_storage_key(authority),
            crate::handlers::recovery::recovery_guardian_public_key_storage_key(authority),
        ] {
            assert!(effects.retrieve(&key).await.unwrap().is_none());
        }
        crypto.set_ed25519_fault(None);
        let identity = guardian_recovery_keypair(&effects, authority)
            .await
            .unwrap();
        let original = provider.stored_bytes().await;
        let transcript = GuardianConfirmationPayload {
            invitation_id: InvitationId::new("configured-guardian-native-provider"),
            principal: AuthorityId::new_from_entropy([239; 32]),
            guardian: authority,
            expires_at: None,
            established: true,
        };
        let signature = sign_guardian_transcript(&effects, &transcript, &identity.private)
            .await
            .unwrap();
        for fault in [CustomEd25519Fault::Sign, CustomEd25519Fault::Verify] {
            crypto.set_ed25519_fault(Some(fault));
            let continuity = guardian_recovery_keypair(&effects, authority)
                .await
                .err()
                .expect("configured continuity failure cannot issue identity");
            assert!(has_source::<CustomEd25519Fault>(&continuity));
            let response = match fault {
                CustomEd25519Fault::Sign => {
                    sign_guardian_transcript(&effects, &transcript, &identity.private)
                        .await
                        .map(|_| ())
                }
                CustomEd25519Fault::Verify => {
                    verify_guardian_transcript(&effects, &transcript, &signature, &identity.public)
                        .await
                        .map(|_| ())
                }
                CustomEd25519Fault::Generate => unreachable!("only response operations selected"),
            };
            let failure = response.expect_err("provider fault cannot count as a signed response");
            assert!(has_source::<CustomEd25519Fault>(&failure));
            assert_eq!(provider.stored_bytes().await, original);
        }
        crypto.set_ed25519_fault(None);
        assert!(
            verify_guardian_transcript(&effects, &transcript, &signature, &identity.public)
                .await
                .unwrap()
        );
        let restored = guardian_recovery_keypair(&effects, authority)
            .await
            .unwrap();
        assert_eq!(restored.public, identity.public);
        assert!(
            restored.private == identity.private,
            "original private key remains unchanged"
        );
        agent
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(5))
            .await
            .unwrap();
    }

    fn owned_effects(profile: &std::path::Path, authority: AuthorityId) -> Arc<AuraEffectSystem> {
        let config = crate::AgentConfig {
            storage: crate::core::config::StorageConfig {
                base_path: profile.to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        };
        let owner = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
            .expect("actual exclusive selected profile");
        Arc::new(
            AuraEffectSystem::testing_with_owned_profile(&config, authority, None, owner, None)
                .expect("actual selected encrypted storage and physical custody"),
        )
    }
    fn has_source<T: std::error::Error + 'static>(
        error: &(dyn std::error::Error + 'static),
    ) -> bool {
        let mut cursor = Some(error);
        while let Some(cause) = cursor {
            if cause.is::<T>() {
                return true;
            }
            cursor = cause.source();
        }
        false
    }

    fn assert_native_crypto_input_failure(error: &AgentError) {
        let AgentError::Aura(aura_core::AuraError::Crypto {
            source: Some(source),
            ..
        }) = error
        else {
            panic!("required crypto boundary must preserve its native source");
        };
        assert!(matches!(
            source.downcast_ref::<aura_core::AuraError>(),
            Some(aura_core::AuraError::Invalid { .. })
        ));
    }

    #[tokio::test]
    async fn required_guardian_original_window_bounds_held_import_before_key_birth() {
        use futures::FutureExt;
        for provider_fault in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let authority = AuthorityId::new_from_entropy([245; 32]);
            let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(1_000));
            let original = owned_effects(&directory.path().join("profile"), authority);
            let effects = Arc::new(
                Arc::try_unwrap(original)
                    .unwrap_or_else(|_| panic!("fixture retains one actual runtime owner"))
                    .with_physical_time_provider(clock.clone()),
            );
            let handler = InvitationHandler::new(AuthorityContext::new(authority)).unwrap();
            let guardian = InvitationGuardianHandler::new(&handler);
            let invitation = Invitation {
                invitation_id: InvitationId::new("held-original-guardian-import"),
                context_id: ContextId::new_from_entropy([246; 32]),
                sender_id: AuthorityId::new_from_entropy([247; 32]),
                receiver_id: authority,
                invitation_type: InvitationType::Guardian {
                    subject_authority: AuthorityId::new_from_entropy([247; 32]),
                },
                status: InvitationStatus::Pending,
                created_at: 1_000,
                expires_at: None,
                message: None,
                receiver_nickname: None,
            };
            let held = effects.acquire_imported_invitation_decision().await;
            let operation =
                guardian.execute_guardian_invitation_guardian(effects.clone(), &invitation);
            tokio::pin!(operation);
            assert!(
                operation.as_mut().now_or_never().is_none(),
                "original operation window starts before waiting for import custody"
            );
            if provider_fault {
                clock
                    .fail_next_sleep(aura_core::effects::TimeError::OperationFailed {
                        reason: "original Guardian clock provider outage".into(),
                    })
                    .await;
            } else {
                clock.set_time(1_000 + INVITATION_VM_LOOP_TIMEOUT_MS + 1);
            }
            let error = operation
                .await
                .expect_err("held preparation cannot renew the operation window");
            if provider_fault {
                assert!(!error.is_timeout());
                assert!(has_source::<aura_core::effects::TimeError>(&error));
            } else {
                assert!(error.is_timeout());
                assert!(has_source::<aura_core::TimeoutBudgetError>(&error));
            }
            assert!(effects
                .retrieve(
                    &crate::handlers::recovery::recovery_guardian_private_key_storage_key(
                        authority
                    )
                )
                .await
                .unwrap()
                .is_none());
            assert!(effects
                .retrieve(
                    &crate::handlers::recovery::recovery_guardian_public_key_storage_key(authority)
                )
                .await
                .unwrap()
                .is_none());
            drop(held);
        }
    }

    #[tokio::test]
    async fn required_guardian_partial_key_loss_reopens_without_replacement() {
        for missing_private in [true, false] {
            let directory = tempfile::tempdir().expect("isolated profile");
            let profile = directory.path().join("profile");
            let authority = AuthorityId::new_from_entropy([241; 32]);
            let effects = owned_effects(&profile, authority);
            let identity = guardian_recovery_keypair(&effects, authority)
                .await
                .expect("fresh owned pair");
            let private_key =
                crate::handlers::recovery::recovery_guardian_private_key_storage_key(authority);
            let public_key =
                crate::handlers::recovery::recovery_guardian_public_key_storage_key(authority);
            let (missing, retained) = if missing_private {
                (&private_key, &public_key)
            } else {
                (&public_key, &private_key)
            };
            let original =
                zeroize::Zeroizing::new(effects.retrieve(retained).await.unwrap().unwrap());
            drop(identity);
            assert!(effects.remove(missing).await.unwrap());
            drop(effects);
            let reopened = owned_effects(&profile, authority);
            let error = guardian_recovery_keypair(&reopened, authority)
                .await
                .err()
                .expect("missing original half fails");
            assert!(has_source::<GuardianRecoveryKeyError>(&error));
            assert!(reopened.retrieve(missing).await.unwrap().is_none());
            assert_eq!(
                reopened
                    .retrieve(retained)
                    .await
                    .unwrap()
                    .unwrap()
                    .as_slice(),
                original.as_slice()
            );
        }
    }

    #[tokio::test]
    async fn required_guardian_concurrent_birth_preserves_one_original_pair() {
        let directory = tempfile::tempdir().expect("isolated profile");
        let authority = AuthorityId::new_from_entropy([242; 32]);
        let effects = owned_effects(&directory.path().join("profile"), authority);
        let (first, second) = tokio::join!(
            guardian_recovery_keypair(&effects, authority),
            guardian_recovery_keypair(&effects, authority),
        );
        let first = first.expect("first acknowledged pair");
        let second = second.expect("same original pair under shared runtime owner");
        assert_eq!(first.public, second.public);
        assert_eq!(first.private.as_slice(), second.private.as_slice());
        let failure = guardian_recovery_keypair(&effects, AuthorityId::new_from_entropy([243; 32]))
            .await
            .err()
            .expect("foreign authority cannot allocate a pair");
        assert!(has_source::<GuardianRecoveryKeyError>(&failure));
        let (unrelated_private, unrelated_public) =
            effects.ed25519_generate_keypair().await.unwrap();
        let _unrelated_private = zeroize::Zeroizing::new(unrelated_private);
        let public_key =
            crate::handlers::recovery::recovery_guardian_public_key_storage_key(authority);
        effects
            .store(&public_key, unrelated_public.clone())
            .await
            .unwrap();
        let mismatch = guardian_recovery_keypair(&effects, authority)
            .await
            .err()
            .expect("mismatched original evidence cannot authorize replacement");
        let AgentError::Aura(aura_core::AuraError::Crypto {
            source: Some(source),
            ..
        }) = mismatch
        else {
            panic!("mismatched pair must retain typed cryptographic failure");
        };
        assert!(matches!(
            source.downcast_ref::<GuardianRecoveryKeyError>(),
            Some(GuardianRecoveryKeyError::Mismatch)
        ));
        assert_eq!(
            effects.retrieve(&public_key).await.unwrap().unwrap(),
            unrelated_public
        );
        let private_key =
            crate::handlers::recovery::recovery_guardian_private_key_storage_key(authority);
        assert_eq!(
            effects
                .retrieve(&private_key)
                .await
                .unwrap()
                .unwrap()
                .as_slice(),
            first.private.as_slice()
        );
    }

    #[tokio::test]
    async fn required_guardian_signer_and_verifier_preserve_native_failure() {
        let effects = crate::testing::simulation_effect_system_arc(&crate::AgentConfig::default());
        let payload = GuardianConfirmationPayload {
            invitation_id: InvitationId::new("required-guardian-source"),
            principal: effects.runtime_authority_id(),
            guardian: AuthorityId::new_from_entropy([244; 32]),
            expires_at: None,
            established: true,
        };
        let error = sign_guardian_transcript(&effects, &payload, &[0; 31])
            .await
            .unwrap_err();
        assert_native_crypto_input_failure(&error);
        let (private, public) = effects.ed25519_generate_keypair().await.unwrap();
        let private = zeroize::Zeroizing::new(private);
        let mut signature = sign_guardian_transcript(&effects, &payload, private.as_ref())
            .await
            .unwrap();
        assert!(
            verify_guardian_transcript(&effects, &payload, &signature, &public)
                .await
                .unwrap()
        );
        let failure = verify_guardian_transcript(&effects, &payload, &signature, &[0; 31])
            .await
            .unwrap_err();
        assert_native_crypto_input_failure(&failure);
        signature[0] ^= 1;
        assert!(
            !verify_guardian_transcript(&effects, &payload, &signature, &public)
                .await
                .unwrap()
        );
    }

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
