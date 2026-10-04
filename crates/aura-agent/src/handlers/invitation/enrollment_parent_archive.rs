//! Public historical verifiers retained only from a locally durable confirmed receipt.
use super::enrollment_manifest_admission::{
    load_confirmed_enrollment, DurableConfirmedEnrollmentCapability,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::{AuraError, AuthorityId, DeviceId};
use aura_invitation::enrollment_manifest::EnrollmentParentVerifier;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum ParentArchiveError {
    #[error("confirmed parent archive differs from original locally pinned receipt")]
    Binding,
    #[error("confirmed parent archive exceeds its bounded schema")]
    Shape,
}
fn reject(error: ParentArchiveError) -> AuraError {
    AuraError::PermissionDenied {
        message: "confirmed parent archive rejected".into(),
        source: Some(Arc::new(error)),
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Archive {
    version: u16,
    physical_device: DeviceId,
    provisional: AuthorityId,
    subject: AuthorityId,
    invitation: aura_core::InvitationId,
    manifest_digest: [u8; 32],
    confirmed_history_digest: [u8; 32],
    // Canonical encoded signed tuples; none are synthesized from an epoch root.
    tuples: Vec<u8>,
}
/// Exact public generation from the canonical invitation retained by the
/// original reverified committed receipt. No current private key is consulted.
fn pending_template(
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<EnrollmentParentVerifier, AuraError> {
    let proof = confirmed.confirmation();
    let manifest = proof.manifest();
    let super::InvitationType::DeviceEnrollment {
        public_key_package,
        threshold_config,
        ..
    } = &proof.canonical_invitation().invitation_type
    else {
        return Err(reject(ParentArchiveError::Binding));
    };
    if aura_core::hash::hash(public_key_package) != manifest.pending_public_key_package_digest
        || aura_core::hash::hash(threshold_config)
            != *manifest.pending_threshold_config_digest.as_bytes()
    {
        return Err(reject(ParentArchiveError::Binding));
    }
    let policy =
        aura_invitation::enrollment_manifest::EnrollmentTrustManifest::decode_pending_policy(
            threshold_config,
        )
        .map_err(|source| AuraError::PermissionDenied {
            message: "verify confirmed archive signed pending policy".into(),
            source: Some(Arc::new(source)),
        })?;
    let state = proof.committed_transition().state();
    if state.epoch.value() != manifest.pending_epoch {
        return Err(reject(ParentArchiveError::Binding));
    }
    Ok(EnrollmentParentVerifier {
        epoch: manifest.pending_epoch,
        commitment: state.root_commitment,
        signing_node: aura_core::tree::NodeIndex(0),
        mode: policy.signing_mode(),
        threshold: policy.threshold(),
        participants: policy.participants().to_vec(),
        public_key_package: public_key_package.clone(),
        agreement: policy.agreement(),
    })
}
fn source(
    effects: &AuraEffectSystem,
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<Archive, AuraError> {
    let proof = confirmed.confirmation();
    let manifest = proof.manifest();
    if manifest.invitee_device != effects.device_id() {
        return Err(reject(ParentArchiveError::Binding));
    }
    let mut tuples = manifest.parents.clone();
    tuples.extend_from_slice(manifest.final_inventory().map_err(|error| {
        AuraError::PermissionDenied {
            message: "confirmed final inventory missing".into(),
            source: Some(Arc::new(error)),
        }
    })?);
    tuples.push(pending_template(confirmed)?);
    let encoded = aura_core::util::serialization::to_vec(&tuples)?;
    if encoded.len() > 2 * 1024 * 1024 {
        return Err(reject(ParentArchiveError::Shape));
    }
    let history =
        aura_core::util::serialization::to_vec(&proof.committed_transition().ops().to_vec())?;
    Ok(Archive {
        version: 2,
        physical_device: effects.device_id(),
        provisional: manifest.invitee_authority,
        subject: manifest.subject,
        invitation: manifest.invitation.clone(),
        manifest_digest: proof.manifest_digest(),
        confirmed_history_digest: aura_core::hash::hash(&history),
        tuples: encoded,
    })
}
fn location(record: &Archive) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "confirmed_enrollment_parent_inventory_v2",
        record.physical_device.to_string(),
        format!("{}:{}", record.subject, record.invitation),
    )
}
/// Borrow retains the actual reverified receipt, not only its decoded archive.
pub(crate) struct ConfirmedParentInventoryCapability<'runtime> {
    effects: &'runtime AuraEffectSystem,
    _confirmed: DurableConfirmedEnrollmentCapability,
    tuples: Vec<EnrollmentParentVerifier>,
}
impl ConfirmedParentInventoryCapability<'_> {
    /// Consumes archive evidence only with the actual physical runtime owner and
    /// original cryptographically checked history prefix. Later operations are
    /// verified against admitted node policy, never an ambient package cache.
    pub(crate) fn verify_imported_history(
        &self,
        effects: &AuraEffectSystem,
        subject: aura_core::AuthorityId,
        history: &[aura_core::AttestedOp],
    ) -> Result<super::enrollment_vm_admission::VerifiedEnrollmentTreeExtension, AuraError> {
        if !std::ptr::eq(effects, self.effects)
            || subject != self._confirmed.confirmation().manifest().subject
            || effects.device_id() != self._confirmed.confirmation().manifest().invitee_device
        {
            return Err(reject(ParentArchiveError::Binding));
        }
        self._confirmed
            .confirmation()
            .verify_local_extension(history)
            .map_err(|source| match source {
                crate::core::AgentError::Aura(error) => error,
                source => AuraError::Crypto {
                    message: "verify archived imported history".into(),
                    source: Some(Arc::new(source)),
                },
            })
    }
    pub(crate) fn require_verified_inventory(
        &self,
        verified: &super::enrollment_vm_admission::VerifiedEnrollmentTreeExtension,
    ) -> Result<(), AuraError> {
        if verified.manifest_digest() != self._confirmed.confirmation().manifest_digest() {
            return Err(reject(ParentArchiveError::Binding));
        }
        for captured in verified.parent_inventory() {
            if !self.tuples.iter().any(|origin| {
                origin.epoch == captured.epoch
                    && origin.signing_node == captured.signing_node
                    && origin.mode == captured.mode
                    && origin.threshold == captured.threshold
                    && origin.participants == captured.participants
                    && origin.public_key_package == captured.public_key_package
                    && origin.agreement == captured.agreement
            }) {
                return Err(reject(ParentArchiveError::Binding));
            }
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn inventory(&self) -> &[EnrollmentParentVerifier] {
        &self.tuples
    }
}
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "DurableConfirmedEnrollmentCapability",
    family = "runtime_helper"
)]
pub(crate) async fn retain_confirmed_parent_archive(
    effects: &AuraEffectSystem,
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<(), AuraError> {
    let record = source(effects, confirmed)?;
    // The supplied sealed capability cannot transplant a receipt into another
    // profile with the same caller-controlled identity values. Revalidate its
    // original durable receipt on this actual selected provider before publish.
    let local = load_confirmed_enrollment(effects, record.provisional, &record.invitation)
        .await
        .map_err(|source| AuraError::Internal {
            message: "require local original archive receipt".into(),
            source: Some(Arc::new(source)),
        })?;
    if source(effects, &local)? != record {
        return Err(reject(ParentArchiveError::Binding));
    }
    let key = location(&record);
    let bytes = aura_core::util::serialization::to_vec(&record)?;
    effects
        .secure_store_immutable(&key, &bytes, &[SecureStorageCapability::Write])
        .await?;
    // AlreadyExists never establishes success: compare original immutable bytes.
    let original = effects
        .secure_retrieve(&key, &[SecureStorageCapability::Read])
        .await?;
    if original != bytes {
        return Err(reject(ParentArchiveError::Binding));
    }
    Ok(())
}
/// IDs select the original receipt. Independent pin and actual committed proof
/// are revalidated before the archive's public bytes become usable evidence.
pub(crate) async fn load_confirmed_parent_archive<'runtime>(
    effects: &'runtime AuraEffectSystem,
    provisional: AuthorityId,
    invitation: &aura_core::InvitationId,
) -> Result<ConfirmedParentInventoryCapability<'runtime>, AuraError> {
    let confirmed = load_confirmed_enrollment(effects, provisional, invitation)
        .await
        .map_err(|error| AuraError::Internal {
            message: "reverify original parent archive receipt".into(),
            source: Some(Arc::new(error)),
        })?;
    let expected = source(effects, &confirmed)?;
    let bytes = effects
        .secure_retrieve(&location(&expected), &[SecureStorageCapability::Read])
        .await?;
    if bytes.len() > 2 * 1024 * 1024 + 4096 {
        return Err(reject(ParentArchiveError::Shape));
    }
    let actual: Archive = aura_core::util::serialization::from_slice(&bytes)?;
    if actual != expected {
        return Err(reject(ParentArchiveError::Binding));
    }
    let tuples = aura_core::util::serialization::from_slice(&actual.tuples)?;
    Ok(ConfirmedParentInventoryCapability {
        effects,
        _confirmed: confirmed,
        tuples,
    })
}

