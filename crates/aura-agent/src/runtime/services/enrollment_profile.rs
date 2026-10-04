//! Durable profile handoff is authorized by the actual retained signed commit.
//! Prepared JSON only locates/co-ordinates that proof; it cannot select authority.
use crate::handlers::invitation::enrollment_manifest_admission::DurableConfirmedEnrollmentCapability;
use crate::runtime::{services::threshold_signing::ThresholdSigningService, AuraEffectSystem};
use aura_core::effects::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation, StorageCoreEffects,
};
use aura_core::{AuraError, AuthorityId, CeremonyId, DeviceId, InvitationId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
enum ProfileHandoffError {
    #[error("profile handoff does not match the original confirmed device/generation")]
    Binding,
    #[error("profile projection changed outside its handoff owner")]
    ProjectionConflict,
    #[error("profile handoff record exceeds its wire bound")]
    Bounds,
}
fn denied(source: ProfileHandoffError) -> AuraError {
    AuraError::PermissionDenied {
        message: "confirmed profile handoff rejected".into(),
        source: Some(Arc::new(source)),
    }
}
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PreparedProfile {
    version: u16,
    provisional: AuthorityId,
    subject: AuthorityId,
    device: DeviceId,
    invitation: InvitationId,
    ceremony: CeremonyId,
    epoch: u64,
    manifest_digest: [u8; 32],
    original_account: Option<Vec<u8>>,
    final_account: Option<Vec<u8>>,
}
fn location(device: DeviceId, phase: &str) -> SecureStorageLocation {
    SecureStorageLocation::with_sub_key("enrollment_profile_handoff_v1", device.to_string(), phase)
}
fn encode(record: &PreparedProfile) -> Result<Vec<u8>, AuraError> {
    let bytes = aura_core::util::serialization::to_vec(record).map_err(|source| {
        AuraError::Serialization {
            message: "encode confirmed profile handoff".into(),
            source: Some(Arc::new(source)),
        }
    })?;
    if bytes.len() > 300_000 {
        return Err(denied(ProfileHandoffError::Bounds));
    }
    Ok(bytes)
}
fn decode(bytes: &[u8]) -> Result<PreparedProfile, AuraError> {
    if bytes.len() > 300_000 {
        return Err(denied(ProfileHandoffError::Bounds));
    }
    aura_core::util::serialization::from_slice(bytes).map_err(|source| AuraError::Serialization {
        message: "decode confirmed profile handoff".into(),
        source: Some(Arc::new(source)),
    })
}
async fn immutable(
    effects: &AuraEffectSystem,
    key: &SecureStorageLocation,
    record: &PreparedProfile,
) -> Result<(), AuraError> {
    let bytes = encode(record)?;
    let caps = [
        SecureStorageCapability::Read,
        SecureStorageCapability::Write,
    ];
    if effects.secure_store_immutable(key, &bytes, &caps).await?
        == aura_core::effects::secure::ImmutableSecureStoreOutcome::AlreadyExists
        && effects.secure_retrieve(key, &caps).await? != bytes
    {
        return Err(denied(ProfileHandoffError::Binding));
    }
    Ok(())
}
fn validate_binding(
    effects: &AuraEffectSystem,
    record: &PreparedProfile,
    proof: &DurableConfirmedEnrollmentCapability,
) -> Result<(), AuraError> {
    let manifest = proof.confirmation().manifest();
    if record.version != 1
        || record.device != effects.device_id()
        || record.device != manifest.invitee_device
        || record.provisional != manifest.invitee_authority
        || record.subject != manifest.subject
        || record.invitation != manifest.invitation
        || record.ceremony != manifest.ceremony
        || record.epoch != manifest.pending_epoch
        || record.manifest_digest != proof.confirmation().manifest_digest()
    {
        return Err(denied(ProfileHandoffError::Binding));
    }
    if final_account(&record.original_account, record.provisional, record.subject)?
        != record.final_account
    {
        return Err(denied(ProfileHandoffError::Binding));
    }
    Ok(())
}
async fn read_account(effects: &AuraEffectSystem) -> Result<Option<Vec<u8>>, AuraError> {
    effects
        .retrieve("account.json")
        .await
        .map_err(|source| AuraError::Storage {
            message: "read original profile handoff projection".into(),
            source: Some(Arc::new(source)),
        })
}
fn identity_value(value: impl Serialize) -> Result<serde_json::Value, AuraError> {
    serde_json::to_value(value).map_err(|source| AuraError::Serialization {
        message: "encode canonical profile identity".into(),
        source: Some(Arc::new(source)),
    })
}
fn final_account(
    original: &Option<Vec<u8>>,
    provisional: AuthorityId,
    subject: AuthorityId,
) -> Result<Option<Vec<u8>>, AuraError> {
    let Some(bytes) = original else {
        return Ok(None);
    };
    if bytes.len() > 131_072 {
        return Err(denied(ProfileHandoffError::Bounds));
    }
    let mut value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
            message: "decode original profile account projection".into(),
            source: Some(Arc::new(source)),
        })?;
    let map = value
        .as_object_mut()
        .ok_or_else(|| denied(ProfileHandoffError::Binding))?;
    if map.get("authority_id") != Some(&identity_value(provisional)?) {
        return Err(denied(ProfileHandoffError::Binding));
    }
    map.insert("authority_id".into(), identity_value(subject)?);
    map.insert(
        "context_id".into(),
        identity_value(crate::core::context::default_context_id_for_authority(
            subject,
        ))?,
    );
    serde_json::to_vec(&value)
        .map(Some)
        .map_err(|source| AuraError::Serialization {
            message: "encode confirmed profile account projection".into(),
            source: Some(Arc::new(source)),
        })
}
/// A consumed proof controls prepared publication, activation, and final publication.
/// Retrying this same receipt is idempotent; this function does not select a
/// restarted runtime identity or claim that a historical receipt is globally fresh.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "durable_confirmed_enrollment",
    capability_type = DurableConfirmedEnrollmentCapability,
    family = "runtime_helper"
)]
/// No caller-provided identity or timestamp is accepted.
pub(crate) async fn complete_confirmed_handoff(
    effects: &AuraEffectSystem,
    signing: &ThresholdSigningService,
    proof: DurableConfirmedEnrollmentCapability,
) -> Result<(), AuraError> {
    let _profile = effects.enrollment_profile_handoff_guard().await;
    let manifest = proof.confirmation().manifest();
    let prepared_key = location(effects.device_id(), "prepared");
    let record = if effects.secure_exists(&prepared_key).await? {
        let record = decode(
            &effects
                .secure_retrieve(&prepared_key, &[SecureStorageCapability::Read])
                .await?,
        )?;
        validate_binding(effects, &record, &proof)?;
        record
    } else {
        let original = read_account(effects).await?;
        let record = PreparedProfile {
            version: 1,
            provisional: manifest.invitee_authority,
            subject: manifest.subject,
            device: effects.device_id(),
            invitation: manifest.invitation.clone(),
            ceremony: manifest.ceremony.clone(),
            epoch: manifest.pending_epoch,
            manifest_digest: proof.confirmation().manifest_digest(),
            final_account: final_account(&original, manifest.invitee_authority, manifest.subject)?,
            original_account: original,
        };
        validate_binding(effects, &record, &proof)?;
        immutable(effects, &prepared_key, &record).await?;
        record
    };
    // Settings mutations use the same profile gate. Recovery accepts only exact
    // original/final projection bytes; unrelated changes are never rolled back.
    let committed_key = location(record.device, "committed");
    let already_committed = effects.secure_exists(&committed_key).await?;
    if already_committed {
        let committed = decode(
            &effects
                .secure_retrieve(&committed_key, &[SecureStorageCapability::Read])
                .await?,
        )?;
        if committed != record {
            return Err(denied(ProfileHandoffError::Binding));
        }
    }
    let current = read_account(effects).await?;
    let final_matches = current == record.final_account;
    let preserve_later_metadata = if already_committed && !final_matches {
        match &current {
            Some(bytes) => {
                if bytes.len() > 131_072 {
                    return Err(denied(ProfileHandoffError::Bounds));
                }
                let value: serde_json::Value =
                    serde_json::from_slice(bytes).map_err(|source| AuraError::Serialization {
                        message: "decode current committed account projection".into(),
                        source: Some(Arc::new(source)),
                    })?;
                value.get("authority_id") == Some(&identity_value(record.subject)?)
                    && value.get("context_id")
                        == Some(&identity_value(
                            crate::core::context::default_context_id_for_authority(record.subject),
                        )?)
            }
            None => record.final_account.is_none(),
        }
    } else {
        false
    };
    if current != record.original_account && !final_matches && !preserve_later_metadata {
        return Err(denied(ProfileHandoffError::ProjectionConflict));
    }
    signing.activate_confirmed_enrollment(proof).await?;
    immutable(effects, &location(record.device, "committed"), &record).await?;
    if !final_matches && !preserve_later_metadata {
        if let Some(bytes) = &record.final_account {
            effects
                .store("account.json", bytes.clone())
                .await
                .map_err(|source| AuraError::Storage {
                    message: "publish committed profile projection".into(),
                    source: Some(Arc::new(source)),
                })?;
        }
    }
    Ok(())
}
// Staged inside runtime/services/enrollment_profile.rs. The protected committed
// record is a locator only; native receipt+archive revalidation supplies origin.
pub(crate) async fn load_original_active_profile_archive<'runtime>(
    effects: &'runtime AuraEffectSystem,
) -> Result<
    Option<
        crate::handlers::invitation::enrollment_parent_archive::ConfirmedParentInventoryCapability<
            'runtime,
        >,
    >,
    AuraError,
