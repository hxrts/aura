use std::sync::Arc;
mod confirmed_activation;

/// Exact retained package location and original storage cause, without
/// changing absence into another layout after a failed required read.
#[derive(Debug, thiserror::Error)]
#[error(
    "read retained parent package for authority {authority}, epoch {epoch}, location {location:?}: {source}"
)]
struct ParentInventoryStorageError {
    authority: AuthorityId,
    epoch: u64,
    location: SecureStorageLocation,
    #[source]
    source: AuraError,
}

fn parent_inventory_storage_error(
    authority: AuthorityId,
    epoch: u64,
    location: SecureStorageLocation,
    source: AuraError,
) -> AuraError {
    let contextual_source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>> =
        Some(std::sync::Arc::new(ParentInventoryStorageError {
            authority,
            epoch,
            location,
            source: source.clone(),
        }));
    let message = "read exact retained parent inventory package".into();
    match source {
        AuraError::Invalid { .. } => AuraError::Invalid {
            message,
            source: contextual_source,
        },
        AuraError::NotFound { .. } => AuraError::NotFound {
            message,
            source: contextual_source,
        },
        AuraError::PermissionDenied { .. } => AuraError::PermissionDenied {
            message,
            source: contextual_source,
        },
        AuraError::Crypto { .. } => AuraError::Crypto {
            message,
            source: contextual_source,
        },
        AuraError::Network { .. } => AuraError::Network {
            message,
            source: contextual_source,
        },
        AuraError::Serialization { .. } => AuraError::Serialization {
            message,
            source: contextual_source,
        },
        AuraError::Storage { .. } => AuraError::Storage {
            message,
            source: contextual_source,
        },
        AuraError::Internal { .. } | AuraError::Terminal(_) => AuraError::Internal {
            message,
            source: contextual_source,
        },
    }
}

/// Required recovery scans validate all invitation records before the caller
/// can conclude absence or recover/register an issued canonical invitation.
fn required_committed_invitation_facts(
    facts: Vec<aura_journal::Fact>,
) -> Result<Vec<aura_invitation::InvitationFact>, AuraError> {
    if facts.len() > 4096 {
        return Err(AuraError::invalid(
            "orphan recovery fact scan exceeds bounds",
        ));
    }
    let mut payload_bytes = 0usize;
    let mut invitations = Vec::new();
    for fact in facts {
        let aura_journal::fact::FactContent::Relational(
            aura_journal::fact::RelationalFact::Generic {
                context_id,
                envelope,
            },
        ) = fact.content
        else {
            continue;
        };
        if envelope.type_id.as_str() != aura_invitation::INVITATION_FACT_TYPE_ID {
            continue;
        }
        payload_bytes = payload_bytes
            .checked_add(envelope.payload.len())
            .ok_or_else(|| AuraError::invalid("orphan recovery payload bound overflow"))?;
        if payload_bytes > 8 * 1024 * 1024 {
            return Err(AuraError::invalid(
                "orphan recovery payload scan exceeds bounds",
            ));
        }
        invitations.push(
            aura_invitation::InvitationFact::try_from_envelope_in_context(&envelope, context_id)
                .map_err(|error| AuraError::Serialization {
                    message: "decode required committed invitation before orphan retirement".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?,
        );
    }
    Ok(invitations)
}

use super::AuraEffectSystem;
use async_trait::async_trait;
use aura_core::crypto::single_signer::{
    SigningMode, SingleSignerKeyPackage, SingleSignerPublicKeyPackage,
};
use aura_core::crypto::tree_signing;
use aura_core::effects::crypto::{
    FrostKeyGenResult, FrostSigningPackage, KeyDerivationContext, KeyGenerationMethod,
    SigningKeyGenResult,
};
use aura_core::effects::{
    CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::secrets::SecretExportContext;
use aura_core::threshold::ParticipantIdentity;
use aura_core::{AuraError, AuthorityId};
use aura_signature::threshold_signing_context_transcript_bytes;
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use serde::{Deserialize, Serialize};

const PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION: u8 = 1;
const PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN: &str = "aura:participant-key-package-envelope:v1";

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParticipantEnvelopeBoundsError {
    #[error("participant envelope exceeds the 131072-byte admission bound")]
    EnvelopeTooLarge,
    #[error("participant envelope ciphertext is empty")]
    EmptyCiphertext,
    #[error("participant envelope ciphertext exceeds the 65536-byte admission bound")]
    CiphertextTooLarge,
}

fn require_participant_ciphertext_bounds(ciphertext: &[u8]) -> Result<(), AuraError> {
    let cause = if ciphertext.is_empty() {
        ParticipantEnvelopeBoundsError::EmptyCiphertext
    } else if ciphertext.len() > 65_536 {
        ParticipantEnvelopeBoundsError::CiphertextTooLarge
    } else {
        return Ok(());
    };
    Err(AuraError::Invalid {
        message: cause.to_string(),
        source: Some(std::sync::Arc::new(cause)),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParticipantKeyPackageEnvelope {
    version: u8,
    authority: AuthorityId,
    epoch: u64,
    recipient: ParticipantIdentity,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum RequiredSigningParticipantError {
    #[error("required signing policy has no participant owned by this runtime")]
    Missing,
    #[error("required signing policy has multiple local participant identities")]
    Ambiguous,
    #[error("threshold {threshold} signing requires an owned quorum coordinator")]
    QuorumOwnerRequired { threshold: u16 },
    #[error("required private signing material does not match the retained public package")]
    KeyMismatch,
}

/// Protected original identity and invitation clock anchor. Never a live grant.
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct StoredOriginalEnrollmentReservation {
    version: u16,
    authority: AuthorityId,
    device: aura_core::DeviceId,
    invitation: aura_core::InvitationId,
    created_at_ms: u64,
}
const MAX_ORIGINAL_RESERVATION_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
enum OriginalEnrollmentReservationError {
    #[error("original enrollment reservation belongs to another effect owner")]
    Owner,
    #[error("original enrollment reservation exceeds its bound")]
    Oversized,
    #[error("original enrollment reservation contradicts the protected first decision")]
    FirstDecision,
}
fn original_reservation_error(reason: OriginalEnrollmentReservationError) -> AuraError {
    AuraError::Crypto {
        message: "original enrollment reservation custody failed".into(),
        source: Some(std::sync::Arc::new(reason)),
    }
}

/// Physical generation custody. Its private guard is acquired from this exact
/// effect owner; no raw guard, deserialization or caller-supplied lock can mint it.
/// Private fields restrict construction to the actual effect-owned acquisition path.
pub(crate) struct EnrollmentGenerationCustodyCapability<'a> {
    effects: EnrollmentRuntimeOrigin<'a>,
    _guard: EnrollmentGenerationGuard<'a>,
}
pub(crate) enum EnrollmentRuntimeOrigin<'a> {
    Borrowed(&'a AuraEffectSystem),
    Owned(Arc<AuraEffectSystem>),
}
impl EnrollmentRuntimeOrigin<'_> {
    pub(crate) fn effects(&self) -> &AuraEffectSystem {
        match self {
            Self::Borrowed(effects) => effects,
            Self::Owned(effects) => effects,
        }
    }
}
impl std::ops::Deref for EnrollmentRuntimeOrigin<'_> {
    type Target = AuraEffectSystem;
    fn deref(&self) -> &Self::Target {
        self.effects()
    }
}
enum EnrollmentGenerationGuard<'a> {
    Borrowed {
        _guard: tokio::sync::MutexGuard<'a, ()>,
    },
    Owned {
        _guard: tokio::sync::OwnedMutexGuard<()>,
    },
}
impl EnrollmentGenerationCustodyCapability<'_> {
    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }
}

/// A held, authenticated roster decision. Raw identifiers cannot construct it.
/// Both custody guards remain held until the original issuer registration ends.
pub(crate) struct AuthenticatedEnrollmentRotationPlan<'a> {
    effects: EnrollmentRuntimeOrigin<'a>,
    setup_digest: [u8; 32],
    baseline: OriginalEnrollmentBaseline,
    state: aura_journal::commitment_tree::state::TreeState,
    participants: Vec<ParticipantIdentity>,
    threshold: u16,
    prestate: aura_core::Hash32,
    tree: aura_protocol::handlers::tree::TreeDecisionLease<'a>,
    generation:
        crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'a>,
}
impl AuthenticatedEnrollmentRotationPlan<'_> {
    pub(crate) fn participants(&self) -> &[ParticipantIdentity] {
        &self.participants
    }
    pub(crate) fn threshold(&self) -> u16 {
        self.threshold
    }
    pub(crate) fn prestate(&self) -> aura_core::Hash32 {
        self.prestate
    }
    fn require_effects(
        &self,
        effects: &AuraEffectSystem,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.effects(), effects) || self.setup_digest != setup.digest() {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }
}
/// Original negative cleanup custody. It cannot authorize live registration.
struct RecoveredEnrollmentCleanupCustody<'a> {
    effects: &'a AuraEffectSystem,
    owner: StoredEnrollmentGenerationProfile,
    original: crate::runtime::services::ceremony_tracker::TrackedCeremony,
    generation: EnrollmentGenerationCustodyCapability<'a>,
}
#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentRosterError {
    #[error("physical issuer is not a current authenticated device member")]
    CurrentDeviceNotMember,
    #[error("invitee is already a current authenticated device member")]
    AlreadyEnrolled,
    #[error("current authenticated tree and active signing epoch disagree")]
    ActiveEpochMismatch,
    #[error("current authenticated device roster and signing policy disagree")]
    SigningPolicyMismatch,
}
fn roster_error(reason: EnrollmentRosterError) -> AuraError {
    match reason {
        EnrollmentRosterError::CurrentDeviceNotMember => AuraError::PermissionDenied {
            message: "current enrollment roster denies issuer".into(),
            source: Some(std::sync::Arc::new(reason)),
        },
        EnrollmentRosterError::AlreadyEnrolled => AuraError::Invalid {
            message: "invitee already enrolled".into(),
            source: Some(std::sync::Arc::new(reason)),
        },
        EnrollmentRosterError::ActiveEpochMismatch
        | EnrollmentRosterError::SigningPolicyMismatch => AuraError::Crypto {
            message: "current enrollment authority evidence disagrees".into(),
            source: Some(std::sync::Arc::new(reason)),
        },
    }
}

/// Keeps the one local generation writer reserved through complete issuer retention.
/// Captured under the original physical generation, tracker decision and tree
/// leases. A caller cannot substitute a pure manifest tuple after capture.
#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentFinalInventoryError {
    #[error("final inventory original owner binding mismatch")]
    OwnerBinding,
    #[error("final inventory substituted after capture")]
    Substituted,
    #[error("issuance baseline differs from the held physical tree")]
    BaselineMismatch,
    #[error("final tree and active package epochs differ")]
    EpochMismatch,
    #[error("final active policy or roster differs from authenticated membership")]
    PolicyMismatch,
    #[error("exact signing package unavailable for node {0:?}")]
    NonrootInventoryUnavailable(aura_core::tree::NodeIndex),
    #[error("authenticated leaf has no signing parent")]
    MissingLeafParent,
}
fn final_inventory_error(source: EnrollmentFinalInventoryError) -> AuraError {
    AuraError::PermissionDenied {
        message: "owned final enrollment verifier inventory rejected".into(),
        source: Some(std::sync::Arc::new(source)),
    }
}
pub(crate) struct EnrollmentFinalVerifierInventoryCapability<'owner, 'runtime> {
    reservation: EnrollmentFinalReservationOrigin<'owner, 'runtime>,
    inventory: Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>,
}
enum EnrollmentFinalReservationOrigin<'owner, 'runtime> {
    Borrowed(&'owner EnrollmentGenerationReservation<'runtime>),
    Owned(Arc<EnrollmentGenerationReservation<'static>>),
}
impl EnrollmentFinalVerifierInventoryCapability<'_, '_> {
    fn reservation(&self) -> &EnrollmentGenerationReservation<'_> {
        match &self.reservation {
            EnrollmentFinalReservationOrigin::Borrowed(reservation) => reservation,
            EnrollmentFinalReservationOrigin::Owned(reservation) => reservation.as_ref(),
        }
    }
    pub(crate) fn inventory(
        &self,
    ) -> &[aura_invitation::enrollment_manifest::EnrollmentParentVerifier] {
        &self.inventory
    }
    pub(crate) fn generation(&self) -> &EnrollmentGenerationCustodyCapability<'_> {
        self.reservation().generation()
    }
    pub(crate) fn require_manifest(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> Result<(), AuraError> {
        let reservation = self.reservation();
        reservation.require_effects(effects)?;
        if manifest.ceremony != *reservation.ceremony_id()
            || manifest.invitation != *reservation.invitation_id()
            || manifest.pending_epoch != reservation.pending_epoch()
            || manifest.setup.digest != reservation.setup_digest()
            || manifest.version != 3
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        let expected = aura_core::util::serialization::to_vec(&self.inventory)?;
        let supplied_inventory = manifest.final_inventory().map_err(|source| {
            AuraError::crypto_with_source(
                "missing final verifier inventory",
                std::sync::Arc::new(source),
            )
        })?;
        let supplied = aura_core::util::serialization::to_vec(&supplied_inventory)?;
        if expected != supplied {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::Substituted,
            ));
        }
        Ok(())
    }
}

pub(crate) struct EnrollmentGenerationReservation<'a> {
    effects: EnrollmentRuntimeOrigin<'a>,
    owner: StoredEnrollmentGenerationProfile,
    _tree: aura_protocol::handlers::tree::TreeDecisionLease<'a>,
    _owner: crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'a>,
    original_birth: Option<OriginalEnrollmentAllocationAcknowledgment>,
}

/// Custody of the original authenticated decision before an allocation is
/// acknowledged. A pending storage call is not evidence that nothing was
/// written. The required issuance owner retains this value across frame Drop.
pub(crate) struct OriginalEnrollmentAllocationProducer<'a> {
    origin: EnrollmentRuntimeOrigin<'a>,
    launch: Option<Arc<()>>,
    plan: Option<AuthenticatedEnrollmentRotationPlan<'a>>,
    progress: OriginalEnrollmentAllocationProgress,
    retirement: Option<OriginalOrphanAllocationRetirement>,
    unwritten: Option<OriginalUnwrittenAllocationDisposition>,
    pub(crate) cleanup_failure: Option<AuraError>,
    pub(crate) cleanup_validation_failure: Option<aura_core::time::timeout::TimeoutBudgetError>,
    acknowledged: bool,
    acknowledgment: Option<
        crate::runtime::services::enrollment_window::OriginalAllocationCleanupAcknowledgment,
    >,
    pub(crate) cleanup_observation:
        Option<aura_core::time::timeout::NegativeCleanupExecution<AuraError, (), AuraError>>,
}

#[derive(Default)]
struct OriginalEnrollmentAllocationProgress {
    original: Option<StoredEnrollmentGenerationProfile>,
    immutable_write_started: bool,
    immutable_write_acknowledged: bool,
    immutable_readback_acknowledged: bool,
    live_write_started: bool,
    live_write_acknowledged: bool,
    live_readback_acknowledged: bool,
}

impl<'a> OriginalEnrollmentAllocationProducer<'a> {
    pub(crate) fn from_original(plan: AuthenticatedEnrollmentRotationPlan<'a>) -> Self {
        let origin = match &plan.effects {
            EnrollmentRuntimeOrigin::Borrowed(effects) => {
                EnrollmentRuntimeOrigin::Borrowed(effects)
            }
            EnrollmentRuntimeOrigin::Owned(effects) => {
                EnrollmentRuntimeOrigin::Owned(effects.clone())
            }
        };
        Self {
            origin,
            launch: None,
            plan: Some(plan),
            progress: OriginalEnrollmentAllocationProgress::default(),
            retirement: None,
            unwritten: None,
            cleanup_failure: None,
            cleanup_validation_failure: None,
            acknowledged: false,
            acknowledgment: None,
            cleanup_observation: None,
        }
    }
    pub(crate) fn bind_original_launch(
        &mut self,
        effects: &AuraEffectSystem,
        launch: &crate::runtime::services::enrollment_window::OriginalIssuerLaunchWindow,
    ) -> Result<(), AuraError> {
        self.require_original(effects)?;
        if let Some(original) = self.launch.as_ref() {
            if !Arc::ptr_eq(original, launch.observation_identity()) {
                return Err(AuraError::permission_denied(
                    "allocation launch cannot be replaced",
                ));
            }
        } else {
            self.launch = Some(launch.observation_identity().clone());
        }
        Ok(())
    }

    fn require_original(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        let plan = self.plan.as_ref().ok_or_else(|| {
            AuraError::invalid("original allocation decision already transferred")
        })?;
        if !std::ptr::eq(plan.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        if let Some(original) = self.progress.original.as_ref() {
            if original.registered
                || original.authority != effects.authority_id
                || plan.state.epoch.value().checked_add(1) != Some(original.pending_epoch)
                || original.prestate != plan.prestate
                || original.setup_digest != plan.setup_digest
                || original.baseline != Some(plan.baseline)
                || original.threshold != plan.threshold
                || original.participants != plan.participants
            {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
        }
        Ok(())
    }

    pub(crate) async fn retire_original(
        &mut self,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        self.require_original(effects)?;
        let Some(original) = self.progress.original.as_ref() else {
            if self.progress.immutable_write_started || self.progress.live_write_started {
                return Err(AuraError::invalid(
                    "allocation write began without original intent",
                ));
            }
            if self.unwritten.is_none() {
                self.unwritten = Some(OriginalUnwrittenAllocationDisposition {
                    identity: Arc::new(()),
                });
            }
            return Ok(());
        };
        let bytes = serde_json::to_vec(original).map_err(|source| AuraError::Serialization {
            message: "validate original partial allocation bytes".into(),
            source: Some(Arc::new(source)),
        })?;
        let active = effects
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", original.authority.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let active = u64::from_le_bytes(
            active
                .try_into()
                .map_err(|_| AuraError::invalid("invalid active signing epoch"))?,
        );
        if active >= original.pending_epoch {
            return Err(AuraError::permission_denied(
                "partial allocation no longer owns inactive epoch",
            ));
        }
        for location in [
            SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                original.ceremony.to_string(),
            ),
            super::enrollment_generation_profile_location(
                &original.authority,
                original.pending_epoch,
            ),
        ] {
            if effects.secure_exists(&location).await?
                && effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await?
                    != bytes
            {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
        }
        for namespace in [
            "device_enrollment_activation_v1",
            "device_enrollment_allocated_registration_v1",
        ] {
            if effects
                .secure_exists(&SecureStorageLocation::new(
                    namespace,
                    original.ceremony.to_string(),
                ))
                .await?
            {
                return Err(AuraError::permission_denied(
                    "partial allocation already transferred to registered or activation owner",
                ));
            }
        }
        for invitation in required_committed_invitation_facts(
            effects.load_committed_facts(original.authority).await?,
        )? {
            if let aura_invitation::InvitationFact::Sent { invitation_id, .. } = invitation {
                if invitation_id == original.invitation {
                    return Err(AuraError::permission_denied(
                        "partial allocation already has committed issued invitation",
                    ));
                }
            }
        }
        if self.retirement.is_none() {
            let plan = self
                .plan
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original allocation decision missing"))?;
            match effects
                .retire_original_orphan_allocation(original, plan.generation.generation())
                .await
            {
                Ok(receipt) => self.retirement = Some(receipt),
                Err(source) => {
                    self.cleanup_failure.get_or_insert_with(|| source.clone());
                    return Err(source);
                }
            }
        }
        self.retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original allocation retirement missing"))?
            .require_original(effects, original)?;
        Ok(())
    }

    pub(crate) fn acknowledge_original(
        &mut self,
        effects: &AuraEffectSystem,
    ) -> Result<OriginalAllocationCleanupReceipt<'_>, AuraError> {
        self.require_original(effects)?;
        if let Some(unwritten) = self.unwritten.as_ref() {
            if self.progress.original.is_some()
                || self.progress.immutable_write_started
                || self.progress.live_write_started
            {
                return Err(AuraError::permission_denied(
                    "unwritten disposition cannot replace allocation retirement",
                ));
            }
            return Ok(OriginalAllocationCleanupReceipt::Unwritten(unwritten));
        }
        let original = self
            .progress
            .original
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original allocation write intent missing"))?;
        let receipt = self
            .retirement
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original allocation retirement missing"))?;
        receipt.require_original(effects, original)?;
        Ok(OriginalAllocationCleanupReceipt::Retired(receipt))
    }

