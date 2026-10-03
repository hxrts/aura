//! One runtime owner installs the exact independently admitted generation.
//! Prepared records coordinate publication; they never authorize admission or
//! activation without the separately retained signed transfer/commit proof.
use crate::handlers::invitation::enrollment_manifest_admission::{
    self, AdmittedEnrollmentManifest,
};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{SecureStorageCapability, SecureStorageEffects, SecureStorageLocation};
use aura_core::{AuraError, AuthorityId, CeremonyId, DeviceId, InvitationId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum EnrollmentImportPublicationError {
    #[error("retained import publication belongs to another authenticated generation")]
    Binding,
    #[error("legacy partial imported generation lacks its original publication owner")]
    MissingOriginalOwner,
    #[error(
        "existing pending generation payload differs from the authenticated manifest: {payload}"
    )]
    Payload { payload: &'static str },
    #[error("active generation has no authenticated committed enrollment receipt")]
    MissingCommittedReceipt,
}
fn reject(error: EnrollmentImportPublicationError) -> AuraError {
    AuraError::PermissionDenied {
        message: "enrollment import publication rejected".into(),
        source: Some(Arc::new(error)),
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct OriginalPublication {
    version: u16,
    provisional: AuthorityId,
    subject: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    ceremony: CeremonyId,
    epoch: u64,
    manifest_digest: [u8; 32],
    original_tree_digest: [u8; 32],
}
fn original_location(
    manifest: &aura_invitation::enrollment_manifest::EnrollmentTrustManifest,
) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key(
        "enrollment_import_original_v1",
        manifest.subject.to_string(),
        manifest.pending_epoch.to_string(),
    )
}
async fn publish_exact(
    effects: &AuraEffectSystem,
    location: &SecureStorageLocation,
    bytes: &[u8],
    payload: &'static str,
) -> Result<(), AuraError> {
    use aura_core::effects::secure::ImmutableSecureStoreOutcome;
    let caps = [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
    ];
    let result = effects
        .secure_store_immutable(location, bytes, &caps)
        .await?;
    if result == ImmutableSecureStoreOutcome::AlreadyExists {
        let actual = effects.secure_retrieve(location, &caps).await?;
        if actual != bytes {
            return Err(reject(EnrollmentImportPublicationError::Payload {
                payload,
            }));
        }
    }
    Ok(())
}
pub(crate) async fn install_admitted_generation(
    effects: &AuraEffectSystem,
    admitted: &AdmittedEnrollmentManifest,
) -> Result<(), AuraError> {
    let _generation = effects.enrollment_retirement_generation_guard().await;
    enrollment_manifest_admission::require_admitted_import_generation(effects, admitted)
        .await
        .map_err(|source| AuraError::Internal {
            message: "authenticated enrollment import owner failed".into(),
            source: Some(Arc::new(source)),
        })?;
    if let Some(confirmed) =
        enrollment_manifest_admission::load_optional_confirmed_enrollment(effects, admitted)
            .await
            .map_err(|source| AuraError::Internal {
                message: "authenticated enrollment import owner failed".into(),
                source: Some(Arc::new(source)),
            })?
    {
        enrollment_manifest_admission::require_confirmed_import_generation(effects, &confirmed)
            .await
            .map_err(|source| AuraError::Internal {
                message: "authenticated enrollment import owner failed".into(),
                source: Some(Arc::new(source)),
            })?;
        // No raw share/config or older baseline is ever replayed after commit.
        // The activation owner separately verifies the exact retained envelope.
        return Ok(());
    }
    let manifest = admitted.manifest();
    let epoch_location = SecureStorageLocation::new("epoch_state", manifest.subject.to_string());
    if effects.secure_exists(&epoch_location).await? {
        let bytes = effects
            .secure_retrieve(&epoch_location, &[SecureStorageCapability::Read])
            .await?;
        let actual = u64::from_le_bytes(
            bytes
                .try_into()
                .map_err(|_| AuraError::invalid("invalid retained active enrollment epoch"))?,
        );
        if actual >= manifest.pending_epoch {
            return Err(reject(
                EnrollmentImportPublicationError::MissingCommittedReceipt,
            ));
        }
    }
    let crate::handlers::invitation::InvitationType::DeviceEnrollment {
        key_package,
        threshold_config,
        public_key_package,
        ..
    } = &admitted.canonical_invitation().invitation_type
    else {
        return Err(reject(EnrollmentImportPublicationError::Binding));
    };
    let participant = aura_core::threshold::ParticipantIdentity::device(effects.device_id());
    let share = SecureStorageLocation::with_sub_key(
        "participant_shares",
        format!("{}:{}", manifest.subject, manifest.pending_epoch),
        participant.storage_key(),
    );
    let config = SecureStorageLocation::with_sub_key(
        "threshold_config",
        manifest.subject.to_string(),
        manifest.pending_epoch.to_string(),
    );
    let package = SecureStorageLocation::with_sub_key(
        "threshold_pubkey",
        manifest.subject.to_string(),
        manifest.pending_epoch.to_string(),
    );
    let key = original_location(manifest);
    let original = if effects.secure_exists(&key).await? {
        let bytes = effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?;
        if bytes.len() > 4096 {
            return Err(reject(EnrollmentImportPublicationError::Binding));
        }
        let original: OriginalPublication = aura_core::util::serialization::from_slice(&bytes)
            .map_err(|source| AuraError::Serialization {
                message: "decode original enrollment publication".into(),
                source: Some(Arc::new(source)),
            })?;
        if original.version != 1
            || original.provisional != manifest.invitee_authority
            || original.subject != manifest.subject
            || original.device != effects.device_id()
            || original.invitation != manifest.invitation
            || original.ceremony != manifest.ceremony
            || original.epoch != manifest.pending_epoch
            || original.manifest_digest != admitted.manifest_digest()
        {
            return Err(reject(EnrollmentImportPublicationError::Binding));
        }
        original
    } else {
        let current = effects
            .export_tree_ops()
            .await
            .map_err(|source| AuraError::Internal {
                message: "authenticated enrollment import owner failed".into(),
                source: Some(Arc::new(source)),
            })?;
        let baseline = admitted.baseline().ops();
        let mut prefix = current.len() >= baseline.len();
        if prefix {
            for (left, right) in current.iter().zip(baseline) {
                if aura_protocol::handlers::tree::PersistentTreeHandler::ordered_ops_digest(
                    std::slice::from_ref(left),
                )? != aura_protocol::handlers::tree::PersistentTreeHandler::ordered_ops_digest(
                    std::slice::from_ref(right),
                )? {
                    prefix = false;
                    break;
                }
            }
        }
        if !prefix
            && (effects.secure_exists(&share).await?
                || effects.secure_exists(&config).await?
                || effects.secure_exists(&package).await?)
        {
            return Err(reject(
                EnrollmentImportPublicationError::MissingOriginalOwner,
            ));
        }
        let original = OriginalPublication {
            version: 1,
            provisional: manifest.invitee_authority,
            subject: manifest.subject,
            device: effects.device_id(),
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
            epoch: manifest.pending_epoch,
            manifest_digest: admitted.manifest_digest(),
            original_tree_digest:
                aura_protocol::handlers::tree::PersistentTreeHandler::ordered_ops_digest(&current)?,
        };
        let bytes = aura_core::util::serialization::to_vec(&original).map_err(|source| {
            AuraError::Serialization {
                message: "encode original enrollment publication".into(),
                source: Some(Arc::new(source)),
            }
        })?;
        publish_exact(effects, &key, &bytes, "original publication").await?;
        original
    };
    effects
        .install_admitted_enrollment_baseline(admitted, original.original_tree_digest)
        .await?;
    publish_exact(effects, &share, key_package, "participant share").await?;
    publish_exact(
        effects,
        &config,
        threshold_config,
        "threshold configuration",
    )
    .await?;
    publish_exact(
        effects,
        &package,
        public_key_package,
        "public signing package",
    )
    .await?;
    Ok(())
}