> {
    let committed_key = location(effects.device_id(), "committed");
    if !effects.secure_exists(&committed_key).await? {
        return Ok(None);
    }
    let record = decode(
        &effects
            .secure_retrieve(&committed_key, &[SecureStorageCapability::Read])
            .await?,
    )?;
    let confirmed =
        crate::handlers::invitation::enrollment_manifest_admission::load_confirmed_enrollment(
            effects,
            record.provisional,
            &record.invitation,
        )
        .await
        .map_err(|source| AuraError::PermissionDenied {
            message: "reverify original adopted profile history origin".into(),
            source: Some(Arc::new(source)),
        })?;
    validate_binding(effects, &record, &confirmed)?;
    if aura_guards::GuardContextProvider::authority_id(effects) != record.subject {
        return Err(denied(ProfileHandoffError::Binding));
    }
    let archive = crate::handlers::invitation::enrollment_parent_archive::load_confirmed_parent_archive_from_confirmed(
        effects, &confirmed,
    ).await?;
    Ok(Some(archive))
}
/// Read-only original committed-profile receipt. Provisional identity permits
/// its own local secret read after commit; it never grants signing approval.
pub(crate) async fn load_original_committed_profile_confirmation(
    effects: &AuraEffectSystem,
) -> Result<Option<DurableConfirmedEnrollmentCapability>, AuraError> {
    let key = location(effects.device_id(), "committed");
    if !effects.secure_exists(&key).await? {
        return Ok(None);
    }
    let record = decode(
        &effects
            .secure_retrieve(&key, &[SecureStorageCapability::Read])
            .await?,
    )?;
    let confirmed =
        crate::handlers::invitation::enrollment_manifest_admission::load_confirmed_enrollment(
            effects,
            record.provisional,
            &record.invitation,
        )
        .await
        .map_err(|source| AuraError::PermissionDenied {
            message: "reverify original committed profile secret-read receipt".into(),
            source: Some(Arc::new(source)),
        })?;
    validate_binding(effects, &record, &confirmed)?;
    let configured = aura_guards::GuardContextProvider::authority_id(effects);
    if configured != record.subject && configured != record.provisional {
        return Err(denied(ProfileHandoffError::Binding));
    }
    crate::handlers::invitation::enrollment_manifest_admission::require_confirmed_import_generation(effects, &confirmed)
        .await.map_err(|source| AuraError::PermissionDenied { message: "require original committed profile import custody".into(), source: Some(Arc::new(source)) })?;
    Ok(Some(confirmed))
}