    pub(crate) fn observe_original_cleanup_acknowledged(
        &self,
        effects: &AuraEffectSystem,
    ) -> Result<bool, AuraError> {
        if !std::ptr::eq(self.origin.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        if !self.acknowledged {
            return Ok(false);
        }
        let identity = if let Some(receipt) = self.retirement.as_ref() {
            &receipt.identity
        } else if let Some(unwritten) = self.unwritten.as_ref() {
            &unwritten.identity
        } else {
            return Err(AuraError::invalid("original cleanup evidence missing"));
        };
        self.acknowledgment
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original cleanup acknowledgment missing"))?
            .require_original(
                effects,
                identity,
                self.launch
                    .as_ref()
                    .ok_or_else(|| AuraError::invalid("original allocation launch missing"))?,
            )?;
        Ok(true)
    }

    pub(crate) fn release_original(
        &mut self,
        effects: &AuraEffectSystem,
        acknowledgment: crate::runtime::services::enrollment_window::OriginalAllocationCleanupAcknowledgment,
    ) -> Result<(), AuraError> {
        self.require_original(effects)?;
        let identity = self
            .acknowledge_original(effects)?
            .observation_identity()
            .clone();
        acknowledgment.require_original(
            effects,
            &identity,
            self.launch
                .as_ref()
                .ok_or_else(|| AuraError::invalid("original allocation launch missing"))?,
        )?;
        self.acknowledgment = Some(acknowledgment);
        self.acknowledged = true;
        self.plan = None;
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn acknowledged_wrap_original_for_test(
        &self,
    ) -> (aura_core::CeremonyId, AuthorityId, u64, Vec<u8>) {
        assert!(self.progress.live_readback_acknowledged);
        assert!(self.retirement.is_some() && self.acknowledgment.is_some() && self.plan.is_none());
        let original = self.progress.original.as_ref().unwrap();
        (
            original.ceremony.clone(),
            original.authority,
            original.pending_epoch,
            serde_json::to_vec(original).unwrap(),
        )
    }
    #[cfg(test)]
    pub(crate) fn assert_pending_allocation_retired_for_test(&self) {
        assert!(self.progress.immutable_write_started);
        assert!(
            !self.progress.immutable_write_acknowledged,
            "provider ACK never reached original caller"
        );
        assert!(!self.progress.live_write_started);
        assert!(self.acknowledged);
        assert!(
            self.plan.is_none(),
            "actual generation/tree/decision guards released only after cleanup ACK"
        );
        assert!(
            self.retirement.is_some(),
            "actual immutable cleaned readback retained"
        );
        let observation = self
            .cleanup_observation
            .as_ref()
            .expect("same original negative observation retained");
        assert!(matches!(observation.operation, Some(Ok(()))));
        assert!(observation.validation.is_ok());
    }
    #[cfg(test)]
    pub(crate) fn assert_unwritten_allocation_disposed_for_test(&self) {
        assert!(self.progress.original.is_none());
        assert!(!self.progress.immutable_write_started && !self.progress.live_write_started);
        assert!(self.unwritten.is_some());
        assert!(
            self.retirement.is_none(),
            "no retirement receipt manufactured without a write intent"
        );
        assert!(self.acknowledgment.is_some() && self.plan.is_none());
        let observation = self
            .cleanup_observation
            .as_ref()
            .expect("original negative observation retained");
        assert!(matches!(observation.operation, Some(Ok(()))) && observation.validation.is_ok());
    }
}

/// Actual required retirement observed under the original physical decision
/// lease. No serialization or public constructor can manufacture this receipt.
/// Actual canonical orphan retirement: immutable cleaned readback and pending
/// slot deletion completed under the original held generation capability.
pub(crate) struct OriginalOrphanAllocationRetirement {
    identity: Arc<()>,
    runtime_identity: Arc<()>,
    ceremony: aura_core::CeremonyId,
    authority: AuthorityId,
    epoch: u64,
    original_digest: [u8; 32],
}
pub(crate) struct OriginalUnwrittenAllocationDisposition {
    identity: Arc<()>,
}
pub(crate) enum OriginalAllocationCleanupReceipt<'a> {
    Retired(&'a OriginalOrphanAllocationRetirement),
    Unwritten(&'a OriginalUnwrittenAllocationDisposition),
}
impl OriginalAllocationCleanupReceipt<'_> {
    pub(crate) fn observation_identity(&self) -> &Arc<()> {
        match self {
            Self::Retired(receipt) => receipt.observation_identity(),
            Self::Unwritten(receipt) => &receipt.identity,
        }
    }
}
impl OriginalOrphanAllocationRetirement {
    pub(crate) fn observation_identity(&self) -> &Arc<()> {
        &self.identity
    }
    fn require_original(
        &self,
        effects: &AuraEffectSystem,
        owner: &StoredEnrollmentGenerationProfile,
    ) -> Result<(), AuraError> {
        let bytes = serde_json::to_vec(owner).map_err(|source| AuraError::Serialization {
            message: "validate original orphan retirement binding".into(),
            source: Some(Arc::new(source)),
        })?;
        if !Arc::ptr_eq(
            &self.runtime_identity,
            &effects.crypto.lifetime_owner_identity(),
        ) || self.ceremony != owner.ceremony
            || self.authority != owner.authority
            || self.epoch != owner.pending_epoch
            || self.original_digest != aura_core::hash::hash(&bytes)
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }
}

enum EnrollmentRetirementStorageDisposition {
    RequiredWritesAcknowledged,
    LaterGenerationObservation,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
#[derive(Debug, thiserror::Error)]
#[error("retirement storage unavailable")]
pub(crate) struct InjectedEnrollmentRetirementFailure;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) type EnrollmentRetirementFault = (u64, Option<Arc<()>>);
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) struct ScopedEnrollmentRetirementRefusal {
    effects: Arc<AuraEffectSystem>,
    identity: Arc<()>,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
impl Drop for ScopedEnrollmentRetirementRefusal {
    fn drop(&mut self) {
        let mut fault = self
            .effects
            .enrollment_retirement_fault
            .lock()
            .expect("retirement fault lock");
        if fault
            .as_ref()
            .and_then(|(_, scope)| scope.as_ref())
            .is_some_and(|scope| Arc::ptr_eq(scope, &self.identity))
        {
            *fault = None;
        }
    }
}

pub(crate) struct AcknowledgedEnrollmentRetirement {
    disposition: EnrollmentRetirementStorageDisposition,
    runtime_identity: std::sync::Arc<()>,
    ceremony: aura_core::CeremonyId,
    prestate: aura_core::Hash32,
    binding: (aura_core::AuthorityId, u64, [u8; 32], [u8; 32]),
}
impl AcknowledgedEnrollmentRetirement {
    pub(in crate::runtime) fn require_original_held(
        &self,
        reservation: &EnrollmentGenerationReservation<'static>,
        effects: &AuraEffectSystem,
    ) -> Result<(), AuraError> {
        reservation.require_effects(effects)?;
        if !matches!(
            self.disposition,
            EnrollmentRetirementStorageDisposition::RequiredWritesAcknowledged
        ) || !Arc::ptr_eq(
            &self.runtime_identity,
            &effects.crypto.lifetime_owner_identity(),
        ) || self.ceremony != reservation.owner.ceremony
            || self.prestate != reservation.owner.prestate
            || self.binding.0 != reservation.owner.authority
            || self.binding.1 != reservation.owner.pending_epoch
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }
    pub(crate) fn prestate(&self) -> aura_core::Hash32 {
        self.prestate
    }
    pub(crate) fn require_original(
        &self,
        effects: &AuraEffectSystem,
        original: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        original.require_runtime_owner(effects)?;
        if matches!(
            self.disposition,
            EnrollmentRetirementStorageDisposition::LaterGenerationObservation
        ) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let manifest = original.manifest();
        if !std::sync::Arc::ptr_eq(
            &self.runtime_identity,
            &effects.crypto.lifetime_owner_identity(),
        ) || self.ceremony != manifest.ceremony
            || self.binding.0 != manifest.subject
            || self.binding.1 != manifest.pending_epoch
            || self.binding.2 != manifest.pending_public_key_package_digest
            || self.binding.3 != *manifest.pending_threshold_config_digest.as_bytes()
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }
}

/// Actual fresh physical allocation acknowledgment. Protected bytes cannot
/// decode this owner, and recovery never manufactures a replacement.
pub(in crate::runtime) struct OriginalEnrollmentAllocationAcknowledgment {
    runtime_identity: std::sync::Arc<()>,
    original: StoredEnrollmentGenerationProfile,
}

impl OriginalEnrollmentAllocationAcknowledgment {
    pub(in crate::runtime) fn require_registered_generation(
        &self,
        effects: &AuraEffectSystem,
        generation: &RegisteredEnrollmentGenerationCapability<'_>,
    ) -> Result<(), AuraError> {
        require_original_allocation_generation_binding(
            &self.runtime_identity,
            &self.original,
            effects,
            generation,
        )
    }
    pub(in crate::runtime) fn require_issued_control(
        &self,
        effects: &AuraEffectSystem,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        issued.require_runtime_owner(effects)?;
        self.require_manifest(effects, issued.manifest())
    }
    pub(in crate::runtime) fn require_manifest(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> Result<(), AuraError> {
        require_original_allocation_manifest_binding(
            &self.runtime_identity,
            &self.original,
            effects,
            manifest,
        )
    }
}

fn require_original_allocation_generation_binding(
    runtime_identity: &Arc<()>,
    original: &StoredEnrollmentGenerationProfile,
    effects: &AuraEffectSystem,
    generation: &RegisteredEnrollmentGenerationCapability<'_>,
) -> Result<(), AuraError> {
    generation.require_effects(effects)?;
    let invitation = generation.canonical_invitation();
    let aura_invitation::InvitationType::DeviceEnrollment {
        subject_authority,
        ceremony_id,
        pending_epoch,
        setup_binding,
        initiator_device_id,
        device_id,
        ..
    } = &invitation.invitation_type
    else {
        return Err(held_registration_error(
            HeldEnrollmentRegistrationError::InvitationKind,
        ));
    };
    if !Arc::ptr_eq(runtime_identity, &effects.crypto.lifetime_owner_identity())
        || original.authority != invitation.sender_id
        || original.authority != *subject_authority
        || original.ceremony != *ceremony_id
        || original.invitation != invitation.invitation_id
        || original.pending_epoch != *pending_epoch
        || original.setup_digest != setup_binding.digest
        || *initiator_device_id != effects.device_id()
        || !original
            .participants
            .contains(&ParticipantIdentity::device(*device_id))
    {
        return Err(held_registration_error(
            HeldEnrollmentRegistrationError::Binding,
        ));
    }
    Ok(())
}

fn require_original_allocation_manifest_binding(
    runtime_identity: &Arc<()>,
    original: &StoredEnrollmentGenerationProfile,
    effects: &AuraEffectSystem,
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> Result<(), AuraError> {
    if !std::sync::Arc::ptr_eq(runtime_identity, &effects.crypto.lifetime_owner_identity())
        || original.authority != manifest.subject
        || original.ceremony != manifest.ceremony
        || original.invitation != manifest.invitation
        || original.pending_epoch != manifest.pending_epoch
        || original.setup_digest != manifest.setup.digest
        || manifest.initiator_device != effects.device_id()
        || !original
            .participants
            .contains(&ParticipantIdentity::device(manifest.invitee_device))
    {
        return Err(held_registration_error(
            HeldEnrollmentRegistrationError::Binding,
        ));
    }
    Ok(())
}

struct RotatedKeyMaterial {
    epoch: u64,
    packages: Vec<Vec<u8>>,
    public: Vec<u8>,
}

enum RotatedOwnedProfile {
    Ordinary(RotatedKeyMaterial),
    Enrollment {
        material: RotatedKeyMaterial,
        original_birth: Box<OriginalEnrollmentAllocationAcknowledgment>,
    },
}

/// Response quorum is distinct from the threshold policy of the new signing key.
/// Its original protected allocation commitment authorizes tracker comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnrollmentResponsePolicy {
    required: u16,
    total: u16,
}
impl EnrollmentResponsePolicy {
    fn select_for_original_signing_policy(
        signing_threshold: u16,
        signing_total: u16,
    ) -> Result<Self, AuraError> {
        let total = signing_total
            .checked_sub(1)
            .filter(|total| *total > 0)
            .ok_or_else(|| {
                held_registration_error(HeldEnrollmentRegistrationError::ResponsePolicyMismatch)
            })?;
        if signing_threshold == 0 || signing_threshold > signing_total {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::ResponsePolicyMismatch,
            ));
        }
        // Explicit original response policy: the local issuer is not a remote
        // responder. This selection occurs before immutable allocation; recovery
        // compares the stored response commitment exactly and never recomputes it.
        Ok(Self {
            required: signing_threshold.min(total),
            total,
        })
    }

    fn require_exact(&self, observed_required: u16, observed_total: u16) -> Result<(), AuraError> {
        if self.required != observed_required || self.total != observed_total {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::ResponsePolicyBinding {
                    expected_required: self.required,
                    expected_total: self.total,
                    observed_required,
                    observed_total,
                },
            ));
        }
        Ok(())
    }
    pub(crate) fn required(&self) -> u16 {
        self.required
    }
    pub(crate) fn total(&self) -> u16 {
        self.total
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEnrollmentGenerationProfile {
    registered: bool,
    version: u16,
    authority: AuthorityId,
    pending_epoch: u64,
    invitation: aura_core::InvitationId,
    ceremony: aura_core::CeremonyId,
    prestate: aura_core::Hash32,
    setup_digest: [u8; 32],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    baseline: Option<OriginalEnrollmentBaseline>,
    threshold: u16,
    response_policy: EnrollmentResponsePolicy,
    participants: Vec<ParticipantIdentity>,
}
impl StoredEnrollmentGenerationProfile {
    fn response_policy(&self) -> Result<EnrollmentResponsePolicy, AuraError> {
        let policy = self.response_policy;
        let total = self.participants.len().checked_sub(1).ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::ResponsePolicyMismatch)
        })?;
        if usize::from(policy.total) != total
            || policy.required == 0
            || policy.required > policy.total
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::ResponsePolicyMismatch,
            ));
        }
        Ok(policy)
    }
}
/// Canonical history identity, recorded only from the authenticated held roster plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OriginalEnrollmentBaseline {
    count: u32,
    digest: [u8; 32],
}
fn original_baseline_fingerprint(
    ops: &[aura_core::AttestedOp],
) -> Result<OriginalEnrollmentBaseline, AuraError> {
    use aura_invitation::enrollment_manifest::EnrollmentTrustManifest as Manifest;
    if ops.len() > Manifest::MAX_BASELINE_OPS {
        return Err(held_registration_error(
            HeldEnrollmentRegistrationError::BaselineBinding,
        ));
    }
    let rows = ops
        .iter()
        .map(aura_core::util::serialization::to_vec)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| AuraError::Serialization {
            message: "encode original baseline operations".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
    let bytes = aura_core::util::serialization::to_vec(&rows).map_err(|source| {
        AuraError::Serialization {
            message: "encode original baseline fingerprint".into(),
            source: Some(std::sync::Arc::new(source)),
        }
    })?;
    if bytes.len() > Manifest::MAX_BYTES {
        return Err(held_registration_error(
            HeldEnrollmentRegistrationError::BaselineBinding,
        ));
    }
    let count = u32::try_from(ops.len()).map_err(|source| AuraError::Serialization {
        message: "bound original baseline count".into(),
        source: Some(std::sync::Arc::new(source)),
    })?;
    Ok(OriginalEnrollmentBaseline {
        count,
        digest: aura_core::hash::hash(&bytes),
    })
}

/// Process-local capability minted after required original registration storage.
/// It cannot be constructed from an invitation, id, or deserialized state.
pub(crate) struct RegisteredEnrollmentGenerationCapability<'runtime> {
    effects: EnrollmentRuntimeOrigin<'runtime>,
    tracker: crate::runtime::services::ceremony_tracker::CeremonyTracker,
    canonical_invitation: aura_invitation::Invitation,
    // The exact original guards survive the registration write and its outer
    // observation until signed-control custody takes over negative cleanup.
    original_registration: Option<EnrollmentGenerationReservation<'runtime>>,
}
impl<'runtime> RegisteredEnrollmentGenerationCapability<'runtime> {
    pub(in crate::runtime) fn require_original_execution_root(
        &self,
        root: &crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
    ) -> Result<(), AuraError> {
        let original = self.original_registration.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::RequiredOwner)
        })?;
        self.require_effects(original.effects.effects())?;
        self.require_tracker(&original._owner.tracker())?;
        root.origin().require_original_held_reservation(original)
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentGenerationCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn take_original_registration(
        registered: &mut Option<RegisteredEnrollmentGenerationCapability<'runtime>>,
        effects: &AuraEffectSystem,
        pending: &mut Option<EnrollmentGenerationReservation<'runtime>>,
    ) -> Result<(), AuraError> {
        let actual = registered.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::RequiredOwner)
        })?;
        actual.require_effects(effects)?;
        if pending.is_some() {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        }
        let original = actual.original_registration.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::RequiredOwner)
        })?;
        original.require_effects(effects)?;
        original.require_tracker(&actual.tracker)?;
        // No fallible operation or await follows the original custody transfer.
        if let Some(mut actual) = registered.take() {
            *pending = actual.original_registration.take();
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "RegisteredEnrollmentGenerationCapability",
        family = "runtime_helper",
        receiver_type = RegisteredEnrollmentGenerationCapability<'runtime>
    )]
    pub(crate) fn release_original_registration(
        &mut self,
        effects: &AuraEffectSystem,
        root: &crate::runtime::services::enrollment_window::EnrollmentExecutionRoot<
            crate::runtime::services::ceremony_tracker::RegisteredEnrollmentWindowCapability,
        >,
        issued: &crate::handlers::invitation::enrollment_trust::RetainedEnrollmentVmControl,
    ) -> Result<(), AuraError> {
        self.require_effects(effects)?;
        issued.require_runtime_owner(effects)?;
        root.child().require_issued_notice_owner(issued)?;
        let expected = aura_core::util::serialization::to_vec(&self.canonical_invitation)?;
        let actual = aura_core::util::serialization::to_vec(issued.canonical_invitation())?;
        if expected != actual {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let original = self.original_registration.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::RequiredOwner)
        })?;
        original.require_effects(effects)?;
        original.require_tracker(&self.tracker)?;
        // Actual signed control is already bound to the retained root. Only
        // now may its guard-bearing predecessor be released before child work.
        self.original_registration = None;
        Ok(())
    }

    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    pub(crate) fn require_tracker(
        &self,
        tracker: &crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<(), AuraError> {
        self.tracker.require_same_owner(tracker)
    }

    pub(crate) fn canonical_invitation(&self) -> &aura_invitation::Invitation {
        &self.canonical_invitation
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnrollmentAllocationConflict {
    ExistingLiveGeneration,
    ImmutableAllocationAlreadyPresent,
}
#[derive(Debug, thiserror::Error)]
pub(crate) enum HeldEnrollmentRegistrationError {
    #[error(
        "response quorum binding differs: expected {expected_required}/{expected_total}, observed {observed_required}/{observed_total}"
    )]
    ResponsePolicyBinding {
        expected_required: u16,
        expected_total: u16,
        observed_required: u16,
        observed_total: u16,
    },
    #[error("original response quorum differs from its protected roster")]
    ResponsePolicyMismatch,
    #[error("registration uses another physical effect owner")]
    EffectIdentity,
    #[error("persistent enrollment allocation requires the held generation owner")]
    RequiredOwner,
    #[error("registration requires a device enrollment invitation")]
    InvitationKind,
    #[error("original authenticated baseline is absent or diverged")]
    BaselineBinding,
    #[error("registration differs from held enrollment generation bindings")]
    Binding,
    #[error("the immutable original enrollment allocation was already published")]
    AllocationAlreadyPublished {
        origin: EnrollmentAllocationConflict,
    },
    #[error("enrollment execution requires the actual fresh physical allocation acknowledgment")]
    MissingOriginalBirth,
}
pub(crate) fn held_registration_error(reason: HeldEnrollmentRegistrationError) -> AuraError {
    AuraError::Invalid {
        message: "owned enrollment registration rejected".into(),
        source: Some(std::sync::Arc::new(reason)),
    }
}
impl<'runtime> EnrollmentGenerationReservation<'runtime> {
    pub(in crate::runtime) fn require_original_birth(&self) -> Result<(), AuraError> {
        let birth = self.original_birth.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::MissingOriginalBirth)
        })?;
        self.require_effects(self.effects.effects())?;
        if !std::sync::Arc::ptr_eq(
            &birth.runtime_identity,
            &self.effects.crypto.lifetime_owner_identity(),
        ) || birth.original != self.owner
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }

    pub(in crate::runtime) fn take_original_birth(
        &mut self,
    ) -> Result<OriginalEnrollmentAllocationAcknowledgment, AuraError> {
        self.require_original_birth()?;
        self.original_birth.take().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::MissingOriginalBirth)
        })
    }

    pub(crate) fn response_policy(&self) -> Result<EnrollmentResponsePolicy, AuraError> {
        self.owner.response_policy()
    }

    pub(crate) fn require_effects(&self, effects: &AuraEffectSystem) -> Result<(), AuraError> {
        self.generation().require_effects(effects)?;
        if !std::ptr::eq(self.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    pub(crate) fn require_tracker(
        &self,
        tracker: &crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<(), AuraError> {
        self.decision().require_tracker(tracker)
    }
    pub(crate) fn decision(
        &self,
    ) -> &crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<'runtime>
    {
        &self._owner
    }
    pub(crate) fn generation(&self) -> &EnrollmentGenerationCustodyCapability<'_> {
        self._owner.generation()
    }
    pub(crate) fn ceremony_id(&self) -> &aura_core::CeremonyId {
        &self.owner.ceremony
    }
    pub(crate) fn invitation_id(&self) -> &aura_core::InvitationId {
        &self.owner.invitation
    }
    pub(crate) fn pending_epoch(&self) -> u64 {
        self.owner.pending_epoch
    }
    pub(crate) fn setup_digest(&self) -> [u8; 32] {
        self.owner.setup_digest
    }

    pub(crate) fn validate_allocated_registration(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    ) -> Result<(), AuraError> {
        self.validate_held_registration_binding(effects, state, true)
    }

    pub(in crate::runtime) fn validate_held_negative_registration(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    ) -> Result<(), AuraError> {
        self.validate_held_registration_binding(effects, state, false)?;
        if state.terminal_outcome
            != Some(
                crate::runtime::services::ceremony_tracker::CeremonyTerminalOutcome::Failed(
                    crate::runtime::services::ceremony_tracker::CeremonyFailureReason::Cancelled,
                ),
            )
            || !state.has_failed
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }

    fn validate_held_registration_binding(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
        require_pending: bool,
    ) -> Result<(), AuraError> {
        self.require_effects(effects)?;
        if !std::ptr::eq(self.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        let initiator = ParticipantIdentity::device(effects.device_id());
        let expected: std::collections::HashSet<_> = self
            .owner
            .participants
            .iter()
            .filter(|participant| **participant != initiator)
            .cloned()
            .collect();
        self.owner
            .response_policy()?
            .require_exact(state.threshold_k, state.total_n)?;
        if self.owner.registered
            || state.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
            || state.initiator_id != self.owner.authority
            || state.ceremony_id != self.owner.ceremony
            || state.prestate_hash != self.owner.prestate
            || state.new_epoch != self.pending_epoch()
            || state
                .enrollment_device_id
                .map(ParticipantIdentity::device)
                .is_none_or(|device| !expected.contains(&device))
            || state.participants != expected
            || usize::from(state.total_n) != expected.len()
            || state.threshold_k != self.owner.response_policy()?.required()
            || !state.accepted_participants.is_empty()
            || state.is_committed
            || state.is_superseded
            || (require_pending && (state.has_failed || state.terminal_outcome.is_some()))
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }

    /// Authorize registration from this held physical generation allocation.
    pub(crate) fn validate_pending_registration(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
        invitation: &aura_invitation::Invitation,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects.effects(), effects) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        let aura_invitation::InvitationType::DeviceEnrollment {
            subject_authority,
            initiator_device_id,
            device_id,
            ceremony_id,
            pending_epoch,
            setup_binding,
            ..
        } = &invitation.invitation_type
        else {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::InvitationKind,
            ));
        };
        let initiator = ParticipantIdentity::device(effects.device_id());
        let expected: std::collections::HashSet<_> = self
            .owner
            .participants
            .iter()
            .filter(|participant| **participant != initiator)
            .cloned()
            .collect();
        self.owner
            .response_policy()?
            .require_exact(state.threshold_k, state.total_n)?;
        if self.owner.registered
            || invitation.sender_id != self.owner.authority
            || invitation.invitation_id != self.owner.invitation
            || *subject_authority != self.owner.authority
            || *initiator_device_id != effects.device_id()
            || *ceremony_id != self.owner.ceremony
            || *pending_epoch != self.pending_epoch()
            || setup_binding.digest != self.setup_digest()
            || state.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
            || state.initiator_id != self.owner.authority
            || state.ceremony_id != self.owner.ceremony
            || state.prestate_hash != self.owner.prestate
            || state.new_epoch != self.pending_epoch()
            || state.enrollment_device_id != Some(*device_id)
            || !expected.contains(&ParticipantIdentity::device(*device_id))
            || state.participants != expected
            || usize::from(state.total_n) != expected.len()
            || state.threshold_k != self.owner.response_policy()?.required()
            || !state.accepted_participants.is_empty()
            || state.is_committed
            || state.has_failed
            || state.is_superseded
            || state.terminal_outcome.is_some()
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        capability_type = RegisteredEnrollmentGenerationCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn complete_registration_retained(
        held: &mut Option<Self>,
        registered: &mut Option<RegisteredEnrollmentGenerationCapability<'runtime>>,
    ) -> Result<(), AuraError> {
        if registered.is_some() {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        }
        let actual = held.as_ref().ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::RequiredOwner)
        })?;
        let canonical_invitation =
            crate::handlers::invitation::enrollment_trust::verify_generation_registration_binding(
                actual.effects.effects(),
                actual.owner.authority,
                actual.pending_epoch(),
                &actual.owner.ceremony,
                actual.owner.prestate,
                actual.invitation_id(),
                actual.setup_digest(),
            )
            .await
            .map_err(|error| AuraError::Internal {
                message: "complete owned enrollment registration".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        let location = super::enrollment_generation_profile_location(
            &actual.owner.authority,
            actual.pending_epoch(),
        );
        let old = actual
            .effects
            .read_owned_enrollment_generation_profile(
                actual.owner.authority,
                actual.pending_epoch(),
            )
            .await?;
        let expected = serde_json::to_vec(&actual.owner).map_err(|error| AuraError::Internal {
            message: "encode allocated enrollment owner".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        // Only ordinary profile data is cloned. The original reservation and
        // its guards remain in the retained slot through every awaited write.
        let mut registered_owner = actual.owner.clone();
        registered_owner.registered = true;
        let bytes = serde_json::to_vec(&registered_owner).map_err(|error| AuraError::Internal {
            message: "encode registered enrollment owner".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        // The same retained invocation can resume after its own live write
        // committed but observation was dropped. Exact original bytes remain
        // required; neither a different profile nor a reconstructed owner fits.
        if old != expected && old != bytes {
            return Err(AuraError::invalid(
                "allocated enrollment generation was replaced",
            ));
        }
        actual
            .effects
            .require_original_generation_history(&registered_owner)
            .await?;
        #[cfg(test)]
        actual.effects.require_registration_seal_for_test().await?;
        let history = registration_history_location(&actual.owner.ceremony);
        actual
            .effects
            .secure_store_immutable(
                &history,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        let retained = actual
            .effects
            .secure_retrieve(&history, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 131_072 || retained != bytes {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::RegistrationBinding,
            ));
        }
        actual
            .effects
            .secure_store(
                &location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        #[cfg(all(test, not(target_arch = "wasm32")))]
        actual
            .effects
            .await_original_registration_readback_for_test()
            .await?;
        let retained = actual
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 131_072 || retained != bytes {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::RegistrationBinding,
            ));
        }
        let tracker = actual._owner.tracker();
        // No await or callback follows removal from the retained slot. Deposit
        // the actual original capability before returning Ready to the owner.
        let Some(actual) = held.take() else {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::RequiredOwner,
            ));
        };
        let effects = match &actual.effects {
            EnrollmentRuntimeOrigin::Borrowed(effects) => {
                EnrollmentRuntimeOrigin::Borrowed(effects)
            }
            EnrollmentRuntimeOrigin::Owned(effects) => {
                EnrollmentRuntimeOrigin::Owned(Arc::clone(effects))
            }
        };
        *registered = Some(RegisteredEnrollmentGenerationCapability {
            effects,
            tracker,
            canonical_invitation,
            original_registration: Some(actual),
        });
        Ok(())
    }
}
/// Structural attribution for required generation retirement effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnrollmentRetirementStage {
    WrappingSecret,
    ParticipantShare,
    SigningShare,
    SoloPublicPackage,
    PublicPackage,
    ThresholdConfig,
    RetiredReceipt,
    PendingSlot,
}
#[derive(Debug, thiserror::Error)]
#[error("required enrollment retirement {stage:?} failed")]
pub(crate) struct EnrollmentRetirementError {
    pub(crate) stage: EnrollmentRetirementStage,
    #[source]
    pub(crate) source: AuraError,
}
fn retirement_effect_error(stage: EnrollmentRetirementStage, source: AuraError) -> AuraError {
    AuraError::Internal {
        message: "required enrollment retirement effect failed".into(),
        source: Some(std::sync::Arc::new(EnrollmentRetirementError {
            stage,
            source,
        })),
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentGenerationHistoryError {
    #[error("generation live slot differs from protected original allocation")]
    OriginalBinding,
    #[error("generation live slot claims registration without immutable registration evidence")]
    RegistrationBinding,
    #[error("generation already has an authenticated completed retirement decision")]
    Retired,
    #[error("generation history codec failed")]
    Codec(#[source] serde_json::Error),
}
fn generation_history_error(source: EnrollmentGenerationHistoryError) -> AuraError {
    AuraError::Internal {
        message: "require enrollment generation history".into(),
        source: Some(std::sync::Arc::new(source)),
    }
}
fn registration_history_location(ceremony: &aura_core::CeremonyId) -> SecureStorageLocation {
    SecureStorageLocation::new(
        "device_enrollment_generation_registered_v1",
        ceremony.to_string(),
    )
}

impl AuraEffectSystem {
    /// Mutable slots select protected original history; their fields never
    /// establish signer/roster/setup or registration authority by themselves.
    async fn require_original_generation_history(
        &self,
        owner: &StoredEnrollmentGenerationProfile,
    ) -> Result<(), AuraError> {
        if owner.version != 1
            || owner.authority != self.authority_id
            || owner.pending_epoch == 0
            || owner.participants.is_empty()
            || owner.participants.len() > 1024
            || owner.threshold == 0
            || usize::from(owner.threshold) > owner.participants.len()
            || owner
                .participants
                .iter()
                .enumerate()
                .any(|(index, participant)| owner.participants[..index].contains(participant))
        {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        let mut allocated = owner.clone();
        allocated.registered = false;
        let expected = serde_json::to_vec(&allocated).map_err(|source| {
            generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
        })?;
        let actual = self
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "device_enrollment_generation_allocation_v1",
                    owner.ceremony.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if actual.len() > 131_072 || actual != expected {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        Ok(())
    }

    async fn generation_retirement_completed(
        &self,
        owner: &StoredEnrollmentGenerationProfile,
    ) -> Result<bool, AuraError> {
        let orphan = SecureStorageLocation::new(
            "device_enrollment_orphan_cleaned_v1",
            owner.ceremony.to_string(),
        );
        if self.secure_exists(&orphan).await? {
            let bytes = self
                .secure_retrieve(&orphan, &[SecureStorageCapability::Read])
                .await?;
            let expected = serde_json::to_vec(owner).map_err(|source| {
                generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
            })?;
            if bytes.len() > 131_072 || bytes != expected {
                return Err(generation_history_error(
                    EnrollmentGenerationHistoryError::OriginalBinding,
                ));
            }
            return Ok(true);
        }
        let location = SecureStorageLocation::new(
            "device_enrollment_generation_retired_v1",
            owner.ceremony.to_string(),
        );
        if !self.secure_exists(&location).await? {
            return Ok(false);
        }
        let bytes = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 4096 {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        let receipt: (
            u16,
            AuthorityId,
            u64,
            aura_core::CeremonyId,
            aura_core::Hash32,
            [u8; 32],
            [u8; 32],
        ) = serde_json::from_slice(&bytes).map_err(|source| {
            generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
        })?;
        if receipt.0 != 1
            || receipt.1 != owner.authority
            || receipt.2 != owner.pending_epoch
            || receipt.3 != owner.ceremony
            || receipt.4 != owner.prestate
            || serde_json::to_vec(&receipt).map_err(|source| {
                generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
            })? != bytes
        {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        Ok(true)
    }

    async fn validate_generation_profile_bytes(
        &self,
        authority: AuthorityId,
        epoch: u64,
        bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        if bytes.len() > 131_072 {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        let mut owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(bytes).map_err(|source| {
                generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
            })?;
        if owner.authority != authority || owner.pending_epoch != epoch {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        if serde_json::to_vec(&owner).map_err(|source| {
            generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
        })? != bytes
        {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        self.require_original_generation_history(&owner).await?;
        owner.response_policy()?;
        if self.generation_retirement_completed(&owner).await? {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::Retired,
            ));
        }
        let location = registration_history_location(&owner.ceremony);
        let registered_exists = self.secure_exists(&location).await?;
        if !registered_exists {
            if owner.registered {
                return Err(generation_history_error(
                    EnrollmentGenerationHistoryError::RegistrationBinding,
                ));
            }
            return Ok(bytes.to_vec());
        }
        owner.registered = true;
        let expected = serde_json::to_vec(&owner).map_err(|source| {
            generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
        })?;
        let retained = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 131_072 || retained != expected {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::RegistrationBinding,
            ));
        }
        // Registration's immutable first decision wins a crash before the
        // mutable slot phase write. It cannot grant a new allocation interval.
        Ok(expected)
    }

    pub(crate) async fn read_owned_enrollment_generation_profile(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<Vec<u8>, AuraError> {
        let bytes = self
            .secure_retrieve(
                &super::enrollment_generation_profile_location(&authority, epoch),
                &[SecureStorageCapability::Read],
            )
            .await?;
        self.validate_generation_profile_bytes(authority, epoch, &bytes)
            .await
    }

    pub(crate) async fn has_live_enrollment_generation(
        &self,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<bool, AuraError> {
        if self
            .secure_exists(&super::enrollment_generation_profile_location(
                &authority, epoch,
            ))
            .await?
        {
            let bytes = self
                .secure_retrieve(
                    &super::enrollment_generation_profile_location(&authority, epoch),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            self.validate_generation_profile_bytes(authority, epoch, &bytes)
                .await?;
            // Any slot, including interrupted retirement, continues fencing a
            // generic mutation until the required owner releases that slot.
            return Ok(true);
        }
        Ok(false)
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "reserved",
        capability_type = ReservedInvitationIssuance,
        family = "runtime_helper"
    )]
    pub(crate) async fn retain_original_enrollment_reservation(
        &self,
        reserved: &crate::handlers::invitation::ReservedInvitationIssuance,
    ) -> Result<(), AuraError> {
        let (authority, device) = reserved.issuer_binding();
        if !reserved.owns_effects(self)
            || authority != self.authority_id
            || device != self.device_id()
        {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::Owner,
            ));
        }
        let original = StoredOriginalEnrollmentReservation {
            version: 1,
            authority,
            device,
            invitation: reserved.invitation_id().clone(),
            created_at_ms: reserved.created_at_ms(),
        };
        let bytes = serde_json::to_vec(&original).map_err(|source| AuraError::Serialization {
            message: "encode original enrollment reservation".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        if bytes.len() > MAX_ORIGINAL_RESERVATION_BYTES {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::Oversized,
            ));
        }
        let location = SecureStorageLocation::with_sub_key(
            "device_enrollment_original_reservation_v1",
            authority.to_string(),
            reserved.invitation_id().to_string(),
        );
        self.secure_store_immutable(&location, &bytes, &[SecureStorageCapability::Write])
            .await?;
        self.require_original_enrollment_reservation(reserved).await
    }

    #[cfg(test)]
    async fn require_original_baseline(
        &self,
        owner: &StoredEnrollmentGenerationProfile,
    ) -> Result<Vec<aura_core::AttestedOp>, AuraError> {
        let expected = owner.baseline.ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::BaselineBinding)
        })?;
        let ops = self
            .export_tree_ops()
            .await
            .map_err(|source| AuraError::Internal {
                message: "read original authenticated baseline prefix".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        let count = usize::try_from(expected.count).map_err(|source| AuraError::Serialization {
            message: "decode original baseline count".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let prefix = ops.get(..count).ok_or_else(|| {
            held_registration_error(HeldEnrollmentRegistrationError::BaselineBinding)
        })?;
        if original_baseline_fingerprint(prefix)? != expected {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::BaselineBinding,
            ));
        }
        Ok(prefix.to_vec())
    }

    async fn require_original_enrollment_reservation(
        &self,
        reserved: &crate::handlers::invitation::ReservedInvitationIssuance,
    ) -> Result<(), AuraError> {
        let (authority, device) = reserved.issuer_binding();
        if !reserved.owns_effects(self)
            || authority != self.authority_id
            || device != self.device_id()
        {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::Owner,
            ));
        }
        let location = SecureStorageLocation::with_sub_key(
            "device_enrollment_original_reservation_v1",
            authority.to_string(),
            reserved.invitation_id().to_string(),
        );
        let bytes = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > MAX_ORIGINAL_RESERVATION_BYTES {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::Oversized,
            ));
        }
        let retained: StoredOriginalEnrollmentReservation = serde_json::from_slice(&bytes)
            .map_err(|source| AuraError::Serialization {
                message: "decode original enrollment reservation".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        if retained.version != 1
            || retained.authority != authority
            || retained.device != device
            || retained.invitation != *reserved.invitation_id()
            || retained.created_at_ms != reserved.created_at_ms()
        {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::FirstDecision,
            ));
        }
        let canonical =
            serde_json::to_vec(&retained).map_err(|source| AuraError::Serialization {
                message: "reencode original enrollment reservation".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        if canonical != bytes {
            return Err(original_reservation_error(
                OriginalEnrollmentReservationError::FirstDecision,
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn recover_initial_enrollment_allocation<'a>(
        &'a self,
        ceremony: &aura_core::CeremonyId,
        tracker: &'a crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<
        (
            EnrollmentGenerationReservation<'a>,
            crate::runtime::services::ceremony_tracker::TrackedCeremony,
        ),
        AuraError,
    > {
        let decision = tracker.acquire_enrollment_generation_decision(self).await?;
        decision.require_effects(self)?;
        let (owner, original) = self
            .read_initial_enrollment_allocation(ceremony, decision.generation())
            .await?;
        let tree = self.lock_tree_decision().await;
        let device = original
            .enrollment_device_id
            .ok_or_else(|| held_registration_error(HeldEnrollmentRegistrationError::Binding))?;
        let (_, participants, threshold, prestate, _) =
            self.authenticate_current_enrollment_roster(device).await?;
        if participants != owner.participants
            || threshold != owner.threshold
            || prestate != owner.prestate
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        self.require_original_baseline(&owner).await?;
        Ok((
            EnrollmentGenerationReservation {
                effects: EnrollmentRuntimeOrigin::Borrowed(self),
                owner,
                _tree: tree,
                _owner: decision,
                original_birth: None,
            },
            original,
        ))
    }

    /// Reacquire actual custody from the original protected pre-live allocation.
    /// The ceremony is a lookup selector; caller snapshots cannot supply state.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationCustodyCapability",
        family = "runtime_helper"
    )]
    async fn recover_initial_enrollment_allocation_with_custody<'a>(
        &'a self,
        ceremony: &aura_core::CeremonyId,
        guard: EnrollmentGenerationCustodyCapability<'a>,
    ) -> Result<RecoveredEnrollmentCleanupCustody<'a>, AuraError> {
        let (owner, original) = self
            .read_initial_enrollment_allocation(ceremony, &guard)
            .await?;
        Ok(RecoveredEnrollmentCleanupCustody {
            effects: self,
            owner,
            original,
            generation: guard,
        })
    }
    async fn read_initial_enrollment_allocation(
        &self,
        ceremony: &aura_core::CeremonyId,
        generation: &EnrollmentGenerationCustodyCapability<'_>,
    ) -> Result<
        (
            StoredEnrollmentGenerationProfile,
            crate::runtime::services::ceremony_tracker::TrackedCeremony,
        ),
        AuraError,
    > {
        generation.require_effects(self)?;
        let original = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(self, ceremony)
            .await.map_err(|source| AuraError::Internal { message: "read original initial allocation".into(), source: Some(std::sync::Arc::new(source)) })?;
        let bytes = self
            .read_owned_enrollment_generation_profile(self.authority_id, original.new_epoch)
            .await?;
        if bytes.len() > 131_072 {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode original initial allocation custody".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;

        owner.response_policy()?;
        let original_profile_location = SecureStorageLocation::new(
            "device_enrollment_generation_allocation_v1",
            ceremony.to_string(),
        );
        let original_profile = self
            .secure_retrieve(&original_profile_location, &[SecureStorageCapability::Read])
            .await?;
        if original_profile.len() > 131_072 || original_profile != bytes {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        if owner.version != 1
            || owner.registered
            || owner.authority != self.authority_id
            || owner.pending_epoch == 0
            || owner.participants.is_empty()
            || owner.participants.len() > 1024
            || owner.threshold == 0
            || usize::from(owner.threshold) > owner.participants.len()
            || owner
                .participants
                .iter()
                .enumerate()
                .any(|(index, member)| owner.participants[..index].contains(member))
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let epoch_bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", self.authority_id.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let epoch_array: [u8; 8] = epoch_bytes
            .try_into()
            .map_err(|_| held_registration_error(HeldEnrollmentRegistrationError::Binding))?;
        if u64::from_le_bytes(epoch_array).checked_add(1) != Some(owner.pending_epoch) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let expected: std::collections::HashSet<_> = owner
            .participants
            .iter()
            .filter(|participant| **participant != ParticipantIdentity::device(self.device_id()))
            .cloned()
            .collect();
        if owner.ceremony != original.ceremony_id
            || owner.prestate != original.prestate_hash
            || owner.authority != original.initiator_id
            || owner.pending_epoch != original.new_epoch
            || original.participants != expected
            || usize::from(original.total_n) != expected.len()
            || original.threshold_k != owner.response_policy()?.required()
            || original
                .enrollment_device_id
                .map(ParticipantIdentity::device)
                .is_none_or(|device| !expected.contains(&device))
            || original.terminal_outcome.is_some()
            || !original.accepted_participants.is_empty()
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        crate::handlers::invitation::enrollment_trust::validate_original_allocation_setup(
            self,
            &original,
            owner.setup_digest,
        )
        .await
        .map_err(|source| AuraError::Internal {
            message: "validate original independently pinned allocation".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let (package_digest, config_digest) =
            crate::handlers::invitation::enrollment_trust::failed_generation_binding(
                self,
                ceremony,
                original.initiator_id,
                original.new_epoch,
                original.prestate_hash,
            )
            .await
            .map_err(|source| AuraError::Internal {
                message: "validate retained original signing generation".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        let package = self
            .secure_retrieve(
                &Self::threshold_public_key_location(&self.authority_id, original.new_epoch),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let config = self
            .secure_retrieve(
                &SecureStorageLocation::with_sub_key(
                    "threshold_config",
                    self.authority_id.to_string(),
                    original.new_epoch.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if aura_core::hash::hash(&package) != package_digest
            || aura_core::hash::hash(&config) != config_digest
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok((owner, original))
    }

    pub(crate) async fn require_retired_orphan_registration(
        &self,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    ) -> Result<(), AuraError> {
        let key = SecureStorageLocation::new(
            "device_enrollment_orphan_retirement_v1",
            state.ceremony_id.to_string(),
        );
        let bytes = self
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized original orphan decision"));
        }
        let original: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode original orphan decision".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;

        original.response_policy()?;
        let accepted_roster: std::collections::HashSet<_> = original
            .participants
            .iter()
            .filter(|participant| **participant != ParticipantIdentity::device(self.device_id()))
            .cloned()
            .collect();
        if original.version != 1
            || original.registered
            || original.authority != self.authority_id
            || state.initiator_id != original.authority
            || state.new_epoch != original.pending_epoch
            || state.ceremony_id != original.ceremony
            || state.prestate_hash != original.prestate
            || state.participants != accepted_roster
            || state.threshold_k != original.response_policy()?.required()
        {
            return Err(AuraError::invalid(
                "original orphan decision registration mismatch",
            ));
        }
        if self
            .secure_exists(&SecureStorageLocation::new(
                "device_enrollment_activation_v1",
                state.ceremony_id.to_string(),
            ))
            .await?
        {
            return Err(AuraError::invalid(
                "prepared activation cannot observe orphan failure",
            ));
        }
        Ok(())
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn take_enrollment_retirement_fault(&self, epoch: u64) -> bool {
        let mut fault = self
            .enrollment_retirement_fault
            .lock()
            .expect("retirement fault lock");
        if fault
            .as_ref()
            .is_some_and(|(selected, _)| *selected == epoch)
        {
            if fault.as_ref().is_some_and(|(_, scope)| scope.is_none()) {
                *fault = None;
            }
            true
        } else {
            false
        }
    }
    pub(crate) async fn require_registered_enrollment_profile(
        &self,
        authority: AuthorityId,
        epoch: u64,
        ceremony: &aura_core::CeremonyId,
        prestate: aura_core::Hash32,
    ) -> Result<(), AuraError> {
        let bytes = self
            .read_owned_enrollment_generation_profile(authority, epoch)
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized activation owner"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode activation generation owner".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if owner.version != 1
            || !owner.registered
            || owner.authority != authority
            || authority != self.authority_id
            || owner.pending_epoch != epoch
            || owner.ceremony != *ceremony
            || owner.prestate != prestate
        {
            return Err(AuraError::invalid(
                "activation generation registration incomplete",
            ));
        }
        Ok(())
    }
    /// After an immutable first negative decision, recover deletion custody
    /// independently of live material eligibility. This cannot mint a live owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationCustodyCapability",
        family = "runtime_helper"
    )]
    async fn recover_negative_cleanup_from_first_decision<'a>(
        &'a self,
        owner: &StoredEnrollmentGenerationProfile,
        generation: EnrollmentGenerationCustodyCapability<'a>,
    ) -> Result<RecoveredEnrollmentCleanupCustody<'a>, AuraError> {
        generation.require_effects(self)?;
        let verified_owner = owner.clone();
        verified_owner.response_policy()?;
        let owner = &verified_owner;
        self.require_original_generation_history(owner).await?;
        let expected = serde_json::to_vec(owner).map_err(|source| {
            generation_history_error(EnrollmentGenerationHistoryError::Codec(source))
        })?;
        for location in [
            super::enrollment_generation_profile_location(&owner.authority, owner.pending_epoch),
            SecureStorageLocation::new(
                "device_enrollment_orphan_retirement_v1",
                owner.ceremony.to_string(),
            ),
        ] {
            let actual = self
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await?;
            if actual.len() > 131_072 || actual != expected {
                return Err(generation_history_error(
                    EnrollmentGenerationHistoryError::OriginalBinding,
                ));
            }
        }
        let original = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(self, &owner.ceremony)
            .await.map_err(|source| AuraError::Internal { message: "recover original negative cleanup allocation".into(), source: Some(std::sync::Arc::new(source)) })?;
        let participants: std::collections::HashSet<_> = owner
            .participants
            .iter()
            .filter(|participant| **participant != ParticipantIdentity::device(self.device_id()))
            .cloned()
            .collect();
        if owner.registered
            || original.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
            || original.ceremony_id != owner.ceremony
            || original.initiator_id != owner.authority
            || original.prestate_hash != owner.prestate
            || original.new_epoch != owner.pending_epoch
            || original.participants != participants
            || usize::from(original.total_n) != participants.len()
            || original.threshold_k != owner.response_policy()?.required()
            || original
                .enrollment_device_id
                .map(ParticipantIdentity::device)
                .is_none_or(|device| !participants.contains(&device))
            || !original.accepted_participants.is_empty()
            || original.is_committed
        {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        crate::handlers::invitation::enrollment_trust::validate_original_allocation_setup(
            self,
            &original,
            owner.setup_digest,
        )
        .await
        .map_err(|source| AuraError::Internal {
            message: "require original negative cleanup setup binding".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        // The original package/config binding remains protected even if a prior
        // required deletion removed one material record. Remaining records must
        // still match; absence is allowed only on this negative recovery path.
        let (public_digest, config_digest) =
            crate::handlers::invitation::enrollment_trust::failed_generation_binding(
                self,
                &owner.ceremony,
                owner.authority,
                owner.pending_epoch,
                owner.prestate,
            )
            .await
            .map_err(|source| AuraError::Internal {
                message: "require original negative generation binding".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        for (location, digest) in [
            (
                Self::threshold_public_key_location(&owner.authority, owner.pending_epoch),
                public_digest,
            ),
            (
                SecureStorageLocation::with_sub_key(
                    "threshold_config",
                    owner.authority.to_string(),
                    owner.pending_epoch.to_string(),
                ),
                config_digest,
            ),
        ] {
            if self.secure_exists(&location).await? {
                let bytes = self
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await?;
                if bytes.len() > 131_072 || aura_core::hash::hash(&bytes) != digest {
                    return Err(generation_history_error(
                        EnrollmentGenerationHistoryError::OriginalBinding,
                    ));
                }
            }
        }
        Ok(RecoveredEnrollmentCleanupCustody {
            effects: self,
            owner: owner.clone(),
            original,
            generation,
        })
    }

    /// Recover only an allocation for which this issuer never committed an invitation.
    /// A committed invitation with interrupted registration must be reconstructed,
    /// and is never eligible for orphan deletion.
    pub(crate) async fn retire_unissued_enrollment_allocation(&self) -> Result<bool, AuraError> {
        let _generation = self.acquire_enrollment_generation_custody().await;
        let authority = self.authority_id;
        let current = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", authority.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let current = u64::from_le_bytes(
            current
                .try_into()
                .map_err(|_| AuraError::invalid("invalid active signing epoch"))?,
        );
        let epoch = current
            .checked_add(1)
            .ok_or_else(|| AuraError::invalid("pending epoch overflow"))?;
        let profile = super::enrollment_generation_profile_location(&authority, epoch);
        if !self.secure_exists(&profile).await? {
            return Ok(false);
        }
        let bytes = self
            .secure_retrieve(&profile, &[SecureStorageCapability::Read])
            .await?;

        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized generation allocation"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode allocated generation".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;

        owner.response_policy()?;
        if owner.version != 1
            || owner.authority != authority
            || owner.pending_epoch != epoch
            || owner.participants.is_empty()
            || owner.participants.len() > 1024
            || owner.threshold == 0
            || usize::from(owner.threshold) > owner.participants.len()
            || owner
                .participants
                .iter()
                .enumerate()
                .any(|(i, participant)| owner.participants[..i].contains(participant))
        {
            return Err(AuraError::invalid("invalid generation allocation owner"));
        }
        self.require_original_generation_history(&owner).await?;
        if self.generation_retirement_completed(&owner).await? {
            self.secure_delete(&profile, &[SecureStorageCapability::Delete])
                .await
                .map_err(|source| {
                    retirement_effect_error(EnrollmentRetirementStage::PendingSlot, source)
                })?;
            return Ok(true);
        }
        self.validate_generation_profile_bytes(authority, epoch, &bytes)
            .await?;
        if owner.registered {
            return Ok(false);
        }
        // Original mutable custody never replaces the immutable reservation.
        let original_location = SecureStorageLocation::new(
            "device_enrollment_generation_allocation_v1",
            owner.ceremony.to_string(),
        );
        let original = self
            .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
            .await?;
        if original.len() > 131_072 || original != bytes {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let allocated = SecureStorageLocation::new(
            "device_enrollment_allocated_registration_v1",
            owner.ceremony.to_string(),
        );
        let _generation = if self.secure_exists(&allocated).await? {
            // Consume the already-held gate into the same custody validator used
            // by restart admission. No release/reacquire or fresh deadline.
            let first = SecureStorageLocation::new(
                "device_enrollment_orphan_retirement_v1",
                owner.ceremony.to_string(),
            );
            let custody = if self.secure_exists(&first).await? {
                self.recover_negative_cleanup_from_first_decision(&owner, _generation)
                    .await?
            } else {
                self.recover_initial_enrollment_allocation_with_custody(
                    &owner.ceremony,
                    _generation,
                )
                .await?
            };
            if !std::ptr::eq(custody.effects, self)
                || custody.owner.ceremony != owner.ceremony
                || custody.original.ceremony_id != owner.ceremony
            {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::EffectIdentity,
                ));
            }
            custody.generation
        } else {
            _generation
        };
        if self
            .secure_exists(&SecureStorageLocation::new(
                "device_enrollment_activation_v1",
                owner.ceremony.to_string(),
            ))
            .await?
        {
            return Err(AuraError::invalid(
                "prepared activation cannot be retired as orphan",
            ));
        }
        let invitations =
            required_committed_invitation_facts(self.load_committed_facts(authority).await?)?;
        for invitation in invitations {
            if let aura_invitation::InvitationFact::Sent {
                ref invitation_id, ..
            } = invitation
            {
                if invitation_id == &owner.invitation {
                    // The issued canonical entity owns recovery, not orphan cleanup.
                    let aura_invitation::InvitationFact::Sent {
                        context_id,
                        invitation_id,
                        sender_id,
                        receiver_id,
                        invitation_type,
                        sent_at,
                        expires_at,
                        receiver_nickname,
                        message,
                    } = invitation
                    else {
                        return Err(AuraError::invalid(
                            "expected canonical committed invitation",
                        ));
                    };
                    let canonical = aura_invitation::Invitation {
                        invitation_id,
                        context_id,
                        sender_id,
                        receiver_id,
                        invitation_type,
                        created_at: sent_at.ts_ms,
                        expires_at: expires_at.map(|time| time.ts_ms),
                        status: aura_invitation::InvitationStatus::Pending,
                        receiver_nickname,
                        message,
                    };
                    crate::handlers::invitation::enrollment_trust::finish_interrupted_canonical_registration(self,
                        &owner.ceremony, &canonical, owner.prestate, &owner.participants, &owner.response_policy()?).await
                        .map_err(|error| AuraError::Internal { message: "recover committed enrollment registration".into(), source: Some(std::sync::Arc::new(error)) })?;
                    return Ok(false);
                }
            }
        }
        self.retire_original_orphan_allocation(&owner, &_generation)
            .await?;
        Ok(true)
    }

    /// Shared destructive retirement under the already-held original physical
    /// generation custody. Callers validate their actual original profile and
    /// absence of issued/activated ownership; this body never reacquires gates.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "original_orphan_generation",
        capability_type = EnrollmentGenerationCustodyCapability,
        family = "runtime_helper"
    )]
    async fn retire_original_orphan_allocation(
        &self,
        owner: &StoredEnrollmentGenerationProfile,
        generation: &EnrollmentGenerationCustodyCapability<'_>,
    ) -> Result<OriginalOrphanAllocationRetirement, AuraError> {
        generation.require_effects(self)?;
        if owner.registered || owner.authority != self.authority_id || owner.pending_epoch == 0 {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        let authority = owner.authority;
        let epoch = owner.pending_epoch;
        let profile = super::enrollment_generation_profile_location(&authority, epoch);
        let bytes = serde_json::to_vec(owner).map_err(|source| AuraError::Serialization {
            message: "encode original orphan retirement owner".into(),
            source: Some(Arc::new(source)),
        })?;
        // Eligibility is checked before publishing the irreversible first
        // decision. The original generation lease excludes same-generation
        // inventory changes while the following retirement is in flight.
        for participant in &owner.participants {
            if self
                .secure_exists(&Self::participant_wrap_key_location(
                    &authority,
                    epoch,
                    participant,
                ))
                .await?
            {
                return Err(AuraError::invalid(
                    "legacy permanent orphan wrap has no birth retirement policy",
                ));
            }
        }
        {
            let mut custody = self.crypto.allocation_lifetimes.lock().await;
            let inventory = custody.ready().await?;
            for reference in inventory.references() {
                let _: EnrollmentSecretScope =
                    serde_json::from_slice(&reference.scope).map_err(|source| {
                        AuraError::Serialization {
                            message: "validate original orphan birth scope before first decision"
                                .into(),
                            source: Some(Arc::new(source)),
                        }
                    })?;
            }
        }
        let first = SecureStorageLocation::new(
            "device_enrollment_orphan_retirement_v1",
            owner.ceremony.to_string(),
        );
        self.secure_store_immutable(
            &first,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
        let retained = self
            .secure_retrieve(&first, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 131_072 || retained != bytes {
            return Err(AuraError::invalid(
                "orphan retirement owner differs from first decision",
            ));
        }
        let delete = &[SecureStorageCapability::Delete];
        // Ordered inventory survives in the immutable first decision even if
        // config creation was interrupted or a prior cleanup already deleted it.
        for participant in &owner.participants {
            let legacy = Self::participant_wrap_key_location(&authority, epoch, participant);
            if self.secure_exists(&legacy).await? {
                return Err(AuraError::invalid(
                    "legacy permanent orphan wrap has no birth retirement policy",
                ));
            }
            let expected = EnrollmentSecretScope::original(owner, participant, &[])?;
            let mut custody = self.crypto.allocation_lifetimes.lock().await;
            let inventory = custody.ready().await?;
            for reference in inventory.references() {
                let scope: EnrollmentSecretScope = serde_json::from_slice(&reference.scope)
                    .map_err(|source| AuraError::Serialization {
                        message: "decode original orphan birth scope".into(),
                        source: Some(std::sync::Arc::new(source)),
                    })?;
                if scope.same_generation(&expected) {
                    inventory
                        .retire_original(
                            &reference,
                            &OwnedSecretNegativeCapability {
                                runtime_identity: self.crypto.lifetime_owner_identity(),
                                proof: OriginalNegativeSecretOwner::Orphan(generation),
                                original: owner,
                                decision: &retained,
                            },
                        )
                        .await?;
                }
            }
            drop(custody);
            for key in [Self::participant_share_location(
                &authority,
                epoch,
                participant,
            )] {
                if self.secure_exists(&key).await? {
                    self.secure_delete(&key, delete).await?;
                }
            }
        }
        for index in 1..=owner.participants.len() {
            let key = SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{authority}:{epoch}"),
                index.to_string(),
            );
            if self.secure_exists(&key).await? {
                self.secure_delete(&key, delete).await?;
            }
        }
        for key in [
            Self::solo_public_key_location(&authority, epoch),
            SecureStorageLocation::with_sub_key(
                "threshold_pubkey",
                authority.to_string(),
                epoch.to_string(),
            ),
            SecureStorageLocation::with_sub_key(
                "threshold_config",
                authority.to_string(),
                epoch.to_string(),
            ),
        ] {
            if self.secure_exists(&key).await? {
                self.secure_delete(&key, delete).await?;
            }
        }
        #[cfg(all(test, not(target_arch = "wasm32")))]
        if self.take_enrollment_retirement_fault(epoch) {
            return Err(AuraError::Internal {
                message: "injected orphan profile release failure".into(),
                source: Some(std::sync::Arc::new(std::io::Error::other(
                    "retirement storage unavailable",
                ))),
            });
        }
        let cleaned = SecureStorageLocation::new(
            "device_enrollment_orphan_cleaned_v1",
            owner.ceremony.to_string(),
        );
        self.secure_store_immutable(
            &cleaned,
            &bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
        let retained = self
            .secure_retrieve(&cleaned, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 131_072 || retained != bytes {
            return Err(generation_history_error(
                EnrollmentGenerationHistoryError::OriginalBinding,
            ));
        }
        if self.secure_exists(&profile).await? {
            self.secure_delete(&profile, delete).await?;
        }
        Ok(OriginalOrphanAllocationRetirement {
            identity: Arc::new(()),
            runtime_identity: self.crypto.lifetime_owner_identity(),
            ceremony: owner.ceremony.clone(),
            authority,
            epoch,
            original_digest: aura_core::hash::hash(&bytes),
        })
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn fail_next_enrollment_retirement_for_test(&self, epoch: u64) {
        *self
            .enrollment_retirement_fault
            .lock()
            .expect("retirement fault lock") = Some((epoch, None));
    }
    /// Hold the actual epoch's final retirement write refused across all retries.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn refuse_enrollment_retirement_until_released_for_test(
        self: &Arc<Self>,
        epoch: u64,
    ) -> ScopedEnrollmentRetirementRefusal {
        let identity = Arc::new(());
        let mut fault = self
            .enrollment_retirement_fault
            .lock()
            .expect("retirement fault lock");
        assert!(
            fault.is_none(),
            "an original retirement fault is already admitted"
        );
        *fault = Some((epoch, Some(identity.clone())));
        ScopedEnrollmentRetirementRefusal {
            effects: self.clone(),
            identity,
        }
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "physical_generation_custody",
        capability_type = EnrollmentGenerationCustodyCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn acquire_enrollment_generation_custody(
        &self,
    ) -> EnrollmentGenerationCustodyCapability<'_> {
        EnrollmentGenerationCustodyCapability {
            effects: EnrollmentRuntimeOrigin::Borrowed(self),
            _guard: EnrollmentGenerationGuard::Borrowed {
                _guard: self.enrollment_generation_gate.lock().await,
            },
        }
    }
    pub(crate) async fn enrollment_retirement_generation_guard(
        &self,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        self.enrollment_generation_gate.lock().await
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "failed_enrollment_generation",
        capability_type = EnrollmentRetirementCapability,
        family = "runtime_helper"
    )]
    pub(crate) async fn retire_pinned_enrollment_generation(
        &self,
        lease: &crate::runtime::services::ceremony_tracker::EnrollmentRetirementCapability<'_>,
    ) -> Result<AcknowledgedEnrollmentRetirement, AuraError> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct RetiredThresholdConfigMetadata {
            threshold_k: u16,
            total_n: u16,
            #[serde(default)]
            participants: Vec<ParticipantIdentity>,
            mode: SigningMode,
            #[serde(default)]
            agreement_mode: aura_core::threshold::AgreementMode,
        }
        if !std::ptr::eq(self, lease.effects()) {
            return Err(AuraError::invalid(
                "retirement lease belongs to another runtime",
            ));
        }

        let (authority, epoch, package_digest, config_digest) = lease.binding();
        let receipt = |disposition| AcknowledgedEnrollmentRetirement {
            disposition,
            runtime_identity: self.crypto.lifetime_owner_identity(),
            ceremony: lease.ceremony().clone(),
            prestate: lease.prestate(),
            binding: lease.binding(),
        };
        let retired_location = SecureStorageLocation::new(
            "device_enrollment_generation_retired_v1",
            lease.ceremony().to_string(),
        );
        let retired_bytes = serde_json::to_vec(&(
            1_u16,
            authority,
            epoch,
            lease.ceremony(),
            lease.prestate(),
            package_digest,
            config_digest,
        ))
        .map_err(|error| AuraError::Internal {
            message: "encode retired generation receipt".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        if self.secure_exists(&retired_location).await? {
            if self
                .secure_retrieve(&retired_location, &[SecureStorageCapability::Read])
                .await?
                != retired_bytes
            {
                return Err(AuraError::invalid(
                    "retired generation receipt contradiction",
                ));
            }
            let profile = super::enrollment_generation_profile_location(&authority, epoch);
            if !self.secure_exists(&profile).await? {
                // The exact immutable receipt above was written only after all
                // generation secrets were deleted. An absent released profile
                // makes this original failed decision an idempotent observation.
                return Ok(receipt(
                    EnrollmentRetirementStorageDisposition::RequiredWritesAcknowledged,
                ));
            }
            let profile_bytes = self
                .secure_retrieve(&profile, &[SecureStorageCapability::Read])
                .await?;
            if profile_bytes.len() > 131_072 {
                return Err(AuraError::invalid("oversized retired generation profile"));
            }
            let current: StoredEnrollmentGenerationProfile = serde_json::from_slice(&profile_bytes)
                .map_err(|error| AuraError::Internal {
                    message: "decode already retired profile".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
            self.require_original_generation_history(&current).await?;
            // A later owned generation may reuse the epoch after the old release.
            // The old immutable receipt permits observation only, never deletion.
            if current.ceremony != *lease.ceremony() {
                return Ok(receipt(
                    EnrollmentRetirementStorageDisposition::LaterGenerationObservation,
                ));
            }
            if current.authority != authority
                || current.pending_epoch != epoch
                || current.prestate != lease.prestate()
            {
                return Err(generation_history_error(
                    EnrollmentGenerationHistoryError::OriginalBinding,
                ));
            }
            self.secure_delete(&profile, &[SecureStorageCapability::Delete])
                .await
                .map_err(|source| {
                    retirement_effect_error(EnrollmentRetirementStage::PendingSlot, source)
                })?;
            return Ok(receipt(
                EnrollmentRetirementStorageDisposition::RequiredWritesAcknowledged,
            ));
        }
        if authority != self.authority_id {
            return Err(AuraError::invalid("retirement authority mismatch"));
        }
        let active = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", authority.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let active = u64::from_le_bytes(
            active
                .try_into()
                .map_err(|_| AuraError::invalid("invalid active signing epoch"))?,
        );
        if active >= epoch {
            return Err(AuraError::invalid(
                "cannot retire an active signing generation",
            ));
        }
        let profile = super::enrollment_generation_profile_location(&authority, epoch);
        if !self.secure_exists(&profile).await? {
            return Err(AuraError::PermissionDenied {
                message: "original retirement profile and receipt are both missing".into(),
                source: Some(std::sync::Arc::new(
                    EnrollmentGenerationHistoryError::OriginalBinding,
                )),
            });
        }
        let bytes = self
            .read_owned_enrollment_generation_profile(authority, epoch)
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized retirement profile"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode retirement owner profile".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if owner.version != 1
            || owner.authority != authority
            || owner.pending_epoch != epoch
            || owner.ceremony != *lease.ceremony()
            || owner.prestate != lease.prestate()
            || owner.participants.len() > 1024
            || owner.threshold == 0
            || usize::from(owner.threshold) > owner.participants.len()
        {
            return Err(AuraError::invalid("retirement profile generation mismatch"));
        }

        let public = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            authority.to_string(),
            epoch.to_string(),
        );
        let config = SecureStorageLocation::with_sub_key(
            "threshold_config",
            authority.to_string(),
            epoch.to_string(),
        );
        let read = &[SecureStorageCapability::Read];

        let delete = &[SecureStorageCapability::Delete];
        if self.secure_exists(&public).await?
            && aura_core::hash::hash(&self.secure_retrieve(&public, read).await?) != package_digest
        {
            return Err(AuraError::invalid("retirement public package was replaced"));
        }
        if self.secure_exists(&config).await? {
            let bytes = self.secure_retrieve(&config, read).await?;
            let mut metadata: RetiredThresholdConfigMetadata = serde_json::from_slice(&bytes)
                .map_err(|error| AuraError::Internal {
                    message: "decode retired generation config".into(),
                    source: Some(std::sync::Arc::new(error)),
                })?;
            if metadata.participants.len() > 1024 {
                return Err(AuraError::invalid("oversized retired signer inventory"));
            }
            metadata.agreement_mode = aura_core::threshold::AgreementMode::Provisional;
            let bytes = serde_json::to_vec(&metadata).map_err(|error| AuraError::Internal {
                message: "encode retired generation config".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
            if aura_core::hash::hash(&bytes) != config_digest {
                return Err(AuraError::invalid("retirement config was replaced"));
            }
            // Keep config until all shares are deleted, so interrupted cleanup retains its inventory.
            for participant in &metadata.participants {
                // Legacy immutable wraps have no allocation birth policy and
                // cannot acquire retirement authority through migration.
                let legacy = Self::participant_wrap_key_location(&authority, epoch, participant);
                if self.secure_exists(&legacy).await? {
                    return Err(AuraError::invalid(
                        "legacy permanent wrapping secret has no allocation retirement policy",
                    ));
                }
                let expected = EnrollmentSecretScope::original(&owner, participant, &[])?;
                let mut custody = self.crypto.allocation_lifetimes.lock().await;
                let inventory = custody.ready().await?;
                let mut found = false;
                for reference in inventory.references() {
                    let scope: EnrollmentSecretScope = serde_json::from_slice(&reference.scope)
                        .map_err(|source| AuraError::Serialization {
                            message: "decode original failed secret scope".into(),
                            source: Some(std::sync::Arc::new(source)),
                        })?;
                    if scope.same_generation(&expected) {
                        inventory
                            .retire_original(
                                &reference,
                                &OwnedSecretNegativeCapability {
                                    runtime_identity: self.crypto.lifetime_owner_identity(),
                                    proof: OriginalNegativeSecretOwner::Failed(lease),
                                    original: &owner,
                                    decision: &retired_bytes,
                                },
                            )
                            .await
                            .map_err(|source| {
                                retirement_effect_error(
                                    EnrollmentRetirementStage::WrappingSecret,
                                    source,
                                )
                            })?;
                        found = true;
                    }
                }
                if !found {
                    return Err(AuraError::storage(
                        "failed generation lacks original wrapping allocation",
                    ));
                }
                drop(custody);

                let key = SecureStorageLocation::with_sub_key(
                    "participant_shares",
                    format!("{authority}:{epoch}"),
                    participant.storage_key(),
                );
                if self.secure_exists(&key).await? {
                    self.secure_delete(&key, delete).await.map_err(|source| {
                        retirement_effect_error(EnrollmentRetirementStage::ParticipantShare, source)
                    })?;
                }
            }
            for index in 1..=metadata.participants.len() {
                let key = SecureStorageLocation::with_sub_key(
                    "signing_keys",
                    format!("{authority}:{epoch}"),
                    index.to_string(),
                );
                if self.secure_exists(&key).await? {
                    self.secure_delete(&key, delete).await.map_err(|source| {
                        retirement_effect_error(EnrollmentRetirementStage::SigningShare, source)
                    })?;
                }
            }
            let solo_public = Self::solo_public_key_location(&authority, epoch);
            if self.secure_exists(&solo_public).await? {
                self.secure_delete(&solo_public, delete)
                    .await
                    .map_err(|source| {
                        retirement_effect_error(
                            EnrollmentRetirementStage::SoloPublicPackage,
                            source,
                        )
                    })?;
            }
            if self.secure_exists(&public).await? {
                self.secure_delete(&public, delete)
                    .await
                    .map_err(|source| {
                        retirement_effect_error(EnrollmentRetirementStage::PublicPackage, source)
                    })?;
            }
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if self.take_enrollment_retirement_fault(epoch) {
                return Err(AuraError::Internal {
                    message: "injected required retirement deletion failure".into(),
                    source: Some(std::sync::Arc::new(std::io::Error::other(
                        InjectedEnrollmentRetirementFailure,
                    ))),
                });
            }
            self.secure_delete(&config, delete)
                .await
                .map_err(|source| {
                    retirement_effect_error(EnrollmentRetirementStage::ThresholdConfig, source)
                })?;
        } else if self.secure_exists(&public).await? {
            self.secure_delete(&public, delete)
                .await
                .map_err(|source| {
                    retirement_effect_error(EnrollmentRetirementStage::PublicPackage, source)
                })?;
        }
        self.secure_store_immutable(
            &retired_location,
            &retired_bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await
        .map_err(|source| {
            retirement_effect_error(EnrollmentRetirementStage::RetiredReceipt, source)
        })?;
        let retained = self
            .secure_retrieve(&retired_location, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 4096 || retained != retired_bytes {
            return Err(AuraError::invalid(
                "retired generation receipt contradiction",
            ));
        }
        // Profile release is the final required write. Any prior failure remains failclosed.
        self.secure_delete(&profile, delete)
            .await
            .map_err(|source| {
                retirement_effect_error(EnrollmentRetirementStage::PendingSlot, source)
            })?;
        Ok(receipt(
            EnrollmentRetirementStorageDisposition::RequiredWritesAcknowledged,
        ))
    }

    async fn rotate_keys_for_owned_profile(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
        enrollment: Option<(
            &StoredEnrollmentGenerationProfile,
            &AuthenticatedEnrollmentRotationPlan<'_>,
        )>,
        generation: &EnrollmentGenerationCustodyCapability<'_>,
        mut allocation_progress: Option<&mut OriginalEnrollmentAllocationProgress>,
    ) -> Result<RotatedOwnedProfile, AuraError> {
        generation.require_effects(self)?;
        tracing::info!(
            ?authority,
            new_threshold,
            new_total_participants,
            num_participants = participants.len(),
            "Rotating threshold keys via AuraEffectSystem"
        );

        // Validate inputs
        if participants.len() != new_total_participants as usize {
            return Err(AuraError::invalid(format!(
                "Participant count ({}) must match total_participants ({})",
                participants.len(),
                new_total_participants
            )));
        }

        // Get current epoch and calculate new epoch
        let current_epoch = if enrollment.is_some() {
            let bytes = self
                .secure_retrieve(
                    &SecureStorageLocation::new("epoch_state", authority.to_string()),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            u64::from_le_bytes(
                bytes
                    .try_into()
                    .map_err(|_| AuraError::invalid("invalid active signing epoch"))?,
            )
        } else {
            self.get_current_epoch(authority).await
        };

        let new_epoch = current_epoch
            .checked_add(1)
            .ok_or_else(|| AuraError::invalid("signing epoch overflow"))?;
        let profile = super::enrollment_generation_profile_location(authority, new_epoch);
        if self.secure_exists(&profile).await? {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::AllocationAlreadyPublished {
                    origin: EnrollmentAllocationConflict::ExistingLiveGeneration,
                },
            ));
        }
        let original_birth = if let Some((owner, _)) = enrollment {
            // Reserve activation ownership before any pending package becomes visible.
            let mut record = owner.clone();
            record.pending_epoch = new_epoch;
            let bytes = serde_json::to_vec(&record).map_err(|error| AuraError::Internal {
                message: "encode owned enrollment generation profile".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
            let original_location = SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                record.ceremony.to_string(),
            );
            if let Some(progress) = allocation_progress.as_deref_mut() {
                progress.original = Some(record.clone());
                progress.immutable_write_started = true;
            }
            if bytes.len() > 131_072 {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
            let immutable = self
                .secure_store_immutable(
                    &original_location,
                    &bytes,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await?;
            if immutable != aura_core::effects::secure::ImmutableSecureStoreOutcome::Created {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::AllocationAlreadyPublished {
                        origin: EnrollmentAllocationConflict::ImmutableAllocationAlreadyPresent,
                    },
                ));
            }
            if let Some(progress) = allocation_progress.as_deref_mut() {
                progress.immutable_write_acknowledged = true;
            }
            let original = self
                .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
                .await?;
            if original.len() > 131_072 || original != bytes {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
            if let Some(progress) = allocation_progress.as_deref_mut() {
                progress.immutable_readback_acknowledged = true;
                progress.live_write_started = true;
            }
            let created = self
                .secure_create_mutable(
                    &profile,
                    &bytes,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await?;
            if created != aura_core::effects::secure::ImmutableSecureStoreOutcome::Created {
                return Err(AuraError::invalid(
                    "an enrollment already owns this pending generation",
                ));
            }
            if let Some(progress) = allocation_progress.as_deref_mut() {
                progress.live_write_acknowledged = true;
            }
            let mutable = self
                .secure_retrieve(&profile, &[SecureStorageCapability::Read])
                .await?;
            if mutable != bytes {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
            if let Some(progress) = allocation_progress {
                progress.live_readback_acknowledged = true;
            }
            Some(OriginalEnrollmentAllocationAcknowledgment {
                runtime_identity: self.crypto.lifetime_owner_identity(),
                original: record,
            })
        } else {
            None
        };

        tracing::debug!(
            ?authority,
            current_epoch,
            new_epoch,
            "Rotating keys from epoch {} to {}",
            current_epoch,
            new_epoch
        );

        // Generate new threshold keys
        let key_result = if new_threshold >= 2 {
            self.crypto
                .handler()
                .frost_rotate_keys(&[], 0, new_threshold, new_total_participants)
                .await?
        } else {
            let result = self
                .crypto
                .handler()
                .generate_signing_keys(new_threshold, new_total_participants)
                .await?;
            let (key_packages, public_key_package, _mode) = result.into_parts();
            FrostKeyGenResult {
                key_packages,
                public_key_package,
            }
        };

        // Store guardian key packages
        let caps = vec![
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        for (participant, key_package) in participants.iter().zip(key_result.key_packages.iter()) {
            let location = Self::participant_share_location(authority, new_epoch, participant);
            let envelope = if let Some((owner, plan)) = enrollment {
                let mut original = owner.clone();
                original.pending_epoch = new_epoch;
                self.encrypt_owned_enrollment_package(plan, &original, participant, key_package)
                    .await?
            } else {
                self.encrypt_participant_key_package(authority, new_epoch, participant, key_package)
                    .await?
            };
            self.crypto
                .secure_storage()
                .secure_store(&location, &envelope, &caps)
                .await?;
        }

        // Store public key package
        let pub_location = SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", new_epoch),
        );
        self.crypto
            .secure_storage()
            .secure_store(&pub_location, &key_result.public_key_package, &caps)
            .await?;

        // Store threshold config metadata for the new epoch
        self.store_threshold_config_metadata(
            authority,
            new_epoch,
            new_threshold,
            new_total_participants,
            participants,
            if enrollment.is_some() {
                aura_core::threshold::AgreementMode::Provisional
            } else {
                aura_core::threshold::AgreementMode::CoordinatorSoftSafe
            },
        )
        .await?;

        let (key_packages, public_key_package) = key_result.into_parts();
        let material = RotatedKeyMaterial {
            epoch: new_epoch,
            packages: key_packages,
            public: public_key_package,
        };
        Ok(match original_birth {
            Some(original_birth) => RotatedOwnedProfile::Enrollment {
                material,
                original_birth: Box::new(original_birth),
            },
            None => RotatedOwnedProfile::Ordinary(material),
        })
    }

    /// Mint the only live roster/prestate authority under generation then tree custody.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "authenticate_current_enrollment_roster",
        capability_type = UserTransferredEnrollmentSetup,
        family = "runtime_helper"
    )]
    #[cfg(test)]
    pub(crate) async fn prepare_authenticated_enrollment_rotation<'a>(
        &'a self,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        tracker: &'a crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<AuthenticatedEnrollmentRotationPlan<'a>, AuraError> {
        let generation = tracker.acquire_enrollment_generation_decision(self).await?;
        let tree = self.lock_tree_decision().await;
        let (state, participants, threshold, prestate, baseline) = self
            .authenticate_current_enrollment_roster(setup.statement().device)
            .await?;
        Ok(AuthenticatedEnrollmentRotationPlan {
            effects: EnrollmentRuntimeOrigin::Borrowed(self),
            setup_digest: setup.digest(),
            baseline,
            state,
            participants,
            threshold,
            prestate,
            generation,
            tree,
        })
    }
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "authenticate_current_enrollment_roster", capability_type = UserTransferredEnrollmentSetup, family = "runtime_helper")]
    pub(crate) async fn prepare_owned_authenticated_enrollment_rotation(
        self: &Arc<Self>,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        tracker: &crate::runtime::services::ceremony_tracker::CeremonyTracker,
    ) -> Result<AuthenticatedEnrollmentRotationPlan<'static>, AuraError> {
        let generation = tracker
            .retain_owned_enrollment_generation_decision(EnrollmentGenerationCustodyCapability {
                effects: EnrollmentRuntimeOrigin::Owned(Arc::clone(self)),
                _guard: EnrollmentGenerationGuard::Owned {
                    _guard: Arc::clone(&self.enrollment_generation_gate)
                        .lock_owned()
                        .await,
                },
            })
            .await?;
        let tree = self.tree_handler.lock_owned_decision().await;
        let (state, participants, threshold, prestate, baseline) = self
            .authenticate_current_enrollment_roster(setup.statement().device)
            .await?;
        Ok(AuthenticatedEnrollmentRotationPlan {
            effects: EnrollmentRuntimeOrigin::Owned(Arc::clone(self)),
            setup_digest: setup.digest(),
            baseline,
            state,
            participants,
            threshold,
            prestate,
            generation,
            tree,
        })
    }
    // Callers hold the actual generation and tree gates. This helper performs no
    // mutations and never resolves participants from an observational cache.
    async fn authenticate_current_enrollment_roster(
        &self,
        new_device: aura_core::DeviceId,
    ) -> Result<
        (
            aura_journal::commitment_tree::state::TreeState,
            Vec<ParticipantIdentity>,
            u16,
            aura_core::Hash32,
            OriginalEnrollmentBaseline,
        ),
        AuraError,
    > {
        let ops = self
            .export_tree_ops()
            .await
            .map_err(|source| match source {
                crate::core::AgentError::Aura(error) => error,
                source => AuraError::Internal {
                    message: "read authoritative enrollment history".into(),
                    source: Some(std::sync::Arc::new(source)),
                },
            })?;
        self.collect_enrollment_parent_inventory(&ops).await?;
        let state = aura_journal::commitment_tree::reduce(&ops).map_err(|source| {
            AuraError::crypto_with_source(
                "reduce authenticated enrollment roster",
                std::sync::Arc::new(source),
            )
        })?;
        let bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", self.authority_id.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let active: [u8; 8] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|source| AuraError::Serialization {
                    message: "decode enrollment roster active epoch".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
        if state.epoch.value() != u64::from_le_bytes(active) {
            return Err(roster_error(EnrollmentRosterError::ActiveEpochMismatch));
        }
        let metadata = self
            .require_threshold_config_metadata(&self.authority_id, state.epoch.value())
            .await?;
        let mut devices: Vec<_> = state
            .leaves
            .values()
            .filter(|leaf| leaf.role == aura_core::tree::LeafRole::Device)
            .map(|leaf| leaf.device_id)
            .collect();
        if !devices.contains(&self.device_id()) {
            return Err(roster_error(EnrollmentRosterError::CurrentDeviceNotMember));
        }
        if devices.contains(&new_device) {
            return Err(roster_error(EnrollmentRosterError::AlreadyEnrolled));
        }
        if devices.is_empty()
            || devices.len() >= 1024
            || metadata.participants.len() != devices.len()
            || usize::from(metadata.total_n) != devices.len()
            || metadata.threshold_k == 0
            || metadata.threshold_k > metadata.total_n
            || metadata
                .participants
                .iter()
                .any(|participant| match participant {
                    ParticipantIdentity::Device(id) => !devices.contains(id),
                    _ => true,
                })
            || metadata
                .participants
                .iter()
                .enumerate()
                .any(|(index, participant)| metadata.participants[..index].contains(participant))
        {
            return Err(roster_error(EnrollmentRosterError::SigningPolicyMismatch));
        }
        devices.retain(|device| *device != self.device_id());
        devices.sort_by_key(|device| device.to_string());
        devices.insert(0, self.device_id());
        devices.push(new_device);
        let total = u16::try_from(devices.len()).map_err(|source| AuraError::Invalid {
            message: "enrollment roster count".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let threshold = metadata.threshold_k.max(2).min(total);
        let bytes = serde_json::to_vec(&(state.epoch, state.root_commitment, &devices)).map_err(
            |source| AuraError::Serialization {
                message: "encode authenticated enrollment prestate".into(),
                source: Some(std::sync::Arc::new(source)),
            },
        )?;
        let prestate = aura_core::Prestate::new(
            vec![(self.authority_id, aura_core::Hash32(state.root_commitment))],
            aura_core::Hash32(aura_core::hash::hash(&bytes)),
        )
        .map_err(|source| AuraError::Invalid {
            message: "authenticated enrollment prestate".into(),
            source: Some(std::sync::Arc::new(source)),
        })?
        .compute_hash();
        Ok((
            state,
            devices
                .into_iter()
                .map(ParticipantIdentity::device)
                .collect(),
            threshold,
            prestate,
            original_baseline_fingerprint(&ops)?,
        ))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "require_effects",
        capability_type = EnrollmentGenerationReservation,
        family = "runtime_helper"
    )]
    pub(crate) async fn prepare_pinned_enrollment_rotation<'a>(
        &self,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        reserved: &crate::handlers::invitation::ReservedInvitationIssuance,
        ceremony: &aura_core::CeremonyId,
        producer: &mut OriginalEnrollmentAllocationProducer<'a>,
    ) -> Result<
        (
            u64,
            Vec<Vec<u8>>,
            Vec<u8>,
            EnrollmentGenerationReservation<'a>,
        ),
        AuraError,
    > {
        let plan = producer
            .plan
            .as_ref()
            .ok_or_else(|| AuraError::invalid("original allocation plan already transferred"))?;
        plan.require_effects(self, setup)?;
        self.require_original_enrollment_reservation(reserved)
            .await?;
        let prestate = plan.prestate;
        let threshold = plan.threshold;
        let total =
            u16::try_from(plan.participants.len()).map_err(|source| AuraError::Invalid {
                message: "owned roster total".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        let participants = plan.participants.as_slice();
        let now = aura_core::effects::PhysicalTimeEffects::physical_time(self)
            .await
            .map_err(|error| AuraError::Internal {
                message: "check enrollment rotation eligibility".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if now.ts_ms < setup.statement().issued_at_ms
            || now.ts_ms >= setup.statement().expires_at_ms
            || setup.statement().device == self.device_id()
            || participants.len() > 1024
            || participants.len() != usize::from(total)
            || threshold == 0
            || threshold > total
            || participants
                .iter()
                .any(|participant| !matches!(participant, ParticipantIdentity::Device(_)))
            || participants
                .iter()
                .enumerate()
                .any(|(index, participant)| participants[..index].contains(participant))
            || !participants.contains(&ParticipantIdentity::device(setup.statement().device))
            || !participants.contains(&ParticipantIdentity::device(self.device_id()))
        {
            return Err(AuraError::invalid(
                "pinned enrollment rotation eligibility mismatch",
            ));
        }
        if self
            .secure_exists(&SecureStorageLocation::new(
                "device_enrollment_orphan_retirement_v1",
                ceremony.to_string(),
            ))
            .await?
        {
            return Err(AuraError::invalid(
                "retired allocation ceremony cannot be reused",
            ));
        }
        let mut profile_owner = StoredEnrollmentGenerationProfile {
            registered: false,
            version: 1,
            authority: self.authority_id,
            pending_epoch: 0,
            invitation: reserved.invitation_id().clone(),
            ceremony: ceremony.clone(),
            prestate,
            setup_digest: setup.digest(),
            baseline: Some(plan.baseline),
            response_policy: EnrollmentResponsePolicy::select_for_original_signing_policy(
                threshold, total,
            )?,
            threshold,
            participants: participants.to_vec(),
        };
        let rotated = self
            .rotate_keys_for_owned_profile(
                &self.authority_id,
                threshold,
                total,
                participants,
                Some((&profile_owner, plan)),
                plan.generation.generation(),
                Some(&mut producer.progress),
            )
            .await?;
        let RotatedOwnedProfile::Enrollment {
            material:
                RotatedKeyMaterial {
                    epoch,
                    packages,
                    public,
                },
            original_birth,
        } = rotated
        else {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::MissingOriginalBirth,
            ));
        };
        if plan.state.epoch.value().checked_add(1) != Some(epoch) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        profile_owner.pending_epoch = epoch;
        let plan = producer
            .plan
            .take()
            .ok_or_else(|| AuraError::invalid("original allocation plan already transferred"))?;
        Ok((
            epoch,
            packages,
            public,
            EnrollmentGenerationReservation {
                effects: plan.effects,
                owner: profile_owner,
                _owner: plan.generation,
                _tree: plan.tree,
                original_birth: Some(*original_birth),
            },
        ))
    }

    /// Retain the same original reservation while capturing its authenticated
    /// inventory. The borrowed producer performs all validation before the
    /// projection becomes independently retained by signing material.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        family = "runtime_helper"
    )]
    pub(crate) async fn capture_owned_enrollment_final_inventory(
        &self,
        reservation: Arc<EnrollmentGenerationReservation<'static>>,
        baseline: &[aura_core::AttestedOp],
    ) -> Result<EnrollmentFinalVerifierInventoryCapability<'static, 'static>, AuraError> {
        let inventory = self
            .capture_enrollment_final_inventory(reservation.as_ref(), baseline)
            .await?
            .inventory;
        Ok(EnrollmentFinalVerifierInventoryCapability {
            reservation: EnrollmentFinalReservationOrigin::Owned(reservation),
            inventory,
        })
    }

    /// Capture the active root package from the original physical issuer while
    /// its generation/decision/tree reservation is still held. Historical op
    /// parents are authenticated separately and never select the active epoch.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentGenerationReservation",
        family = "runtime_helper"
    )]
    pub(crate) async fn capture_enrollment_final_inventory<'owner, 'runtime>(
        &self,
        reservation: &'owner EnrollmentGenerationReservation<'runtime>,
        baseline: &[aura_core::AttestedOp],
    ) -> Result<EnrollmentFinalVerifierInventoryCapability<'owner, 'runtime>, AuraError> {
        reservation.require_effects(self)?;
        self.collect_enrollment_parent_inventory(baseline).await?;
        let current = self
            .export_tree_ops()
            .await
            .map_err(|source| match source {
                crate::core::AgentError::Aura(error) => error,
                source => AuraError::Internal {
                    message: "read held final inventory baseline".into(),
                    source: Some(std::sync::Arc::new(source)),
                },
            })?;
        if aura_core::util::serialization::to_vec(&current)?
            != aura_core::util::serialization::to_vec(&baseline)?
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::BaselineMismatch,
            ));
        }
        let state = aura_journal::commitment_tree::reduce(baseline).map_err(|source| {
            AuraError::crypto_with_source(
                "reduce authenticated final enrollment head",
                std::sync::Arc::new(source),
            )
        })?;
        // There is presently no retained exact-node package provider for a
        // nonroot branch. Reject it explicitly; do not copy the root package.
        if let Some(node) = state
            .branches
            .keys()
            .find(|node| **node != aura_core::tree::NodeIndex(0))
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::NonrootInventoryUnavailable(*node),
            ));
        }
        for leaf in state.leaves.keys() {
            match state.get_leaf_parent(*leaf) {
                Some(aura_core::tree::NodeIndex(0)) => {}
                Some(node) => {
                    return Err(final_inventory_error(
                        EnrollmentFinalInventoryError::NonrootInventoryUnavailable(node),
                    ));
                }
                None => {
                    return Err(final_inventory_error(
                        EnrollmentFinalInventoryError::MissingLeafParent,
                    ));
                }
            }
        }
        let epoch_bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", self.authority_id.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let epoch = u64::from_le_bytes(epoch_bytes.as_slice().try_into().map_err(|source| {
            AuraError::Serialization {
                message: "decode held final active signing epoch".into(),
                source: Some(std::sync::Arc::new(source)),
            }
        })?);
        if epoch != state.epoch.value() || epoch.checked_add(1) != Some(reservation.pending_epoch())
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::EpochMismatch,
            ));
        }
        let metadata = self
            .require_threshold_config_metadata(&self.authority_id, epoch)
            .await?;
        let (_, threshold, package) = self
            .trusted_tree_parent_verifier_inventory(&self.authority_id, epoch)
            .await?;
        if threshold != metadata.threshold_k
            || metadata.participants.len() != usize::from(metadata.total_n)
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::PolicyMismatch,
            ));
        }
        let devices: std::collections::BTreeSet<_> = state
            .leaves
            .values()
            .filter(|leaf| leaf.role == aura_core::tree::LeafRole::Device)
            .map(|leaf| leaf.device_id)
            .collect();
        if metadata.participants.len() != devices.len()
            || metadata
                .participants
                .iter()
                .any(|participant| match participant {
                    ParticipantIdentity::Device(device) => !devices.contains(device),
                    _ => true,
                })
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::PolicyMismatch,
            ));
        }
        Ok(EnrollmentFinalVerifierInventoryCapability {
            reservation: EnrollmentFinalReservationOrigin::Borrowed(reservation),
            inventory: vec![
                aura_invitation::enrollment_manifest::EnrollmentParentVerifier {
                    epoch,
                    commitment: state.root_commitment,
                    signing_node: aura_core::tree::NodeIndex(0),
                    mode: metadata.mode,
                    threshold,
                    participants: metadata.participants,
                    public_key_package: package,
                    agreement: metadata.agreement_mode,
                },
            ],
        })
    }

    /// Read and validate the actual issuer's historical inventory. Authority
    /// and exact parent references come from the staged local baseline.
    /// Imported history uses an explicitly reverified original confirmation
    /// source. Missing native records never select this path implicitly.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "ConfirmedParentInventoryCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn collect_imported_enrollment_parent_inventory(
        &self,
        archived: &crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<'_>,
        history: &[aura_core::AttestedOp],
    ) -> Result<Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>, AuraError>
    {
        let verified =
            archived.verify_imported_history(self, archived.manifest().subject, history)?;
        archived.require_verified_inventory(&verified)?;
        Ok(verified.parent_inventory().to_vec())
    }

    pub(crate) async fn collect_enrollment_parent_inventory(
        &self,
        ops: &[aura_core::AttestedOp],
    ) -> Result<Vec<aura_invitation::enrollment_manifest::EnrollmentParentVerifier>, AuraError>
    {
        use aura_core::tree::verification::extract_target_node;
        let mut staged = Vec::new();
        let mut result = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for op in ops {
            let state = aura_journal::commitment_tree::reduce(&staged).map_err(|e| {
                AuraError::crypto_with_source(
                    "invalid local enrollment baseline",
                    std::sync::Arc::new(e),
                )
            })?;
            if op.op.parent_epoch != state.epoch || op.op.parent_commitment != state.root_commitment
            {
                return Err(AuraError::crypto("local baseline has divergent parent"));
            }
            let node = extract_target_node(&op.op.op)
                .or_else(|| match &op.op.op {
                    aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                        state.get_remove_leaf_affected_parent(leaf)
                    }
                    _ => None,
                })
                .ok_or_else(|| AuraError::crypto("baseline lacks signing node"))?;
            // The runtime epoch store currently represents the root package.
            // Never reinterpret it as a verifier for an arbitrary branch.
            if node != aura_core::tree::NodeIndex(0) {
                return Err(AuraError::crypto(
                    "enrollment baseline needs a separately retained branch inventory",
                ));
            }
            let epoch = op.op.parent_epoch.value();
            let (key, threshold, package) = self
                .trusted_tree_parent_verifier_inventory(&self.authority_id, epoch)
                .await?;
            if state
                .get_signing_key(&node)
                .is_some_and(|stored| stored != &key)
            {
                return Err(AuraError::crypto(
                    "baseline parent conflicts with locally retained package",
                ));
            }
            aura_core::tree::verify_attested_op(op, &key, threshold, state.epoch).map_err(
                |error| {
                    AuraError::crypto_with_source(
                        "invalid local baseline signature",
                        std::sync::Arc::new(error),
                    )
                },
            )?;
            let metadata = self
                .require_threshold_config_metadata(&self.authority_id, epoch)
                .await?;
            if metadata.participants.len() != usize::from(metadata.total_n)
                || metadata.participants.is_empty()
                || metadata.participants.len() > 1024
                || op.signer_count > metadata.total_n
            {
                return Err(AuraError::crypto(
                    "incomplete exact parent participant inventory",
                ));
            }
            if seen.insert((epoch, op.op.parent_commitment, node)) {
                result.push(
                    aura_invitation::enrollment_manifest::EnrollmentParentVerifier {
                        epoch,
                        commitment: op.op.parent_commitment,
                        signing_node: node,
                        mode: metadata.mode,
                        threshold: metadata.threshold_k,
                        participants: metadata.participants,
                        public_key_package: package,
                        agreement: metadata.agreement_mode,
                    },
                );
            }
            staged.push(op.clone());
        }
        Ok(result)
    }

    pub(super) async fn trusted_tree_parent_verifier(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<(aura_core::tree::BranchSigningKey, u16), AuraError> {
        let (key, threshold, _) = self
            .trusted_tree_parent_verifier_inventory(authority, epoch)
            .await?;
        Ok((key, threshold))
    }

    async fn trusted_tree_parent_verifier_inventory(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<(aura_core::tree::BranchSigningKey, u16, Vec<u8>), AuraError> {
        let metadata = self
            .require_threshold_config_metadata(authority, epoch)
            .await?;
        if metadata.threshold_k == 0 || metadata.threshold_k > metadata.total_n {
            return Err(AuraError::crypto("Invalid trusted parent-epoch threshold"));
        }
        let caps = [SecureStorageCapability::Read];
        let canonical_location = Self::threshold_public_key_location(authority, epoch);
        let location = match metadata.mode {
            SigningMode::SingleSigner => {
                if metadata.threshold_k != 1 || metadata.total_n != 1 {
                    return Err(AuraError::crypto(
                        "Invalid single-signer parent-epoch policy",
                    ));
                }
                // The runtime service owns the canonical layout even for 1-of-1.
                // Legacy selection requires explicit absence of that canonical record;
                // canonical presence commits this read to it, including corrupt bytes.
                if self
                    .crypto
                    .secure_storage()
                    .secure_exists(&canonical_location)
                    .await
                    .map_err(|source| {
                        parent_inventory_storage_error(
                            *authority,
                            epoch,
                            canonical_location.clone(),
                            source,
                        )
                    })?
                {
                    canonical_location
                } else {
                    let legacy_location = Self::solo_public_key_location(authority, epoch);
                    if self
                        .crypto
                        .secure_storage()
                        .secure_exists(&legacy_location)
                        .await
                        .map_err(|source| {
                            parent_inventory_storage_error(
                                *authority,
                                epoch,
                                legacy_location.clone(),
                                source,
                            )
                        })?
                    {
                        legacy_location
                    } else {
                        canonical_location
                    }
                }
            }
            SigningMode::Threshold => canonical_location,
        };
        let package = self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await
            .map_err(|source| {
                parent_inventory_storage_error(*authority, epoch, location, source)
            })?;
        let group_key: [u8; 32] = match metadata.mode {
            SigningMode::SingleSigner => SingleSignerPublicKeyPackage::from_bytes(&package)
                .map_err(|error| {
                    AuraError::crypto_with_source(
                        "Invalid trusted single-signer package",
                        std::sync::Arc::new(error),
                    )
                })?
                .verifying_key()
                .try_into()
                .map_err(|error| {
                    AuraError::crypto_with_source(
                        "Invalid trusted single-signer key length",
                        std::sync::Arc::new(error),
                    )
                })?,
            SigningMode::Threshold => tree_signing::public_key_package_from_bytes(&package)?
                .group_public_key
                .as_slice()
                .try_into()
                .map_err(|error| {
                    AuraError::crypto_with_source(
                        "Invalid trusted threshold key length",
                        std::sync::Arc::new(error),
                    )
                })?,
        };
        Ok((
            aura_core::tree::BranchSigningKey::new(group_key, aura_core::Epoch::new(epoch)),
            metadata.threshold_k,
            package,
        ))
    }

    fn participant_share_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_wrap_key_location(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "participant_share_wrap_keys",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        )
    }

    fn participant_key_package_aad(
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Vec<u8> {
        format!(
            "{}:{}:{}:{}",
            PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN,
            authority,
            epoch,
            participant.storage_key()
        )
        .into_bytes()
    }

    async fn load_or_create_participant_wrap_key(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
    ) -> Result<[u8; 32], AuraError> {
        let location = Self::participant_wrap_key_location(authority, epoch, participant);
        if !self.secure_exists(&location).await? {
            let key = self.random_bytes_32().await;
            self.secure_store_immutable(
                &location,
                &key,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        }
        let bytes = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        bytes
            .try_into()
            .map_err(|_| AuraError::storage("participant wrapping key has invalid length"))
    }

    async fn encrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        key_package: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let wrap_key = self
            .load_or_create_participant_wrap_key(authority, epoch, participant)
            .await?;
        let cipher = ChaCha20Poly1305::new((&wrap_key).into());
        let nonce = self.random_bytes(12).await;
        let aad = Self::participant_key_package_aad(authority, epoch, participant);
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: key_package,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encrypt actual participant envelope",
                    std::sync::Arc::new(source),
                )
            })?;
        let envelope = ParticipantKeyPackageEnvelope {
            version: PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION,
            authority: *authority,
            epoch,
            recipient: participant.clone(),
            nonce,
            ciphertext,
        };
        serde_json::to_vec(&envelope).map_err(|e| AuraError::Serialization {
            message: "encode actual participant envelope".into(),
            source: Some(std::sync::Arc::new(e)),
        })
    }

    pub(crate) async fn decrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        if envelope_bytes.len() > 131_072 {
            let cause = ParticipantEnvelopeBoundsError::EnvelopeTooLarge;
            return Err(AuraError::Invalid {
                message: cause.to_string(),
                source: Some(std::sync::Arc::new(cause)),
            });
        }
        #[derive(Deserialize)]
        struct EnvelopeVersion {
            version: u8,
        }
        let version: EnvelopeVersion =
            serde_json::from_slice(envelope_bytes).map_err(|source| AuraError::Serialization {
                message: "decode participant envelope version".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        if version.version == 2 {
            let owned: AllocationParticipantEnvelope = serde_json::from_slice(envelope_bytes)
                .map_err(|source| AuraError::Serialization {
                    message: "decode owned participant envelope".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
            require_participant_ciphertext_bounds(&owned.ciphertext)?;
            return self
                .decrypt_owned_enrollment_package(authority, epoch, participant, owned)
                .await;
        }
        let envelope: ParticipantKeyPackageEnvelope = serde_json::from_slice(envelope_bytes)
            .map_err(|source| AuraError::Serialization {
                message: "decode retained participant envelope".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        require_participant_ciphertext_bounds(&envelope.ciphertext)?;
        if envelope.version != PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION
            || envelope.authority != *authority
            || envelope.epoch != epoch
            || envelope.recipient != *participant
            || envelope.nonce.len() != 12
        {
            return Err(AuraError::permission_denied(
                "key package envelope metadata does not match storage location",
            ));
        }
        let bytes = self
            .secure_retrieve(
                &Self::participant_wrap_key_location(authority, epoch, participant),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let wrap_key: [u8; 32] = bytes.try_into().map_err(|_| {
            AuraError::storage("retained participant wrapping key has invalid length")
        })?;
        let cipher = ChaCha20Poly1305::new((&wrap_key).into());
        let aad = Self::participant_key_package_aad(authority, epoch, participant);
        cipher
            .decrypt(
                Nonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "decrypt retained participant envelope",
                    std::sync::Arc::new(source),
                )
            })
    }

    async fn signing_mode_for_epoch(&self, authority: &AuthorityId, epoch: u64) -> SigningMode {
        match self.get_threshold_config_metadata(authority, epoch).await {
            Some(metadata) if metadata.threshold_k > 1 => SigningMode::Threshold,
            _ => SigningMode::SingleSigner,
        }
    }

    fn solo_signing_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key("signing_keys", format!("{}:{}", authority, epoch), "1")
    }

    fn solo_public_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::new("signing_keys_public", format!("{}:{}", authority, epoch))
    }

    fn threshold_public_key_location(authority: &AuthorityId, epoch: u64) -> SecureStorageLocation {
        SecureStorageLocation::with_sub_key(
            "threshold_pubkey",
            format!("{}", authority),
            format!("{}", epoch),
        )
    }

    /// Dispatch current private reads from the independently retained original
    /// profile receipt. Matching ids check scope; they do not authorize a new
    /// envelope or select a replacement after required-read failure.
    async fn read_current_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, AuraError> {
        let confirmed = crate::runtime::services::enrollment_profile::load_original_committed_profile_confirmation(self).await?;
        if let Some(confirmed) = &confirmed {
            let manifest = confirmed.confirmation().manifest();
            if manifest.subject == *authority && manifest.pending_epoch == epoch {
                if *participant != ParticipantIdentity::device(manifest.invitee_device) {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
                let public = self
                    .secure_retrieve(
                        &Self::threshold_public_key_location(authority, epoch),
                        &[SecureStorageCapability::Read],
                    )
                    .await?;
                if aura_core::hash::hash(&public) != manifest.pending_public_key_package_digest {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
                let owner = self.load_confirmed_activation_envelope(confirmed).await?;
                return self.decrypt_confirmed_activation_envelope(&owner).await;
            }
        }
        let envelope = self
            .secure_retrieve(location, &[SecureStorageCapability::Read])
            .await?;
        self.decrypt_participant_key_package(authority, epoch, participant, &envelope)
            .await
    }

    /// Validate exact local physical signer policy before any identity secret read.
    pub(crate) async fn require_local_physical_solo_identity_policy(
        &self,
        authority: &AuthorityId,
        epoch: u64,
    ) -> Result<[u8; 32], AuraError> {
        if *authority != self.authority_id {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        let metadata = self
            .require_threshold_config_metadata(authority, epoch)
            .await?;
        if metadata.mode == SigningMode::Threshold {
            // A required quorum is a service boundary only after actual local
            // material agrees with the supported native threshold policy.
            // Public-package DTO defaults cannot establish that policy.
            if metadata.threshold_k < 2
                || metadata.threshold_k > metadata.total_n
                || metadata.total_n > 1024
                || metadata.participants.len() != usize::from(metadata.total_n)
                || metadata
                    .participants
                    .iter()
                    .any(|participant| !matches!(participant, ParticipantIdentity::Device(_)))
                || metadata
                    .participants
                    .iter()
                    .enumerate()
                    .any(|(index, participant)| {
                        metadata.participants[..index].contains(participant)
                    })
            {
                return Err(roster_error(EnrollmentRosterError::SigningPolicyMismatch));
            }
            let local = ParticipantIdentity::device(self.device_id());
            let index = metadata
                .participants
                .iter()
                .position(|participant| participant == &local)
                .ok_or_else(|| AuraError::PermissionDenied {
                    message: "required threshold identity has no current physical participant"
                        .into(),
                    source: Some(std::sync::Arc::new(
                        RequiredSigningParticipantError::Missing,
                    )),
                })?;
            let signer_index = u16::try_from(index + 1).map_err(|source| AuraError::Crypto {
                message: "required threshold identity index exceeds native domain".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
            let public = self
                .secure_retrieve(
                    &Self::threshold_public_key_location(authority, epoch),
                    &[SecureStorageCapability::Read],
                )
                .await?;
            if public.len() > 131_072 {
                return Err(AuraError::crypto_with_source(
                    "required threshold public package exceeds bounds",
                    std::sync::Arc::new(
                        aura_core::crypto::signature_input::SignatureInputError::Bounds,
                    ),
                ));
            }
            let native_public = frost_ed25519::keys::PublicKeyPackage::deserialize(&public)
                .map_err(|source| {
                    AuraError::crypto_with_source(
                        "decode actual required threshold public package",
                        std::sync::Arc::new(source),
                    )
                })?;
            if native_public.verifying_shares().len() != usize::from(metadata.total_n)
                || !(1..=metadata.total_n).all(|index| {
                    frost_ed25519::Identifier::try_from(index)
                        .is_ok_and(|id| native_public.verifying_shares().contains_key(&id))
                })
            {
                return Err(AuraError::crypto_with_source(
                    "required threshold public participant inventory differs from policy",
                    std::sync::Arc::new(
                        tree_signing::RetainedThresholdKeyError::ParticipantInventoryMismatch,
                    ),
                ));
            }
            let key = zeroize::Zeroizing::new(
                self.read_current_participant_key_package(
                    authority,
                    epoch,
                    &local,
                    &Self::participant_share_location(authority, epoch, &local),
                )
                .await?,
            );
            tree_signing::validate_retained_threshold_key_package(
                &key,
                &public,
                signer_index,
                metadata.threshold_k,
                metadata.total_n,
            )
            .map_err(|source| AuraError::Crypto {
                message: "required current threshold identity material contradicts native policy"
                    .into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
            return Err(AuraError::Internal {
                message: "required current threshold identity needs the owned quorum service"
                    .into(),
                source: Some(std::sync::Arc::new(
                    RequiredSigningParticipantError::QuorumOwnerRequired {
                        threshold: metadata.threshold_k,
                    },
                )),
            });
        }
        if metadata.mode != SigningMode::SingleSigner
            || metadata.threshold_k != 1
            || metadata.total_n != 1
            || metadata.participants != vec![ParticipantIdentity::device(self.device_id())]
        {
            return Err(roster_error(EnrollmentRosterError::SigningPolicyMismatch));
        }
        let (key, threshold) = self.trusted_tree_parent_verifier(authority, epoch).await?;
        if threshold != 1 {
            return Err(roster_error(EnrollmentRosterError::SigningPolicyMismatch));
        }
        Ok(*key.group_key())
    }

    pub(crate) async fn lan_discovery_signing_key(
        &self,
        authority: &AuthorityId,
    ) -> Result<[u8; 32], AuraError> {
        let bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", authority.to_string()),
                &[SecureStorageCapability::Read],
            )
            .await?;
        let epoch: [u8; 8] =
            bytes
                .as_slice()
                .try_into()
                .map_err(|source| AuraError::Serialization {
                    message: "decode required LAN signing epoch".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
        let current_epoch = u64::from_le_bytes(epoch);
        let metadata = self
            .require_threshold_config_metadata(authority, current_epoch)
            .await?;
        if metadata.mode != SigningMode::SingleSigner
            || metadata.threshold_k != 1
            || metadata.total_n != 1
            || metadata.participants.len() != 1
        {
            return Err(AuraError::invalid(
                "LAN discovery requires an authoritative single-signer context",
            ));
        }
        let participant = metadata.participants[0].clone();
        match &participant {
            ParticipantIdentity::Device(id) if *id == self.device_id() => {}
            ParticipantIdentity::Guardian(id) if id == authority => {}
            _ => {
                return Err(AuraError::crypto(
                    "LAN signer participant does not own current physical identity",
                ));
            }
        }
        let solo = Self::solo_signing_key_location(authority, current_epoch);
        let location = if self.secure_exists(&solo).await? {
            solo
        } else {
            Self::participant_share_location(authority, current_epoch, &participant)
        };
        let key_package = zeroize::Zeroizing::new(
            self.read_current_participant_key_package(
                authority,
                current_epoch,
                &participant,
                &location,
            )
            .await?,
        );
        let package = SingleSignerKeyPackage::import_from_secure_storage(
            &key_package,
            SecretExportContext::secure_storage(
                "aura-agent::runtime::effects::lan_discovery_signing_key",
            ),
        )
        .map_err(|error| {
            AuraError::crypto_with_source(
                "invalid LAN discovery single-signer identity key",
                std::sync::Arc::new(error),
            )
        })?;
        let signing_key: [u8; 32] = package.signing_key().try_into().map_err(|source| {
            AuraError::crypto_with_source(
                "LAN discovery identity signing key must be 32 bytes",
                std::sync::Arc::new(source),
            )
        })?;
        if signing_key.iter().all(|byte| *byte == 0) {
            return Err(AuraError::invalid(
                "LAN discovery identity signing key must not be all zero",
            ));
        }
        Ok(signing_key)
    }
}

// Implementation of RandomCoreEffects
#[async_trait]
impl RandomCoreEffects for AuraEffectSystem {
    #[allow(clippy::disallowed_methods)]
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        if let Some(provider) = &self.custom_random {
            return provider.random_bytes(len).await;
        }
        self.crypto.random_bytes(len)
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        if let Some(provider) = &self.custom_random {
            return provider.random_bytes_32().await;
        }
        self.crypto.random_32_bytes()
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_u64(&self) -> u64 {
        if let Some(provider) = &self.custom_random {
            return provider.random_u64().await;
        }
        self.crypto.random_u64()
    }
}

// Implementation of CryptoCoreEffects
#[async_trait]
impl CryptoCoreEffects for AuraEffectSystem {
    async fn kdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: u32,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .kdf_derive(ikm, salt, info, output_len)
            .await
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &KeyDerivationContext,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().derive_key(master_key, context).await
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.crypto.handler().ed25519_generate_keypair().await
    }

    async fn ed25519_sign(
        &self,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .ed25519_sign(message, private_key)
            .await
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .ed25519_verify(message, signature, public_key)
            .await
    }

    fn is_simulated(&self) -> bool {
        self.crypto.handler().is_simulated()
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        self.crypto.handler().crypto_capabilities()
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        self.crypto.handler().constant_time_eq(a, b)
    }

    fn secure_zero(&self, data: &mut [u8]) {
        self.crypto.handler().secure_zero(data);
    }
}

// Implementation of CryptoExtendedEffects
#[async_trait]
impl CryptoExtendedEffects for AuraEffectSystem {
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .frost_generate_keys(threshold, max_signers)
            .await
    }

    async fn frost_generate_nonces(
        &self,
        key_package: &[u8],
    ) -> Result<aura_core::effects::crypto::FrostNonces, CryptoError> {
        self.crypto
            .handler()
            .frost_generate_nonces(key_package)
            .await
    }

    async fn frost_create_public_signing_package(
        &self,
        message: &[u8],
        commitments: &[aura_core::effects::crypto::FrostPublicCommitment],
        public_key_package: &[u8],
        threshold: u16,
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto
            .handler()
            .frost_create_public_signing_package(
                message,
                commitments,
                public_key_package,
                threshold,
            )
            .await
    }

    async fn frost_sign_share_for_message(
        &self,
        package: &FrostSigningPackage,
        local_key_share: &[u8],
        nonces: aura_core::effects::crypto::RetiredFrostNonces,
        expected_message: &[u8],
        expected_public_key_package: &[u8],
        expected_threshold: u16,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_sign_share_for_message(
                package,
                local_key_share,
                nonces,
                expected_message,
                expected_public_key_package,
                expected_threshold,
            )
            .await
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_aggregate_signatures(signing_package, signature_shares)
            .await
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .frost_verify(message, signature, public_key)
            .await
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto.handler().ed25519_public_key(private_key).await
    }

    async fn convert_ed25519_to_x25519_public(
        &self,
        ed25519_public_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.crypto
            .handler()
            .convert_ed25519_to_x25519_public(ed25519_public_key)
            .await
    }

    async fn convert_ed25519_to_x25519_private(
        &self,
        ed25519_private_key: &[u8],
    ) -> Result<[u8; 32], CryptoError> {
        self.crypto
            .handler()
            .convert_ed25519_to_x25519_private(ed25519_private_key)
            .await
    }

    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .chacha20_encrypt(plaintext, key, nonce)
            .await
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .chacha20_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_encrypt(plaintext, key, nonce)
            .await
    }

    async fn aes_gcm_encrypt_with_aad(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_encrypt_with_aad(plaintext, key, nonce, aad)
            .await
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_decrypt(ciphertext, key, nonce)
            .await
    }

    async fn aes_gcm_decrypt_with_aad(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
        aad: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .aes_gcm_decrypt_with_aad(ciphertext, key, nonce, aad)
            .await
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<FrostKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .frost_rotate_keys(old_shares, old_threshold, new_threshold, new_max_signers)
            .await
    }

    async fn generate_signing_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .generate_signing_keys(threshold, max_signers)
            .await
    }

    async fn generate_signing_keys_with(
        &self,
        method: KeyGenerationMethod,
        threshold: u16,
        max_signers: u16,
    ) -> Result<SigningKeyGenResult, CryptoError> {
        self.crypto
            .handler()
            .generate_signing_keys_with(method, threshold, max_signers)
            .await
    }

    async fn sign_participant_key_proof(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .sign_participant_key_proof(message, key_package, mode)
            .await
    }

    async fn sign_with_key(
        &self,
        message: &[u8],
        key_package: &[u8],
        mode: SigningMode,
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .sign_with_key(message, key_package, mode)
            .await
    }

    async fn verify_signature(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key_package: &[u8],
        mode: SigningMode,
    ) -> Result<bool, CryptoError> {
        self.crypto
            .handler()
            .verify_signature(message, signature, public_key_package, mode)
            .await
    }
}

#[derive(Debug, thiserror::Error)]
#[error(
    "direct bootstrap requires absent signing material; restoration requires the signing owner"
)]
struct DirectBootstrapRetainedMaterial;

// Implementation of ThresholdSigningEffects
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl aura_core::effects::ThresholdSigningEffects for AuraEffectSystem {
    async fn bootstrap_authority(&self, authority: &AuthorityId) -> Result<Vec<u8>, AuraError> {
        let _generation = self.enrollment_generation_gate.lock().await;
        // This infrastructure primitive may initialize only a truly absent
        // generation. Retained restoration belongs to the signing service.
        let epoch = SecureStorageLocation::new("epoch_state", authority.to_string());
        let config =
            SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0");
        let private =
            SecureStorageLocation::with_sub_key("signing_keys", authority.to_string(), "0");
        let frost = SecureStorageLocation::with_sub_key("frost_keys", authority.to_string(), "0");
        let public = Self::threshold_public_key_location(authority, 0);
        let legacy_public = Self::solo_public_key_location(authority, 0);
        for location in [&epoch, &config, &private, &frost, &public, &legacy_public] {
            if self.secure_exists(location).await? {
                return Err(AuraError::Invalid {
                    message: "direct bootstrap cannot replace retained signing material".into(),
                    source: Some(std::sync::Arc::new(DirectBootstrapRetainedMaterial)),
                });
            }
        }

        // Generate 1-of-1 signing keys (uses Ed25519 for single-signer mode)
        let signing_keys = self.crypto.handler().generate_signing_keys(1, 1).await?;

        // Store key package in secure storage
        // Location varies by mode: signing_keys/ for Ed25519, frost_keys/ for FROST
        let key_prefix = match signing_keys.mode {
            SigningMode::SingleSigner => "signing_keys",
            SigningMode::Threshold => "frost_keys",
        };
        let location = SecureStorageLocation::with_sub_key(
            key_prefix,
            format!("{}:0", authority), // epoch 0
            "1",                        // signer index 1
        );
        let caps = vec![SecureStorageCapability::Write];
        let participant = ParticipantIdentity::device(self.device_id());
        let key_package_envelope = self
            .encrypt_participant_key_package(
                authority,
                0,
                &participant,
                &signing_keys.key_packages[0],
            )
            .await?;
        self.crypto
            .secure_storage()
            .secure_store(&location, &key_package_envelope, &caps)
            .await?;

        // Store public key package in both the legacy single-signer path and the
        // canonical threshold path so runtime bootstrap implementations share one layout.
        let pub_location = SecureStorageLocation::new(
            format!("{}_public", key_prefix),
            format!("{}:0", authority),
        );
        self.crypto
            .secure_storage()
            .secure_store(&pub_location, &signing_keys.public_key_package, &caps)
            .await?;
        self.crypto
            .secure_storage()
            .secure_store(
                &Self::threshold_public_key_location(authority, 0),
                &signing_keys.public_key_package,
                &caps,
            )
            .await?;

        // Store threshold config metadata for epoch 0 (bootstrap case: 1-of-1 single signer)
        self.store_threshold_config_metadata(
            authority,
            0,   // epoch 0
            1,   // threshold
            1,   // total_participants
            &[], // 1-of-1 bootstrap: participant set is implicit (local signer)
            aura_core::threshold::AgreementMode::Provisional,
        )
        .await?;

        // Bootstrap Biscuit authorization tokens
        self.bootstrap_biscuit_tokens(authority).await?;

        let (_key_packages, public_key_package, _mode) = signing_keys.into_parts();
        Ok(public_key_package)
    }

    async fn sign(
        &self,
        context: aura_core::threshold::SigningContext,
    ) -> Result<aura_core::threshold::ThresholdSignature, AuraError> {
        // Signing material has one lifecycle owner through read and signature.
        let _generation = self.enrollment_generation_gate.lock().await;
        let caps = [SecureStorageCapability::Read];
        let epoch_bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new("epoch_state", context.authority.to_string()),
                &caps,
            )
            .await?;
        let epoch: [u8; 8] =
            epoch_bytes
                .as_slice()
                .try_into()
                .map_err(|source| AuraError::Serialization {
                    message: "decode required active signing epoch".into(),
                    source: Some(std::sync::Arc::new(source)),
                })?;
        let current_epoch = u64::from_le_bytes(epoch);
        let metadata = self
            .require_threshold_config_metadata(&context.authority, current_epoch)
            .await?;
        let (key, threshold, public_key_package) = self
            .trusted_tree_parent_verifier_inventory(&context.authority, current_epoch)
            .await?;
        if threshold != metadata.threshold_k {
            return Err(AuraError::crypto_with_source(
                "active signing policy changed",
                std::sync::Arc::new(RequiredSigningParticipantError::KeyMismatch),
            ));
        }
        let mut local = metadata
            .participants
            .iter()
            .filter(|participant| match participant {
                ParticipantIdentity::Device(device) => *device == self.device_id(),
                ParticipantIdentity::Guardian(authority) => *authority == self.authority_id,
                ParticipantIdentity::GroupMember { .. } => false,
            });
        let participant = local.next().ok_or_else(|| AuraError::PermissionDenied {
            message: "required signing participant is not locally owned".into(),
            source: Some(std::sync::Arc::new(
                RequiredSigningParticipantError::Missing,
            )),
        })?;
        if local.next().is_some() {
            return Err(AuraError::PermissionDenied {
                message: "required signing participant is ambiguous".into(),
                source: Some(std::sync::Arc::new(
                    RequiredSigningParticipantError::Ambiguous,
                )),
            });
        }
        // This effect implementation is a single-party signer. A genuine
        // threshold response must come from the owned agreement path; one
        // partial share cannot be represented as completed quorum signing.
        if metadata.mode == SigningMode::Threshold
            || metadata.threshold_k != 1
            || metadata.total_n != 1
        {
            return Err(AuraError::Internal {
                message: "required threshold signing coordinator is unavailable".into(),
                source: Some(std::sync::Arc::new(
                    RequiredSigningParticipantError::QuorumOwnerRequired {
                        threshold: metadata.threshold_k,
                    },
                )),
            });
        }
        let message = threshold_signing_context_transcript_bytes(&context, current_epoch).map_err(
            |source| AuraError::Serialization {
                message: "encode required signing context transcript".into(),
                source: Some(std::sync::Arc::new(source)),
            },
        )?;
        let canonical =
            Self::participant_share_location(&context.authority, current_epoch, participant);
        let location = if self.secure_exists(&canonical).await? {
            canonical
        } else {
            Self::solo_signing_key_location(&context.authority, current_epoch)
        };
        let key_package = zeroize::Zeroizing::new(
            self.read_current_participant_key_package(
                &context.authority,
                current_epoch,
                participant,
                &location,
            )
            .await?,
        );
        let package = SingleSignerKeyPackage::import_from_secure_storage(
            &key_package,
            SecretExportContext::secure_storage("aura-agent::required-active-effect-signer"),
        )
        .map_err(|source| {
            AuraError::crypto_with_source(
                "decode required local signing package",
                std::sync::Arc::new(source),
            )
        })?;
        if package.verifying_key() != key.group_key().as_slice() {
            return Err(AuraError::crypto_with_source(
                "required signing key disagrees with retained public package",
                std::sync::Arc::new(RequiredSigningParticipantError::KeyMismatch),
            ));
        }
        let derived = self
            .crypto
            .handler()
            .ed25519_public_key(package.signing_key())
            .await?;
        if derived.as_slice() != key.group_key().as_slice() {
            return Err(AuraError::crypto_with_source(
                "required signing secret disagrees with retained public package",
                std::sync::Arc::new(RequiredSigningParticipantError::KeyMismatch),
            ));
        }
        let signature = self
            .crypto
            .handler()
            .sign_with_key(&message, &key_package, SigningMode::SingleSigner)
            .await
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "required single-signer signing failed",
                    std::sync::Arc::new(source),
                )
            })?;
        Ok(aura_core::threshold::ThresholdSignature::single_signer(
            signature,
            public_key_package,
            current_epoch,
        ))
    }

    async fn threshold_config(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdConfig> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold config metadata for this epoch
        self.get_threshold_config_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdConfig {
                threshold: metadata.threshold_k,
                total_participants: metadata.total_n,
            })
    }

    async fn threshold_state(
        &self,
        authority: &AuthorityId,
    ) -> Option<aura_core::threshold::ThresholdState> {
        // Get current epoch for this authority
        let current_epoch = self.get_current_epoch(authority).await;

        // Retrieve stored threshold config metadata for this epoch
        self.get_threshold_config_metadata(authority, current_epoch)
            .await
            .map(|metadata| aura_core::threshold::ThresholdState {
                epoch: current_epoch,
                threshold: metadata.threshold_k,
                total_participants: metadata.total_n,
                participants: metadata.resolved_participants(),
                agreement_mode: metadata.agreement_mode,
            })
    }

    async fn has_signing_capability(&self, authority: &AuthorityId) -> bool {
        let current_epoch = self.get_current_epoch(authority).await;
        let location = match self.signing_mode_for_epoch(authority, current_epoch).await {
            SigningMode::SingleSigner => Self::solo_signing_key_location(authority, current_epoch),
            SigningMode::Threshold => Self::participant_share_location(
                authority,
                current_epoch,
                &ParticipantIdentity::guardian(*authority),
            ),
        };
        self.crypto
            .secure_storage()
            .secure_exists(&location)
            .await
            .unwrap_or(false)
    }

    async fn public_key_package(&self, authority: &AuthorityId) -> Option<Vec<u8>> {
        let current_epoch = self.get_current_epoch(authority).await;
        let caps = vec![SecureStorageCapability::Read];
        match self.signing_mode_for_epoch(authority, current_epoch).await {
            SigningMode::SingleSigner => match self
                .crypto
                .secure_storage()
                .secure_retrieve(
                    &Self::solo_public_key_location(authority, current_epoch),
                    &caps,
                )
                .await
            {
                Ok(bytes) => Some(bytes),
                Err(_) => self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::threshold_public_key_location(authority, current_epoch),
                        &caps,
                    )
                    .await
                    .ok(),
            },
            SigningMode::Threshold => self
                .crypto
                .secure_storage()
                .secure_retrieve(
                    &Self::threshold_public_key_location(authority, current_epoch),
                    &caps,
                )
                .await
                .ok(),
        }
    }

    async fn rotate_keys(
        &self,
        authority: &AuthorityId,
        threshold: u16,
        total: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
        let owner = self.acquire_enrollment_generation_custody().await;
        match self
            .rotate_keys_for_owned_profile(
                authority,
                threshold,
                total,
                participants,
                None,
                &owner,
                None,
            )
            .await?
        {
            RotatedOwnedProfile::Ordinary(material) => {
                Ok((material.epoch, material.packages, material.public))
            }
            RotatedOwnedProfile::Enrollment { .. } => Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            )),
        }
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        let _generation = self.enrollment_generation_gate.lock().await;
        if self
            .has_live_enrollment_generation(*authority, new_epoch)
            .await?
        {
            return Err(AuraError::invalid(
                "enrollment activation requires its verified decision lease",
            ));
        }
        if self.secure_exists(&crate::handlers::invitation::enrollment_manifest_admission::imported_generation_location(authority,new_epoch)).await? {
            return Err(AuraError::invalid("imported enrollment activation requires its durable verified committed confirmation"));
        }
        tracing::info!(
            ?authority,
            new_epoch,
            "Committing key rotation via AuraEffectSystem"
        );
        // Activate the new epoch by updating the current epoch state
        self.set_current_epoch(authority, new_epoch).await?;
        tracing::debug!(
            ?authority,
            new_epoch,
            "Epoch state updated - new keys are now active"
        );
        Ok(())
    }

    async fn rollback_key_rotation(
        &self,
        authority: &AuthorityId,
        failed_epoch: u64,
    ) -> Result<(), AuraError> {
        let _generation = self.enrollment_generation_gate.lock().await;
        if self
            .has_live_enrollment_generation(*authority, failed_epoch)
            .await?
        {
            return Err(AuraError::invalid(
                "generic rollback cannot retire an enrollment-owned generation",
            ));
        }
        tracing::warn!(
            ?authority,
            failed_epoch,
            "Rolling back key rotation via AuraEffectSystem"
        );
        // Delete orphaned keys from the failed epoch to prevent storage leakage
        self.delete_epoch_keys(authority, failed_epoch).await?;
        tracing::info!(
            ?authority,
            failed_epoch,
            "Successfully deleted orphaned keys from failed rotation"
        );
        Ok(())
    }
}

