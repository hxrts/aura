/// Exact retained package location and original storage cause, without
/// changing absence into another layout after a failed required read.
#[derive(Debug, thiserror::Error)]
#[error("read retained parent package for authority {authority}, epoch {epoch}, location {location:?}: {source}")]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ParticipantKeyPackageEnvelope {
    version: u8,
    authority: AuthorityId,
    epoch: u64,
    recipient: ParticipantIdentity,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

/// A held, authenticated roster decision. Raw identifiers cannot construct it.
/// Both custody guards remain held until the original issuer registration ends.
pub(crate) struct AuthenticatedEnrollmentRotationPlan<'a> {
    effects: &'a AuraEffectSystem,
    setup_digest: [u8; 32],
    state: aura_journal::commitment_tree::state::TreeState,
    participants: Vec<ParticipantIdentity>,
    threshold: u16,
    prestate: aura_core::Hash32,
    generation: tokio::sync::MutexGuard<'a, ()>,
    tree: aura_protocol::handlers::tree::TreeDecisionLease<'a>,
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
        if !std::ptr::eq(self.effects, effects) || self.setup_digest != setup.digest() {
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
    generation: tokio::sync::MutexGuard<'a, ()>,
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
pub(crate) struct EnrollmentGenerationReservation<'a> {
    effects: &'a AuraEffectSystem,
    owner: StoredEnrollmentGenerationProfile,
    _owner: tokio::sync::MutexGuard<'a, ()>,
    _tree: aura_protocol::handlers::tree::TreeDecisionLease<'a>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    threshold: u16,
    participants: Vec<ParticipantIdentity>,
}
/// Process-local capability minted after required original registration storage.
/// It cannot be constructed from an invitation, id, or deserialized state.
/// ```compile_fail
/// use aura_agent::runtime::effects::RegisteredEnrollmentGenerationCapability;
/// fn forge(invitation: aura_invitation::Invitation) -> RegisteredEnrollmentGenerationCapability {
///     RegisteredEnrollmentGenerationCapability { canonical_invitation: invitation }
/// }
/// ```
pub(crate) struct RegisteredEnrollmentGenerationCapability {
    canonical_invitation: aura_invitation::Invitation,
}
impl RegisteredEnrollmentGenerationCapability {
    pub(crate) fn canonical_invitation(&self) -> &aura_invitation::Invitation {
        &self.canonical_invitation
    }
}
#[derive(Debug, thiserror::Error)]
pub(crate) enum HeldEnrollmentRegistrationError {
    #[error("registration uses another physical effect owner")]
    EffectIdentity,
    #[error("persistent enrollment allocation requires the held generation owner")]
    RequiredOwner,
    #[error("registration requires a device enrollment invitation")]
    InvitationKind,
    #[error("registration differs from held enrollment generation bindings")]
    Binding,
}
pub(crate) fn held_registration_error(reason: HeldEnrollmentRegistrationError) -> AuraError {
    AuraError::Invalid {
        message: "owned enrollment registration rejected".into(),
        source: Some(std::sync::Arc::new(reason)),
    }
}
impl EnrollmentGenerationReservation<'_> {
    pub(crate) fn validate_allocated_registration(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects, effects) {
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
        if self.owner.registered
            || state.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
            || state.initiator_id != self.owner.authority
            || state.ceremony_id != self.owner.ceremony
            || state.prestate_hash != self.owner.prestate
            || state.new_epoch != self.owner.pending_epoch
            || state
                .enrollment_device_id
                .map(ParticipantIdentity::device)
                .map_or(true, |device| !expected.contains(&device))
            || state.participants != expected
            || usize::from(state.total_n) != expected.len()
            || state.threshold_k != self.owner.threshold.min(state.total_n)
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

    /// Authorize registration from this held physical generation allocation.
    pub(crate) fn validate_pending_registration(
        &self,
        effects: &AuraEffectSystem,
        state: &crate::runtime::services::ceremony_tracker::TrackedCeremony,
        invitation: &aura_invitation::Invitation,
    ) -> Result<(), AuraError> {
        if !std::ptr::eq(self.effects, effects) {
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
        if self.owner.registered
            || invitation.sender_id != self.owner.authority
            || invitation.invitation_id != self.owner.invitation
            || *subject_authority != self.owner.authority
            || *initiator_device_id != effects.device_id()
            || *ceremony_id != self.owner.ceremony
            || *pending_epoch != self.owner.pending_epoch
            || setup_binding.as_ref().map(|binding| binding.digest) != Some(self.owner.setup_digest)
            || state.kind != aura_app::runtime_bridge::CeremonyKind::DeviceEnrollment
            || state.initiator_id != self.owner.authority
            || state.ceremony_id != self.owner.ceremony
            || state.prestate_hash != self.owner.prestate
            || state.new_epoch != self.owner.pending_epoch
            || state.enrollment_device_id != Some(*device_id)
            || !expected.contains(&ParticipantIdentity::device(*device_id))
            || state.participants != expected
            || usize::from(state.total_n) != expected.len()
            || state.threshold_k != self.owner.threshold.min(state.total_n)
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
        capability = "owned_enrollment_generation",
        family = "runtime_helper"
    )]
    pub(crate) async fn complete_registration(
        mut self,
    ) -> Result<RegisteredEnrollmentGenerationCapability, AuraError> {
        let canonical_invitation =
            crate::handlers::invitation::enrollment_trust::verify_generation_registration_binding(
                self.effects,
                self.owner.authority,
                self.owner.pending_epoch,
                &self.owner.ceremony,
                self.owner.prestate,
                &self.owner.invitation,
                self.owner.setup_digest,
            )
            .await
            .map_err(|error| AuraError::Internal {
                message: "complete owned enrollment registration".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        let location = super::enrollment_generation_profile_location(
            &self.owner.authority,
            self.owner.pending_epoch,
        );
        let old = self
            .effects
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        let expected = serde_json::to_vec(&self.owner).map_err(|error| AuraError::Internal {
            message: "encode allocated enrollment owner".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        if old != expected {
            return Err(AuraError::invalid(
                "allocated enrollment generation was replaced",
            ));
        }
        self.owner.registered = true;
        let bytes = serde_json::to_vec(&self.owner).map_err(|error| AuraError::Internal {
            message: "encode registered enrollment owner".into(),
            source: Some(std::sync::Arc::new(error)),
        })?;
        self.effects
            .secure_store(
                &location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
        Ok(RegisteredEnrollmentGenerationCapability {
            canonical_invitation,
        })
    }
}
impl AuraEffectSystem {
    #[cfg(test)]
    pub(crate) async fn recover_initial_enrollment_allocation<'a>(
        &'a self,
        ceremony: &aura_core::CeremonyId,
    ) -> Result<
        (
            EnrollmentGenerationReservation<'a>,
            crate::runtime::services::ceremony_tracker::TrackedCeremony,
        ),
        AuraError,
    > {
        let cleanup = self
            .recover_initial_enrollment_allocation_with_custody(
                ceremony,
                self.enrollment_generation_gate.lock().await,
            )
            .await?;
        let tree = self.lock_tree_decision().await;
        let device = cleanup
            .original
            .enrollment_device_id
            .ok_or_else(|| held_registration_error(HeldEnrollmentRegistrationError::Binding))?;
        let (_, participants, threshold, prestate) =
            self.authenticate_current_enrollment_roster(device).await?;
        if participants != cleanup.owner.participants
            || threshold != cleanup.owner.threshold
            || prestate != cleanup.owner.prestate
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        Ok((
            EnrollmentGenerationReservation {
                effects: self,
                owner: cleanup.owner,
                _owner: cleanup.generation,
                _tree: tree,
            },
            cleanup.original,
        ))
    }
    /// Reacquire actual custody from the original protected pre-live allocation.
    /// The ceremony is a lookup selector; caller snapshots cannot supply state.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "validate_original_allocation_setup",
        family = "runtime_helper"
    )]
    async fn recover_initial_enrollment_allocation_with_custody<'a>(
        &'a self,
        ceremony: &aura_core::CeremonyId,
        guard: tokio::sync::MutexGuard<'a, ()>,
    ) -> Result<RecoveredEnrollmentCleanupCustody<'a>, AuraError> {
        let original = crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(self, ceremony)
            .await.map_err(|source| AuraError::Internal { message: "read original initial allocation".into(), source: Some(std::sync::Arc::new(source)) })?;
        let location =
            super::enrollment_generation_profile_location(&self.authority_id, original.new_epoch);
        let bytes = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
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
            || original.threshold_k != owner.threshold.min(original.total_n)
            || original
                .enrollment_device_id
                .map(ParticipantIdentity::device)
                .map_or(true, |device| !expected.contains(&device))
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
        Ok(RecoveredEnrollmentCleanupCustody {
            effects: self,
            owner,
            original,
            generation: guard,
        })
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
            || state.threshold_k != original.threshold.min(state.total_n)
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
        if *fault == Some(epoch) {
            *fault = None;
            true
        } else {
            false
        }
    }
    pub(crate) async fn resume_owned_enrollment_registration(
        &self,
        authority: AuthorityId,
        epoch: u64,
        ceremony: &aura_core::CeremonyId,
        prestate: aura_core::Hash32,
    ) -> Result<RegisteredEnrollmentGenerationCapability, AuraError> {
        let owner_guard = self.enrollment_generation_gate.lock().await;
        let location = super::enrollment_generation_profile_location(&authority, epoch);
        let bytes = self
            .secure_retrieve(&location, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 131_072 {
            return Err(AuraError::invalid("oversized registration owner"));
        }
        let owner: StoredEnrollmentGenerationProfile =
            serde_json::from_slice(&bytes).map_err(|error| AuraError::Internal {
                message: "decode recovered registration owner".into(),
                source: Some(std::sync::Arc::new(error)),
            })?;
        if owner.version != 1
            || owner.authority != self.authority_id
            || owner.authority != authority
            || owner.pending_epoch != epoch
            || owner.ceremony != *ceremony
            || owner.prestate != prestate
        {
            return Err(AuraError::invalid(
                "recovered registration owner binding mismatch",
            ));
        }
        if owner.registered {
            let canonical_invitation = crate::handlers::invitation::enrollment_trust::verify_generation_registration_binding(self,
                owner.authority, owner.pending_epoch, &owner.ceremony, owner.prestate, &owner.invitation,
                owner.setup_digest).await.map_err(|error| AuraError::Internal { message: "verify restored registered generation".into(), source: Some(std::sync::Arc::new(error)) })?;
            return Ok(RegisteredEnrollmentGenerationCapability {
                canonical_invitation,
            });
        }

        let original=crate::handlers::invitation::enrollment_trust::recover_allocated_enrollment_registration(self,ceremony).await.map_err(|source|AuraError::Internal {message:"read original continuation allocation".into(),source:Some(std::sync::Arc::new(source))})?;
        crate::handlers::invitation::enrollment_trust::validate_original_allocation_setup(
            self,
            &original,
            owner.setup_digest,
        )
        .await
        .map_err(|source| AuraError::Internal {
            message: "verify original continuation setup".into(),
            source: Some(std::sync::Arc::new(source)),
        })?;
        let tree = self.lock_tree_decision().await;
        let device = original
            .enrollment_device_id
            .ok_or_else(|| held_registration_error(HeldEnrollmentRegistrationError::Binding))?;
        let (_, participants, threshold, current_prestate) =
            self.authenticate_current_enrollment_roster(device).await?;
        if participants != owner.participants
            || threshold != owner.threshold
            || current_prestate != owner.prestate
        {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        EnrollmentGenerationReservation {
            effects: self,
            owner,
            _owner: owner_guard,
            _tree: tree,
        }
        .complete_registration()
        .await
    }
    pub(crate) async fn require_registered_enrollment_profile(
        &self,
        authority: AuthorityId,
        epoch: u64,
        ceremony: &aura_core::CeremonyId,
        prestate: aura_core::Hash32,
    ) -> Result<(), AuraError> {
        let bytes = self
            .secure_retrieve(
                &super::enrollment_generation_profile_location(&authority, epoch),
                &[SecureStorageCapability::Read],
            )
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
    /// Recover only an allocation for which this issuer never committed an invitation.
    /// A committed invitation with interrupted registration must be reconstructed,
    /// and is never eligible for orphan deletion.
    pub(crate) async fn retire_unissued_enrollment_allocation(&self) -> Result<bool, AuraError> {
        let _generation = self.enrollment_generation_gate.lock().await;
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
            let custody = self
                .recover_initial_enrollment_allocation_with_custody(&owner.ceremony, _generation)
                .await?;
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
                        &owner.ceremony, &canonical, owner.prestate, &owner.participants, owner.threshold).await
                        .map_err(|error| AuraError::Internal { message: "recover committed enrollment registration".into(), source: Some(std::sync::Arc::new(error)) })?;
                    return Ok(false);
                }
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
            for key in [
                Self::participant_share_location(&authority, epoch, participant),
                Self::participant_wrap_key_location(&authority, epoch, participant),
            ] {
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
        self.secure_delete(&profile, delete).await?;
        Ok(true)
    }
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn fail_next_enrollment_retirement_for_test(&self, epoch: u64) {
        *self
            .enrollment_retirement_fault
            .lock()
            .expect("retirement fault lock") = Some(epoch);
    }
    pub(crate) async fn enrollment_retirement_generation_guard(
        &self,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        self.enrollment_generation_gate.lock().await
    }
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "failed_enrollment_generation",
        family = "runtime_helper"
    )]
    pub(crate) async fn retire_pinned_enrollment_generation(
        &self,
        lease: &crate::runtime::services::ceremony_tracker::EnrollmentRetirementCapability<'_>,
    ) -> Result<(), AuraError> {
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
                return Ok(());
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
            // A later owned generation may reuse the epoch after the old release.
            // The old immutable receipt permits observation only, never deletion.
            if current.ceremony != *lease.ceremony() {
                return Ok(());
            }
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
            return Ok(());
        }
        let bytes = self
            .secure_retrieve(&profile, &[SecureStorageCapability::Read])
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
                let wrap = Self::participant_wrap_key_location(&authority, epoch, participant);
                if self.secure_exists(&wrap).await? {
                    self.secure_delete(&wrap, delete).await?;
                }
                let key = SecureStorageLocation::with_sub_key(
                    "participant_shares",
                    format!("{authority}:{epoch}"),
                    participant.storage_key(),
                );
                if self.secure_exists(&key).await? {
                    self.secure_delete(&key, delete).await?;
                }
            }
            for index in 1..=metadata.participants.len() {
                let key = SecureStorageLocation::with_sub_key(
                    "signing_keys",
                    format!("{authority}:{epoch}"),
                    index.to_string(),
                );
                if self.secure_exists(&key).await? {
                    self.secure_delete(&key, delete).await?;
                }
            }
            let solo_public = Self::solo_public_key_location(&authority, epoch);
            if self.secure_exists(&solo_public).await? {
                self.secure_delete(&solo_public, delete).await?;
            }
            if self.secure_exists(&public).await? {
                self.secure_delete(&public, delete).await?;
            }
            #[cfg(all(test, not(target_arch = "wasm32")))]
            if self.take_enrollment_retirement_fault(epoch) {
                return Err(AuraError::Internal {
                    message: "injected required retirement deletion failure".into(),
                    source: Some(std::sync::Arc::new(std::io::Error::other(
                        "retirement storage unavailable",
                    ))),
                });
            }
            self.secure_delete(&config, delete).await?;
        } else if self.secure_exists(&public).await? {
            self.secure_delete(&public, delete).await?;
        }
        self.secure_store_immutable(
            &retired_location,
            &retired_bytes,
            &[
                SecureStorageCapability::Read,
                SecureStorageCapability::Write,
            ],
        )
        .await?;
        let retained = self
            .secure_retrieve(&retired_location, &[SecureStorageCapability::Read])
            .await?;
        if retained.len() > 4096 || retained != retired_bytes {
            return Err(AuraError::invalid(
                "retired generation receipt contradiction",
            ));
        }
        // Profile release is the final required write. Any prior failure remains failclosed.
        self.secure_delete(&profile, delete).await?;
        Ok(())
    }

    async fn rotate_keys_for_owned_profile(
        &self,
        authority: &AuthorityId,
        new_threshold: u16,
        new_total_participants: u16,
        participants: &[aura_core::threshold::ParticipantIdentity],
        enrollment: Option<&StoredEnrollmentGenerationProfile>,
    ) -> Result<(u64, Vec<Vec<u8>>, Vec<u8>), AuraError> {
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
            return Err(AuraError::invalid("an enrollment owns this pending generation; recovery is required before replacement"));
        }
        if let Some(owner) = enrollment {
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
            if bytes.len() > 131_072 {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
            }
            self.secure_store_immutable(
                &original_location,
                &bytes,
                &[
                    SecureStorageCapability::Read,
                    SecureStorageCapability::Write,
                ],
            )
            .await?;
            let original = self
                .secure_retrieve(&original_location, &[SecureStorageCapability::Read])
                .await?;
            if original.len() > 131_072 || original != bytes {
                return Err(held_registration_error(
                    HeldEnrollmentRegistrationError::Binding,
                ));
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
        }

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
            let envelope = self
                .encrypt_participant_key_package(authority, new_epoch, participant, key_package)
                .await?;
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
        Ok((new_epoch, key_packages, public_key_package))
    }

    /// Mint the only live roster/prestate authority under generation then tree custody.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "authenticate_current_enrollment_roster",
        family = "runtime_helper"
    )]
    pub(crate) async fn prepare_authenticated_enrollment_rotation<'a>(
        &'a self,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
    ) -> Result<AuthenticatedEnrollmentRotationPlan<'a>, AuraError> {
        let generation = self.enrollment_generation_gate.lock().await;
        let tree = self.lock_tree_decision().await;
        let (state, participants, threshold, prestate) = self
            .authenticate_current_enrollment_roster(setup.statement().device)
            .await?;
        Ok(AuthenticatedEnrollmentRotationPlan {
            effects: self,
            setup_digest: setup.digest(),
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
        ))
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "require_effects",
        family = "runtime_helper"
    )]
    pub(crate) async fn prepare_pinned_enrollment_rotation<'a>(
        &'a self,
        setup: &aura_app::ui::workflows::ceremonies::UserTransferredEnrollmentSetup,
        reserved: &crate::handlers::invitation::ReservedInvitationIssuance,
        ceremony: &aura_core::CeremonyId,
        plan: AuthenticatedEnrollmentRotationPlan<'a>,
    ) -> Result<
        (
            u64,
            Vec<Vec<u8>>,
            Vec<u8>,
            EnrollmentGenerationReservation<'a>,
        ),
        AuraError,
    > {
        plan.require_effects(self, setup)?;
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
            threshold,
            participants: participants.to_vec(),
        };
        let (epoch, packages, public) = self
            .rotate_keys_for_owned_profile(
                &self.authority_id,
                threshold,
                total,
                participants,
                Some(&profile_owner),
            )
            .await?;
        if plan.state.epoch.value().checked_add(1) != Some(epoch) {
            return Err(held_registration_error(
                HeldEnrollmentRegistrationError::Binding,
            ));
        }
        profile_owner.pending_epoch = epoch;
        Ok((
            epoch,
            packages,
            public,
            EnrollmentGenerationReservation {
                effects: self,
                owner: profile_owner,
                _owner: plan.generation,
                _tree: plan.tree,
            },
        ))
    }

    /// Read and validate the actual issuer's historical inventory. Authority
    /// and exact parent references come from the staged local baseline.
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
        let caps = [
            SecureStorageCapability::Read,
            SecureStorageCapability::Write,
        ];
        match self
            .crypto
            .secure_storage()
            .secure_retrieve(&location, &caps)
            .await
        {
            Ok(bytes) => bytes.try_into().map_err(|_| {
                AuraError::internal("participant share wrapping key has invalid length")
            }),
            Err(_) => {
                let key = self.random_bytes_32().await;
                self.crypto
                    .secure_storage()
                    .secure_store(&location, &key, &caps)
                    .await?;
                Ok(key)
            }
        }
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
            .map_err(|e| AuraError::internal(format!("Failed to encrypt key package: {e}")))?;
        let envelope = ParticipantKeyPackageEnvelope {
            version: PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION,
            authority: *authority,
            epoch,
            recipient: participant.clone(),
            nonce,
            ciphertext,
        };
        serde_json::to_vec(&envelope).map_err(|e| {
            AuraError::internal(format!("Failed to serialize key package envelope: {e}"))
        })
    }

    async fn decrypt_participant_key_package(
        &self,
        authority: &AuthorityId,
        epoch: u64,
        participant: &ParticipantIdentity,
        envelope_bytes: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let envelope: ParticipantKeyPackageEnvelope = serde_json::from_slice(envelope_bytes)
            .map_err(|e| AuraError::internal(format!("Invalid key package envelope: {e}")))?;
        if envelope.version != PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION
            || envelope.authority != *authority
            || envelope.epoch != epoch
            || envelope.recipient != *participant
            || envelope.nonce.len() != 12
        {
            return Err(AuraError::internal(
                "key package envelope metadata does not match storage location",
            ));
        }
        let wrap_key = self
            .load_or_create_participant_wrap_key(authority, epoch, participant)
            .await?;
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
            .map_err(|e| AuraError::internal(format!("Failed to decrypt key package: {e}")))
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

    pub(crate) async fn lan_discovery_signing_key(
        &self,
        authority: &AuthorityId,
    ) -> Result<[u8; 32], AuraError> {
        let current_epoch = self.get_current_epoch(authority).await;
        if self.signing_mode_for_epoch(authority, current_epoch).await != SigningMode::SingleSigner
        {
            return Err(AuraError::invalid(
                "LAN discovery announcements require a single-signer identity key",
            ));
        }

        let caps = [SecureStorageCapability::Read];
        let participant = ParticipantIdentity::guardian(*authority);
        let envelope = match self
            .crypto
            .secure_storage()
            .secure_retrieve(
                &Self::solo_signing_key_location(authority, current_epoch),
                &caps,
            )
            .await
        {
            Ok(bytes) => bytes,
            Err(solo_error) => self
                .crypto
                .secure_storage()
                .secure_retrieve(
                    &Self::participant_share_location(authority, current_epoch, &participant),
                    &caps,
                )
                .await
                .map_err(|share_error| {
                    AuraError::storage(format!(
                        "missing LAN discovery identity key for epoch {current_epoch}: \
                         signing_keys error: {solo_error}; participant_shares error: {share_error}"
                    ))
                })?,
        };

        let key_package = self
            .decrypt_participant_key_package(authority, current_epoch, &participant, &envelope)
            .await?;
        let package = SingleSignerKeyPackage::import_from_secure_storage(
            &key_package,
            SecretExportContext::secure_storage(
                "aura-agent::runtime::effects::lan_discovery_signing_key",
            ),
        )
        .map_err(|error| {
            AuraError::internal(format!(
                "invalid LAN discovery single-signer identity key: {error}"
            ))
        })?;
        let signing_key: [u8; 32] = package.signing_key().try_into().map_err(|_| {
            AuraError::internal("LAN discovery identity signing key must be 32 bytes")
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
        self.crypto.random_bytes(len)
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_bytes_32(&self) -> [u8; 32] {
        self.crypto.random_32_bytes()
    }

    #[allow(clippy::disallowed_methods)]
    async fn random_u64(&self) -> u64 {
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

    async fn frost_generate_nonces(&self, key_package: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_generate_nonces(key_package)
            .await
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
        public_key_package: &[u8],
    ) -> Result<FrostSigningPackage, CryptoError> {
        self.crypto
            .handler()
            .frost_create_signing_package(message, nonces, participants, public_key_package)
            .await
    }

    async fn frost_sign_share(
        &self,
        signing_package: &FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.crypto
            .handler()
            .frost_sign_share(signing_package, key_share, nonces)
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
        let participant = ParticipantIdentity::guardian(*authority);
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
        let current_epoch = self.get_current_epoch(&context.authority).await;
        let message =
            threshold_signing_context_transcript_bytes(&context, current_epoch).map_err(|e| {
                AuraError::internal(format!("Failed to encode signing context transcript: {e}"))
            })?;
        let caps = vec![SecureStorageCapability::Read];
        let mode = self
            .signing_mode_for_epoch(&context.authority, current_epoch)
            .await;

        match mode {
            SigningMode::SingleSigner => {
                let key_location =
                    Self::solo_signing_key_location(&context.authority, current_epoch);
                let key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(&key_location, &caps)
                    .await?;
                let participant = ParticipantIdentity::guardian(context.authority);
                let key_package = self
                    .decrypt_participant_key_package(
                        &context.authority,
                        current_epoch,
                        &participant,
                        &key_package,
                    )
                    .await?;

                let public_key_package = match self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::solo_public_key_location(&context.authority, current_epoch),
                        &caps,
                    )
                    .await
                {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        self.crypto
                            .secure_storage()
                            .secure_retrieve(
                                &Self::threshold_public_key_location(
                                    &context.authority,
                                    current_epoch,
                                ),
                                &caps,
                            )
                            .await?
                    }
                };

                let signature = self
                    .crypto
                    .handler()
                    .sign_with_key(&message, &key_package, SigningMode::SingleSigner)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Single-signer signing failed: {e}"))
                    })?;

                Ok(aura_core::threshold::ThresholdSignature::single_signer(
                    signature,
                    public_key_package,
                    current_epoch,
                ))
            }
            SigningMode::Threshold => {
                let participant = ParticipantIdentity::guardian(context.authority);
                let key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::participant_share_location(
                            &context.authority,
                            current_epoch,
                            &participant,
                        ),
                        &caps,
                    )
                    .await?;
                let key_package = self
                    .decrypt_participant_key_package(
                        &context.authority,
                        current_epoch,
                        &participant,
                        &key_package,
                    )
                    .await?;
                let public_key_package = self
                    .crypto
                    .secure_storage()
                    .secure_retrieve(
                        &Self::threshold_public_key_location(&context.authority, current_epoch),
                        &caps,
                    )
                    .await?;

                let share =
                    tree_signing::share_from_key_package_bytes(&key_package).map_err(|e| {
                        AuraError::internal(format!("Failed to decode threshold key package: {e}"))
                    })?;
                let nonces = self
                    .crypto
                    .handler()
                    .frost_generate_nonces(&key_package)
                    .await
                    .map_err(|e| AuraError::internal(format!("Nonce generation failed: {e}")))?;
                let participants = vec![share.identifier];
                let signing_package = self
                    .crypto
                    .handler()
                    .frost_create_signing_package(
                        &message,
                        std::slice::from_ref(&nonces),
                        &participants,
                        &public_key_package,
                    )
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signing package creation failed: {e}"))
                    })?;
                let partial = self
                    .crypto
                    .handler()
                    .frost_sign_share(&signing_package, &key_package, &nonces)
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signature share creation failed: {e}"))
                    })?;
                let signature = self
                    .crypto
                    .handler()
                    .frost_aggregate_signatures(&signing_package, &[partial])
                    .await
                    .map_err(|e| {
                        AuraError::internal(format!("Signature aggregation failed: {e}"))
                    })?;

                Ok(aura_core::threshold::ThresholdSignature::new(
                    signature,
                    1,
                    participants,
                    public_key_package,
                    current_epoch,
                ))
            }
        }
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
        let _owner = self.enrollment_generation_gate.lock().await;
        self.rotate_keys_for_owned_profile(authority, threshold, total, participants, None)
            .await
    }

    async fn commit_key_rotation(
        &self,
        authority: &AuthorityId,
        new_epoch: u64,
    ) -> Result<(), AuraError> {
        let _generation = self.enrollment_generation_gate.lock().await;
        if self
            .secure_exists(&super::enrollment_generation_profile_location(
                authority, new_epoch,
            ))
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
            .secure_exists(&super::enrollment_generation_profile_location(
                authority,
                failed_epoch,
            ))
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
        <RegisteredEnrollmentGenerationCapability as AmbiguousIfDeserializable<_>>::assert_absent;
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
        AgentRuntimeBridge::new(issuer.clone())
            .cancel_key_rotation_ceremony(&start.ceremony_id)
            .await
            .unwrap();
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
            .prepare_authenticated_enrollment_rotation(&setup)
            .await
            .expect("authenticated current roster");
        let prestate = plan.prestate();
        let participants = plan.participants().to_vec();
        let (epoch, _packages, _public, allocation) = effects
            .prepare_pinned_enrollment_rotation(&setup, &reserved, &ceremony, plan)
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
                1 => envelope.schema_version = 3,
                2 => envelope.encoding = FactEncoding::Json,
                3 => outer = aura_core::ContextId::new_from_entropy([98; 32]),
                _ => unreachable!(),
            }
            let committed = effects
                .commit_relational_facts(vec![aura_journal::RelationalFact::Generic {
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
        impl<T: ?Sized + Clone> AmbiguousClone<CloneImplemented> for T {}
        let _ = <AuthenticatedEnrollmentRotationPlan<'static> as AmbiguousClone<_>>::check;
        let _ = <EnrollmentGenerationReservation<'static> as AmbiguousClone<_>>::check;
        trait AmbiguousDeserialize<A> {
            fn check() {}
        }
        impl<T: ?Sized> AmbiguousDeserialize<()> for T {}
        struct DeserializeImplemented;
        impl<T: ?Sized + for<'de> serde::Deserialize<'de>>
            AmbiguousDeserialize<DeserializeImplemented> for T
        {
        }
        let _ = <AuthenticatedEnrollmentRotationPlan<'static> as AmbiguousDeserialize<_>>::check;
        let _ = <EnrollmentGenerationReservation<'static> as AmbiguousDeserialize<_>>::check;
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
            effects
                .apply_attested_op(aura_core::AttestedOp {
                    op,
                    agg_sig: proof.signature,
                    signer_count: proof.signer_count,
                })
                .await
                .expect("authenticated same-epoch removal");
            let error = match effects
                .prepare_authenticated_enrollment_rotation(&pin)
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
            assert!(!effects
                .secure_exists(&super::super::enrollment_generation_profile_location(
                    &issuer.authority_id(),
                    state.epoch.value() + 1
                ))
                .await
                .expect("required pending profile absence"));
        });
    }
}