impl ConfirmedParentInventoryCapability<'_> {
    pub(crate) fn manifest(
        &self,
    ) -> &aura_invitation::enrollment_manifest::EnrollmentTrustManifest {
        self._confirmed.confirmation().manifest()
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "DurableConfirmedEnrollmentCapability",
    family = "runtime_helper"
)]
pub(crate) async fn load_confirmed_parent_archive_from_confirmed<'runtime>(
    effects: &'runtime AuraEffectSystem,
    confirmed: &DurableConfirmedEnrollmentCapability,
) -> Result<ConfirmedParentInventoryCapability<'runtime>, AuraError> {
    let expected = source(effects, confirmed)?;
    let loaded =
        load_confirmed_parent_archive(effects, expected.provisional, &expected.invitation).await?;
    if source(effects, &loaded._confirmed)? != expected {
        return Err(reject(ParentArchiveError::Binding));
    }
    Ok(loaded)
}

#[cfg(test)]
mod guards {
    use super::*;
    trait AmbiguousIfClone<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfClone<()> for T {}
    impl<T: Clone> AmbiguousIfClone<u8> for T {}
    trait AmbiguousIfDeserialize<A> {
        fn marker() {}
    }
    impl<T: ?Sized> AmbiguousIfDeserialize<()> for T {}
    impl<T: serde::de::DeserializeOwned> AmbiguousIfDeserialize<u8> for T {}
    #[test]
    fn confirmed_archive_evidence_is_move_owned_and_not_deserializable() {
        let _ = <ConfirmedParentInventoryCapability<'static> as AmbiguousIfClone<_>>::marker;
        let _ = <ConfirmedParentInventoryCapability<'static> as AmbiguousIfDeserialize<_>>::marker;
    }
}