// Intended inside runtime/effects/crypto.rs, not a public constructor module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentSecretScope {
    version: u16,
    authority: AuthorityId,
    epoch: u64,
    ceremony: aura_core::CeremonyId,
    invitation: aura_core::InvitationId,
    original_profile_digest: [u8; 32],
    participant: ParticipantIdentity,
    package_digest: [u8; 32],
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AllocationParticipantEnvelope {
    version: u8,
    scope: EnrollmentSecretScope,
    allocation: aura_core::effects::secret_lifetime::SecretAllocationReference,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}
impl EnrollmentSecretScope {
    fn confirmed(
        confirmed: &crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability,
    ) -> Result<Self, AuraError> {
        let proof = confirmed.confirmation();
        let manifest = proof.manifest();
        Ok(Self {
            version: 2,
            authority: manifest.subject,
            epoch: manifest.pending_epoch,
            ceremony: manifest.ceremony.clone(),
            invitation: manifest.invitation.clone(),
            original_profile_digest: proof.manifest_digest(),
            participant: ParticipantIdentity::device(manifest.invitee_device),
            package_digest: manifest.pending_share_digest,
        })
    }

    fn original(
        owner: &StoredEnrollmentGenerationProfile,
        participant: &ParticipantIdentity,
        package: &[u8],
    ) -> Result<Self, AuraError> {
        let mut original = owner.clone();
        original.registered = false;
        let bytes = serde_json::to_vec(&original).map_err(|source| AuraError::Serialization {
            message: "encode original secret birth binding".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        Ok(Self {
            version: 1,
            authority: owner.authority,
            epoch: owner.pending_epoch,
            ceremony: owner.ceremony.clone(),
            invitation: owner.invitation.clone(),
            original_profile_digest: aura_core::hash::hash(&bytes),
            participant: participant.clone(),
            package_digest: aura_core::hash::hash(package),
        })
    }
    fn same_generation(&self, expected: &Self) -> bool {
        self.version == expected.version
            && self.authority == expected.authority
            && self.epoch == expected.epoch
            && self.ceremony == expected.ceremony
            && self.invitation == expected.invitation
            && self.original_profile_digest == expected.original_profile_digest
            && self.participant == expected.participant
    }
    fn encode(&self) -> Result<Vec<u8>, AuraError> {
        serde_json::to_vec(self).map_err(|source| AuraError::Serialization {
            message: "encode original allocation scope".into(),
            source: Some(std::sync::Arc::new(source)),
        })
    }
}
impl AuraEffectSystem {
    // The authenticated plan remains held through birth and private envelope
    // publication; physical generation custody alone cannot authorize this scope.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "AuthenticatedEnrollmentRotationPlan",
        family = "runtime_helper"
    )]
    async fn encrypt_owned_enrollment_package(
        &self,
        plan: &AuthenticatedEnrollmentRotationPlan<'_>,
        owner: &StoredEnrollmentGenerationProfile,
        participant: &ParticipantIdentity,
        package: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        plan.generation.require_effects(self)?;
        if !std::ptr::eq(plan.effects.effects(), self)
            || owner.authority != self.authority_id
            || owner.prestate != plan.prestate
            || owner.setup_digest != plan.setup_digest
            || owner.participants != plan.participants
            || owner.threshold != plan.threshold
            || plan.state.epoch.value().checked_add(1) != Some(owner.pending_epoch)
            || !owner.participants.contains(participant)
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        self.require_original_generation_history(owner).await?;
        let scope = EnrollmentSecretScope::original(owner, participant, package)?;
        let wrap = zeroize::Zeroizing::new(self.random_bytes_32().await);
        let mut custody = self.crypto.allocation_lifetimes.lock().await;
        let inventory = custody.ready().await?;
        let allocation = inventory
            .fresh_birth(
                &OwnedSecretBirthCapability {
                    runtime_identity: self.crypto.lifetime_owner_identity(),
                    origin: OriginalSecretBirthOrigin::Plan(plan),
                    scope: scope.clone(),
                },
                wrap.as_ref(),
            )
            .await?;
        drop(custody);
        #[cfg(all(test, not(target_arch = "wasm32")))]
        self.await_original_allocation_birth_for_test(&allocation)
            .await?;
        let nonce = self.random_bytes(12).await;
        let aad = serde_json::to_vec(&(
            "aura:participant-allocation-envelope:v2",
            &scope,
            &allocation,
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode allocation envelope AAD".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let cipher = ChaCha20Poly1305::new((&*wrap).into());
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: package,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "encrypt owned pending participant package",
                    std::sync::Arc::new(source),
                )
            })?;
        serde_json::to_vec(&AllocationParticipantEnvelope {
            version: 2,
            scope,
            allocation,
            nonce,
            ciphertext,
        })
        .map_err(|source| AuraError::Serialization {
            message: "encode allocation envelope".to_string(),
            source: Some(std::sync::Arc::new(source)),
        })
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "OwnedSecretReadCapability",
        family = "proof_issuer"
    )]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    async fn verify_owned_enrollment_package_reader(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope: &AllocationParticipantEnvelope,
    ) -> Result<OwnedSecretReadCapability, AuraError> {
        if envelope.version != 2
            || envelope.scope.authority != *authority
            || envelope.scope.epoch != epoch
            || envelope.scope.participant != *participant
            || envelope.nonce.len() != 12
            || envelope.allocation.scope != envelope.scope.encode()?
        {
            return Err(AuraError::invalid(
                "owned package scope differs from requested participant",
            ));
        }
        // Required immutable original allocation is independent of the envelope.
        // This read verifies an existing actor-owned birth, never mints an owner.
        let bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "device_enrollment_generation_allocation_v1",
                    envelope.scope.ceremony.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized original package birth"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode original package birth".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        self.require_original_generation_history(&owner).await?;
        let expected = EnrollmentSecretScope::original(&owner, participant, &[])?;
        if !envelope.scope.same_generation(&expected) || owner.authority != self.authority_id {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }

        Ok(OwnedSecretReadCapability {
            runtime_identity: self.crypto.lifetime_owner_identity(),
            scope: envelope.scope.clone(),
        })
    }
    async fn decrypt_owned_enrollment_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope: AllocationParticipantEnvelope,
    ) -> Result<Vec<u8>, AuraError> {
        let reader = self
            .verify_owned_enrollment_package_reader(authority, epoch, participant, &envelope)
            .await?;
        let mut custody = self.crypto.allocation_lifetimes.lock().await;
        let inventory = custody.ready().await?;
        let key = zeroize::Zeroizing::new(
            inventory
                .read_original(&envelope.allocation, &reader)
                .await?,
        );
        let key: &[u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| AuraError::storage("owned wrapping key length"))?;
        let aad = serde_json::to_vec(&(
            "aura:participant-allocation-envelope:v2",
            &envelope.scope,
            &envelope.allocation,
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode retained allocation AAD".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let cipher = ChaCha20Poly1305::new(key.into());
        let package = cipher
            .decrypt(
                Nonce::from_slice(&envelope.nonce),
                Payload {
                    msg: &envelope.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "decrypt owned participant package",
                    std::sync::Arc::new(source),
                )
            })?;
        if aura_core::hash::hash(&package) != envelope.scope.package_digest {
            return Err(AuraError::crypto(
                "original participant package digest mismatch",
            ));
        }
        Ok(package)
    }
}

