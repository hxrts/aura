//! Runtime-retained expected enrollment verifier, independent of peer response keys.

use crate::core::{AgentError, AgentResult};
use crate::runtime::AuraEffectSystem;
use aura_core::crypto::single_signer::SigningMode;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::{
    CryptoExtendedEffects, SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::hash::hash;
use aura_core::threshold::{SigningContext, ThresholdSignature};
use aura_core::{AuthorityId, CeremonyId, DeviceId};
use aura_invitation::enrollment_setup::DeviceEnrollmentSetupStatement;
use aura_signature::{threshold_signing_context_transcript_bytes, SecurityTranscript};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const VERSION: u16 = 1;
const MAX_RECORD_BYTES: usize = 131_072;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentVerifier {
    version: u16,
    subject: AuthorityId,
    ceremony: CeremonyId,
    pending_epoch: u64,
    initiator_device: DeviceId,
    setup_digest: [u8; 32],
    setup: DeviceEnrollmentSetupStatement,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentVerifierError {
    #[error("enrollment has no retained user-transferred verifier; legacy enrollment requires new setup transfer")]
    Missing,
    #[error("retained enrollment verifier record is oversized")]
    Oversized,
    #[error("retained enrollment verifier record binding is invalid")]
    RecordBinding,
    #[error("enrollment control belongs to another runtime owner")]
    RuntimeOwner,
    #[error("retained enrollment verifier already exists with another binding")]
    ExistingBinding,
    #[error("setup admission or invitation response deadline expired")]
    OutsideDeadline,
    #[error("enrollment verifier physical time failed")]
    Time(#[source] aura_core::effects::time::TimeError),
    #[error("enrollment response signer differs from the user-transferred setup verifier")]
    ProofBinding,
    #[error("enrollment response signature does not verify under the retained setup verifier")]
    InvalidSignature,
    #[error("retained enrollment verifier codec failed")]
    Codec(#[source] serde_json::Error),
    #[error("enrollment response transcript failed")]
    Transcript(#[source] aura_signature::AuthenticationError),
}

impl From<EnrollmentVerifierError> for AgentError {
    fn from(error: EnrollmentVerifierError) -> Self {
        aura_core::AuraError::crypto_with_source(
            "enrollment verifier validation failed",
            Arc::new(error),
        )
        .into()
    }
}

fn location(subject: AuthorityId, ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "enrollment_expected_verifier_v1",
        subject.to_string(),
        ceremony.to_string(),
    )
}

/// Only validated owner entry points in this module call this publisher.
/// Provider custody does not validate enrollment bindings; the caller does.
async fn publish_exact_immutable_enrollment_record(
    effects: &AuraEffectSystem,
    location: &SecureStorageLocation,
    bytes: &[u8],
) -> AgentResult<()> {
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    match effects
        .secure_store_immutable(
            location,
            bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?
    {
        aura_core::effects::secure::ImmutableSecureStoreOutcome::Created => Ok(()),
        aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists => {
            let original = effects
                .secure_retrieve(location, &[SecureStorageCapability::Read])
                .await?;
            if original.len() > MAX_RECORD_BYTES {
                return Err(EnrollmentVerifierError::Oversized.into());
            }
            if original != bytes {
                return Err(EnrollmentVerifierError::ExistingBinding.into());
            }
            Ok(())
        }
    }
}

/// Validate the independent original setup selected by protected allocation custody.
pub(crate) async fn validate_original_allocation_setup(
    effects: &AuraEffectSystem,
    original: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    setup_digest: [u8; 32],
) -> AgentResult<()> {
    let bytes = effects
        .secure_retrieve(
            &location(original.initiator_id, &original.ceremony_id),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let stored: StoredEnrollmentVerifier =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if stored.setup_digest != setup_digest
        || Some(stored.setup.device) != original.enrollment_device_id
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    RetainedEnrollmentVerifier::load(
        effects,
        original.initiator_id,
        &original.ceremony_id,
        original.new_epoch,
        effects.device_id(),
        stored.setup.authority,
        stored.setup.device,
    )
    .await?;
    Ok(())
}

/// Only the explicit app transfer token can authorize this runtime record.
pub(crate) async fn retain_user_transferred_verifier(
    effects: &AuraEffectSystem,
    subject: AuthorityId,
    ceremony: &CeremonyId,
    pending_epoch: u64,
    initiator_device: DeviceId,
    transferred: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
) -> AgentResult<()> {
    if initiator_device != effects.device_id() {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let record = StoredEnrollmentVerifier {
        version: VERSION,
        subject,
        ceremony: ceremony.clone(),
        pending_epoch,
        initiator_device,
        setup_digest: transferred.digest(),
        setup: transferred.statement().clone(),
    };
    let bytes = serde_json::to_vec(&record).map_err(EnrollmentVerifierError::Codec)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let key = location(subject, ceremony);
    if effects
        .secure_exists(&key)
        .await
        .map_err(AgentError::from)?
    {
        let retained = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await
            .map_err(AgentError::from)?;
        if retained != bytes {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        return Ok(());
    }
    let now = effects
        .physical_time()
        .await
        .map_err(EnrollmentVerifierError::Time)?
        .ts_ms;
    if now < record.setup.issued_at_ms || now >= record.setup.expires_at_ms {
        return Err(EnrollmentVerifierError::OutsideDeadline.into());
    }
    publish_exact_immutable_enrollment_record(effects, &key, &bytes).await
}

/// This handle is neither public nor deserializable and never comes from a peer.
pub(crate) struct RetainedEnrollmentVerifier(StoredEnrollmentVerifier);

/// Authenticated remote response; cannot be deserialized or constructed by callers.
#[derive(Debug, Clone)]
pub(crate) struct VerifiedEnrollmentResponse {
    ceremony: CeremonyId,
    invitation: aura_core::InvitationId,
    subject: AuthorityId,
    device: DeviceId,
    pending_epoch: u64,
    setup_digest: [u8; 32],
    acceptance: aura_invitation::protocol::DeviceEnrollmentAccept,
    canonical_invitation: super::Invitation,
    admitted_at_ms: u64,
}

impl VerifiedEnrollmentResponse {
    pub(crate) fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony
    }
    pub(crate) fn invitation_id(&self) -> aura_core::InvitationId {
        self.invitation.clone()
    }
    pub(crate) fn subject(&self) -> AuthorityId {
        self.subject
    }
    pub(crate) fn device_id(&self) -> DeviceId {
        self.device
    }
    pub(crate) fn pending_epoch(&self) -> u64 {
        self.pending_epoch
    }
    pub(crate) fn setup_digest(&self) -> [u8; 32] {
        self.setup_digest
    }
    pub(crate) fn acceptance(&self) -> &aura_invitation::protocol::DeviceEnrollmentAccept {
        &self.acceptance
    }
}

/// Only verification under the independently retained setup pin mints this
/// rejection capability. No JSON, transport source or local status constructor.
pub(crate) struct VerifiedEnrollmentRejectionCapability {
    ceremony: CeremonyId,
    subject: AuthorityId,
    device: DeviceId,
    pending_epoch: u64,
}
impl VerifiedEnrollmentRejectionCapability {
    pub(crate) fn ceremony_id(&self) -> &CeremonyId {
        &self.ceremony
    }
    pub(crate) fn subject(&self) -> AuthorityId {
        self.subject
    }
    pub(crate) fn device_id(&self) -> DeviceId {
        self.device
    }
    pub(crate) fn pending_epoch(&self) -> u64 {
        self.pending_epoch
    }
}

pub(crate) enum VerifiedEnrollmentResponseDispositionCapability {
    Accepted(Box<VerifiedEnrollmentResponse>),
    Refused(VerifiedEnrollmentRejectionCapability),
}
/// Loaded from the actual immutable issuer control and retained setup pin.
/// It owns the strongest canonical invitation for all subsequent wire validation.
pub(crate) struct PinnedEnrollmentResponseVerifierCapability {
    verifier: RetainedEnrollmentVerifier,
    canonical_invitation: super::Invitation,
    manifest_digest: [u8; 32],
}
impl PinnedEnrollmentResponseVerifierCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "pinned_enrollment_response_verifier",
        capability_type = PinnedEnrollmentResponseVerifierCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn acquire(
        effects: &AuraEffectSystem,
        retained: &RetainedEnrollmentVmControl,
    ) -> AgentResult<PinnedEnrollmentResponseVerifierCapability> {
        retained
            .require_runtime_owner(effects)
            .map_err(AgentError::from)?;
        let manifest = retained.manifest();
        let verifier = RetainedEnrollmentVerifier::load(
            effects,
            manifest.subject,
            &manifest.ceremony,
            manifest.pending_epoch,
            manifest.initiator_device,
            manifest.invitee_authority,
            manifest.invitee_device,
        )
        .await?;
        if verifier.0.setup_digest != manifest.setup.digest
            || verifier.0.setup.nonce != manifest.setup.nonce
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        Ok(Self {
            verifier,
            canonical_invitation: retained.canonical_invitation().clone(),
            manifest_digest: retained.digest(),
        })
    }

    /// Invalid unauthenticated packets are discarded without moving the VM or
    /// settling the owner. Required clock/crypto faults still fail with sources.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "pinned_enrollment_response_verifier",
        capability_type = VerifiedEnrollmentResponseDispositionCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn verify_received_response(
        &self,
        effects: &AuraEffectSystem,
        response: &aura_invitation::protocol::DeviceEnrollmentResponse,
    ) -> AgentResult<Option<VerifiedEnrollmentResponseDispositionCapability>> {
        let (binding, refused) = match response {
            aura_invitation::protocol::DeviceEnrollmentResponse::Accepted(binding) => {
                (binding, false)
            }
            aura_invitation::protocol::DeviceEnrollmentResponse::Refused(refusal) => {
                (&refusal.binding, true)
            }
        };
        let pin = &self.verifier.0;
        if binding.manifest_digest != Some(self.manifest_digest)
            || binding.invitation_id != self.canonical_invitation.invitation_id
            || binding.ceremony_id != pin.ceremony
            || binding.device_id != pin.setup.device
            || binding.acceptor_id != pin.setup.authority
            || binding.signature.signature.len() != 64
            || self.verifier.validate_proof(&binding.signature).is_err()
        {
            return Ok(None);
        }
        let now = effects
            .physical_time()
            .await
            .map_err(EnrollmentVerifierError::Time)?
            .ts_ms;
        if self.canonical_invitation.is_expired(now) {
            return Ok(None);
        }
        let transcript = super::DeviceEnrollmentAcceptanceTranscript {
            manifest_digest: self.manifest_digest,
            invitation: &self.canonical_invitation,
            acceptor_id: pin.setup.authority,
            subject_authority: pin.subject,
            ceremony_id: pin.ceremony.clone(),
            device_id: pin.setup.device,
        };
        let verified = if refused {
            self.verifier
                .verify_threshold_signing_context_transcript(
                    effects,
                    &super::DeviceEnrollmentRefusalTranscript(transcript),
                    &binding.signature,
                )
                .await
        } else {
            self.verifier
                .verify_threshold_signing_context_transcript(
                    effects,
                    &transcript,
                    &binding.signature,
                )
                .await
        };
        if let Err(error) = verified {
            use std::error::Error;
            let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
            while let Some(source) = cause {
                if let Some(validation) = source.downcast_ref::<EnrollmentVerifierError>() {
                    let unverified = match validation {
                        EnrollmentVerifierError::InvalidSignature
                        | EnrollmentVerifierError::ProofBinding => true,
                        EnrollmentVerifierError::Missing
                        | EnrollmentVerifierError::RuntimeOwner
                        | EnrollmentVerifierError::Oversized
                        | EnrollmentVerifierError::RecordBinding
                        | EnrollmentVerifierError::ExistingBinding
                        | EnrollmentVerifierError::OutsideDeadline
                        | EnrollmentVerifierError::Time(_)
                        | EnrollmentVerifierError::Codec(_)
                        | EnrollmentVerifierError::Transcript(_) => false,
                    };
                    if unverified {
                        return Ok(None);
                    }
                }
                cause = source.source();
            }
            return Err(error);
        }
        if refused {
            Ok(Some(
                VerifiedEnrollmentResponseDispositionCapability::Refused(
                    VerifiedEnrollmentRejectionCapability {
                        ceremony: pin.ceremony.clone(),
                        subject: pin.subject,
                        device: pin.setup.device,
                        pending_epoch: pin.pending_epoch,
                    },
                ),
            ))
        } else {
            Ok(Some(
                VerifiedEnrollmentResponseDispositionCapability::Accepted(Box::new(
                    VerifiedEnrollmentResponse {
                        ceremony: pin.ceremony.clone(),
                        invitation: self.canonical_invitation.invitation_id.clone(),
                        subject: pin.subject,
                        device: pin.setup.device,
                        pending_epoch: pin.pending_epoch,
                        setup_digest: pin.setup_digest,
                        acceptance: binding.clone(),
                        canonical_invitation: self.canonical_invitation.clone(),
                        admitted_at_ms: now,
                    },
                )),
            ))
        }
    }
}

impl RetainedEnrollmentVerifier {
    pub(crate) async fn load(
        effects: &AuraEffectSystem,
        subject: AuthorityId,
        ceremony: &CeremonyId,
        pending_epoch: u64,
        initiator_device: DeviceId,
        invitee_authority: AuthorityId,
        invitee_device: DeviceId,
    ) -> AgentResult<Self> {
        let key = location(subject, ceremony);
        if !effects
            .secure_exists(&key)
            .await
            .map_err(AgentError::from)?
        {
            return Err(EnrollmentVerifierError::Missing.into());
        }
        let bytes = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await
            .map_err(AgentError::from)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(EnrollmentVerifierError::Oversized.into());
        }
        let record: StoredEnrollmentVerifier =
            serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
        let digest = hash(
            &record
                .setup
                .transcript_bytes()
                .map_err(EnrollmentVerifierError::Transcript)?,
        );
        if record.version != VERSION || record.subject != subject || &record.ceremony != ceremony
            || record.pending_epoch != pending_epoch || record.initiator_device != initiator_device
            || record.setup.authority != invitee_authority || record.setup.device != invitee_device
            || record.setup_digest != digest
            || record.setup.version != aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::VERSION
            || record.setup.threshold == 0 || record.setup.threshold > record.setup.participants
            || initiator_device != effects.device_id()
            || record.setup.public_key_package.is_empty()
            || record.setup.public_key_package.len() > aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_PUBLIC_KEY_PACKAGE_BYTES
            || record.setup.expires_at_ms.checked_sub(record.setup.issued_at_ms).map_or(true, |lifetime| lifetime == 0 || lifetime > aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_VALIDITY_MS)
            || record.setup.issued_at_ms >= record.setup.expires_at_ms
            || match record.setup.signing_mode {
                SigningMode::SingleSigner => record.setup.threshold != 1 || record.setup.participants != 1,
                SigningMode::Threshold => record.setup.threshold < 2,
            }
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        Ok(Self(record))
    }

    fn validate_proof(&self, proof: &ThresholdSignature) -> Result<(), EnrollmentVerifierError> {
        let expected = &self.0.setup;
        let count = proof.signer_count;
        if proof.epoch != expected.signing_epoch
            || proof.public_key_package != expected.public_key_package
            || proof.signature.is_empty()
            || proof.signature.len() > aura_invitation::enrollment_setup::DeviceEnrollmentSetupRequest::MAX_SIGNATURE_BYTES
            || count < expected.threshold || count > expected.participants
            || proof.signers.len() != usize::from(count)
            || proof.signers.iter().any(|i| *i == 0 || *i > expected.participants)
            || proof.signers.windows(2).any(|w| w[0] >= w[1])
            || match expected.signing_mode {
                SigningMode::SingleSigner => count != 1 || proof.signers != [1],
                SigningMode::Threshold => count < 2,
            }
        {
            return Err(EnrollmentVerifierError::ProofBinding);
        }
        Ok(())
    }

    async fn verify_threshold_signing_context_transcript<T: SecurityTranscript + ?Sized>(
        &self,
        effects: &AuraEffectSystem,
        transcript: &T,
        proof: &ThresholdSignature,
    ) -> AgentResult<()> {
        self.validate_proof(proof)?;
        let payload = transcript
            .transcript_bytes()
            .map_err(EnrollmentVerifierError::Transcript)?;
        let context = SigningContext::message(
            self.0.setup.authority,
            T::DOMAIN_SEPARATOR.to_string(),
            payload,
        );
        let message =
            threshold_signing_context_transcript_bytes(&context, self.0.setup.signing_epoch)
                .map_err(EnrollmentVerifierError::Transcript)?;
        let verified = effects
            .verify_signature(
                &message,
                proof.signature_bytes(),
                &self.0.setup.public_key_package,
                self.0.setup.signing_mode,
            )
            .await
            .map_err(AgentError::from)?;
        if !verified {
            return Err(EnrollmentVerifierError::InvalidSignature.into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn verify_acceptance(
        &self,
        effects: &AuraEffectSystem,
        invitation: &super::Invitation,
        accept: &aura_invitation::protocol::DeviceEnrollmentAccept,
    ) -> AgentResult<VerifiedEnrollmentResponse> {
        let now = effects
            .physical_time()
            .await
            .map_err(EnrollmentVerifierError::Time)?
            .ts_ms;
        if invitation.is_expired(now) {
            // An admitted canonical decision remains idempotent after its live
            // window. A receipt grants no eligibility to a different decision.
            let (original, _) =
                recover_verified_response_receipt(effects, &self.0.ceremony).await?;
            let transcript = super::DeviceEnrollmentAcceptanceTranscript {
                manifest_digest: accept
                    .manifest_digest
                    .ok_or(EnrollmentVerifierError::RecordBinding)?,
                invitation,
                acceptor_id: accept.acceptor_id,
                subject_authority: self.0.subject,
                ceremony_id: self.0.ceremony.clone(),
                device_id: accept.device_id,
            };
            let original_transcript = super::DeviceEnrollmentAcceptanceTranscript {
                manifest_digest: original
                    .acceptance
                    .manifest_digest
                    .ok_or(EnrollmentVerifierError::RecordBinding)?,
                invitation: &original.canonical_invitation,
                acceptor_id: original.acceptance.acceptor_id,
                subject_authority: original.subject,
                ceremony_id: original.ceremony.clone(),
                device_id: original.device,
            };
            if transcript
                .transcript_bytes()
                .map_err(EnrollmentVerifierError::Transcript)?
                != original_transcript
                    .transcript_bytes()
                    .map_err(EnrollmentVerifierError::Transcript)?
            {
                return Err(EnrollmentVerifierError::ExistingBinding.into());
            }
            return self
                .verify_acceptance_at(effects, invitation, accept, original.admitted_at_ms)
                .await;
        }
        self.verify_acceptance_at(effects, invitation, accept, now)
            .await
    }

    // Only live effect time and secure receipt recovery may supply this time.
    async fn verify_acceptance_at(
        &self,
        effects: &AuraEffectSystem,
        invitation: &super::Invitation,
        accept: &aura_invitation::protocol::DeviceEnrollmentAccept,
        now: u64,
    ) -> AgentResult<VerifiedEnrollmentResponse> {
        if invitation.is_expired(now) {
            return Err(EnrollmentVerifierError::OutsideDeadline.into());
        }
        let manifest_digest =
            load_issued_enrollment_manifest_digest(effects, &self.0, invitation).await?;
        if accept.manifest_digest != Some(manifest_digest) {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let super::InvitationType::DeviceEnrollment {
            subject_authority,
            pending_epoch,
            initiator_device_id,
            device_id,
            ceremony_id,
            setup_binding,
            ..
        } = &invitation.invitation_type
        else {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        };
        if self.0.subject != *subject_authority
            || setup_binding.as_ref().map_or(true, |binding| {
                binding.digest != self.0.setup_digest || binding.nonce != self.0.setup.nonce
            })
            || self.0.pending_epoch != *pending_epoch
            || self.0.ceremony != *ceremony_id
            || self.0.initiator_device != *initiator_device_id
            || effects.device_id() != *initiator_device_id
            || invitation.sender_id != self.0.subject
            || invitation.receiver_id != self.0.setup.authority
            || *device_id != self.0.setup.device
            || accept.device_id != *device_id
            || accept.acceptor_id != self.0.setup.authority
            || accept.invitation_id != invitation.invitation_id
            || accept.ceremony_id != self.0.ceremony
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let transcript = super::DeviceEnrollmentAcceptanceTranscript {
            manifest_digest: accept
                .manifest_digest
                .ok_or(EnrollmentVerifierError::RecordBinding)?,
            invitation,
            acceptor_id: self.0.setup.authority,
            subject_authority: self.0.subject,
            ceremony_id: self.0.ceremony.clone(),
            device_id: self.0.setup.device,
        };
        self.verify_threshold_signing_context_transcript(effects, &transcript, &accept.signature)
            .await?;
        Ok(VerifiedEnrollmentResponse {
            ceremony: self.0.ceremony.clone(),
            invitation: invitation.invitation_id.clone(),
            subject: self.0.subject,
            device: self.0.setup.device,
            pending_epoch: self.0.pending_epoch,
            setup_digest: self.0.setup_digest,
            acceptance: accept.clone(),
            canonical_invitation: invitation.clone(),
            admitted_at_ms: now,
        })
    }
}

const RESPONSE_RECEIPT_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentRegistration {
    subject: AuthorityId,
    ceremony: CeremonyId,
    threshold: u16,
    total: u16,
    participants: Vec<aura_core::threshold::ParticipantIdentity>,
    accepted_existing: Vec<aura_core::threshold::ParticipantIdentity>,
    pending_epoch: u64,
    device: DeviceId,
    nickname: Option<String>,
    started_at: aura_core::time::PhysicalTime,
    timeout_ms: u64,
    budget: aura_core::TimeoutBudget,
}

impl StoredEnrollmentRegistration {
    fn from_state(
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    ) -> AgentResult<Self> {
        let device = state
            .enrollment_device_id
            .ok_or(EnrollmentVerifierError::RecordBinding)?;
        // This inventory is set-valued; signer indices remain in the pinned signing config.
        let canonical_inventory = |set: &std::collections::HashSet<
            aura_core::threshold::ParticipantIdentity,
        >|
         -> AgentResult<
            Vec<aura_core::threshold::ParticipantIdentity>,
        > {
            let mut entries = set
                .iter()
                .map(|identity| {
                    serde_json::to_vec(identity)
                        .map(|key| (key, identity.clone()))
                        .map_err(EnrollmentVerifierError::Codec)
                })
                .collect::<Result<Vec<_>, _>>()?;
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Ok(entries.into_iter().map(|(_, identity)| identity).collect())
        };
        Ok(Self {
            subject: state.initiator_id,
            ceremony: state.ceremony_id.clone(),
            threshold: state.threshold_k,
            total: state.total_n,
            participants: canonical_inventory(&state.participants)?,
            accepted_existing: canonical_inventory(&state.accepted_participants)?,
            pending_epoch: state.new_epoch,
            device,
            nickname: state.enrollment_nickname_suggestion.clone(),
            started_at: state.started_at.clone(),
            budget: state.timeout_budget.clone(),
            timeout_ms: u64::try_from(state.timeout.as_millis())
                .map_err(|_| EnrollmentVerifierError::RecordBinding)?,
        })
    }

    fn restore(
        self,
        proof: &VerifiedEnrollmentResponse,
        prestate_hash: aura_core::Hash32,
    ) -> AgentResult<crate::runtime::services::ceremony_tracker::TrackedCeremony> {
        self.restore_bound(
            proof.subject(),
            proof.ceremony_id(),
            proof.device_id(),
            proof.pending_epoch(),
            Some(proof.admitted_at_ms),
            prestate_hash,
        )
    }

    fn restore_bound(
        self,
        subject: AuthorityId,
        ceremony: &CeremonyId,
        device: DeviceId,
        epoch: u64,
        admitted_at_ms: Option<u64>,
        prestate_hash: aura_core::Hash32,
    ) -> AgentResult<crate::runtime::services::ceremony_tracker::TrackedCeremony> {
        use aura_core::threshold::ParticipantIdentity;
        let participants: std::collections::HashSet<_> =
            self.participants.iter().cloned().collect();
        let mut accepted: std::collections::HashSet<_> =
            self.accepted_existing.iter().cloned().collect();
        if self.subject != subject
            || &self.ceremony != ceremony
            || self.device != device
            || self.pending_epoch != epoch
            || self.threshold == 0
            || self.threshold > self.total
            || self.total == 0
            || self.total > 1024
            || participants.len() != usize::from(self.total)
            || participants.len() != self.participants.len()
            || accepted.len() != self.accepted_existing.len()
            || !accepted.is_subset(&participants)
            || !participants.contains(&ParticipantIdentity::device(self.device))
            || self.budget.started_at_ms() != self.started_at.ts_ms
            || self.budget.timeout_ms() != self.timeout_ms
            || self.timeout_ms == 0
            || self.timeout_ms > 86_400_000
            || self.nickname.as_ref().is_some_and(|name| name.len() > 4096)
            || admitted_at_ms.is_some_and(|at| {
                at < self.started_at.ts_ms
                    || at.saturating_sub(self.started_at.ts_ms) >= self.timeout_ms
            })
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        if admitted_at_ms.is_some() {
            accepted.insert(ParticipantIdentity::device(self.device));
        }
        Ok(
            crate::runtime::services::ceremony_tracker::TrackedCeremony {
                ceremony_id: self.ceremony,
                kind: aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
                initiator_id: self.subject,
                threshold_k: self.threshold,
                total_n: self.total,
                participants,
                accepted_participants: accepted,
                new_epoch: self.pending_epoch,
                enrollment_device_id: Some(self.device),
                enrollment_nickname_suggestion: self.nickname,
                started_at: self.started_at,
                has_failed: false,
                is_committed: false,
                is_superseded: false,
                superseded_by: None,
                supersedes: Vec::new(),
                agreement_mode: aura_core::threshold::policy_for(
                    aura_core::threshold::CeremonyFlow::DeviceEnrollment,
                )
                .initial_mode(),
                error_message: None,
                terminal_outcome: None,
                failure_reason: None,
                timeout: std::time::Duration::from_millis(self.timeout_ms),
                timeout_budget: self.budget,
                enrollment_window_lease: Arc::new(tokio::sync::Semaphore::new(1)),
                prestate_hash,
                committed_at: None,
                committed_consensus_id: None,
            },
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentResponseReceipt {
    version: u16,
    prestate_hash: aura_core::Hash32,
    registration: StoredEnrollmentRegistration,
    pending_generation: StoredPendingGeneration,
    setup_digest: [u8; 32],
    admitted_at_ms: u64,
    invitation: super::Invitation,
    acceptance: aura_invitation::protocol::DeviceEnrollmentAccept,
}

fn response_receipt_location(ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new(
        "device_enrollment_response_receipt_v1",
        ceremony.to_string(),
    )
}

/// Called only while the tracker holds its serialized enrollment decision gate.
/// The first canonical decision is immutable; alternate valid signature bytes
/// cannot overwrite it or change the original admission time.
pub(crate) async fn persist_verified_response_receipt(
    effects: &AuraEffectSystem,
    evidence: &VerifiedEnrollmentResponse,
    registration: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
) -> AgentResult<()> {
    let prestate_hash = registration.prestate_hash;
    let location = response_receipt_location(evidence.ceremony_id());
    if effects.secure_exists(&location).await? {
        let (original, original_prestate) =
            recover_verified_response_receipt(effects, evidence.ceremony_id()).await?;
        if original_prestate.prestate_hash != prestate_hash
            || original.invitation_id() != evidence.invitation_id()
            || original.setup_digest() != evidence.setup_digest()
            || original.subject() != evidence.subject()
            || original.device_id() != evidence.device_id()
            || original.pending_epoch() != evidence.pending_epoch()
            || original.acceptance().acceptor_id != evidence.acceptance().acceptor_id
        {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        return Ok(());
    }
    let generation_bytes = effects
        .secure_retrieve(
            &pending_generation_location(evidence.ceremony_id()),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if generation_bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let generation: StoredPendingGeneration =
        serde_json::from_slice(&generation_bytes).map_err(EnrollmentVerifierError::Codec)?;
    if generation.version != 1
        || &generation.ceremony != evidence.ceremony_id()
        || generation.prestate != registration.prestate_hash
        || generation.authority != evidence.subject()
        || generation.epoch != evidence.pending_epoch()
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let raw = StoredEnrollmentResponseReceipt {
        version: RESPONSE_RECEIPT_VERSION,
        prestate_hash,
        registration: StoredEnrollmentRegistration::from_state(registration)?,
        pending_generation: generation,
        setup_digest: evidence.setup_digest(),
        admitted_at_ms: evidence.admitted_at_ms,
        invitation: evidence.canonical_invitation.clone(),
        acceptance: evidence.acceptance().clone(),
    };
    raw.registration.clone().restore(evidence, prestate_hash)?;
    let bytes = serde_json::to_vec(&raw).map_err(EnrollmentVerifierError::Codec)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    let inject_receipt_failure = {
        let owner = (
            effects.config().storage.base_path.clone(),
            evidence.ceremony_id().clone(),
        );
        receipt_store_failures().lock().await.remove(&owner)
    };
    #[cfg(all(test, not(target_arch = "wasm32")))]
    if inject_receipt_failure {
        return Err(AgentError::from(aura_core::AuraError::Storage {
            message: "injected required receipt store failure".into(),
            source: Some(std::sync::Arc::new(
                aura_core::effects::storage::StorageError::WriteFailed(
                    "receipt write fault".into(),
                ),
            )),
        }));
    }
    effects
        .secure_store_immutable(
            &location,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    {
        let (original, original_prestate) =
            recover_verified_response_receipt(effects, evidence.ceremony_id()).await?;
        if original_prestate.prestate_hash != prestate_hash
            || original.invitation_id() != evidence.invitation_id()
            || original.setup_digest() != evidence.setup_digest()
            || original.subject() != evidence.subject()
            || original.device_id() != evidence.device_id()
            || original.pending_epoch() != evidence.pending_epoch()
            || original.acceptance().acceptor_id != evidence.acceptance().acceptor_id
        {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        Ok(())
    }
}

/// Secure-storage bytes are decoded as raw evidence, never as a sealed witness.
/// Cryptography and the original retained verifier are checked again before
/// returning a capability. The saved admission time is local receipt metadata,
/// not a new caller-selected clock or a consensus fact.
pub(crate) async fn recover_verified_response_receipt(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
) -> AgentResult<(
    VerifiedEnrollmentResponse,
    crate::runtime::services::ceremony_tracker::TrackedCeremony,
)> {
    let bytes = effects
        .secure_retrieve(
            &response_receipt_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let raw: StoredEnrollmentResponseReceipt =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if raw.version != RESPONSE_RECEIPT_VERSION || &raw.acceptance.ceremony_id != ceremony {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let super::InvitationType::DeviceEnrollment {
        subject_authority,
        pending_epoch,
        initiator_device_id,
        device_id,
        ceremony_id,
        ..
    } = &raw.invitation.invitation_type
    else {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    };
    if ceremony_id != ceremony {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let expected = RetainedEnrollmentVerifier::load(
        effects,
        *subject_authority,
        ceremony,
        *pending_epoch,
        *initiator_device_id,
        raw.invitation.receiver_id,
        *device_id,
    )
    .await?;
    let verified = expected
        .verify_acceptance_at(
            effects,
            &raw.invitation,
            &raw.acceptance,
            raw.admitted_at_ms,
        )
        .await?;
    if verified.setup_digest() != raw.setup_digest {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let generation_bytes = effects
        .secure_retrieve(
            &pending_generation_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if generation_bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let current: StoredPendingGeneration =
        serde_json::from_slice(&generation_bytes).map_err(EnrollmentVerifierError::Codec)?;
    if current.version != 1
        || &current.ceremony != ceremony
        || current.prestate != raw.prestate_hash
        || current.authority != verified.subject()
        || current.epoch != verified.pending_epoch()
        || current.package_digest != raw.pending_generation.package_digest
        || current.config_digest != raw.pending_generation.config_digest
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let state = raw.registration.restore(&verified, raw.prestate_hash)?;

    Ok((verified, state))
}

/// Used to prohibit ceremony-ID reuse after registry cleanup or restart.
pub(crate) async fn has_response_receipt(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
) -> AgentResult<bool> {
    effects
        .secure_exists(&response_receipt_location(ceremony))
        .await
        .map_err(AgentError::from)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod real_crypto_tests {
    use super::*;
    use crate::core::AgentConfig;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use crate::{AgentBuilder, AuraAgent};
    use aura_app::runtime_bridge::RuntimeBridge;
    use aura_core::context::EffectContext;
    use aura_core::effects::{ExecutionMode, ThresholdSigningEffects};
    use aura_core::ContextId;

    struct AcceptanceProbe;
    impl SecurityTranscript for AcceptanceProbe {
        type Payload = &'static str;
        const DOMAIN_SEPARATOR: &'static str = "aura.test.retained-enrollment-response";
        fn transcript_payload(&self) -> Self::Payload {
            "accepted exact enrollment"
        }
    }

    async fn runtime(authority: AuthorityId, config: AgentConfig) -> Arc<AuraAgent> {
        let context = EffectContext::new(
            authority,
            ContextId::new_from_entropy([117; 32]),
            ExecutionMode::Testing,
        );
        let agent = Arc::new(
            AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("build actual runtime"),
        );
        AgentRuntimeBridge::new(agent.clone())
            .bootstrap_signing_keys()
            .await
            .expect("bootstrap retained signing identity");
        agent
    }

    async fn fixture() -> (
        Arc<AuraAgent>,
        Arc<AuraAgent>,
        AgentConfig,
        aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) {
        let issuer_config = AgentConfig {
            device_id: DeviceId::new_from_entropy([113; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::Builder::new()
                    .prefix("aura-retained-verifier-issuer-")
                    .tempdir()
                    .expect("issuer storage root")
                    .keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let invitee_config = AgentConfig {
            device_id: DeviceId::new_from_entropy([114; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::Builder::new()
                    .prefix("aura-retained-verifier-invitee-")
                    .tempdir()
                    .expect("invitee storage root")
                    .keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let issuer = runtime(AuthorityId::new_from_entropy([115; 32]), issuer_config).await;
        let invitee = runtime(
            AuthorityId::new_from_entropy([116; 32]),
            invitee_config.clone(),
        )
        .await;
        let code = AgentRuntimeBridge::new(invitee.clone())
            .export_device_enrollment_setup_request()
            .await
            .expect("actual invitee export");
        let app = Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                Arc::new(AgentRuntimeBridge::new(issuer.clone())),
            )
            .expect("app runtime"),
        ));
        let transferred =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &app, code,
            )
            .await
            .expect("explicit user transfer through app");
        (issuer, invitee, invitee_config, transferred)
    }

    fn run<F: std::future::Future<Output = ()> + Send + 'static>(future: F) {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("create enrollment test runtime")
                    .block_on(future);
            })
            .expect("spawn enrollment test thread")
            .join()
            .expect("join enrollment test thread");
    }

    #[test]
    fn actual_immutable_setup_owner_publication_preserves_first_binding() {
        run(async {
            let (issuer, _invitee, _config, transferred) = fixture().await;
            let effects = issuer.runtime().effects();
            let subject = issuer.authority_id();
            let ceremony = CeremonyId::new("immutable-real-provider-binding");
            let retain = || {
                retain_user_transferred_verifier(
                    effects.as_ref(),
                    subject,
                    &ceremony,
                    1,
                    effects.device_id(),
                    &transferred,
                )
            };
            let (first, duplicate) = futures::join!(retain(), retain());
            first.expect("actual first immutable admission");
            duplicate.expect("same actual setup binding is idempotent");
            let key = location(subject, &ceremony);
            let original = effects
                .secure_retrieve(&key, &[SecureStorageCapability::Read])
                .await
                .expect("read original admitted bytes");
            retain_user_transferred_verifier(
                effects.as_ref(),
                subject,
                &ceremony,
                2,
                effects.device_id(),
                &transferred,
            )
            .await
            .expect_err("different generation cannot overwrite first binding");
            assert_eq!(
                effects
                    .secure_retrieve(&key, &[SecureStorageCapability::Read])
                    .await
                    .expect("read retained original bytes"),
                original
            );
        });
    }

    #[test]
    fn exported_transferred_retained_verifier_checks_actual_crypto_and_recreated_signer() {
        run(async {
            let (issuer, invitee, invitee_config, transferred) = fixture().await;
            let ceremony = CeremonyId::new("retained-real-crypto");
            let subject = issuer.authority_id();
            let effects = issuer.runtime().effects();
            retain_user_transferred_verifier(
                effects.as_ref(),
                subject,
                &ceremony,
                1,
                effects.device_id(),
                &transferred,
            )
            .await
            .unwrap();
            let load = || {
                RetainedEnrollmentVerifier::load(
                    effects.as_ref(),
                    subject,
                    &ceremony,
                    1,
                    effects.device_id(),
                    transferred.statement().authority,
                    transferred.statement().device,
                )
            };
            let expected = load().await.unwrap();
            let context = SigningContext::message(
                invitee.authority_id(),
                AcceptanceProbe::DOMAIN_SEPARATOR.to_string(),
                AcceptanceProbe.transcript_bytes().unwrap(),
            );
            let proof = invitee
                .runtime()
                .effects()
                .sign(context.clone())
                .await
                .expect("actual provisional signer");
            expected
                .verify_threshold_signing_context_transcript(
                    effects.as_ref(),
                    &AcceptanceProbe,
                    &proof,
                )
                .await
                .unwrap();
            let mut corrupt = proof.clone();
            corrupt.signature[0] ^= 1;
            assert!(expected
                .verify_threshold_signing_context_transcript(
                    effects.as_ref(),
                    &AcceptanceProbe,
                    &corrupt
                )
                .await
                .is_err());
            let mut substitute = proof.clone();
            substitute.public_key_package[0] ^= 1;
            assert!(expected
                .verify_threshold_signing_context_transcript(
                    effects.as_ref(),
                    &AcceptanceProbe,
                    &substitute
                )
                .await
                .is_err());
            let invitee_authority = invitee.authority_id();
            // An owned profile stays leased until its runtime shuts down.
            let Ok(invitee) = Arc::try_unwrap(invitee) else {
                panic!("invitee runtime must be unshared before reopening its profile");
            };
            invitee
                .shutdown(&EffectContext::new(
                    invitee_authority,
                    ContextId::new_from_entropy([117; 32]),
                    ExecutionMode::Testing,
                ))
                .await
                .expect("shut down the original invitee runtime");
            let recreated = runtime(invitee_authority, invitee_config).await;
            let recovered = recreated
                .runtime()
                .effects()
                .sign(context)
                .await
                .expect("recreated retained provisional signer");
            load()
                .await
                .unwrap()
                .verify_threshold_signing_context_transcript(
                    effects.as_ref(),
                    &AcceptanceProbe,
                    &recovered,
                )
                .await
                .unwrap();
            assert_eq!(
                recovered.public_key_package,
                transferred.statement().public_key_package
            );
            // Runtime recreation is not OS-process restart or actual authority adoption evidence.
        });
    }

    #[test]
    fn missing_corrupt_and_wrong_physical_owner_records_fail_closed_after_real_transfer() {
        run(async {
            let (issuer, _invitee, _config, transferred) = fixture().await;
            let subject = issuer.authority_id();
            let effects = issuer.runtime().effects();
            let ceremony = CeremonyId::new("retained-negative-storage");
            let load = || {
                RetainedEnrollmentVerifier::load(
                    effects.as_ref(),
                    subject,
                    &ceremony,
                    1,
                    effects.device_id(),
                    transferred.statement().authority,
                    transferred.statement().device,
                )
            };
            assert!(
                load().await.is_err(),
                "legacy missing record cannot authorize peer signer"
            );
            retain_user_transferred_verifier(
                effects.as_ref(),
                subject,
                &ceremony,
                1,
                effects.device_id(),
                &transferred,
            )
            .await
            .unwrap();
            assert!(load().await.is_ok());
            assert!(RetainedEnrollmentVerifier::load(
                effects.as_ref(),
                subject,
                &ceremony,
                1,
                DeviceId::new_from_entropy([118; 32]),
                transferred.statement().authority,
                transferred.statement().device
            )
            .await
            .is_err());
            // Retained trust is immutable: corrupt its backing ciphertext instead.
            effects
                .fault_corrupt_secure_record_for_test(&location(subject, &ceremony))
                .await
                .unwrap();
            assert!(
                load().await.is_err(),
                "corrupt storage cannot fall back to embedded response key"
            );
            assert!(
                retain_user_transferred_verifier(
                    effects.as_ref(),
                    subject,
                    &ceremony,
                    1,
                    effects.device_id(),
                    &transferred
                )
                .await
                .is_err(),
                "corrupt retained trust is not overwritten"
            );
        });
    }

    #[test]
    fn actual_issued_invitation_mints_witness_only_for_exact_pinned_acceptance() {
        run(async {
            let (issuer, _invitee, invitation, start, accept, witness) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "manifest-verifier",
                )
                .await;
            let effects = issuer.runtime().effects();
            let expected = RetainedEnrollmentVerifier::load(
                effects.as_ref(),
                issuer.authority_id(),
                &start.ceremony_id,
                start.pending_epoch.value(),
                issuer.context().device_id(),
                invitation.receiver_id,
                start.device_id,
            )
            .await
            .unwrap();
            assert_eq!(witness.ceremony_id(), &start.ceremony_id);
            assert_eq!(witness.invitation_id(), invitation.invitation_id);
            assert!(accept.manifest_digest.is_some());
            let mut substitute = accept.clone();
            substitute.signature.public_key_package[0] ^= 1;
            assert!(expected
                .verify_acceptance(effects.as_ref(), &invitation, &substitute)
                .await
                .is_err());
            let mut missing_manifest = accept.clone();
            missing_manifest.manifest_digest = None;
            assert!(expected
                .verify_acceptance(effects.as_ref(), &invitation, &missing_manifest)
                .await
                .is_err());
            let mut other_manifest = accept.clone();
            other_manifest.manifest_digest.as_mut().unwrap()[0] ^= 1;
            assert!(expected
                .verify_acceptance(effects.as_ref(), &invitation, &other_manifest)
                .await
                .is_err());
            let mut wrong_invitation = invitation.clone();
            wrong_invitation.invitation_id = aura_core::InvitationId::new("other invitation");
            assert!(expected
                .verify_acceptance(effects.as_ref(), &wrong_invitation, &accept)
                .await
                .is_err());
        });
    }
}

/// Test fixture signs the real canonical response under the provisional runtime.
/// It cannot bypass the retained verifier or manufacture a sealed witness.
#[cfg(test)]
pub(crate) async fn verify_actual_invitee_acceptance_for_test(
    issuer: &AuraEffectSystem,
    invitee: &AuraEffectSystem,
    invitation: &aura_invitation::Invitation,
    subject: AuthorityId,
    ceremony: &CeremonyId,
    device: DeviceId,
    pending_epoch: u64,
) -> AgentResult<VerifiedEnrollmentResponse> {
    use aura_core::effects::ThresholdSigningEffects;
    use aura_signature::SecurityTranscript;
    let expected = RetainedEnrollmentVerifier::load(
        issuer,
        subject,
        ceremony,
        pending_epoch,
        issuer.device_id(),
        invitation.receiver_id,
        device,
    )
    .await?;
    let admission = super::enrollment_manifest_admission::load_admitted_baseline(
        invitee,
        invitation.receiver_id,
        invitation,
    )
    .await
    .map_err(manifest_error)?;
    let manifest_digest = admission.manifest_digest();
    let transcript = super::DeviceEnrollmentAcceptanceTranscript {
        manifest_digest,
        invitation,
        acceptor_id: invitation.receiver_id,
        subject_authority: subject,
        ceremony_id: ceremony.clone(),
        device_id: device,
    };
    let context = aura_core::threshold::SigningContext::message(
        invitation.receiver_id,
        super::DeviceEnrollmentAcceptanceTranscript::DOMAIN_SEPARATOR.to_string(),
        transcript
            .transcript_bytes()
            .map_err(EnrollmentVerifierError::Transcript)?,
    );
    let signature = invitee.sign(context).await?;
    let acceptance = aura_invitation::protocol::DeviceEnrollmentAccept {
        invitation_id: invitation.invitation_id.clone(),
        ceremony_id: ceremony.clone(),
        device_id: device,
        acceptor_id: invitation.receiver_id,
        signature,
        manifest_digest: Some(manifest_digest),
    };
    expected
        .verify_acceptance(issuer, invitation, &acceptance)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (RetainedEnrollmentVerifier, ThresholdSignature) {
        let setup = DeviceEnrollmentSetupStatement {
            version: 1,
            authority: AuthorityId::new_from_entropy([101; 32]),
            device: DeviceId::new_from_entropy([102; 32]),
            nonce: [103; 32],
            issued_at_ms: 100,
            expires_at_ms: 200,
            signing_epoch: 7,
            signing_mode: SigningMode::SingleSigner,
            threshold: 1,
            participants: 1,
            public_key_package: vec![104; 32],
        };
        let proof =
            ThresholdSignature::single_signer(vec![105; 64], setup.public_key_package.clone(), 7);
        let verifier = RetainedEnrollmentVerifier(StoredEnrollmentVerifier {
            version: VERSION,
            subject: AuthorityId::new_from_entropy([106; 32]),
            ceremony: CeremonyId::new("retained-verifier-test"),
            pending_epoch: 3,
            initiator_device: DeviceId::new_from_entropy([107; 32]),
            setup_digest: hash(&setup.transcript_bytes().unwrap()),
            setup,
        });
        (verifier, proof)
    }

    #[test]
    fn retained_verifier_rejects_response_key_epoch_and_signer_substitution() {
        let (expected, proof) = fixture();
        // Shape acceptance alone is not cryptographic verification.
        expected.validate_proof(&proof).unwrap();
        let mut substitutes = Vec::new();
        let mut changed = proof.clone();
        changed.public_key_package[0] ^= 1;
        substitutes.push(changed);
        let mut changed = proof.clone();
        changed.epoch += 1;
        substitutes.push(changed);
        let mut changed = proof.clone();
        changed.signers = vec![2];
        substitutes.push(changed);
        let mut changed = proof.clone();
        changed.signer_count = 2;
        changed.signers = vec![1, 2];
        substitutes.push(changed);
        let mut changed = proof.clone();
        changed.signature.clear();
        substitutes.push(changed);
        for substitute in substitutes {
            assert!(matches!(
                expected.validate_proof(&substitute),
                Err(EnrollmentVerifierError::ProofBinding)
            ));
        }
    }

    #[test]
    fn retained_threshold_verifier_rejects_single_signer_and_duplicate_inventory() {
        let (mut expected, proof) = fixture();
        expected.0.setup.signing_mode = SigningMode::Threshold;
        expected.0.setup.threshold = 2;
        expected.0.setup.participants = 3;
        assert!(expected.validate_proof(&proof).is_err());
        let mut duplicate = proof;
        duplicate.signer_count = 2;
        duplicate.signers = vec![1, 1];
        assert!(expected.validate_proof(&duplicate).is_err());
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPendingGeneration {
    version: u16,
    authority: AuthorityId,
    ceremony: CeremonyId,
    prestate: aura_core::Hash32,
    epoch: u64,
    package_digest: [u8; 32],
    config_digest: [u8; 32],
}
fn pending_generation_location(ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new(
        "device_enrollment_pending_generation_v1",
        ceremony.to_string(),
    )
}

pub(crate) async fn retain_pending_signing_generation(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
    generation: &crate::runtime::services::threshold_signing::VerifiedPendingSigningGeneration,
    prestate: aura_core::Hash32,
) -> AgentResult<()> {
    let raw = StoredPendingGeneration {
        version: 1,
        ceremony: ceremony.clone(),
        prestate,
        authority: generation.authority(),
        epoch: generation.epoch(),
        package_digest: generation.package_digest(),
        config_digest: generation.config_digest(),
    };
    let bytes = serde_json::to_vec(&raw).map_err(EnrollmentVerifierError::Codec)?;
    let key = pending_generation_location(ceremony);
    if effects.secure_exists(&key).await? {
        if effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?
            != bytes
        {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        return Ok(());
    }
    publish_exact_immutable_enrollment_record(effects, &key, &bytes).await?;
    Ok(())
}

pub(crate) async fn restore_pending_signing_generation(
    effects: &AuraEffectSystem,
    signing: &crate::runtime::services::threshold_signing::ThresholdSigningService,
    ceremony: &CeremonyId,
    authority: AuthorityId,
    epoch: u64,
    prestate: aura_core::Hash32,
) -> AgentResult<crate::runtime::services::threshold_signing::VerifiedPendingSigningGeneration> {
    let bytes = effects
        .secure_retrieve(
            &pending_generation_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let raw: StoredPendingGeneration =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if raw.version != 1
        || &raw.ceremony != ceremony
        || raw.prestate != prestate
        || raw.authority != authority
        || raw.epoch != epoch
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    signing
        .verify_retained_pending_generation(
            &authority,
            epoch,
            raw.package_digest,
            raw.config_digest,
        )
        .await
        .map_err(AgentError::from)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPendingEnrollment {
    version: u16,
    registration: StoredEnrollmentRegistration,
    prestate: aura_core::Hash32,
    invitation: super::Invitation,
}
fn pending_registration_location(ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new("device_enrollment_registration_v1", ceremony.to_string())
}

/// Live publication requires the still-held physical generation reservation.
pub(crate) async fn persist_pending_enrollment_registration(
    effects: &AuraEffectSystem,
    generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
    state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    invitation: &super::Invitation,
) -> AgentResult<()> {
    generation.validate_pending_registration(effects, state, invitation)?;
    let allocated = recover_allocated_enrollment_registration(effects, &state.ceremony_id).await?;
    if state.started_at != allocated.started_at
        || state.timeout != allocated.timeout
        || state.enrollment_nickname_suggestion != allocated.enrollment_nickname_suggestion
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    state
        .timeout_budget
        .validate_checkpoint_continuation_from(&allocated.timeout_budget)
        .map_err(aura_core::AuraError::from)?;

    persist_validated_pending_enrollment_registration(effects, state, invitation).await
}

/// Private storage path shared only by held live admission and validated recovery.
async fn persist_validated_pending_enrollment_registration(
    effects: &AuraEffectSystem,
    state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    invitation: &super::Invitation,
) -> AgentResult<()> {
    let raw = StoredPendingEnrollment {
        version: 1,
        registration: StoredEnrollmentRegistration::from_state(state)?,
        prestate: state.prestate_hash,
        invitation: invitation.clone(),
    };
    raw.registration.clone().restore_bound(
        state.initiator_id,
        &state.ceremony_id,
        state
            .enrollment_device_id
            .ok_or(EnrollmentVerifierError::RecordBinding)?,
        state.new_epoch,
        None,
        state.prestate_hash,
    )?;
    let bytes = serde_json::to_vec(&raw).map_err(EnrollmentVerifierError::Codec)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let location = pending_registration_location(&state.ceremony_id);
    if effects.secure_exists(&location).await? {
        if effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?
            != bytes
        {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        return Ok(());
    }
    publish_exact_immutable_enrollment_record(effects, &location, &bytes).await?;
    Ok(())
}

pub(crate) async fn recover_pending_enrollment_registration(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
) -> AgentResult<crate::runtime::services::ceremony_tracker::TrackedCeremony> {
    let bytes = effects
        .secure_retrieve(
            &pending_registration_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let raw: StoredPendingEnrollment =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    let super::InvitationType::DeviceEnrollment {
        subject_authority,
        ceremony_id,
        device_id,
        pending_epoch,
        initiator_device_id,
        setup_binding,
        ..
    } = &raw.invitation.invitation_type
    else {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    };
    let retained = RetainedEnrollmentVerifier::load(
        effects,
        *subject_authority,
        ceremony,
        *pending_epoch,
        *initiator_device_id,
        raw.invitation.receiver_id,
        *device_id,
    )
    .await?;
    if raw.version != 1
        || ceremony_id != ceremony
        || setup_binding.as_ref().map_or(true, |binding| {
            binding.nonce != retained.0.setup.nonce || binding.digest != retained.0.setup_digest
        })
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    raw.registration.restore_bound(
        *subject_authority,
        ceremony,
        *device_id,
        *pending_epoch,
        None,
        raw.prestate,
    )
}
#[cfg(all(test, not(target_arch = "wasm32")))]
fn receipt_store_failures(
) -> &'static async_lock::Mutex<std::collections::HashSet<(std::path::PathBuf, CeremonyId)>> {
    static FAILURES: std::sync::OnceLock<
        async_lock::Mutex<std::collections::HashSet<(std::path::PathBuf, CeremonyId)>>,
    > = std::sync::OnceLock::new();
    FAILURES.get_or_init(Default::default)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) async fn fail_next_receipt_store_for_test(
    effects: &AuraEffectSystem,
    ceremony: CeremonyId,
) {
    let owner = (effects.config().storage.base_path.clone(), ceremony);
    receipt_store_failures().lock().await.insert(owner);
}

/// Immutable expected issuer artifact. It is selected by secure owner storage,
/// never by an acceptance message or by an artifact embedded in a receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIssuedEnrollmentManifest {
    version: u16,
    signed_code: String,
    digest: [u8; 32],
    confirmation_verifier: Vec<u8>,
}

/// Initial owned-storage selector. It grants no terminal authority; required
/// sender hydration and the independent retained verifier must still agree.
pub(crate) struct RequiredIssuedEnrollmentSelectorCapability {
    runtime_owner: Arc<AuraEffectSystem>,
    manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    digest: [u8; 32],
}
impl RequiredIssuedEnrollmentSelectorCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "required_issued_enrollment_selector",
        capability_type = RequiredIssuedEnrollmentSelectorCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn load(
        effects: Arc<AuraEffectSystem>,
        ceremony: &CeremonyId,
    ) -> AgentResult<RequiredIssuedEnrollmentSelectorCapability> {
        use aura_invitation::enrollment_manifest::{
            EnrollmentTrustManifest, SignedEnrollmentTrustManifest,
        };
        let subject = effects.runtime_authority_id();
        let key = issued_manifest_location(subject, ceremony);
        // Absence is an error, never permission to derive an invitation ID.
        let bytes = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > EnrollmentTrustManifest::MAX_BYTES * 2 {
            return Err(EnrollmentVerifierError::Oversized.into());
        }
        let stored: StoredIssuedEnrollmentManifest =
            serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
        let digest = verify_stored_issued_manifest(&effects, &stored, subject, ceremony).await?;
        let manifest = SignedEnrollmentTrustManifest::decode(&stored.signed_code)
            .map_err(manifest_error)?
            .manifest;
        Ok(Self {
            runtime_owner: effects,
            manifest,
            digest,
        })
    }

    pub(crate) fn runtime_owner(&self) -> Arc<AuraEffectSystem> {
        self.runtime_owner.clone()
    }

    pub(crate) fn invitation_id(&self) -> &aura_core::InvitationId {
        &self.manifest.invitation
    }

    pub(crate) fn require_control(&self, control: &RetainedEnrollmentVmControl) -> AgentResult<()> {
        control.require_runtime_owner(&self.runtime_owner)?;
        if control.digest() != self.digest
            || control.manifest().subject != self.manifest.subject
            || control.manifest().ceremony != self.manifest.ceremony
            || control.canonical_invitation().invitation_id != self.manifest.invitation
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        Ok(())
    }
}

fn issued_manifest_location(subject: AuthorityId, ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "device_enrollment_issued_manifest_v1",
        subject.to_string(),
        ceremony.to_string(),
    )
}

fn manifest_error(
    error: aura_invitation::enrollment_manifest::EnrollmentManifestError,
) -> AgentError {
    aura_core::AuraError::crypto_with_source(
        "issued enrollment manifest verification failed",
        Arc::new(error),
    )
    .into()
}

pub(crate) async fn retain_issued_enrollment_manifest(
    effects: &AuraEffectSystem,
    issued: &crate::handlers::invitation_service::IssuedEnrollmentManifestBinding,
) -> AgentResult<()> {
    issued.require_effects(effects)?;
    let manifest = issued.manifest();
    if manifest.initiator_device != effects.device_id()
        || manifest.initiator_confirmation_verifier != issued.confirmation_verifier()
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let generation_bytes = effects
        .secure_retrieve(
            &pending_generation_location(&manifest.ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if generation_bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let generation: StoredPendingGeneration =
        serde_json::from_slice(&generation_bytes).map_err(EnrollmentVerifierError::Codec)?;
    if generation.version != 1
        || generation.authority != manifest.subject
        || generation.ceremony != manifest.ceremony
        || generation.epoch != manifest.pending_epoch
        || generation.package_digest != manifest.pending_public_key_package_digest
        || generation.config_digest != *manifest.pending_threshold_config_digest.as_bytes()
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let stored = StoredIssuedEnrollmentManifest {
        version: 1,
        signed_code: issued.signed_code().to_owned(),
        digest: issued.digest(),
        confirmation_verifier: issued.confirmation_verifier().to_vec(),
    };
    let checked =
        verify_stored_issued_manifest(effects, &stored, manifest.subject, &manifest.ceremony)
            .await?;
    if checked != stored.digest {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let bytes = serde_json::to_vec(&stored).map_err(EnrollmentVerifierError::Codec)?;
    if bytes.len() > aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_BYTES * 2 {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let key = issued_manifest_location(manifest.subject, &manifest.ceremony);
    effects
        .secure_store_immutable(
            &key,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    {
        let old = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if old.len() > aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_BYTES * 2
        {
            return Err(EnrollmentVerifierError::Oversized.into());
        }
        let original: StoredIssuedEnrollmentManifest =
            serde_json::from_slice(&old).map_err(EnrollmentVerifierError::Codec)?;
        // Different valid signature bytes do not alter the canonical decision.
        if original.digest != stored.digest
            || original.confirmation_verifier != stored.confirmation_verifier
        {
            return Err(EnrollmentVerifierError::ExistingBinding.into());
        }
        verify_stored_issued_manifest(effects, &original, manifest.subject, &manifest.ceremony)
            .await?;
        Ok(())
    }
}

async fn verify_stored_issued_manifest(
    effects: &AuraEffectSystem,
    stored: &StoredIssuedEnrollmentManifest,
    subject: AuthorityId,
    ceremony: &CeremonyId,
) -> AgentResult<[u8; 32]> {
    use aura_invitation::enrollment_manifest::SignedEnrollmentTrustManifest;
    if stored.version != 1 || stored.confirmation_verifier.len() != 32 {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let signed =
        SignedEnrollmentTrustManifest::decode(&stored.signed_code).map_err(manifest_error)?;
    if signed.manifest.subject != subject
        || signed.manifest.ceremony != *ceremony
        || signed.manifest.initiator_device != effects.device_id()
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let verified = signed
        .manifest
        .verify_signature(effects, &stored.confirmation_verifier, &signed.signature)
        .await
        .map_err(manifest_error)?;
    if verified.digest() != stored.digest {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    Ok(verified.digest())
}

async fn load_issued_enrollment_manifest_digest(
    effects: &AuraEffectSystem,
    expected: &StoredEnrollmentVerifier,
    invitation: &super::Invitation,
) -> AgentResult<[u8; 32]> {
    use aura_invitation::enrollment_manifest::{
        EnrollmentTrustManifest, SignedEnrollmentTrustManifest,
    };
    let key = issued_manifest_location(expected.subject, &expected.ceremony);
    if !effects.secure_exists(&key).await? {
        return Err(EnrollmentVerifierError::Missing.into());
    }
    let bytes = effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > EnrollmentTrustManifest::MAX_BYTES * 2 {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let stored: StoredIssuedEnrollmentManifest =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    let digest =
        verify_stored_issued_manifest(effects, &stored, expected.subject, &expected.ceremony)
            .await?;
    let signed =
        SignedEnrollmentTrustManifest::decode(&stored.signed_code).map_err(manifest_error)?;
    let manifest = signed.manifest;
    if manifest.invitation != invitation.invitation_id
        || manifest.pending_epoch != expected.pending_epoch
        || manifest.invitee_authority != expected.setup.authority
        || manifest.invitee_device != expected.setup.device
        || manifest.setup.digest != expected.setup_digest
        || manifest.setup.nonce != expected.setup.nonce
        || manifest.expires_at_ms != expected.setup.expires_at_ms
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let super::InvitationType::DeviceEnrollment {
        key_package,
        threshold_config,
        public_key_package,
        baseline_tree_ops,
        ..
    } = &invitation.invitation_type
    else {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    };
    let baseline_bytes =
        aura_core::util::serialization::to_vec(baseline_tree_ops).map_err(|error| {
            aura_core::AuraError::Internal {
                message: "encode manifest-bound canonical baseline".into(),
                source: Some(Arc::new(error)),
            }
        })?;
    if hash(key_package) != manifest.pending_share_digest
        || hash(threshold_config) != *manifest.pending_threshold_config_digest.as_bytes()
        || hash(public_key_package) != manifest.pending_public_key_package_digest
        || hash(&baseline_bytes) != manifest.baseline_digest
        || baseline_tree_ops.len() != manifest.baseline_count as usize
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    Ok(digest)
}

pub(crate) async fn failed_generation_binding(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
    authority: AuthorityId,
    epoch: u64,
    prestate: aura_core::Hash32,
) -> AgentResult<([u8; 32], [u8; 32])> {
    let bytes = effects
        .secure_retrieve(
            &pending_generation_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let retained: StoredPendingGeneration =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if retained.version != 1
        || retained.ceremony != *ceremony
        || retained.authority != authority
        || retained.epoch != epoch
        || retained.prestate != prestate
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    Ok((retained.package_digest, retained.config_digest))
}

pub(crate) async fn verify_generation_registration_binding(
    effects: &AuraEffectSystem,
    authority: AuthorityId,
    epoch: u64,
    ceremony: &CeremonyId,
    prestate: aura_core::Hash32,
    invitation_id: &aura_core::InvitationId,
    setup_digest: [u8; 32],
) -> AgentResult<super::Invitation> {
    let current = recover_pending_enrollment_registration(effects, ceremony).await?;
    if current.initiator_id != authority
        || current.new_epoch != epoch
        || current.prestate_hash != prestate
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let bytes = effects
        .secure_retrieve(
            &pending_registration_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let registration: StoredPendingEnrollment =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if registration.invitation.invitation_id != *invitation_id {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let retained = RetainedEnrollmentVerifier::load(
        effects,
        authority,
        ceremony,
        epoch,
        effects.device_id(),
        registration.invitation.receiver_id,
        current
            .enrollment_device_id
            .ok_or(EnrollmentVerifierError::RecordBinding)?,
    )
    .await?;
    if retained.0.setup_digest != setup_digest {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    failed_generation_binding(effects, ceremony, authority, epoch, prestate).await?;
    load_issued_enrollment_manifest_digest(effects, &retained.0, &registration.invitation).await?;
    Ok(registration.invitation)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredAllocatedEnrollmentRegistration {
    version: u16,
    registration: StoredEnrollmentRegistration,
    prestate: aura_core::Hash32,
}
fn allocated_registration_location(ceremony: &CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new(
        "device_enrollment_allocated_registration_v1",
        ceremony.to_string(),
    )
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "validate_allocated_registration",
    capability_type = EnrollmentGenerationReservation,
    family = "runtime_helper"
)]
pub(crate) async fn persist_allocated_enrollment_registration(
    effects: &AuraEffectSystem,
    generation: &crate::runtime::effects::EnrollmentGenerationReservation<'_>,
    state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
) -> AgentResult<crate::runtime::services::ceremony_tracker::TrackedCeremony> {
    generation.validate_allocated_registration(effects, state)?;
    let raw = StoredAllocatedEnrollmentRegistration {
        version: 1,
        registration: StoredEnrollmentRegistration::from_state(state)?,
        prestate: state.prestate_hash,
    };
    raw.registration.clone().restore_bound(
        state.initiator_id,
        &state.ceremony_id,
        state
            .enrollment_device_id
            .ok_or(EnrollmentVerifierError::RecordBinding)?,
        state.new_epoch,
        None,
        state.prestate_hash,
    )?;
    let bytes = serde_json::to_vec(&raw).map_err(EnrollmentVerifierError::Codec)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let location = allocated_registration_location(&state.ceremony_id);
    effects
        .secure_store_immutable(
            &location,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
    let retained = recover_allocated_enrollment_registration(effects, &state.ceremony_id).await?;
    if retained.initiator_id != state.initiator_id
        || retained.kind != state.kind
        || retained.threshold_k != state.threshold_k
        || retained.total_n != state.total_n
        || retained.participants != state.participants
        || retained.new_epoch != state.new_epoch
        || retained.enrollment_device_id != state.enrollment_device_id
        || retained.enrollment_nickname_suggestion != state.enrollment_nickname_suggestion
        || retained.prestate_hash != state.prestate_hash
    {
        return Err(EnrollmentVerifierError::ExistingBinding.into());
    }
    // Time comes from the immutable original allocation, never this retry's clock.
    Ok(retained)
}
pub(crate) async fn finish_interrupted_canonical_registration(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
    invitation: &super::Invitation,
    prestate: aura_core::Hash32,
    ordered_signing_roster: &[aura_core::threshold::ParticipantIdentity],
    response_policy: &crate::runtime::effects::EnrollmentResponsePolicy,
) -> AgentResult<()> {
    let bytes = effects
        .secure_retrieve(
            &allocated_registration_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let raw: StoredAllocatedEnrollmentRegistration =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    let super::InvitationType::DeviceEnrollment {
        subject_authority,
        pending_epoch,
        device_id,
        ceremony_id,
        initiator_device_id,
        ..
    } = &invitation.invitation_type
    else {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    };
    let expected_roster: std::collections::HashSet<_> = ordered_signing_roster
        .iter()
        .filter(|participant| {
            **participant != aura_core::threshold::ParticipantIdentity::device(*initiator_device_id)
        })
        .cloned()
        .collect();
    if raw.version != 1
        || raw.prestate != prestate
        || ceremony_id != ceremony
        || raw.registration.subject != *subject_authority
        || raw.registration.pending_epoch != *pending_epoch
        || raw.registration.device != *device_id
        || raw
            .registration
            .participants
            .iter()
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            != expected_roster
        || raw.registration.threshold != response_policy.required()
        || raw.registration.total != response_policy.total()
    {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let state = raw.registration.restore_bound(
        *subject_authority,
        ceremony,
        *device_id,
        *pending_epoch,
        None,
        prestate,
    )?;
    let retained = RetainedEnrollmentVerifier::load(
        effects,
        *subject_authority,
        ceremony,
        *pending_epoch,
        *initiator_device_id,
        invitation.receiver_id,
        *device_id,
    )
    .await?;
    load_issued_enrollment_manifest_digest(effects, &retained.0, invitation).await?;
    persist_validated_pending_enrollment_registration(effects, &state, invitation).await
}

pub(crate) async fn recover_allocated_enrollment_registration(
    effects: &AuraEffectSystem,
    ceremony: &CeremonyId,
) -> AgentResult<crate::runtime::services::ceremony_tracker::TrackedCeremony> {
    let bytes = effects
        .secure_retrieve(
            &allocated_registration_location(ceremony),
            &[SecureStorageCapability::Read],
        )
        .await?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(EnrollmentVerifierError::Oversized.into());
    }
    let raw: StoredAllocatedEnrollmentRegistration =
        serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
    if raw.version != 1 || raw.registration.ceremony != *ceremony {
        return Err(EnrollmentVerifierError::RecordBinding.into());
    }
    let subject = raw.registration.subject;
    let epoch = raw.registration.pending_epoch;
    let device = raw.registration.device;
    raw.registration
        .restore_bound(subject, ceremony, device, epoch, None, raw.prestate)
}

/// Immutable issuer-selected ceremony control authority. Its private fields
/// cannot be restored from peer wire data or deserialized as an admission.
pub(crate) struct RetainedEnrollmentVmControl {
    runtime_owner: Arc<AuraEffectSystem>,
    manifest: aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    expected_request_verifier: Vec<u8>,
    digest: [u8; 32],
    canonical_invitation: super::Invitation,
}
impl RetainedEnrollmentVmControl {
    pub(crate) fn expected_request_verifier(&self) -> &[u8] {
        &self.expected_request_verifier
    }

    pub(crate) fn canonical_invitation(&self) -> &super::Invitation {
        &self.canonical_invitation
    }

    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        &self.manifest
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub(crate) fn require_runtime_owner(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<(), aura_core::AuraError> {
        if std::ptr::eq(self.runtime_owner.as_ref(), effects) {
            Ok(())
        } else {
            Err(aura_core::AuraError::Invalid {
                message: "retained enrollment control belongs to another runtime".into(),
                source: Some(Arc::new(EnrollmentVerifierError::RuntimeOwner)),
            })
        }
    }

    pub(crate) async fn load_required_sender(
        record: &super::SenderInvitationRecordCapability,
    ) -> AgentResult<Self> {
        Self::load(record.runtime_owner(), record.invitation()).await
    }

    pub(crate) async fn load(
        effects: Arc<AuraEffectSystem>,
        invitation: &super::Invitation,
    ) -> AgentResult<Self> {
        let runtime_owner = effects.clone();
        let effects = effects.as_ref();
        let super::InvitationType::DeviceEnrollment {
            subject_authority,
            ceremony_id,
            pending_epoch,
            initiator_device_id,
            device_id,
            invitee_authority: Some(invitee),
            ..
        } = &invitation.invitation_type
        else {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        };
        let expected = RetainedEnrollmentVerifier::load(
            effects,
            *subject_authority,
            ceremony_id,
            *pending_epoch,
            *initiator_device_id,
            *invitee,
            *device_id,
        )
        .await?;
        let digest =
            load_issued_enrollment_manifest_digest(effects, &expected.0, invitation).await?;
        let bytes = effects
            .secure_retrieve(
                &issued_manifest_location(*subject_authority, ceremony_id),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len()
            > aura_invitation::enrollment_manifest::EnrollmentTrustManifest::MAX_BYTES * 2
        {
            return Err(EnrollmentVerifierError::Oversized.into());
        }
        let stored: StoredIssuedEnrollmentManifest =
            serde_json::from_slice(&bytes).map_err(EnrollmentVerifierError::Codec)?;
        if verify_stored_issued_manifest(effects, &stored, *subject_authority, ceremony_id).await?
            != digest
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        let signed = aura_invitation::enrollment_manifest::SignedEnrollmentTrustManifest::decode(
            &stored.signed_code,
        )
        .map_err(manifest_error)?;
        // Storage may return changed bytes between reads. Revalidate every
        // semantic binding before constructing the sealed result.
        if signed.manifest.invitation != invitation.invitation_id
            || signed.manifest.pending_epoch != *pending_epoch
            || signed.manifest.invitee_authority != *invitee
            || signed.manifest.invitee_device != *device_id
            || signed.manifest.setup.digest != expected.0.setup_digest
            || signed.manifest.setup.nonce != expected.0.setup.nonce
        {
            return Err(EnrollmentVerifierError::RecordBinding.into());
        }
        if let Some(inventory) = &signed.manifest.final_inventory {
            let root = inventory
                .iter()
                .find(|entry| entry.signing_node == aura_core::tree::NodeIndex(0))
                .ok_or(EnrollmentVerifierError::RecordBinding)?;
            let retained_verifier = match root.mode {
                aura_core::crypto::single_signer::SigningMode::SingleSigner => {
                    if root.threshold != 1 || root.participants.len() != 1 {
                        return Err(EnrollmentVerifierError::RecordBinding.into());
                    }
                    aura_core::crypto::single_signer::SingleSignerPublicKeyPackage::from_bytes(
                        &root.public_key_package,
                    )
                    .map_err(aura_core::AuraError::from)?
                    .verifying_key
                }
                aura_core::crypto::single_signer::SigningMode::Threshold => {
                    if root.threshold < 2 || usize::from(root.threshold) > root.participants.len() {
                        return Err(EnrollmentVerifierError::RecordBinding.into());
                    }
                    let native = frost_ed25519::keys::PublicKeyPackage::deserialize(
                        &root.public_key_package,
                    )
                    .map_err(|source| {
                        aura_core::AuraError::crypto_with_source(
                            "decode independently retained enrollment request public package",
                            Arc::new(source),
                        )
                    })?;
                    if native.verifying_shares().len() != root.participants.len() {
                        return Err(EnrollmentVerifierError::RecordBinding.into());
                    }
                    native.verifying_key().serialize().to_vec()
                }
            };
            if retained_verifier != stored.confirmation_verifier {
                return Err(EnrollmentVerifierError::RecordBinding.into());
            }
        }
        Ok(Self {
            expected_request_verifier: stored.confirmation_verifier,
            runtime_owner,
            manifest: signed.manifest,
            digest,
            canonical_invitation: invitation.clone(),
        })
    }
}