impl AuraEffectSystem {
    /// Provider lifetime state follows the genuine held positive domain owner.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "EnrollmentActivationCapability",
        family = "runtime_helper"
    )]
    pub(crate) async fn seal_owned_enrollment_wrapping_allocations(
        &self,
        activation: &crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'_>,
        authority: AuthorityId,
        epoch: u64,
    ) -> Result<(), AuraError> {
        activation.require_effects(self)?;
        activation.require_generation(authority, epoch).await?;
        let bytes = self
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "device_enrollment_generation_allocation_v1",
                    activation.ceremony_id().to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized activation secret birth"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|source| AuraError::Serialization {
                message: "decode original activation secret birth".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        if owner.authority != authority
            || owner.pending_epoch != epoch
            || owner.ceremony != *activation.ceremony_id()
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        self.require_original_generation_history(&owner).await?;
        // Positive publication was already made immutable by the finalizer;
        // verified response alone cannot invent a durable positive decision.
        let positive = self
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "device_enrollment_activation_v1",
                    owner.ceremony.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await?;
        if positive.is_empty() || positive.len() > 1_048_576 {
            return Err(AuraError::invalid("invalid original activation decision"));
        }
        let positive = serde_json::to_vec(&(
            1_u16,
            "enrollment-activated",
            owner.authority,
            owner.pending_epoch,
            &owner.ceremony,
            aura_core::hash::hash(&positive),
        ))
        .map_err(|source| AuraError::Serialization {
            message: "encode bounded positive allocation decision".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let mut custody = self.crypto.allocation_lifetimes.lock().await;
        let inventory = custody.ready().await?;
        for participant in &owner.participants {
            let expected = EnrollmentSecretScope::original(&owner, participant, &[])?;
            let mut found = false;
            for reference in inventory.references() {
                let scope: EnrollmentSecretScope = serde_json::from_slice(&reference.scope)
                    .map_err(|source| AuraError::Serialization {
                        message: "decode owned original secret scope".into(),
                        source: Some(std::sync::Arc::new(source)),
                    })?;
                if scope.same_generation(&expected) {
                    inventory
                        .seal_positive(
                            &reference,
                            &OwnedSecretPositiveCapability {
                                runtime_identity: self.crypto.lifetime_owner_identity(),
                                origin: OriginalSecretPositiveOrigin::Issuer {
                                    activation,
                                    original: &owner,
                                },
                                decision: &positive,
                            },
                        )
                        .await?;
                    found = true;
                }
            }
            if !found {
                return Err(AuraError::storage(
                    "positive activation lacks original wrapping allocation",
                ));
            }
        }
        Ok(())
    }
}

/// Borrowed original birth authorization from a held rotation plan or a
/// reverified confirmed-import envelope owner.
pub(in crate::runtime) struct OwnedSecretBirthCapability<'a, 'owner> {
    runtime_identity: std::sync::Arc<()>,
    origin: OriginalSecretBirthOrigin<'a, 'owner>,
    scope: EnrollmentSecretScope,
}
enum OriginalSecretBirthOrigin<'a, 'owner> {
    Plan(&'a AuthenticatedEnrollmentRotationPlan<'owner>),
    Confirmed(&'a confirmed_activation::ConfirmedActivationEnvelopeCapability<'owner>),
}
impl OwnedSecretBirthCapability<'_, '_> {
    pub(in crate::runtime) fn require_runtime_owner(
        &self,
        identity: &std::sync::Arc<()>,
    ) -> Result<(), AuraError> {
        if !std::sync::Arc::ptr_eq(&self.runtime_identity, identity) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    pub(in crate::runtime) fn scope_bytes(&self) -> Result<Vec<u8>, AuraError> {
        match &self.origin {
            OriginalSecretBirthOrigin::Plan(plan) => {
                plan.generation.require_effects(plan.effects.effects())?;
            }
            OriginalSecretBirthOrigin::Confirmed(owner) => {
                let expected = EnrollmentSecretScope::confirmed(owner.confirmed())?;
                if self.scope != expected {
                    return Err(AuraError::invalid("confirmed original birth scope differs"));
                }
            }
        }
        self.scope.encode()
    }
}
/// Negative infrastructure publication always borrows the genuine domain owner.
pub(in crate::runtime) struct OwnedSecretNegativeCapability<'a, 'owner> {
    runtime_identity: std::sync::Arc<()>,
    proof: OriginalNegativeSecretOwner<'a, 'owner>,
    original: &'a StoredEnrollmentGenerationProfile,
    decision: &'a [u8],
}
enum OriginalNegativeSecretOwner<'a, 'owner> {
    Failed(&'a crate::runtime::services::ceremony_tracker::EnrollmentRetirementCapability<'owner>),
    Orphan(&'a EnrollmentGenerationCustodyCapability<'owner>),
}
impl OwnedSecretNegativeCapability<'_, '_> {
    pub(in crate::runtime) fn require_runtime_owner(
        &self,
        identity: &std::sync::Arc<()>,
    ) -> Result<(), AuraError> {
        if !std::sync::Arc::ptr_eq(&self.runtime_identity, identity) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    pub(in crate::runtime) fn require_scope(&self, bytes: &[u8]) -> Result<(), AuraError> {
        match &self.proof {
            OriginalNegativeSecretOwner::Failed(lease) => {
                if lease.ceremony() != &self.original.ceremony
                    || lease.prestate() != self.original.prestate
                {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
                let (authority, epoch, _, _) = lease.binding();
                if authority != self.original.authority || epoch != self.original.pending_epoch {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
            }
            OriginalNegativeSecretOwner::Orphan(generation) => {
                if generation.effects.effects().authority_id != self.original.authority {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::EffectIdentity,
                    ));
                }
            }
        }
        let scope: EnrollmentSecretScope =
            serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                message: "decode negative original allocation scope".into(),
                source: Some(std::sync::Arc::new(source)),
            })?;
        if !self.original.participants.contains(&scope.participant)
            || !scope.same_generation(&EnrollmentSecretScope::original(
                self.original,
                &scope.participant,
                &[],
            )?)
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok(())
    }
    pub(in crate::runtime) fn decision(&self) -> &[u8] {
        self.decision
    }
}

/// Read custody binds an already recovered owner to original immutable birth.
pub(in crate::runtime) struct OwnedSecretReadCapability {
    runtime_identity: std::sync::Arc<()>,
    scope: EnrollmentSecretScope,
}
impl OwnedSecretReadCapability {
    pub(in crate::runtime) fn require_runtime_owner(
        &self,
        identity: &std::sync::Arc<()>,
    ) -> Result<(), AuraError> {
        if !std::sync::Arc::ptr_eq(&self.runtime_identity, identity) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }
    pub(in crate::runtime) fn require_scope(&self, bytes: &[u8]) -> Result<(), AuraError> {
        if self.scope.encode()? != bytes {
            return Err(AuraError::invalid(
                "owned reader differs from original birth scope",
            ));
        }
        Ok(())
    }
}
/// Positive publication borrows the original issuer activation or the exact
/// reverified confirmed-import envelope owner and its immutable decision.
pub(in crate::runtime) struct OwnedSecretPositiveCapability<'a, 'owner> {
    runtime_identity: std::sync::Arc<()>,
    origin: OriginalSecretPositiveOrigin<'a, 'owner>,
    decision: &'a [u8],
}
enum OriginalSecretPositiveOrigin<'a, 'owner> {
    Issuer {
        activation:
            &'a crate::runtime::services::ceremony_tracker::EnrollmentActivationCapability<'owner>,
        original: &'a StoredEnrollmentGenerationProfile,
    },
    Confirmed(&'a confirmed_activation::ConfirmedActivationEnvelopeCapability<'owner>),
}
impl OwnedSecretPositiveCapability<'_, '_> {
    pub(in crate::runtime) fn require_runtime_owner(
        &self,
        identity: &std::sync::Arc<()>,
    ) -> Result<(), AuraError> {
        if !std::sync::Arc::ptr_eq(&self.runtime_identity, identity) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::EffectIdentity,
            ));
        }
        Ok(())
    }

    pub(in crate::runtime) fn require_scope(&self, bytes: &[u8]) -> Result<(), AuraError> {
        match &self.origin {
            OriginalSecretPositiveOrigin::Issuer {
                activation,
                original,
            } => {
                if activation.ceremony_id() != &original.ceremony {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
                let scope: EnrollmentSecretScope =
                    serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                        message: "decode positive original birth scope".into(),
                        source: Some(Arc::new(source)),
                    })?;
                if !original.participants.contains(&scope.participant)
                    || !scope.same_generation(&EnrollmentSecretScope::original(
                        original,
                        &scope.participant,
                        &[],
                    )?)
                {
                    return Err(held_registration_error(
                        HeldEnrollmentRegistrationError::Binding,
                    ));
                }
            }
            OriginalSecretPositiveOrigin::Confirmed(owner) => {
                if EnrollmentSecretScope::confirmed(owner.confirmed())?.encode()? != bytes {
                    return Err(AuraError::invalid(
                        "confirmed original positive scope differs",
                    ));
                }
            }
        }
        Ok(())
    }
    pub(in crate::runtime) fn decision(&self) -> &[u8] {
        self.decision
    }
}

#[cfg(test)]
#[test]
fn registered_enrollment_generation_has_no_deserialization_implementation() {
    // In this module the type is visible. If Deserialize is added, the second
    // implementation makes the inferred marker ambiguous and compilation fails.
    trait AmbiguousIfDeserializable<Marker> {
        fn assert_absent() {}
    }
    impl<T: ?Sized> AmbiguousIfDeserializable<()> for T {}
    struct Deserializable;
    impl<T: serde::Deserialize<'static>> AmbiguousIfDeserializable<Deserializable> for T {}
    let _ =
        <RegisteredEnrollmentGenerationCapability<'_> as AmbiguousIfDeserializable<_>>::assert_absent;
}

#[cfg(test)]
#[test]
fn registered_enrollment_generation_has_no_clone_implementation() {
    // Visibility is established here; adding Clone makes marker inference ambiguous.
    trait AmbiguousIfClone<Marker> {
        fn assert_absent() {}
    }
    impl<T: ?Sized> AmbiguousIfClone<()> for T {}
    struct Cloneable;
    impl<T: Clone> AmbiguousIfClone<Cloneable> for T {}
    let _ = <RegisteredEnrollmentGenerationCapability<'_> as AmbiguousIfClone<_>>::assert_absent;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
async fn finish_original_retirement_after_closed_fact_publisher(
    issuer: &std::sync::Arc<crate::AuraAgent>,
    ceremony: &aura_core::CeremonyId,
) {
    use aura_app::runtime_bridge::{CeremonyFailureReason, CeremonyTerminalOutcome, RuntimeBridge};
    let publication = crate::runtime_bridge::AgentRuntimeBridge::new(issuer.clone())
        .cancel_key_rotation_ceremony(ceremony)
        .await
        .expect_err("stopped publisher must report real partial cancellation failure");
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(&publication);
    let mut actual_closed_sink = false;
    while let Some(cause) = current {
        actual_closed_sink |= matches!(
            cause.downcast_ref::<crate::runtime::subsystems::journal::JournalSubsystemError>(),
            Some(crate::runtime::subsystems::journal::JournalSubsystemError::SinkClosed { .. })
        );
        current = cause.source();
    }
    assert!(
        actual_closed_sink,
        "required original publication cause must remain in standard source chain: {publication:?}"
    );
    assert!(
        matches!(
            issuer
                .runtime()
                .ceremony_runner()
                .terminal_outcome(ceremony)
                .await
                .expect("required original terminal readout"),
            Some(CeremonyTerminalOutcome::Failed(
                CeremonyFailureReason::Cancelled
            ))
        ),
        "required publication failure must preserve genuine original Cancelled first decision"
    );
    issuer
        .runtime()
        .ceremony_tracker()
        .retire_failed_enrollment_generation(ceremony)
        .await
        .expect(
            "actual retained original negative owner must acknowledge retirement before reissue",
        );
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod required_orphan_fact_tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;
    use aura_core::effects::StorageCoreEffects;
    use aura_core::types::facts::FactEncoding;
    use aura_journal::DomainFact;

    #[tokio::test]
    async fn actual_unissued_generation_does_not_mutate_on_required_fact_faults() {
        let (issuer, invitee, _invitation, start, _accept, _witness) = Box::pin(
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "orphan-required-facts",
            ),
        )
        .await;
        issuer
            .runtime()
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .unwrap();
        finish_original_retirement_after_closed_fact_publisher(&issuer, &start.ceremony_id).await;
        let setup_code = AgentRuntimeBridge::new(invitee.clone())
            .export_device_enrollment_setup_request()
            .await
            .unwrap();
        let app = std::sync::Arc::new(async_lock::RwLock::new(
            aura_app::AppCore::with_runtime(
                aura_app::AppConfig::default(),
                std::sync::Arc::new(AgentRuntimeBridge::new(issuer.clone())),
            )
            .unwrap(),
        ));
        let setup =
            aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                &app, setup_code,
            )
            .await
            .unwrap();
        let reserved = issuer
            .invitations()
            .unwrap()
            .reserve_device_enrollment_invitation()
            .await
            .unwrap();
        let ceremony =
            aura_core::CeremonyId::new(format!("unissued-required:{}", reserved.invitation_id()));
        let effects = issuer.runtime().effects();
        let plan = effects
            .prepare_authenticated_enrollment_rotation(&setup, issuer.runtime().ceremony_tracker())
            .await
            .expect("authenticated current roster");
        let prestate = plan.prestate();
        let (epoch, _packages, _public, allocation) = effects
            .prepare_pinned_enrollment_rotation(
                &setup,
                &reserved,
                &ceremony,
                &mut OriginalEnrollmentAllocationProducer::from_original(plan),
            )
            .await
            .unwrap();
        let raw_registration = issuer.runtime().ceremony_tracker().register(
    ceremony.clone(), aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment,
    issuer.authority_id(), 1, 1,
    vec![ParticipantIdentity::device(invitee.context().device_id())], epoch,
    Some(invitee.context().device_id()), None, prestate,
).await.expect_err("plain facts cannot allocate persistent enrollment even while a real reservation exists");
        assert!(matches!(
            std::error::Error::source(&raw_registration)
                .and_then(|source| source.downcast_ref::<HeldEnrollmentRegistrationError>()),
            Some(HeldEnrollmentRegistrationError::RequiredOwner)
        ));
        assert!(
            !effects
                .secure_exists(&SecureStorageLocation::new(
                    "device_enrollment_allocated_registration_v1",
                    ceremony.to_string()
                ))
                .await
                .expect("required allocation absence check"),
            "rejected raw allocation must not publish durable state"
        );
        let prior_registration = issuer
            .runtime()
            .ceremony_tracker()
            .get(&start.ceremony_id)
            .await
            .expect("read actual prior registration");
        let different_owner = allocation
            .validate_pending_registration(
                invitee.runtime().effects().as_ref(),
                &prior_registration,
                &_invitation,
            )
            .expect_err("physical effect owner cannot be substituted");

        assert!(matches!(
            std::error::Error::source(&different_owner)
                .and_then(|source| source.downcast_ref::<HeldEnrollmentRegistrationError>()),
            Some(HeldEnrollmentRegistrationError::EffectIdentity)
        ));
        let different_binding = allocation
            .validate_pending_registration(effects.as_ref(), &prior_registration, &_invitation)
            .expect_err("actual prior ceremony cannot authorize new generation registration");
        assert!(matches!(
            std::error::Error::source(&different_binding)
                .and_then(|source| source.downcast_ref::<HeldEnrollmentRegistrationError>()),
            Some(HeldEnrollmentRegistrationError::Binding)
        ));
        drop(allocation);
        let caps = [SecureStorageCapability::Read];
        let profile =
            super::super::enrollment_generation_profile_location(&issuer.authority_id(), epoch);
        let original = effects.secure_retrieve(&profile, &caps).await.unwrap();
        let package_location =
            AuraEffectSystem::threshold_public_key_location(&issuer.authority_id(), epoch);
        let package = effects
            .secure_retrieve(&package_location, &caps)
            .await
            .unwrap();
        let retirement = SecureStorageLocation::new(
            "device_enrollment_orphan_retirement_v1",
            ceremony.to_string(),
        );
        let context = issuer.context().default_context_id();
        let fact = aura_invitation::InvitationFact::sent_ms(
            context,
            aura_core::InvitationId::new("unrelated-required-invitation"),
            issuer.authority_id(),
            invitee.authority_id(),
            aura_invitation::InvitationType::Contact { nickname: None },
            100,
            None,
            None,
        );
        for fault in 0..4 {
            let mut envelope = fact.to_envelope();
            let mut outer = context;
            match fault {
                0 => envelope.payload = vec![0xff],
                1 => envelope.schema_version += 1,
                2 => envelope.encoding = FactEncoding::Json,
                3 => outer = aura_core::ContextId::new_from_entropy([98; 32]),
                _ => unreachable!(),
            }
            // The publisher was deliberately stopped above: persist the faulty
            // fact without publishing it.
            let committed = effects
                .persist_relational_facts(vec![aura_journal::RelationalFact::Generic {
                    context_id: outer,
                    envelope,
                }])
                .await
                .unwrap();
            let error = effects
                .retire_unissued_enrollment_allocation()
                .await
                .expect_err("required fault cannot authorize cleanup");
            assert!(matches!(error, AuraError::Serialization { .. }));
            let source = std::error::Error::source(&error).unwrap();
            assert!(source
                .downcast_ref::<aura_invitation::InvitationFactDecodeError>()
                .is_some());
            assert_eq!(
                effects.secure_retrieve(&profile, &caps).await.unwrap(),
                original
            );
            assert_eq!(
                effects
                    .secure_retrieve(&package_location, &caps)
                    .await
                    .unwrap(),
                package
            );
            assert!(!effects.secure_exists(&retirement).await.unwrap());
            // Remove only this injected corrupt test record through storage.
            for record in committed {
                let key =
                    AuraEffectSystem::typed_fact_storage_key(issuer.authority_id(), &record.order);
                assert!(effects.remove(&key).await.unwrap());
            }
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod actual_canonical_parent_package_tests {
    use super::*;

    #[tokio::test]
    async fn actual_service_bootstrap_inventory_uses_canonical_package_and_retains_missing_context()
    {
        let (issuer, _invitee, _invitation, _start, _accept, _witness) = Box::pin(
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "canonical-parent-package",
            ),
        )
        .await;
        let effects = issuer.runtime().effects();
        let authority = issuer.authority_id();
        let legacy = AuraEffectSystem::solo_public_key_location(&authority, 0);
        assert!(
            !effects.secure_exists(&legacy).await.unwrap(),
            "actual runtime bootstrap owns canonical package layout"
        );
        let canonical = AuraEffectSystem::threshold_public_key_location(&authority, 0);
        let read = [SecureStorageCapability::Read];
        let original = effects.secure_retrieve(&canonical, &read).await.unwrap();
        let baseline = effects.export_tree_ops().await.unwrap();
        let inventory = effects
            .collect_enrollment_parent_inventory(&baseline)
            .await
            .unwrap();
        assert!(!inventory.is_empty());
        assert_eq!(inventory[0].public_key_package, original);
        // Even an available legacy package cannot rescue corrupt canonical bytes.
        effects
            .secure_store(&legacy, &original, &[SecureStorageCapability::Write])
            .await
            .unwrap();
        effects
            .secure_store(&canonical, &[0xff], &[SecureStorageCapability::Write])
            .await
            .unwrap();
        let corrupt = effects
            .collect_enrollment_parent_inventory(&baseline)
            .await
            .expect_err("corrupt canonical package cannot fall back to valid legacy bytes");
        assert!(matches!(corrupt, AuraError::Crypto { .. }));
        assert!(std::error::Error::source(&corrupt).is_some());
        effects
            .secure_store(&canonical, &original, &[SecureStorageCapability::Write])
            .await
            .unwrap();
        effects
            .secure_delete(&legacy, &[SecureStorageCapability::Delete])
            .await
            .unwrap();
        effects
            .secure_delete(&canonical, &[SecureStorageCapability::Delete])
            .await
            .unwrap();

        let error = effects
            .collect_enrollment_parent_inventory(&baseline)
            .await
            .expect_err("missing exact retained key cannot fabricate inventory");
        let context = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<ParentInventoryStorageError>()
            .unwrap();
        assert_eq!(context.authority, authority);
        assert_eq!(context.epoch, 0);
        assert_eq!(context.location, canonical);
        assert!(matches!(
            context.source,
            AuraError::Storage { .. } | AuraError::NotFound { .. }
        ));
        effects
            .secure_store(&canonical, &original, &[SecureStorageCapability::Write])
            .await
            .unwrap();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod direct_bootstrap_custody_tests {
    use super::*;
    use aura_core::effects::ThresholdSigningEffects;
    #[tokio::test]
    async fn actual_direct_bootstrap_cannot_replace_retained_package() {
        let temp = tempfile::tempdir().expect("isolated profile");
        let config = crate::core::AgentConfig {
            storage: crate::core::config::StorageConfig {
                base_path: temp.path().join("profile"),
                ..Default::default()
            },
            ..Default::default()
        };
        let effects = crate::testing::simulation_effect_system_arc(&config);
        let authority = AuthorityId::new_from_entropy([193; 32]);
        let owner = effects.enrollment_retirement_generation_guard().await;
        {
            let bootstrap = effects.bootstrap_authority(&authority);
            tokio::pin!(bootstrap);
            assert!(futures::poll!(&mut bootstrap).is_pending());
        }
        drop(owner);
        let original = effects
            .bootstrap_authority(&authority)
            .await
            .expect("first actual bootstrap");
        let failure = effects
            .bootstrap_authority(&authority)
            .await
            .expect_err("existing package must not regenerate");
        assert!(std::error::Error::source(&failure)
            .expect("typed refusal")
            .is::<DirectBootstrapRetainedMaterial>());
        let stored = effects
            .secure_retrieve(
                &AuraEffectSystem::threshold_public_key_location(&authority, 0),
                &[SecureStorageCapability::Read],
            )
            .await
            .expect("original public package");
        assert_eq!(stored, original);
    }
}

#[cfg(test)]
mod authenticated_roster_tests {
    use super::*;
    #[test]
    fn held_roster_and_generation_cannot_be_cloned_or_deserialized() {
        trait AmbiguousClone<A> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousClone<()> for T {}
        struct CloneImplemented;
        impl<T: Clone> AmbiguousClone<CloneImplemented> for T {}
        let _ = <AuthenticatedEnrollmentRotationPlan<'static> as AmbiguousClone<_>>::check;
        let _ = <EnrollmentGenerationReservation<'static> as AmbiguousClone<_>>::check;
        let _ = <EnrollmentFinalVerifierInventoryCapability<'static, 'static> as AmbiguousClone<
            _,
        >>::check;
        let _ = <EnrollmentGenerationCustodyCapability<'static> as AmbiguousClone<_>>::check;
        let _ = <crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<
            'static,
        > as AmbiguousClone<_>>::check;
        trait AmbiguousDeserialize<A> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousDeserialize<()> for T {}
        struct DeserializeImplemented;
        impl<T: for<'de> serde::Deserialize<'de>> AmbiguousDeserialize<DeserializeImplemented> for T {}
        let _ = <AuthenticatedEnrollmentRotationPlan<'static> as AmbiguousDeserialize<_>>::check;
        let _ = <EnrollmentGenerationReservation<'static> as AmbiguousDeserialize<_>>::check;
        let _ = <EnrollmentFinalVerifierInventoryCapability<'static,'static> as AmbiguousDeserialize<_>>::check;
        let _ = <EnrollmentGenerationCustodyCapability<'static> as AmbiguousDeserialize<_>>::check;
        let _ = <crate::runtime::services::ceremony_tracker::EnrollmentGenerationDecisionCapability<
            'static,
        > as AmbiguousDeserialize<_>>::check;
    }
    #[test]
    fn removed_current_device_cannot_mint_rotation_roster_from_cached_signing_material() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            use aura_app::runtime_bridge::RuntimeBridge;
            use aura_core::effects::ThresholdSigningEffects;
            use aura_protocol::effects::TreeEffects;
            let (issuer, invitee, _invitation, start, _acceptance, _verified) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "roster-current-removal",
                ),
            )
            .await;
            let bridge = crate::runtime_bridge::AgentRuntimeBridge::new(issuer.clone());
            bridge
                .cancel_key_rotation_ceremony(&start.ceremony_id)
                .await
                .expect("actual cancellation retires pending generation");
            let code = crate::runtime_bridge::AgentRuntimeBridge::new(invitee.clone())
                .export_device_enrollment_setup_request()
                .await
                .expect("actual physical invitee setup");
            let app = std::sync::Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    std::sync::Arc::new(bridge),
                )
                .expect("actual transfer owner"),
            ));
            let pin =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                    &app, code,
                )
                .await
                .expect("independent actual setup pin");
            let effects = issuer.runtime().effects();
            let state = effects
                .get_current_state()
                .await
                .expect("actual authenticated issuer tree");
            let leaf = state
                .leaves
                .values()
                .find(|leaf| leaf.device_id == issuer.context().device_id())
                .expect("actual issuer leaf")
                .leaf_id;
            let kind = effects
                .remove_leaf(leaf, 0)
                .await
                .expect("actual removal operation");
            let op = aura_core::TreeOp {
                parent_epoch: state.epoch,
                parent_commitment: state.root_commitment,
                op: kind,
                version: 1,
            };
            let proof = issuer
                .threshold_signing()
                .sign(aura_core::threshold::SigningContext::self_tree_op(
                    issuer.authority_id(),
                    op.clone(),
                ))
                .await
                .expect("actual current parent signature");
            let plan = effects
                .prepare_authenticated_enrollment_rotation(
                    &pin,
                    issuer.runtime().ceremony_tracker(),
                )
                .await
                .expect("actual authenticated held roster before removal");
            let reserved = issuer
                .invitations()
                .expect("actual issuance owner")
                .reserve_device_enrollment_invitation()
                .await
                .expect("actual reservation under roster custody");
            let ceremony =
                aura_core::CeremonyId::new(format!("roster-held:{}", reserved.invitation_id()));
            let (_, _, _, generation) = effects
                .prepare_pinned_enrollment_rotation(
                    &pin,
                    &reserved,
                    &ceremony,
                    &mut OriginalEnrollmentAllocationProducer::from_original(plan),
                )
                .await
                .expect("consume held plan into generation reservation");
            let mutation = effects.apply_attested_op(aura_core::AttestedOp {
                op,
                agg_sig: proof.signature,
                signer_count: proof.signer_count,
            });
            tokio::pin!(mutation);
            use futures::FutureExt;
            assert!(
                mutation.as_mut().now_or_never().is_none(),
                "actual tree mutation waits through held generation reservation"
            );
            drop(generation);
            mutation
                .await
                .expect("authenticated same-epoch removal after custody release");
            let error = match effects
                .prepare_authenticated_enrollment_rotation(
                    &pin,
                    issuer.runtime().ceremony_tracker(),
                )
                .await
            {
                Ok(_) => panic!("removed physical issuer cannot authorize a new rotation"),
                Err(error) => error,
            };
            assert!(matches!(
                std::error::Error::source(&error)
                    .and_then(|source| source.downcast_ref::<EnrollmentRosterError>()),
                Some(EnrollmentRosterError::CurrentDeviceNotMember)
            ));
            let original = effects
                .secure_retrieve(
                    &SecureStorageLocation::new(
                        "device_enrollment_generation_allocation_v1",
                        ceremony.to_string(),
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("original immutable allocation");
            let current = effects
                .secure_retrieve(
                    &super::super::enrollment_generation_profile_location(
                        &issuer.authority_id(),
                        state.epoch.value() + 1,
                    ),
                    &[SecureStorageCapability::Read],
                )
                .await
                .expect("unissued original profile remains owned");
            assert_eq!(
                original, current,
                "failed fresh admission cannot replace original generation custody"
            );
            assert!(effects
                .retire_unissued_enrollment_allocation()
                .await
                .expect("negative original custody can retire after issuer removal"));
        });
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod original_enrollment_reservation_tests {
    use super::*;

    #[test]
    fn actual_reservation_retains_first_clock_anchor_and_rejects_other_runtime_owner() {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async {
            let (issuer, invitee, _, _, _, _) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "original-reservation-owner",
                ),
            )
            .await;
            let reserved = issuer
                .invitations()
                .unwrap()
                .reserve_device_enrollment_invitation()
                .await
                .unwrap();
            let effects = issuer.runtime().effects();
            let location = SecureStorageLocation::with_sub_key(
                "device_enrollment_original_reservation_v1",
                issuer.authority_id().to_string(),
                reserved.invitation_id().to_string(),
            );
            let before = effects
                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            effects
                .retain_original_enrollment_reservation(&reserved)
                .await
                .unwrap();
            assert_eq!(
                effects
                    .secure_retrieve(&location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                before,
                "idempotent publication cannot renew the original anchor"
            );
            let original: StoredOriginalEnrollmentReservation =
                serde_json::from_slice(&before).unwrap();
            assert_eq!(original.created_at_ms, reserved.created_at_ms());
            let denied = invitee
                .runtime()
                .effects()
                .retain_original_enrollment_reservation(&reserved)
                .await
                .expect_err("another actual runtime cannot retain this issuer reservation");
            assert!(matches!(
                std::error::Error::source(&denied)
                    .unwrap()
                    .downcast_ref::<OriginalEnrollmentReservationError>(),
                Some(OriginalEnrollmentReservationError::Owner)
            ));
            assert!(!invitee
                .runtime()
                .effects()
                .secure_exists(&location)
                .await
                .unwrap());
            let mut foreign_config = effects.config().clone();
            foreign_config.storage.base_path = tempfile::Builder::new()
                .prefix("aura-equal-id-foreign-reservation-")
                .tempdir()
                .unwrap()
                .keep();
            let foreign_context = aura_core::context::EffectContext::new(
                issuer.authority_id(),
                aura_core::ContextId::new_from_entropy([206; 32]),
                aura_core::effects::ExecutionMode::Testing,
            );
            let foreign = crate::AgentBuilder::new()
                .with_authority(issuer.authority_id())
                .with_config(foreign_config)
                .build_testing_async(&foreign_context)
                .await
                .unwrap();
            assert_eq!(foreign.authority_id(), issuer.authority_id());
            assert_eq!(foreign.runtime().effects().device_id(), effects.device_id());
            let denied = foreign
                .runtime()
                .effects()
                .retain_original_enrollment_reservation(&reserved)
                .await
                .expect_err("equal identifiers do not authorize a different actual runtime owner");
            assert!(matches!(
                std::error::Error::source(&denied)
                    .unwrap()
                    .downcast_ref::<OriginalEnrollmentReservationError>(),
                Some(OriginalEnrollmentReservationError::Owner)
            ));
            assert!(!foreign
                .runtime()
                .effects()
                .secure_exists(&location)
                .await
                .unwrap());
        });
    }
}

#[cfg(all(test, unix))]
mod required_active_signer_fault_tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;
    use aura_core::effects::ThresholdSigningEffects;

    async fn actual_signer() -> std::sync::Arc<crate::AuraAgent> {
        let authority = AuthorityId::new_from_entropy([227; 32]);
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy([228; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::tempdir().expect("isolated signer profile").keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let context = aura_core::context::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy([229; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = std::sync::Arc::new(
            crate::AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .expect("actual runtime"),
        );
        AgentRuntimeBridge::new(agent.clone())
            .bootstrap_signing_keys(
                crate::runtime::services::RuntimeServiceContext::test_original(
                    agent.runtime().tasks(),
                    agent.runtime().effects(),
                )
                .await
                .startup_window(),
            )
            .await
            .expect("actual physical bootstrap");
        agent
    }

    #[tokio::test]
    async fn missing_active_epoch_and_policy_fail_with_storage_source() {
        for policy in [false, true] {
            let agent = actual_signer().await;
            let effects = agent.runtime().effects();
            let authority = agent.authority_id();
            let location = if policy {
                SecureStorageLocation::with_sub_key("threshold_config", authority.to_string(), "0")
            } else {
                SecureStorageLocation::new("epoch_state", authority.to_string())
            };
            assert!(effects
                .fault_remove_secure_record_for_test(&location)
                .await
                .expect("actual selected backing loss"));
            let failure = effects
                .sign(aura_core::threshold::SigningContext::message(
                    authority,
                    "required-signer-negative".into(),
                    vec![1],
                ))
                .await
                .expect_err("missing required state cannot become epoch zero or solo defaults");
            assert!(matches!(failure, AuraError::Storage { .. }));
            let missing = std::error::Error::source(&failure)
                .expect("actual required absence source")
                .downcast_ref::<aura_core::effects::secure::SecureStorageRecordMissing>()
                .expect("logical provider absence remains structural");
            assert_eq!(missing.location(), &location);
            assert!(std::error::Error::source(missing).is_none());
        }
    }

    #[tokio::test]
    async fn malformed_active_epoch_preserves_native_decoder_source() {
        let agent = actual_signer().await;
        let effects = agent.runtime().effects();
        let authority = agent.authority_id();
        effects
            .secure_store(
                &SecureStorageLocation::new("epoch_state", authority.to_string()),
                &[0; 7],
                &[SecureStorageCapability::Write],
            )
            .await
            .expect("mutable epoch fault");
        let failure = effects
            .sign(aura_core::threshold::SigningContext::message(
                authority,
                "required-signer-negative".into(),
                vec![2],
            ))
            .await
            .expect_err("malformed required epoch cannot fall back");
        assert!(matches!(failure, AuraError::Serialization { .. }));
        assert!(std::error::Error::source(&failure)
            .expect("native decode source")
            .is::<std::array::TryFromSliceError>());
    }
    #[tokio::test]
    async fn actual_mutable_share_with_wrong_secret_cannot_sign_under_retained_public_key() {
        let agent = actual_signer().await;
        let effects = agent.runtime().effects();
        let authority = agent.authority_id();
        let (public, _, _) = effects
            .trusted_tree_parent_verifier_inventory(&authority, 0)
            .await
            .expect("original independently retained public package");
        let candidate = effects
            .generate_signing_keys_with(KeyGenerationMethod::SingleSigner, 1, 1)
            .await
            .expect("actual crypto key producer");
        let alternate = SingleSignerKeyPackage::import_from_secure_storage(
            &candidate.key_packages[0],
            SecretExportContext::secure_storage("required-signer-negative"),
        )
        .expect("actual private package");
        assert_ne!(alternate.verifying_key(), public.group_key().as_slice());
        // A copied public field cannot turn a different private key into the
        // original secret. The runtime must derive the public key itself.
        let mismatched = SingleSignerKeyPackage::new(
            alternate.signing_key().to_vec(),
            public.group_key().to_vec(),
        );
        let bytes = zeroize::Zeroizing::new(
            mismatched
                .export_for_secure_storage(SecretExportContext::secure_storage(
                    "required-signer-negative",
                ))
                .expect("valid package codec with inconsistent key pair"),
        );
        let participant = ParticipantIdentity::device(effects.device_id());
        let envelope = effects
            .encrypt_participant_key_package(&authority, 0, &participant, &bytes)
            .await
            .expect("actual required envelope producer");
        let location = AuraEffectSystem::participant_share_location(&authority, 0, &participant);
        effects
            .secure_store(&location, &envelope, &[SecureStorageCapability::Write])
            .await
            .expect("selected participant share is owned mutable key material");
        let failure = effects
            .sign(aura_core::threshold::SigningContext::message(
                authority,
                "required-signer-negative".into(),
                vec![3],
            ))
            .await
            .expect_err("matching embedded public field must not authorize wrong secret");
        assert!(matches!(failure, AuraError::Crypto { .. }));
        assert!(matches!(
            std::error::Error::source(&failure)
                .expect("actual structural mismatch")
                .downcast_ref::<RequiredSigningParticipantError>(),
            Some(RequiredSigningParticipantError::KeyMismatch)
        ));
    }
}

#[cfg(test)]
mod registered_generation_actual_owner_tests {
    use super::*;

    #[tokio::test]
    async fn equal_ids_cannot_handoff_registration_to_another_runtime() {
        let original_clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000));
        let ((issuer, invitee, invitation, _start, _, _), original) = Box::pin(crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
        "registration-actual-owner", Some(original_clock), None,
    )).await;
        let effects = issuer.runtime().effects();
        let tracker = issuer.runtime().ceremony_tracker();
        let original_disposal = original
            .observe_original_running_owner_for_test(effects.as_ref(), tracker)
            .expect("genuine original running continuation");
        let detached =
            crate::runtime::services::ceremony_tracker::CeremonyTracker::new_with_storage(
                effects.clone(),
            );
        let error = original
            .observe_original_running_owner_for_test(effects.as_ref(), &detached)
            .err()
            .expect("same effects cannot authorize unrelated tracker");
        crate::runtime::services::ceremony_tracker::assert_original_running_owner_refusal_for_test(
            &error, false,
        );
        let enrollment_tasks = || {
            let mut tasks: Vec<_> = issuer
                .runtime()
                .tasks()
                .active_tasks()
                .into_iter()
                .filter(|task| task.contains("enrollment"))
                .collect();
            tasks.sort();
            tasks
        };
        let before = enrollment_tasks();
        let repeated = original
            .observe_original_running_owner_for_test(effects.as_ref(), tracker)
            .expect("same original observer reentry");
        assert_eq!(
            enrollment_tasks(),
            before,
            "retained continuation observation cannot admit duplicate tasks"
        );
        let original_config = effects.config().clone();
        let foreign_config = crate::core::AgentConfig {
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::Builder::new()
                    .prefix("aura-foreign-registered-owner-")
                    .tempdir()
                    .unwrap()
                    .keep(),
                ..original_config.storage.clone()
            },
            ..original_config
        };
        let context = aura_core::context::EffectContext::new(
            issuer.authority_id(),
            aura_core::ContextId::new_from_entropy([201; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let runtime = crate::runtime::EffectSystemBuilder::testing()
            .with_authority(issuer.authority_id())
            .with_config(foreign_config)
            .build(&context)
            .await
            .unwrap();
        let foreign = crate::AuraAgent::new(runtime, issuer.authority_id());
        assert_eq!(foreign.authority_id(), issuer.authority_id());
        assert_eq!(foreign.context().device_id(), issuer.context().device_id());
        let error = original
            .observe_original_running_owner_for_test(
                foreign.runtime().effects().as_ref(),
                foreign.runtime().ceremony_tracker(),
            )
            .err()
            .expect("equal identifiers do not confer original runtime custody");
        crate::runtime::services::ceremony_tracker::assert_original_running_owner_refusal_for_test(
            &error, true,
        );
        let admitted = invitee
            .invitations()
            .unwrap()
            .take_original_enrollment_execution(&invitation.invitation_id)
            .await
            .unwrap();
        let epoch = admitted.origin_observation().manifest().pending_epoch;
        invitee
            .invitations()
            .unwrap()
            .return_original_enrollment_execution_for_test(admitted)
            .await
            .unwrap();
        assert!(
            !foreign
                .runtime()
                .effects()
                .secure_exists(&super::super::enrollment_generation_profile_location(
                    &issuer.authority_id(),
                    epoch
                ))
                .await
                .unwrap(),
            "foreign profile remains untouched"
        );
        invitee
            .invitations()
            .unwrap()
            .accept(&invitation.invitation_id)
            .await
            .expect("same original protocol positive completion");
        let actual = original_disposal.await_disposal().await;
        original_disposal.require_positive(&actual).unwrap();
        original
            .require_original_issuer_disposal_for_test(&actual)
            .unwrap();
        let repeated_actual = repeated.await_disposal().await;
        repeated.require_positive(&repeated_actual).unwrap();
        original
            .require_original_issuer_disposal_for_test(&repeated_actual)
            .unwrap();
        for agent in [issuer, invitee, Arc::new(foreign)] {
            match agent
                .runtime()
                .tasks()
                .shutdown_gracefully(std::time::Duration::from_secs(5))
                .await
            {
                Ok(()) => {}
                Err(crate::task_registry::TaskSupervisionError::TaskFailed { source, .. }) => {
                    eprintln!("actual original-owner regression domain failure: {source:?}");
                }
                Err(source) => {
                    panic!("actual original-owner regression resource cleanup failed: {source:?}")
                }
            }
        }
    }
}

#[cfg(test)]
mod original_baseline_custody_tests {
    use super::*;
    #[tokio::test]
    async fn genuine_original_allocation_seals_exact_authenticated_baseline() {
        let (issuer, _invitee, _invitation, start, _, _) = Box::pin(
            crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                "original-baseline-seal",
            ),
        )
        .await;
        let effects = issuer.runtime().effects();
        let decision = effects.lock_tree_decision().await;
        let history = effects
            .export_tree_ops()
            .await
            .expect("actual current history");
        effects
            .collect_enrollment_parent_inventory(&history)
            .await
            .expect("authenticate complete actual baseline");
        let bytes = effects
            .secure_retrieve(
                &SecureStorageLocation::new(
                    "device_enrollment_generation_allocation_v1",
                    start.ceremony_id.to_string(),
                ),
                &[SecureStorageCapability::Read],
            )
            .await
            .expect("actual protected original allocation");
        let original: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).expect("actual original allocation codec");
        let expected = original
            .baseline
            .expect("fresh original captures authenticated history");
        let prefix = effects
            .require_original_baseline(&original)
            .await
            .expect("actual original canonical prefix");
        assert_eq!(
            original_baseline_fingerprint(&prefix).expect("canonical fingerprint"),
            expected
        );
        assert!(
            !prefix.is_empty(),
            "actual bootstrap leaf is part of original baseline"
        );
        let mut missing = original;
        missing.baseline = None;
        let error = effects
            .require_original_baseline(&missing)
            .await
            .expect_err("legacy absence cannot reconstitute original history authority");
        assert!(matches!(
            std::error::Error::source(&error)
                .and_then(|cause| cause.downcast_ref::<HeldEnrollmentRegistrationError>()),
            Some(HeldEnrollmentRegistrationError::BaselineBinding)
        ));
        let mut reordered = prefix.clone();
        reordered.push(prefix[0].clone());
        assert_ne!(
            original_baseline_fingerprint(&reordered).expect("bounded altered fingerprint"),
            expected,
            "same individual authenticated fact cannot stand in for exact original sequence"
        );
        drop(decision);
    }
}

#[cfg(test)]
#[test]
fn required_retirement_stage_preserves_native_immutable_record_cause() {
    use std::error::Error;
    for stage in [
        EnrollmentRetirementStage::WrappingSecret,
        EnrollmentRetirementStage::ParticipantShare,
        EnrollmentRetirementStage::SigningShare,
        EnrollmentRetirementStage::SoloPublicPackage,
        EnrollmentRetirementStage::PublicPackage,
        EnrollmentRetirementStage::ThresholdConfig,
        EnrollmentRetirementStage::RetiredReceipt,
        EnrollmentRetirementStage::PendingSlot,
    ] {
        let actual = AuraError::Storage {
            message: "provider rejected protected record deletion".into(),
            source: Some(std::sync::Arc::new(
                aura_core::effects::secure::ImmutableSecureRecordMutation {
                    operation: "delete",
                },
            )),
        };
        let error = retirement_effect_error(stage, actual);
        let retained = error
            .source()
            .and_then(|source| source.downcast_ref::<EnrollmentRetirementError>())
            .expect("structural effect stage is retained");
        assert_eq!(retained.stage, stage);
        assert!(matches!(retained.source, AuraError::Storage { .. }));
        assert!(retained.source.source().is_some_and(|source| {
            source.is::<aura_core::effects::secure::ImmutableSecureRecordMutation>()
        }));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod enrollment_generation_history_tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;
    use std::sync::Arc;

    #[tokio::test]
    async fn retired_generation_namespace_cannot_restore_current_registered_owner() {
        Box::pin(async {
            let (issuer, _invitee, _invitation, start, _acceptance, _witness) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "legacy-generation-live-slot",
                )
                .await;
            let effects = issuer.runtime().effects().clone();
            let epoch = start.pending_epoch.value();
            let live =
                super::super::enrollment_generation_profile_location(&issuer.authority_id(), epoch);
            let legacy = SecureStorageLocation::with_sub_key(
                "device_enrollment_generation_profile_v1",
                issuer.authority_id().to_string(),
                epoch.to_string(),
            );
            let canonical = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .unwrap();
            let registered = effects
                .secure_retrieve(
                    &registration_history_location(&start.ceremony_id),
                    &[SecureStorageCapability::Read],
                )
                .await
                .unwrap();
            assert_eq!(canonical, registered);
            // Historical encoded layout is populated from the actual sealed
            // production decision; no signing material or caller trust is fabricated.
            effects
                .secure_store_immutable(
                    &legacy,
                    &canonical,
                    &[
                        SecureStorageCapability::Read,
                        SecureStorageCapability::Write,
                    ],
                )
                .await
                .unwrap();
            effects
                .secure_delete(&live, &[SecureStorageCapability::Delete])
                .await
                .unwrap();
            assert!(!effects
                .has_live_enrollment_generation(issuer.authority_id(), epoch)
                .await
                .unwrap());
            let missing = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .expect_err("retired namespace cannot restore current owner");
            let source = std::error::Error::source(&missing)
                .and_then(|source| {
                    source.downcast_ref::<aura_core::effects::secure::SecureStorageRecordMissing>()
                })
                .expect("current slot absence retains native source");
            assert_eq!(source.location(), &live);
            assert!(!effects.secure_exists(&live).await.unwrap());
            assert_eq!(
                effects
                    .secure_retrieve(&legacy, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                canonical
            );
            assert_eq!(
                effects
                    .secure_retrieve(
                        &registration_history_location(&start.ceremony_id),
                        &[SecureStorageCapability::Read]
                    )
                    .await
                    .unwrap(),
                registered
            );
            let mut forged: StoredEnrollmentGenerationProfile =
                serde_json::from_slice(&canonical).unwrap();
            forged.threshold = forged.threshold.saturating_add(1);
            effects
                .secure_store(
                    &live,
                    &serde_json::to_vec(&forged).unwrap(),
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            let failure = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .expect_err("mutable slot cannot change original signing policy");
            assert!(matches!(
                std::error::Error::source(&failure)
                    .and_then(|source| source.downcast_ref::<EnrollmentGenerationHistoryError>()),
                Some(EnrollmentGenerationHistoryError::OriginalBinding)
            ));
            effects
                .secure_store(&live, &canonical, &[SecureStorageCapability::Write])
                .await
                .unwrap();
        })
        .await;
    }

    #[tokio::test]
    async fn same_epoch_reissue_preserves_original_history_and_rejects_mutable_registration_forgery(
    ) {
        Box::pin(async {
            let (issuer, invitee, _invitation, first, _acceptance, _witness) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
                    "same-epoch-generation-history",
                )
                .await;
            let effects = issuer.runtime().effects().clone();
            let original_location = SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                first.ceremony_id.to_string(),
            );
            let registered_location = registration_history_location(&first.ceremony_id);
            let original = effects
                .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let registered = effects
                .secure_retrieve(&registered_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let live = super::super::enrollment_generation_profile_location(
                &issuer.authority_id(),
                first.pending_epoch.value(),
            );
            let mut staged: StoredEnrollmentGenerationProfile =
                serde_json::from_slice(&registered).unwrap();
            staged.registered = false;
            effects
                .secure_store(
                    &live,
                    &serde_json::to_vec(&staged).unwrap(),
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            assert_eq!(
                effects
                    .read_owned_enrollment_generation_profile(
                        issuer.authority_id(),
                        first.pending_epoch.value()
                    )
                    .await
                    .unwrap(),
                registered,
                "immutable registration first decision prevents a mutable phase downgrade"
            );
            // Close only the fact publisher. The original prepared issuer is a
            // required task: cancelling the whole runtime here force-aborts its
            // custody before the negative retirement below can acknowledge it.
            issuer
                .runtime()
                .stop_original_reactive_pipeline_for_test()
                .await
                .unwrap();
            finish_original_retirement_after_closed_fact_publisher(&issuer, &first.ceremony_id)
                .await;
            assert!(!effects.secure_exists(&live).await.unwrap());
            assert_eq!(
                effects
                    .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                original
            );
            assert_eq!(
                effects
                    .secure_retrieve(&registered_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                registered
            );
            let setup_code = AgentRuntimeBridge::new(invitee.clone())
                .export_device_enrollment_setup_request()
                .await
                .unwrap();
            let app = Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(
                    aura_app::AppConfig::default(),
                    Arc::new(AgentRuntimeBridge::new(issuer.clone())),
                )
                .unwrap(),
            ));
            let setup =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                    &app, setup_code,
                )
                .await
                .unwrap();
            let reserved = issuer
                .invitations()
                .unwrap()
                .reserve_device_enrollment_invitation()
                .await
                .unwrap();
            let second_id = aura_core::CeremonyId::new(format!(
                "second-owned-generation-{}",
                reserved.invitation_id()
            ));
            let plan = effects
                .prepare_authenticated_enrollment_rotation(
                    &setup,
                    issuer.runtime().ceremony_tracker(),
                )
                .await
                .unwrap();
            let (epoch, _, _, generation) = effects
                .prepare_pinned_enrollment_rotation(
                    &setup,
                    &reserved,
                    &second_id,
                    &mut OriginalEnrollmentAllocationProducer::from_original(plan),
                )
                .await
                .expect("a genuinely fresh owned allocation can reuse the retired pending epoch");
            assert_eq!(epoch, first.pending_epoch.value());
            assert_ne!(generation.ceremony_id(), &first.ceremony_id);
            let second_original_location = SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                second_id.to_string(),
            );
            let second = effects
                .secure_retrieve(&second_original_location, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            assert_ne!(second, original);
            assert_eq!(
                effects
                    .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                    .await
                    .unwrap(),
                second
            );
            let mut forged: StoredEnrollmentGenerationProfile =
                serde_json::from_slice(&second).unwrap();
            forged.registered = true;
            effects
                .secure_store(
                    &live,
                    &serde_json::to_vec(&forged).unwrap(),
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            let failure = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .expect_err("mutable registration claims cannot authorize activation");
            assert!(matches!(
                std::error::Error::source(&failure)
                    .and_then(|source| source.downcast_ref::<EnrollmentGenerationHistoryError>()),
                Some(EnrollmentGenerationHistoryError::RegistrationBinding)
            ));
            forged.registered = false;
            forged.participants.reverse();
            effects
                .secure_store(
                    &live,
                    &serde_json::to_vec(&forged).unwrap(),
                    &[SecureStorageCapability::Write],
                )
                .await
                .unwrap();
            let failure = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .expect_err(
                    "mutable ordered roster cannot replace original authenticated inventory",
                );
            assert!(matches!(
                std::error::Error::source(&failure)
                    .and_then(|source| source.downcast_ref::<EnrollmentGenerationHistoryError>()),
                Some(EnrollmentGenerationHistoryError::OriginalBinding)
            ));
            effects
                .secure_store(&live, &second, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert_eq!(
                effects
                    .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                original
            );
            assert_eq!(
                effects
                    .secure_retrieve(&registered_location, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                registered
            );
            drop(generation);
        })
        .await;
    }
}

#[test]
fn original_response_quorum_is_distinct_from_signing_policy() {
    let policy = EnrollmentResponsePolicy::select_for_original_signing_policy(2, 2).unwrap();
    assert_eq!(policy.required(), 1);
    assert_eq!(policy.total(), 1);
    let error = policy.require_exact(2, 1).unwrap_err();
    assert!(matches!(
        std::error::Error::source(&error)
            .and_then(|source| source.downcast_ref::<HeldEnrollmentRegistrationError>()),
        Some(HeldEnrollmentRegistrationError::ResponsePolicyBinding {
            expected_required: 1,
            expected_total: 1,
            observed_required: 2,
            observed_total: 1
        })
    ));

    let policy = EnrollmentResponsePolicy::select_for_original_signing_policy(2, 4).unwrap();
    assert_eq!(policy.required(), 2);
    assert_eq!(policy.total(), 3);
    assert!(EnrollmentResponsePolicy::select_for_original_signing_policy(0, 2).is_err());
    assert!(EnrollmentResponsePolicy::select_for_original_signing_policy(1, 1).is_err());
    assert!(EnrollmentResponsePolicy::select_for_original_signing_policy(5, 4).is_err());
}

#[cfg(all(test, unix))]
mod missing_response_policy_history_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn missing_response_policy_refuses_current_owner_without_supplementation() {
        Box::pin(async {
            // The policy fixture needs ordered physical observations, including
            // genuine timeout/rollback errors. A shared quiescent provider keeps
            // its two runtimes on one deterministic clock instead of depending
            // on host wall-clock millisecond ordering.
            let clock = aura_testkit::time::QuiescentClock::start();
            let (issuer, _invitee, _invitation, start, _acceptance, _response) =
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_clock(
                    "missing-current-response-policy", clock.provider(),
                )
                .await;
            let effects = issuer.runtime().effects();
            let epoch = start.pending_epoch.value();
            let live =
                super::super::enrollment_generation_profile_location(&issuer.authority_id(), epoch);
            let allocated = SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                start.ceremony_id.to_string(),
            );
            let registered = registration_history_location(&start.ceremony_id);
            let allocated_before = effects
                .secure_retrieve(&allocated, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let registered_before = effects
                .secure_retrieve(&registered, &[SecureStorageCapability::Read])
                .await
                .unwrap();
            let canonical = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .unwrap();
            let mut malformed: serde_json::Value = serde_json::from_slice(&canonical).unwrap();
            assert!(malformed
                .as_object_mut()
                .unwrap()
                .remove("response_policy")
                .is_some());
            let malformed = serde_json::to_vec(&malformed).unwrap();
            assert!(
                serde_json::from_slice::<StoredEnrollmentGenerationProfile>(&malformed).is_err()
            );
            effects
                .secure_store(&live, &malformed, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            let failure = effects
                .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                .await
                .expect_err("missing mandatory policy cannot be hydrated from history");
            assert!(matches!(
                std::error::Error::source(&failure)
                    .and_then(|source| source.downcast_ref::<EnrollmentGenerationHistoryError>()),
                Some(EnrollmentGenerationHistoryError::Codec(_))
            ));
            assert_eq!(
                effects
                    .secure_retrieve(&live, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                malformed
            );
            assert_eq!(
                effects
                    .secure_retrieve(&allocated, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                allocated_before
            );
            assert_eq!(
                effects
                    .secure_retrieve(&registered, &[SecureStorageCapability::Read])
                    .await
                    .unwrap(),
                registered_before
            );
            effects
                .secure_store(&live, &canonical, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            assert_eq!(
                effects
                    .read_owned_enrollment_generation_profile(issuer.authority_id(), epoch)
                    .await
                    .unwrap(),
                canonical
            );
        })
        .await;
    }
}

#[cfg(test)]
#[test]
fn allocation_lifetime_children_do_not_clone_or_deserialize_domain_authority() {
    trait AmbiguousIfClone<M> {
        fn absent() {}
    }
    impl<T: ?Sized> AmbiguousIfClone<()> for T {}
    struct Cloned;
    impl<T: Clone> AmbiguousIfClone<Cloned> for T {}
    trait AmbiguousIfDeserialize<M> {
        fn absent() {}
    }
    impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
    struct Decoded;
    impl<T: serde::Deserialize<'static>> AmbiguousIfDeserialize<Decoded> for T {}
    let _ = <OwnedSecretBirthCapability<'static, 'static> as AmbiguousIfClone<_>>::absent;
    let _ = <OwnedSecretNegativeCapability<'static, 'static> as AmbiguousIfClone<_>>::absent;
    let _ = <OwnedSecretReadCapability as AmbiguousIfClone<_>>::absent;
    let _ = <OwnedSecretPositiveCapability<'static, 'static> as AmbiguousIfClone<_>>::absent;
    let _ = <OwnedSecretBirthCapability<'static, 'static> as AmbiguousIfDeserialize<_>>::absent;
    let _ = <OwnedSecretNegativeCapability<'static, 'static> as AmbiguousIfDeserialize<_>>::absent;
    let _ = <OwnedSecretReadCapability as AmbiguousIfDeserialize<_>>::absent;
    let _ = <OwnedSecretPositiveCapability<'static, 'static> as AmbiguousIfDeserialize<_>>::absent;
}

#[cfg(all(test, unix))]
#[tokio::test]
async fn owned_enrollment_reader_rejects_equal_scope_on_another_runtime_registry() {
    let (issuer, invitee, _, start, _, _) =
        crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture(
            "owned-secret-registry-identity",
        )
        .await;
    let effects = issuer.runtime().effects();
    let participant = ParticipantIdentity::device(invitee.context().device_id());
    let bytes = effects
        .secure_retrieve(
            &AuraEffectSystem::participant_share_location(
                &issuer.authority_id(),
                start.pending_epoch.value(),
                &participant,
            ),
            &[SecureStorageCapability::Read],
        )
        .await
        .expect("actual v2 original participant envelope");
    let envelope: AllocationParticipantEnvelope =
        serde_json::from_slice(&bytes).expect("actual producer-owned envelope");
    let read = effects
        .verify_owned_enrollment_package_reader(
            &issuer.authority_id(),
            start.pending_epoch.value(),
            &participant,
            &envelope,
        )
        .await
        .expect("original immutable birth and retained registration verify");
    {
        let mut original = effects.crypto.allocation_lifetimes.lock().await;
        let secret = original
            .ready()
            .await
            .expect("actual original inventory")
            .read_original(&envelope.allocation, &read)
            .await
            .expect("original registry accepts actual issued read owner");
        assert_eq!(secret.len(), 32);
    }
    let mut config = effects.config().clone();
    config.storage.base_path = tempfile::tempdir().expect("foreign profile").keep();
    let context = aura_core::context::EffectContext::new(
        issuer.authority_id(),
        issuer.context().default_context_id(),
        aura_core::effects::ExecutionMode::Testing,
    );
    // The foreign registry owns its own profile, so it has an (empty) original
    // inventory to refuse the retargeted read with.
    let foreign_profile = crate::runtime::builder::TestingOwnedProfileCapability::acquire(&config)
        .expect("foreign profile lease");
    let foreign = crate::runtime::EffectSystemBuilder::testing_with_owned_profile(foreign_profile)
        .with_authority(issuer.authority_id())
        .with_config(config)
        .build(&context)
        .await
        .expect("actual second registry with equal authority/device routing");
    let foreign_effects = foreign.effects();
    let mut foreign_inventory = foreign_effects.crypto.allocation_lifetimes.lock().await;
    let failure = foreign_inventory
        .ready()
        .await
        .expect("foreign original empty inventory")
        .read_original(&envelope.allocation, &read)
        .await
        .expect_err(
            "actual held read owner cannot retarget equal routing/scope to another registry",
        );
    assert!(matches!(
        std::error::Error::source(&failure)
            .and_then(|source| source.downcast_ref::<HeldEnrollmentRegistrationError>()),
        Some(HeldEnrollmentRegistrationError::EffectIdentity)
    ));
    drop(foreign_inventory);
    let sender = issuer
        .runtime()
        .tasks()
        .shutdown_with_timeout(std::time::Duration::from_secs(2))
        .await;
    let receiver = invitee
        .runtime()
        .tasks()
        .shutdown_with_timeout(std::time::Duration::from_secs(2))
        .await;
    sender.expect("original sender task teardown");
    receiver.expect("independent invitee task teardown");
}

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) struct OriginalAllocationBirthGate {
    reached: tokio::sync::oneshot::Sender<
        aura_core::effects::secret_lifetime::SecretAllocationReference,
    >,
    resume: tokio::sync::oneshot::Receiver<()>,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) struct OriginalRegistrationReadbackGate {
    committed: tokio::sync::oneshot::Sender<()>,
    continue_readback: tokio::sync::oneshot::Receiver<()>,
}
#[cfg(all(test, not(target_arch = "wasm32")))]
impl AuraEffectSystem {
    pub(crate) fn hold_original_allocation_birth_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<
                aura_core::effects::secret_lifetime::SecretAllocationReference,
            >,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let mut slot = self.original_allocation_birth_gate.lock();
        if slot.is_some() {
            return Err(AuraError::invalid(
                "original allocation birth gate already installed",
            ));
        }
        let (reached, receiver) = tokio::sync::oneshot::channel();
        let (resume, continuation) = tokio::sync::oneshot::channel();
        *slot = Some(OriginalAllocationBirthGate {
            reached,
            resume: continuation,
        });
        Ok((receiver, resume))
    }
    async fn await_original_allocation_birth_for_test(
        &self,
        reference: &aura_core::effects::secret_lifetime::SecretAllocationReference,
    ) -> Result<(), AuraError> {
        let gate = self.original_allocation_birth_gate.lock().take();
        if let Some(gate) = gate {
            gate.reached
                .send(reference.clone())
                .map_err(|_| AuraError::invalid("original birth observer lost"))?;
            gate.resume.await.map_err(|source| AuraError::Internal {
                message: "original birth gate interrupted".into(),
                source: Some(Arc::new(source)),
            })?;
        }
        Ok(())
    }
    pub(crate) async fn observe_original_lifetime_for_test(
        &self,
        reference: &aura_core::effects::secret_lifetime::SecretAllocationReference,
    ) -> Result<
        (
            aura_core::effects::secret_lifetime::SecretLifetimeState,
            bool,
        ),
        AuraError,
    > {
        let inventory = self.crypto.allocation_lifetimes.lock().await;
        inventory.observe_original_for_test(reference).await
    }
    pub(crate) fn hold_original_allocation_prewrite_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let mut gate = self.original_allocation_prewrite_gate.lock();
        if gate.is_some() {
            return Err(AuraError::invalid(
                "original allocation prewrite gate already installed",
            ));
        }
        let (committed, observed) = tokio::sync::oneshot::channel();
        let (continue_readback, held) = tokio::sync::oneshot::channel();
        *gate = Some(OriginalRegistrationReadbackGate {
            committed,
            continue_readback: held,
        });
        Ok((observed, continue_readback))
    }
    pub(crate) async fn await_original_allocation_prewrite_for_test(
        &self,
    ) -> Result<(), AuraError> {
        let gate = self.original_allocation_prewrite_gate.lock().take();
        if let Some(gate) = gate {
            gate.committed
                .send(())
                .map_err(|_| AuraError::invalid("original allocation prewrite observer lost"))?;
            gate.continue_readback
                .await
                .map_err(|source| AuraError::Internal {
                    message: "original allocation prewrite interrupted".into(),
                    source: Some(Arc::new(source)),
                })?;
        }
        Ok(())
    }
    pub(crate) fn hold_original_allocation_commit_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let mut original = self.original_allocation_commit_gate.lock();
        if original.is_some() {
            return Err(AuraError::invalid(
                "original allocation commit gate already installed",
            ));
        }
        let (committed, observed) = tokio::sync::oneshot::channel();
        let (continue_readback, held) = tokio::sync::oneshot::channel();
        *original = Some(OriginalRegistrationReadbackGate {
            committed,
            continue_readback: held,
        });
        Ok((observed, continue_readback))
    }
    pub(super) async fn await_original_allocation_commit_for_test(&self) -> Result<(), AuraError> {
        let original = self.original_allocation_commit_gate.lock().take();
        if let Some(original) = original {
            original
                .committed
                .send(())
                .map_err(|_| AuraError::invalid("original allocation commit observer lost"))?;
            original
                .continue_readback
                .await
                .map_err(|source| AuraError::Internal {
                    message: "original allocation result delivery interrupted".into(),
                    source: Some(Arc::new(source)),
                })?;
        }
        Ok(())
    }
    pub(crate) fn hold_original_registration_readback_for_test(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Receiver<()>,
            tokio::sync::oneshot::Sender<()>,
        ),
        AuraError,
    > {
        let mut original = self.original_registration_readback_gate.lock();
        if original.is_some() {
            return Err(AuraError::invalid(
                "original registration readback gate already installed",
            ));
        }
        let (committed, observed) = tokio::sync::oneshot::channel();
        let (continue_readback, held) = tokio::sync::oneshot::channel();
        *original = Some(OriginalRegistrationReadbackGate {
            committed,
            continue_readback: held,
        });
        Ok((observed, continue_readback))
    }
    async fn await_original_registration_readback_for_test(&self) -> Result<(), AuraError> {
        let original = self.original_registration_readback_gate.lock().take();
        if let Some(original) = original {
            original
                .committed
                .send(())
                .map_err(|_| AuraError::invalid("original registration commit observer lost"))?;
            original
                .continue_readback
                .await
                .map_err(|source| AuraError::Internal {
                    message: "original registration readback was interrupted".into(),
                    source: Some(Arc::new(source)),
                })?;
        }
        Ok(())
    }
}

#[cfg(test)]
static REGISTRATION_SEAL_FAULTS: std::sync::LazyLock<
    tokio::sync::Mutex<std::collections::BTreeSet<usize>>,
> = std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::BTreeSet::new()));
#[cfg(test)]
impl AuraEffectSystem {
    pub(crate) async fn fail_next_registration_seal_for_test(&self) {
        REGISTRATION_SEAL_FAULTS
            .lock()
            .await
            .insert(self as *const Self as usize);
    }
    async fn require_registration_seal_for_test(&self) -> Result<(), AuraError> {
        if REGISTRATION_SEAL_FAULTS
            .lock()
            .await
            .remove(&(self as *const Self as usize))
        {
            return Err(AuraError::Storage {
                message: "injected original registered-history publication fault".into(),
                source: Some(std::sync::Arc::new(std::io::Error::other(
                    "required registration seal write",
                ))),
            });
        }
        Ok(())
    }
}
// Staged inside runtime/effects/crypto.rs, where the protected verifier and
// authenticated history validators remain private. No raw caller tree DTO.
pub(crate) struct ApprovedEnrollmentTreeCustody<'runtime> {
    effects: EnrollmentRuntimeOrigin<'runtime>,
    generation: EnrollmentGenerationCustodyCapability<'runtime>,
    _tree: aura_protocol::handlers::tree::TreeDecisionLease<'runtime>,
    _archive: Option<
        crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<
            'runtime,
        >,
    >,
    inventory: aura_invitation::enrollment_manifest::EnrollmentParentVerifier,
    baseline: Vec<aura_core::AttestedOp>,
}
impl ApprovedEnrollmentTreeCustody<'_> {
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "ApprovedEnrollmentTreeCustody", receiver_type = ApprovedEnrollmentTreeCustody, family = "runtime_helper")]
    pub(crate) async fn install_original_peer_commit(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        proposal: &aura_sync::protocols::device_epoch_rotation::DeviceEpochProposal,
        commit: &aura_sync::protocols::device_epoch_rotation::DeviceEpochCommit,
    ) -> Result<(), AuraError> {
        use aura_signature::SecurityTranscript;
        use aura_sync::protocols::device_epoch_rotation::*;
        self.require_manifest(approval.effects().as_ref(), approval.manifest())?;
        let DeviceEpochProposalPurpose::Enrollment {
            manifest_digest,
            enrolling_device_id,
        } = proposal.purpose
        else {
            return Err(AuraError::permission_denied(
                "peer commit requires enrollment purpose",
            ));
        };
        if manifest_digest
            != aura_core::Hash32::new(aura_core::hash::hash(
                &approval
                    .manifest()
                    .required_transcript_bytes()
                    .map_err(|source| {
                        AuraError::crypto_with_source(
                            "original peer manifest transcript",
                            Arc::new(source),
                        )
                    })?,
            ))
            || proposal.subject_authority != approval.manifest().subject
            || commit.ceremony_id != proposal.ceremony_id
            || commit.new_epoch != proposal.pending_epoch
            || commit.proposal_hash != device_epoch_proposal_hash(proposal)?
            || commit.attested_leaf_op_hash != device_epoch_commit_attested_op_hash(commit)?
        {
            return Err(AuraError::permission_denied(
                "original peer commit scope mismatch",
            ));
        }
        let key = aura_core::TrustedPublicKey::active(
            aura_core::TrustedKeyDomain::AuthorityThreshold,
            Some(self.inventory.epoch),
            self.inventory.public_key_package.clone(),
            aura_core::Hash32::from_bytes(&self.inventory.public_key_package),
        );
        if !verify_device_epoch_authority_signature(
            approval.effects().as_ref(),
            proposal.subject_authority,
            "aura.sync.device_epoch_rotation.enrollment_commit.v2",
            &DeviceEnrollmentEpochCommitTranscript::from_commit(commit)?,
            &commit.authority_signature,
            &key,
            self.inventory.epoch,
        )
        .await?
        {
            return Err(AuraError::permission_denied(
                "original peer commit signature refused",
            ));
        }
        let leaf = commit
            .attested_leaf_op
            .as_ref()
            .ok_or_else(|| AuraError::invalid("peer commit signed leaf missing"))?;
        let fence = commit
            .attested_epoch_op
            .as_ref()
            .ok_or_else(|| AuraError::invalid("peer commit signed fence missing"))?;
        let before = aura_journal::commitment_tree::reduce(&self.baseline).map_err(|source| {
            AuraError::crypto_with_source("reduce original peer signed history", Arc::new(source))
        })?;
        if leaf.op.parent_epoch != before.epoch
            || leaf.op.parent_commitment != before.root_commitment
        {
            return Err(AuraError::permission_denied(
                "peer leaf does not extend original baseline",
            ));
        }
        let aura_core::TreeOpKind::AddLeaf { leaf: added, .. } = &leaf.op.op else {
            return Err(AuraError::invalid("peer commit is not AddLeaf"));
        };
        if added.device_id != enrolling_device_id || added.role != aura_core::LeafRole::Device {
            return Err(AuraError::permission_denied(
                "peer commit enrolling leaf mismatch",
            ));
        }
        let mut candidate = self.baseline.clone();
        candidate.push(leaf.clone());
        let after_leaf = aura_journal::commitment_tree::reduce(&candidate).map_err(|source| {
            AuraError::crypto_with_source("reduce original peer signed history", Arc::new(source))
        })?;
        if fence.op.parent_epoch != after_leaf.epoch
            || fence.op.parent_commitment != after_leaf.root_commitment
            || !matches!(&fence.op.op, aura_core::TreeOpKind::RotateEpoch { affected } if affected == &vec![aura_core::NodeIndex(0)])
        {
            return Err(AuraError::permission_denied(
                "peer commit epoch fence mismatch",
            ));
        }
        candidate.push(fence.clone());
        let state = aura_journal::commitment_tree::reduce(&candidate).map_err(|source| {
            AuraError::crypto_with_source("reduce original peer signed history", Arc::new(source))
        })?;
        if state.epoch.value() != proposal.pending_epoch {
            return Err(AuraError::permission_denied(
                "peer commit pending epoch mismatch",
            ));
        }
        match &self._archive {
            Some(archive) => {
                approval
                    .effects()
                    .collect_imported_enrollment_parent_inventory(archive, &candidate)
                    .await?;
            }
            None => {
                approval
                    .effects()
                    .collect_enrollment_parent_inventory(&candidate)
                    .await?;
            }
        }
        self._tree
            .install_authenticated_extension(self.baseline.len(), &candidate)
            .await?;
        Ok(())
    }
    pub(crate) fn original_baseline(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<&[aura_core::AttestedOp], AuraError> {
        self.require_manifest(approval.effects().as_ref(), approval.manifest())?;
        Ok(&self.baseline)
    }
    pub(crate) fn generation(&self) -> &EnrollmentGenerationCustodyCapability<'_> {
        &self.generation
    }
    pub(crate) fn require_manifest(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> Result<(), AuraError> {
        self.generation.require_effects(effects)?;
        if !std::ptr::eq(self.effects.effects(), effects)
            || manifest.subject != effects.authority_id
            || manifest.final_epoch != self.inventory.epoch
            || manifest.final_commitment != self.inventory.commitment
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        let supplied = manifest.final_inventory().map_err(|source| {
            AuraError::crypto_with_source(
                "approved exact active inventory missing",
                Arc::new(source),
            )
        })?;
        if aura_core::util::serialization::to_vec(&supplied)?
            != aura_core::util::serialization::to_vec(&vec![self.inventory.clone()])?
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::Substituted,
            ));
        }
        Ok(())
    }
}
impl AuraEffectSystem {
    async fn authenticate_original_approved_inventory<'runtime>(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
        generation: &EnrollmentGenerationCustodyCapability<'runtime>,
        _tree: &aura_protocol::handlers::tree::TreeDecisionLease<'runtime>,
        archive: &mut Option<crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<'runtime>>,
        origin: EnrollmentRuntimeOrigin<'runtime>,
    ) -> Result<
        (
            aura_invitation::enrollment_manifest::EnrollmentParentVerifier,
            Vec<aura_core::AttestedOp>,
        ),
        AuraError,
    > {
        generation.require_effects(self)?;
        if !std::ptr::eq(self, origin.effects()) || !std::ptr::eq(self, approval.effects().as_ref())
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        let ops = self
            .export_tree_ops()
            .await
            .map_err(|source| AuraError::Crypto {
                message: "read original approved native signing history".into(),
                source: Some(Arc::new(source)),
            })?;
        if archive.is_none() {
            crate::runtime::services::enrollment_profile::load_original_active_profile_archive_from_origin(
                origin, archive,
            ).await?;
        }
        let parents = match archive.as_ref() {
            Some(original) => {
                self.collect_imported_enrollment_parent_inventory(original, &ops)
                    .await?
            }
            None => self.collect_enrollment_parent_inventory(&ops).await?,
        };
        let state = aura_journal::commitment_tree::reduce(&ops).map_err(|source| {
            AuraError::crypto_with_source(
                "reduce original approved authenticated history",
                Arc::new(source),
            )
        })?;
        let baseline = ops
            .iter()
            .map(aura_core::util::serialization::to_vec)
            .collect::<Result<Vec<_>, _>>()?;
        let manifest = approval.manifest();
        manifest.validate_shape().map_err(|source| {
            AuraError::crypto_with_source(
                "validate native approved manifest shape",
                Arc::new(source),
            )
        })?;
        manifest
            .validate_setup_validity(approval.setup().statement())
            .map_err(|source| {
                AuraError::crypto_with_source(
                    "validate native original setup binding",
                    Arc::new(source),
                )
            })?;
        let genesis = aura_journal::commitment_tree::state::TreeState::new();
        if manifest.baseline_count as usize != ops.len()
            || manifest.baseline_digest
                != aura_core::hash::hash(&aura_core::util::serialization::to_vec(&baseline)?)
            || aura_core::util::serialization::to_vec(&manifest.parents)?
                != aura_core::util::serialization::to_vec(&parents)?
            || manifest.starting_epoch != genesis.epoch.value()
            || manifest.starting_commitment != genesis.root_commitment
            || manifest.invitee_authority != approval.setup().statement().authority
            || manifest.invitee_device != approval.setup().statement().device
            || manifest.setup.nonce != approval.setup().statement().nonce
            || manifest.setup.digest != approval.setup().digest()
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::BaselineMismatch,
            ));
        }
        if state
            .branches
            .keys()
            .any(|node| *node != aura_core::tree::NodeIndex(0))
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::PolicyMismatch,
            ));
        }
        if state
            .leaves
            .keys()
            .any(|leaf| state.get_leaf_parent(*leaf) != Some(aura_core::tree::NodeIndex(0)))
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::MissingLeafParent,
            ));
        }
        let epoch = state.epoch.value();
        let metadata = self
            .require_threshold_config_metadata(&self.authority_id, epoch)
            .await?;
        let (_, threshold, package) = self
            .trusted_tree_parent_verifier_inventory(&self.authority_id, epoch)
            .await?;
        let devices: std::collections::BTreeSet<_> = state
            .leaves
            .values()
            .filter(|leaf| leaf.role == aura_core::tree::LeafRole::Device)
            .map(|leaf| leaf.device_id)
            .collect();
        if !devices.contains(&self.device_id())
            || !devices.contains(&manifest.initiator_device)
            || devices.contains(&manifest.invitee_device)
            || metadata.participants.len() != devices.len()
            || metadata
                .participants
                .iter()
                .any(|participant| match participant {
                    ParticipantIdentity::Device(device) => !devices.contains(device),
                    _ => true,
                })
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::PolicyMismatch,
            ));
        }
        Ok((
            aura_invitation::enrollment_manifest::EnrollmentParentVerifier {
                epoch,
                commitment: state.root_commitment,
                signing_node: aura_core::tree::NodeIndex(0),
                mode: metadata.mode,
                threshold,
                participants: metadata.participants,
                public_key_package: package,
                agreement: metadata.agreement_mode,
            },
            ops,
        ))
    }
}
/// Actual original native acquisition lives in the registered participant
/// holder before its first await. Partial acquisition is retained on Drop.
pub(crate) struct OriginalApprovedNativeAdmission {
    effects: Arc<AuraEffectSystem>,
    approved_intent_digest: [u8; 32],
    generation: Option<EnrollmentGenerationCustodyCapability<'static>>,
    tree: Option<aura_protocol::handlers::tree::TreeDecisionLease<'static>>,
    archive: Option<
        crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<
            'static,
        >,
    >,
    authenticated: Option<Arc<ApprovedEnrollmentTreeCustody<'static>>>,
    first_failure: Option<AuraError>,
}
impl OriginalApprovedNativeAdmission {
    pub(crate) fn from_original(
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Self {
        Self {
            effects: Arc::clone(approval.effects()),
            approved_intent_digest: approval.canonical_intent_digest(),
            generation: None,
            tree: None,
            archive: None,
            authenticated: None,
            first_failure: None,
        }
    }
    pub(crate) async fn acquire_original(
        &mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        let result = self.acquire_original_once(approval).await;
        if let Err(source) = &result {
            self.first_failure.get_or_insert_with(|| source.clone());
        }
        result
    }
    async fn acquire_original_once(
        &mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        if let Some(source) = &self.first_failure {
            return Err(source.clone());
        }
        if let Some(actual) = &self.authenticated {
            return actual.require_manifest(self.effects.as_ref(), approval.manifest());
        }
        if self.generation.is_none() {
            let guard = Arc::clone(&self.effects.enrollment_generation_gate)
                .lock_owned()
                .await;
            self.generation = Some(EnrollmentGenerationCustodyCapability {
                effects: EnrollmentRuntimeOrigin::Owned(Arc::clone(&self.effects)),
                _guard: EnrollmentGenerationGuard::Owned { _guard: guard },
            });
        }
        if self.tree.is_none() {
            self.tree = Some(self.effects.tree_handler.lock_owned_decision().await);
        }
        let generation = self
            .generation
            .as_ref()
            .ok_or_else(|| final_inventory_error(EnrollmentFinalInventoryError::OwnerBinding))?;
        let tree = self
            .tree
            .as_ref()
            .ok_or_else(|| final_inventory_error(EnrollmentFinalInventoryError::OwnerBinding))?;
        let (inventory, baseline) = self
            .effects
            .authenticate_original_approved_inventory(
                approval,
                generation,
                tree,
                &mut self.archive,
                EnrollmentRuntimeOrigin::Owned(Arc::clone(&self.effects)),
            )
            .await?;
        // All subsequent moves are synchronous. A refused transition restores
        // every actual guard; no await ever sees an empty retained owner.
        match (self.generation.take(), self.tree.take()) {
            (Some(generation), Some(tree)) => {
                self.authenticated = Some(Arc::new(ApprovedEnrollmentTreeCustody {
                    effects: EnrollmentRuntimeOrigin::Owned(Arc::clone(&self.effects)),
                    generation,
                    _tree: tree,
                    _archive: self.archive.take(),
                    inventory,
                    baseline,
                }));
                Ok(())
            }
            (generation, tree) => {
                self.generation = generation;
                self.tree = tree;
                Err(final_inventory_error(
                    EnrollmentFinalInventoryError::OwnerBinding,
                ))
            }
        }
    }
    pub(crate) fn original_authenticated(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<Arc<ApprovedEnrollmentTreeCustody<'static>>, AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        let actual = self
            .authenticated
            .as_ref()
            .ok_or_else(|| final_inventory_error(EnrollmentFinalInventoryError::OwnerBinding))?;
        actual.require_manifest(self.effects.as_ref(), approval.manifest())?;
        Ok(Arc::clone(actual))
    }
}

// Staged inside runtime/effects/crypto.rs. One narrow borrowed signing owner
// preserves the actual original tree/generation allocation in both roles.
enum EnrollmentTranscriptTreeOrigin<'custody, 'owner, 'runtime> {
    Issuer(&'custody EnrollmentFinalVerifierInventoryCapability<'owner, 'runtime>),
    Participant(&'custody ApprovedEnrollmentTreeCustody<'runtime>),
}
pub(crate) struct EnrollmentTranscriptTreeOwner<'custody, 'owner, 'runtime> {
    origin: EnrollmentTranscriptTreeOrigin<'custody, 'owner, 'runtime>,
}
impl<'custody, 'owner, 'runtime> EnrollmentTranscriptTreeOwner<'custody, 'owner, 'runtime> {
    pub(crate) fn from_original_issuer(
        inventory: &'custody EnrollmentFinalVerifierInventoryCapability<'owner, 'runtime>,
    ) -> Self {
        Self {
            origin: EnrollmentTranscriptTreeOrigin::Issuer(inventory),
        }
    }
    pub(crate) fn from_original_participant(
        custody: &'custody ApprovedEnrollmentTreeCustody<'runtime>,
    ) -> Self {
        Self {
            origin: EnrollmentTranscriptTreeOrigin::Participant(custody),
        }
    }
    pub(crate) fn require_manifest(
        &self,
        effects: &AuraEffectSystem,
        manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
    ) -> Result<(), AuraError> {
        match self.origin {
            EnrollmentTranscriptTreeOrigin::Issuer(inventory) => {
                inventory.require_manifest(effects, manifest)
            }
            EnrollmentTranscriptTreeOrigin::Participant(custody) => {
                custody.require_manifest(effects, manifest)
            }
        }
    }
}

/// Actual original locks have been released; the original confirmed archive
/// remains retained independently of the participant's domain failure.
pub(crate) struct AcknowledgedApprovedNativeDisposal {
    effects: Arc<AuraEffectSystem>,
    approved_intent_digest: [u8; 32],
    _archive: Option<
        crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<
            'static,
        >,
    >,
    first_failure: Option<AuraError>,
    peer_reentry_prepared: bool,
}
impl AcknowledgedApprovedNativeDisposal {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "AcknowledgedApprovedNativeDisposal",
        receiver_type = AcknowledgedApprovedNativeDisposal,
        family = "runtime_helper"
    )]
    pub(crate) fn prepare_original_peer_reentry(
        &mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<OriginalApprovedNativeAdmission, AuraError> {
        self.require_original(approval)?;
        if let Some(source) = self.first_failure.as_ref() {
            return Err(source.clone());
        }
        if self.peer_reentry_prepared {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        let mut actual = OriginalApprovedNativeAdmission::from_original(approval);
        actual.archive = self._archive.take();
        self.peer_reentry_prepared = true;
        Ok(actual)
    }
    pub(crate) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        Ok(())
    }
    pub(crate) fn original_failure(&self) -> Option<&AuraError> {
        self.first_failure.as_ref()
    }
}
impl OriginalApprovedNativeAdmission {
    pub(crate) fn require_original(
        &self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<(), AuraError> {
        if !Arc::ptr_eq(&self.effects, approval.effects())
            || self.approved_intent_digest != approval.canonical_intent_digest()
        {
            return Err(final_inventory_error(
                EnrollmentFinalInventoryError::OwnerBinding,
            ));
        }
        Ok(())
    }
    pub(crate) fn original_failure(&self) -> Option<&AuraError> {
        self.first_failure.as_ref()
    }
    pub(crate) fn dispose_original(
        mut self,
        approval: &crate::runtime_bridge::enrollment_quorum::RuntimeApprovedEnrollmentSigningIntent,
    ) -> Result<AcknowledgedApprovedNativeDisposal, Box<(Self, AuraError)>> {
        if let Err(source) = self.require_original(approval) {
            return Err(Box::new((self, source)));
        }
        if let Some(actual) = self.authenticated.take() {
            match Arc::try_unwrap(actual) {
                Ok(mut actual) => {
                    self.archive = actual._archive.take();
                    // The actual original native leases drop synchronously.
                    drop(actual);
                }
                Err(actual) => {
                    self.authenticated = Some(actual);
                    return Err(Box::new((
                        self,
                        final_inventory_error(EnrollmentFinalInventoryError::OwnerBinding),
                    )));
                }
            }
        }
        self.tree = None;
        self.generation = None;
        Ok(AcknowledgedApprovedNativeDisposal {
            effects: Arc::clone(&self.effects),
            approved_intent_digest: self.approved_intent_digest,
            _archive: self.archive.take(),
            peer_reentry_prepared: false,
            first_failure: self.first_failure.take(),
        })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod owned_enrollment_reservation_tests {
    use super::*;
    use aura_app::runtime_bridge::RuntimeBridge;

    #[derive(Clone, Copy, Debug)]
    enum PreflightFault {
        TypedScopeRoundtrip,
        CommittedContactSent,
        MissingEpoch,
        MalformedEpoch,
        ActiveEpoch,
        AllocationMismatch,
        LiveMismatch,
        Activation,
        Registration,
        PermanentWrap,
        ConflictingFirstDecision,
    }

    async fn partial_storage_snapshot(
        effects: &AuraEffectSystem,
        owner: &StoredEnrollmentGenerationProfile,
    ) -> Vec<Option<Vec<u8>>> {
        let mut locations = vec![
            SecureStorageLocation::new(
                "device_enrollment_orphan_retirement_v1",
                owner.ceremony.to_string(),
            ),
            SecureStorageLocation::new(
                "device_enrollment_orphan_cleaned_v1",
                owner.ceremony.to_string(),
            ),
            SecureStorageLocation::new(
                "device_enrollment_generation_allocation_v1",
                owner.ceremony.to_string(),
            ),
            super::super::enrollment_generation_profile_location(
                &owner.authority,
                owner.pending_epoch,
            ),
        ];
        locations.extend(owner.participants.iter().map(|participant| {
            AuraEffectSystem::participant_share_location(
                &owner.authority,
                owner.pending_epoch,
                participant,
            )
        }));
        let mut snapshot = Vec::new();
        for location in locations {
            snapshot.push(
                if effects
                    .secure_exists(&location)
                    .await
                    .expect("original fixture: secure exists")
                {
                    Some(
                        effects
                            .secure_retrieve(&location, &[SecureStorageCapability::Read])
                            .await
                            .expect("original fixture: secure retrieve"),
                    )
                } else {
                    None
                },
            );
        }
        snapshot
    }

    #[test]
    fn original_secret_birth_scope_roundtrips_only_from_actual_retained_plan() {
        original_owned_rotation_scenario(true, Some(PreflightFault::TypedScopeRoundtrip));
    }

    #[test]
    fn original_partial_retirement_refuses_actual_committed_contact_sent_without_mutation() {
        original_owned_rotation_scenario(true, Some(PreflightFault::CommittedContactSent));
    }

    #[test]
    fn original_partial_retirement_preflight_refusals_preserve_custody_and_storage() {
        for fault in [
            PreflightFault::MissingEpoch,
            PreflightFault::MalformedEpoch,
            PreflightFault::ActiveEpoch,
            PreflightFault::AllocationMismatch,
            PreflightFault::LiveMismatch,
            PreflightFault::Activation,
            PreflightFault::Registration,
            PreflightFault::PermanentWrap,
            PreflightFault::ConflictingFirstDecision,
        ] {
            original_owned_rotation_scenario(true, Some(fault));
        }
    }

    #[test]
    fn original_owned_rotation_retains_exact_runtime_and_excludes_generation_and_tree_until_drop() {
        original_owned_rotation_scenario(false, None);
    }

    #[test]
    fn original_partial_retirement_refuses_foreign_launch_and_failed_validation_before_guard_release(
    ) {
        original_owned_rotation_scenario(true, None);
    }

    fn original_owned_rotation_scenario(cleanup: bool, fault: Option<PreflightFault>) {
        crate::handlers::invitation::tests::run_async_test_on_large_stack(async move {
            let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(5_000));
            let ((issuer, invitee, invitation, _start, _accept, _witness), original) = Box::pin(
                crate::handlers::invitation::tests::actual_pinned_device_enrollment_fixture_with_original_continuation(
                    &format!("{}-{fault:?}", if cleanup { "owned-partial-release" } else { "owned-original-rotation" }), Some(clock.clone()), None,
                ),
            ).await;
            let issuer_disposal = original
                .original_disposal_for_test()
                .expect("original fixture: original disposal for test");
            issuer
                .invitations()
                .expect("original fixture: invitations")
                .cancel(&invitation.invitation_id)
                .await
                .expect("original fixture: cancel");
            original
                .original_cancellation_disposal_for_test()
                .await
                .expect("original fixture: original cancellation disposal for test")
                .await_disposal()
                .await;
            issuer_disposal.await_disposal().await;
            assert!(original
                .observes_retained_retirement_acknowledgment()
                .expect("original fixture: observes retained retirement acknowledgment"));
            let bridge = Arc::new(crate::runtime_bridge::AgentRuntimeBridge::new(
                issuer.clone(),
            ));
            let app = Arc::new(async_lock::RwLock::new(
                aura_app::AppCore::with_runtime(aura_app::AppConfig::default(), bridge)
                    .expect("actual app attaches original runtime"),
            ));
            let code = crate::runtime_bridge::AgentRuntimeBridge::new(invitee.clone())
                .export_device_enrollment_setup_request()
                .await
                .expect("original fixture: export device enrollment setup request");
            let setup =
                aura_app::ui::workflows::ceremonies::pin_user_transferred_device_enrollment_setup(
                    &app, code,
                )
                .await
                .expect("actual setup is pinned by original app");
            let effects = issuer.runtime().effects();
            let plan = effects
                .prepare_owned_authenticated_enrollment_rotation(
                    &setup,
                    issuer.runtime().ceremony_tracker(),
                )
                .await
                .expect("original fixture: ceremony tracker");
            let EnrollmentRuntimeOrigin::Owned(actual) = &plan.effects else {
                panic!("actual owned runtime origin")
            };
            assert!(Arc::ptr_eq(actual, &effects));
            plan.require_effects(effects.as_ref(), &setup)
                .expect("original fixture: require effects");
            assert!(plan
                .require_effects(invitee.runtime().effects().as_ref(), &setup)
                .is_err());
            if cleanup {
                use crate::runtime::services::enrollment_window::{
                    OriginalAllocationCheckpointCustody, OriginalIssuerLaunchWindow,
                };
                let (launch, _) = OriginalIssuerLaunchWindow::select(effects.clone())
                    .await
                    .expect("selected provider issues original launch window");
                let (foreign, _) = OriginalIssuerLaunchWindow::select(effects.clone())
                    .await
                    .expect("foreign launch owns its independent original window");
                let reserved = issuer
                    .invitations()
                    .expect("original fixture: invitations")
                    .reserve_device_enrollment_invitation()
                    .await
                    .expect("original fixture: reserve device enrollment invitation");
                let ceremony = aura_core::CeremonyId::new(format!(
                    "actual-partial-release:{}",
                    reserved.invitation_id()
                ));
                let mut producer = OriginalEnrollmentAllocationProducer::from_original(plan);
                producer
                    .bind_original_launch(effects.as_ref(), &launch)
                    .expect("original fixture: bind original launch");
                let (reached, _resume) = effects
                    .hold_original_allocation_commit_for_test()
                    .expect("original fixture: hold original allocation commit for test");
                {
                    let mut allocating = Box::pin(effects.prepare_pinned_enrollment_rotation(
                        &setup,
                        &reserved,
                        &ceremony,
                        &mut producer,
                    ));
                    tokio::select! {
                        outcome = allocating.as_mut() => panic!("allocation completed before retained backend boundary: {}", outcome.is_ok()),
                        signal = reached => signal.expect("actual backend reached immutable allocation commit"),
                    }
                }
                let primary = AuraError::invalid("actual original allocation interrupted");
                if matches!(fault, Some(PreflightFault::TypedScopeRoundtrip)) {
                    let actual_plan = producer
                        .plan
                        .as_ref()
                        .expect("interrupted producer retains original plan");
                    let actual_profile = producer
                        .progress
                        .original
                        .as_ref()
                        .expect("interrupted producer retains original allocation profile");
                    let scope = EnrollmentSecretScope::original(
                        actual_profile,
                        &actual_profile.participants[0],
                        &[],
                    )
                    .expect("original profile produces exact typed secret scope");
                    let birth = OwnedSecretBirthCapability {
                        runtime_identity: effects.crypto.lifetime_owner_identity(),
                        origin: OriginalSecretBirthOrigin::Plan(actual_plan),
                        scope,
                    };
                    birth
                        .require_runtime_owner(&effects.crypto.lifetime_owner_identity())
                        .expect("original fixture: lifetime owner identity");
                    let bytes = birth.scope_bytes().expect("original fixture: scope bytes");
                    let decoded: EnrollmentSecretScope =
                        serde_json::from_slice(&bytes).expect("canonical secret scope decodes");
                    assert_eq!(decoded, birth.scope);
                    assert_eq!(
                        decoded
                            .encode()
                            .expect("typed secret scope reencodes exactly"),
                        bytes
                    );
                }
                if matches!(fault, Some(PreflightFault::CommittedContactSent)) {
                    let owner = producer
                        .progress
                        .original
                        .as_ref()
                        .expect("producer retains original allocation after contact commit")
                        .clone();
                    let actual = issuer
                        .invitations()
                        .expect("original fixture: invitations")
                        .commit_reserved_contact_for_retirement_test(reserved)
                        .await
                        .expect("original fixture: commit reserved contact for retirement test");
                    assert_eq!(actual.invitation_id, owner.invitation);
                    assert!(matches!(
                        actual.invitation_type,
                        aura_invitation::InvitationType::Contact { .. }
                    ));
                    let before = partial_storage_snapshot(effects.as_ref(), &owner).await;
                    let generation = effects.enrollment_retirement_generation_guard();
                    let tree = effects.lock_tree_decision();
                    futures::pin_mut!(generation, tree);
                    assert!(producer.retire_original(effects.as_ref()).await.is_err());
                    assert!(producer.plan.is_some());
                    assert!(producer.retirement.is_none());
                    assert!(!producer.acknowledged);
                    assert!(futures::poll!(generation.as_mut()).is_pending());
                    assert!(futures::poll!(tree.as_mut()).is_pending());
                    assert_eq!(
                        partial_storage_snapshot(effects.as_ref(), &owner).await,
                        before
                    );
                    // This proves refusal; the actual committed fact is not erased for a cleanup ACK.
                    return;
                }
                if let Some(fault) =
                    fault.filter(|fault| !matches!(fault, PreflightFault::TypedScopeRoundtrip))
                {
                    let owner = producer
                        .progress
                        .original
                        .as_ref()
                        .expect("producer retains original allocation for fault injection")
                        .clone();
                    let location = match fault {
                        PreflightFault::TypedScopeRoundtrip => {
                            unreachable!("typed scope handled above")
                        }
                        PreflightFault::CommittedContactSent => {
                            unreachable!("committed contact handled above")
                        }
                        PreflightFault::MissingEpoch
                        | PreflightFault::MalformedEpoch
                        | PreflightFault::ActiveEpoch => {
                            SecureStorageLocation::new("epoch_state", owner.authority.to_string())
                        }
                        PreflightFault::AllocationMismatch => SecureStorageLocation::new(
                            "device_enrollment_generation_allocation_v1",
                            owner.ceremony.to_string(),
                        ),
                        PreflightFault::LiveMismatch => {
                            super::super::enrollment_generation_profile_location(
                                &owner.authority,
                                owner.pending_epoch,
                            )
                        }
                        PreflightFault::Activation => SecureStorageLocation::new(
                            "device_enrollment_activation_v1",
                            owner.ceremony.to_string(),
                        ),
                        PreflightFault::Registration => SecureStorageLocation::new(
                            "device_enrollment_allocated_registration_v1",
                            owner.ceremony.to_string(),
                        ),
                        PreflightFault::PermanentWrap => {
                            AuraEffectSystem::participant_wrap_key_location(
                                &owner.authority,
                                owner.pending_epoch,
                                owner.participants.first().expect("original fixture: first"),
                            )
                        }
                        PreflightFault::ConflictingFirstDecision => SecureStorageLocation::new(
                            "device_enrollment_orphan_retirement_v1",
                            owner.ceremony.to_string(),
                        ),
                    };
                    let original_bytes = if effects
                        .secure_exists(&location)
                        .await
                        .expect("original fixture: secure exists")
                    {
                        Some(
                            effects
                                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                                .await
                                .expect("original fixture: secure retrieve"),
                        )
                    } else {
                        None
                    };
                    effects
                        .fault_remove_secure_record_for_test(&location)
                        .await
                        .expect("original fixture: fault remove secure record for test");
                    let injected = match fault {
                        PreflightFault::MissingEpoch => None,
                        PreflightFault::ActiveEpoch => {
                            Some(owner.pending_epoch.to_le_bytes().to_vec())
                        }
                        _ => Some(vec![0xff]),
                    };
                    if let Some(bytes) = injected.as_ref() {
                        if matches!(fault, PreflightFault::ConflictingFirstDecision) {
                            effects
                                .secure_store_immutable(
                                    &location,
                                    bytes,
                                    &[
                                        SecureStorageCapability::Read,
                                        SecureStorageCapability::Write,
                                    ],
                                )
                                .await
                                .expect("original fixture: secure store immutable");
                        } else {
                            effects
                                .secure_store(&location, bytes, &[SecureStorageCapability::Write])
                                .await
                                .expect("original fixture: secure store");
                        }
                    }
                    let first = SecureStorageLocation::new(
                        "device_enrollment_orphan_retirement_v1",
                        owner.ceremony.to_string(),
                    );
                    let had_first_decision = effects
                        .secure_exists(&first)
                        .await
                        .expect("original fixture: secure exists");
                    assert_eq!(
                        had_first_decision,
                        matches!(fault, PreflightFault::ConflictingFirstDecision)
                    );
                    let before = partial_storage_snapshot(effects.as_ref(), &owner).await;
                    let generation = effects.enrollment_retirement_generation_guard();
                    let tree = effects.lock_tree_decision();
                    futures::pin_mut!(generation, tree);
                    assert!(
                        producer.retire_original(effects.as_ref()).await.is_err(),
                        "{fault:?}"
                    );
                    assert!(producer.plan.is_some(), "{fault:?}");
                    assert!(producer.retirement.is_none(), "{fault:?}");
                    assert_eq!(
                        effects
                            .secure_exists(&first)
                            .await
                            .expect("original fixture: secure exists"),
                        had_first_decision,
                        "{fault:?}"
                    );
                    assert!(
                        futures::poll!(generation.as_mut()).is_pending(),
                        "{fault:?}"
                    );
                    assert!(futures::poll!(tree.as_mut()).is_pending(), "{fault:?}");
                    assert_eq!(
                        partial_storage_snapshot(effects.as_ref(), &owner).await,
                        before,
                        "{fault:?}"
                    );
                    let retained = if effects
                        .secure_exists(&location)
                        .await
                        .expect("original fixture: secure exists")
                    {
                        Some(
                            effects
                                .secure_retrieve(&location, &[SecureStorageCapability::Read])
                                .await
                                .expect("original fixture: secure retrieve"),
                        )
                    } else {
                        None
                    };
                    assert_eq!(retained, injected, "{fault:?}");
                    effects
                        .fault_remove_secure_record_for_test(&location)
                        .await
                        .expect("original fixture: fault remove secure record for test");
                    if let Some(bytes) = original_bytes {
                        effects
                            .secure_store(&location, &bytes, &[SecureStorageCapability::Write])
                            .await
                            .expect("original fixture: secure store");
                    }
                }
                let work = launch
                    .execute_original_allocation_cleanup(&effects, &primary, || {
                        producer.retire_original(effects.as_ref())
                    })
                    .await
                    .expect("original fixture: retire original");
                assert!(matches!(work.operation, Some(Ok(()))));
                work.validation
                    .expect("original cleanup stays within selected fixed endpoint");
                let generation = effects.enrollment_retirement_generation_guard();
                futures::pin_mut!(generation);
                assert!(futures::poll!(generation.as_mut()).is_pending());
                let native = Arc::new(std::io::Error::other(
                    "original cleanup post-observation failed",
                ));
                let mut checkpoint = OriginalAllocationCheckpointCustody::default();
                let failed = launch.acknowledge_original_allocation_cleanup(&effects, &primary, || async {
                    let receipt = producer.acknowledge_original(effects.as_ref()).expect("original fixture: acknowledge original");
                    clock.fail_next_observation(aura_core::effects::TimeError::ProviderFailure {
                        operation: aura_core::effects::time::TimeProviderOperation::ReadPhysicalClock,
                        source: Some(native.clone()),
                    }).await;
                    Ok(receipt)
                }, &mut checkpoint).await.err().expect("failed post-observation cannot authorize release");
                let mut source: &(dyn std::error::Error + 'static) = &failed;
                loop {
                    if let Some(actual) = source.downcast_ref::<std::io::Error>() {
                        assert!(std::ptr::eq(actual, native.as_ref()));
                        break;
                    }
                    source = source
                        .source()
                        .expect("original native provider cause retained");
                }
                assert!(producer.plan.is_some());
                assert!(futures::poll!(generation.as_mut()).is_pending());
                let mut foreign_checkpoint = OriginalAllocationCheckpointCustody::default();
                let foreign_ack = foreign
                    .acknowledge_original_allocation_cleanup(
                        &effects,
                        &primary,
                        || async {
                            Ok(producer
                                .acknowledge_original(effects.as_ref())
                                .expect("original fixture: acknowledge original"))
                        },
                        &mut foreign_checkpoint,
                    )
                    .await
                    .expect("foreign acknowledgment observation completes with typed refusal");
                assert!(producer
                    .release_original(effects.as_ref(), foreign_ack)
                    .is_err());
                assert!(producer.plan.is_some());
                assert!(producer.retirement.is_some());
                assert!(futures::poll!(generation.as_mut()).is_pending());
                let actual_ack = launch
                    .acknowledge_original_allocation_cleanup(
                        &effects,
                        &primary,
                        || async {
                            Ok(producer
                                .acknowledge_original(effects.as_ref())
                                .expect("original fixture: acknowledge original"))
                        },
                        &mut checkpoint,
                    )
                    .await
                    .expect("original acknowledgment observation completes");
                producer
                    .release_original(effects.as_ref(), actual_ack)
                    .expect("original fixture: release original");
                assert!(producer
                    .observe_original_cleanup_acknowledged(effects.as_ref())
                    .expect("original fixture: observe original cleanup acknowledged"));
                let mut retained_source: &(dyn std::error::Error + 'static) = checkpoint
                    .first_validation_failure_for_test()
                    .expect("first post-observation failure survives successful original retry");
                loop {
                    if let Some(actual) = retained_source.downcast_ref::<std::io::Error>() {
                        assert!(std::ptr::eq(actual, native.as_ref()));
                        break;
                    }
                    retained_source = retained_source
                        .source()
                        .expect("original provider allocation retained");
                }
                drop(generation.await);
                return;
            }
            let generation = effects.enrollment_retirement_generation_guard();
            let tree = effects.lock_tree_decision();
            let tracker = issuer.runtime().ceremony_tracker();
            let decision = tracker.original_enrollment_decision_guard_for_test();
            futures::pin_mut!(generation, tree, decision);
            assert!(futures::poll!(generation.as_mut()).is_pending());
            assert!(futures::poll!(tree.as_mut()).is_pending());
            assert!(futures::poll!(decision.as_mut()).is_pending());
            drop(plan);
            drop(generation.await);
            drop(tree.await);
            drop(decision.await);
        });
    }
}
