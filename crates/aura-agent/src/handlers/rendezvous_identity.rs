use aura_core::crypto::single_signer::SingleSignerKeyPackage;
use aura_core::effects::secure::{
    SecureStorageCapability, SecureStorageEffects, SecureStorageLocation,
};
use aura_core::secrets::SecretExportContext;
use aura_core::threshold::ParticipantIdentity;
use aura_core::types::identifiers::AuthorityId;
use chacha20poly1305::{
    aead::{Aead, Payload},
    ChaCha20Poly1305, KeyInit, Nonce,
};
use serde::Deserialize;
use std::collections::BTreeSet;

const PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION: u8 = 1;
const PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN: &str = "aura:participant-key-package-envelope:v1";

#[derive(Debug)]
pub(crate) struct RequiredIdentityShareDecryptionError(chacha20poly1305::aead::Error);
impl std::fmt::Display for RequiredIdentityShareDecryptionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}
// aead::Error does not implement std::error::Error without its optional std
// feature; retain the concrete cause instead of converting it to a string.
impl std::error::Error for RequiredIdentityShareDecryptionError {}

#[derive(Debug)]
enum RequiredIdentityKeyStage {
    EnvelopeDecode,
    WrapKeyRead,
    ShareDecrypt,
    PackageDecode,
}

#[derive(Debug, thiserror::Error)]
#[error("required issuer identity {stage:?} for authority {authority}, epoch {epoch}, location {location:?}: {source}")]
struct RequiredIdentityKeyError {
    authority: AuthorityId,
    epoch: u64,
    location: SecureStorageLocation,
    stage: RequiredIdentityKeyStage,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

fn required_identity_key_error(
    authority: AuthorityId,
    epoch: u64,
    location: &SecureStorageLocation,
    stage: RequiredIdentityKeyStage,
    source: impl std::error::Error + Send + Sync + 'static,
) -> aura_invitation::enrollment_manifest::EnrollmentManifestError {
    aura_invitation::enrollment_manifest::EnrollmentManifestError::Runtime(Box::new(
        RequiredIdentityKeyError {
            authority,
            epoch,
            location: location.clone(),
            stage,
            source: Box::new(source),
        },
    ))
}

/// Required issuer key lookup.
/// Storage/codec failures do not become absence.
/// This signs only the issuer's new manifest; it grants no response capability.
pub(crate) async fn require_identity_keys<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
) -> Result<([u8; 32], [u8; 32]), aura_invitation::enrollment_manifest::EnrollmentManifestError> {
    use aura_invitation::enrollment_manifest::EnrollmentManifestError as Error;
    let runtime = |e| Error::Runtime(Box::new(e));
    let epoch_location = SecureStorageLocation::new("epoch_state", authority.to_string());
    let caps = [SecureStorageCapability::Read];
    let epoch = if effects
        .secure_exists(&epoch_location)
        .await
        .map_err(runtime)?
    {
        let bytes = effects
            .secure_retrieve(&epoch_location, &caps)
            .await
            .map_err(runtime)?;
        u64::from_le_bytes(bytes.as_slice().try_into().map_err(|_| Error::Shape)?)
    } else {
        0
    };
    let mut epochs = BTreeSet::new();
    epochs.insert(epoch);
    epochs.insert(1);
    epochs.insert(0);
    let participant = ParticipantIdentity::guardian(*authority);
    for epoch in epochs.into_iter().rev() {
        let locations = [
            SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{}:{}", authority, epoch),
                "1",
            ),
            SecureStorageLocation::with_sub_key(
                "participant_shares",
                format!("{}:{}", authority, epoch),
                participant.storage_key(),
            ),
        ];
        for location in locations {
            if !effects.secure_exists(&location).await.map_err(runtime)? {
                continue;
            }
            let bytes = effects
                .secure_retrieve(&location, &caps)
                .await
                .map_err(runtime)?;
            // Both bootstrap writers retain the same encrypted JSON envelope at
            // signing_keys and participant_shares. Neither namespace contains raw
            // DAG-CBOR secrets on this required path.
            if bytes.len() > 131_072 {
                return Err(Error::Shape);
            }
            let plain = {
                let envelope: ParticipantKeyPackageEnvelope = serde_json::from_slice(&bytes)
                    .map_err(|e| {
                        required_identity_key_error(
                            *authority,
                            epoch,
                            &location,
                            RequiredIdentityKeyStage::EnvelopeDecode,
                            e,
                        )
                    })?;
                if envelope.version != PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION
                    || envelope.authority != *authority
                    || envelope.epoch != epoch
                    || envelope.recipient != participant
                    || envelope.nonce.len() != 12
                    || envelope.ciphertext.len() > 65_536
                    || envelope.ciphertext.is_empty()
                {
                    return Err(Error::Shape);
                }
                let wrap_location = SecureStorageLocation::with_sub_key(
                    "participant_share_wrap_keys",
                    format!("{}:{}", authority, epoch),
                    participant.storage_key(),
                );
                let wrap: [u8; 32] = effects
                    .secure_retrieve(&wrap_location, &caps)
                    .await
                    .map_err(|e| {
                        required_identity_key_error(
                            *authority,
                            epoch,
                            &wrap_location,
                            RequiredIdentityKeyStage::WrapKeyRead,
                            e,
                        )
                    })?
                    .try_into()
                    .map_err(|_| Error::Shape)?;
                let cipher = ChaCha20Poly1305::new((&wrap).into());
                let aad = format!(
                    "{}:{}:{}:{}",
                    PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN,
                    authority,
                    epoch,
                    participant.storage_key()
                );
                cipher
                    .decrypt(
                        Nonce::from_slice(&envelope.nonce),
                        Payload {
                            msg: &envelope.ciphertext,
                            aad: aad.as_bytes(),
                        },
                    )
                    .map_err(|e| {
                        required_identity_key_error(
                            *authority,
                            epoch,
                            &location,
                            RequiredIdentityKeyStage::ShareDecrypt,
                            RequiredIdentityShareDecryptionError(e),
                        )
                    })?
            };
            let package = SingleSignerKeyPackage::import_from_secure_storage(
                &plain,
                SecretExportContext::secure_storage("aura-agent::handlers::rendezvous_identity"),
            )
            .map_err(|e| {
                required_identity_key_error(
                    *authority,
                    epoch,
                    &location,
                    RequiredIdentityKeyStage::PackageDecode,
                    e,
                )
            })?;
            let private: [u8; 32] = package.signing_key().try_into().map_err(|_| Error::Shape)?;
            let public: [u8; 32] = package
                .verifying_key()
                .try_into()
                .map_err(|_| Error::Shape)?;
            if private == [0; 32] || public == [0; 32] {
                return Err(Error::Shape);
            }
            return Ok((private, public));
        }
    }
    Err(Error::Unavailable)
}

#[derive(Debug, Deserialize)]
struct ParticipantKeyPackageEnvelope {
    version: u8,
    authority: AuthorityId,
    epoch: u64,
    recipient: ParticipantIdentity,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

pub(crate) async fn retrieve_identity_keys<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
) -> Option<([u8; 32], [u8; 32])> {
    let current_epoch = current_epoch(effects, authority).await;
    let mut epochs = BTreeSet::new();
    epochs.insert(current_epoch);
    epochs.insert(1);
    epochs.insert(0);

    for epoch in epochs.into_iter().rev() {
        if let Some(keys) = retrieve_identity_keys_for_epoch(effects, authority, epoch).await {
            return Some(keys);
        }
    }

    None
}

/// Find the retained local signing key that matches the key in an issued
/// invitation. A current-epoch lookup alone can sign a confirmation with a
/// rotated key that the invitee correctly refuses.
pub(crate) async fn retrieve_identity_keys_matching_public<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
    expected_public: &[u8],
) -> Option<([u8; 32], [u8; 32])> {
    if expected_public.len() != 32 {
        return None;
    }
    for epoch in (0..=current_epoch(effects, authority).await.max(1)).rev() {
        if let Some(keys) = retrieve_identity_keys_for_epoch(effects, authority, epoch).await {
            if keys.1.as_slice() == expected_public {
                return Some(keys);
            }
        }
    }
    None
}

async fn current_epoch<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
) -> u64 {
    let location = SecureStorageLocation::new("epoch_state", format!("{}", authority));
    let caps = [SecureStorageCapability::Read];
    effects
        .secure_retrieve(&location, &caps)
        .await
        .ok()
        .and_then(|data| data.get(..8).and_then(|bytes| bytes.try_into().ok()))
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

async fn retrieve_identity_keys_for_epoch<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
    epoch: u64,
) -> Option<([u8; 32], [u8; 32])> {
    let participant = ParticipantIdentity::guardian(*authority);
    let locations = [
        SecureStorageLocation::with_sub_key(
            "signing_keys",
            format!("{}:{}", authority, epoch),
            "1",
        ),
        SecureStorageLocation::with_sub_key(
            "participant_shares",
            format!("{}:{}", authority, epoch),
            participant.storage_key(),
        ),
    ];
    let caps = [SecureStorageCapability::Read];

    for location in locations {
        let Ok(stored) = effects.secure_retrieve(&location, &caps).await else {
            continue;
        };
        if let Some(keys) = decode_single_signer_package(&stored) {
            return Some(keys);
        }
        if let Some(key_package) =
            decrypt_participant_key_package(effects, authority, epoch, &participant, &stored).await
        {
            if let Some(keys) = decode_single_signer_package(&key_package) {
                return Some(keys);
            }
        }
    }

    None
}

fn decode_single_signer_package(bytes: &[u8]) -> Option<([u8; 32], [u8; 32])> {
    let pkg = SingleSignerKeyPackage::import_from_secure_storage(
        bytes,
        SecretExportContext::secure_storage("aura-agent::handlers::rendezvous_identity"),
    )
    .ok()?;
    let signing_key: [u8; 32] = pkg.signing_key().try_into().ok()?;
    let verifying_key: [u8; 32] = pkg.verifying_key().try_into().ok()?;
    if signing_key == [0u8; 32] || verifying_key == [0u8; 32] {
        return None;
    }
    Some((signing_key, verifying_key))
}

async fn decrypt_participant_key_package<E: SecureStorageEffects + ?Sized>(
    effects: &E,
    authority: &AuthorityId,
    epoch: u64,
    participant: &ParticipantIdentity,
    envelope_bytes: &[u8],
) -> Option<Vec<u8>> {
    let envelope: ParticipantKeyPackageEnvelope = serde_json::from_slice(envelope_bytes).ok()?;
    if envelope.version != PARTICIPANT_KEY_PACKAGE_ENVELOPE_VERSION
        || envelope.authority != *authority
        || envelope.epoch != epoch
        || envelope.recipient != *participant
        || envelope.nonce.len() != 12
    {
        return None;
    }

    let wrap_location = SecureStorageLocation::with_sub_key(
        "participant_share_wrap_keys",
        format!("{}:{}", authority, epoch),
        participant.storage_key(),
    );
    let caps = [SecureStorageCapability::Read];
    let wrap_key: [u8; 32] = effects
        .secure_retrieve(&wrap_location, &caps)
        .await
        .ok()?
        .try_into()
        .ok()?;
    let cipher = ChaCha20Poly1305::new((&wrap_key).into());
    let aad = format!(
        "{}:{}:{}:{}",
        PARTICIPANT_KEY_PACKAGE_AAD_DOMAIN,
        authority,
        epoch,
        participant.storage_key()
    );
    cipher
        .decrypt(
            Nonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.ciphertext,
                aad: aad.as_bytes(),
            },
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::AgentConfig;
    use aura_core::effects::CryptoCoreEffects;

    #[tokio::test]
    async fn issued_invitation_key_survives_identity_rotation() {
        let effects = crate::testing::simulation_effect_system_arc(&AgentConfig::default());
        let authority = AuthorityId::new_from_entropy([91; 32]);
        let mut public_keys = Vec::new();
        for epoch in [1_u64, 2] {
            let (private, public) = effects.ed25519_generate_keypair().await.unwrap();
            let package = SingleSignerKeyPackage::new(private, public.clone());
            let bytes = package
                .export_for_secure_storage(SecretExportContext::secure_storage(
                    "aura-agent::handlers::rendezvous_identity::tests",
                ))
                .unwrap();
            let location = SecureStorageLocation::with_sub_key(
                "signing_keys",
                format!("{}:{}", authority, epoch),
                "1",
            );
            effects
                .secure_store(&location, &bytes, &[SecureStorageCapability::Write])
                .await
                .unwrap();
            public_keys.push(public);
        }
        effects
            .secure_store(
                &SecureStorageLocation::new("epoch_state", authority.to_string()),
                &2_u64.to_le_bytes(),
                &[SecureStorageCapability::Write],
            )
            .await
            .unwrap();

        let (_, current) = retrieve_identity_keys(&*effects, &authority).await.unwrap();
        assert_eq!(current.as_slice(), public_keys[1]);
        let (_, issued) =
            retrieve_identity_keys_matching_public(&*effects, &authority, &public_keys[0])
                .await
                .unwrap();
        assert_eq!(issued.as_slice(), public_keys[0]);
        assert!(
            retrieve_identity_keys_matching_public(&*effects, &authority, &[0; 32])
                .await
                .is_none()
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod required_identity_envelope_tests {
    use super::*;
    use crate::runtime_bridge::AgentRuntimeBridge;
    use aura_app::runtime_bridge::RuntimeBridge;

    #[tokio::test]
    async fn actual_bootstrap_required_identity_decrypts_both_layouts_and_preserves_codec_source() {
        let authority = AuthorityId::new_from_entropy([213; 32]);
        let config = crate::AgentConfig {
            device_id: aura_core::DeviceId::new_from_entropy([214; 32]),
            storage: crate::core::config::StorageConfig {
                base_path: tempfile::Builder::new()
                    .prefix("aura-required-identity-envelope-")
                    .tempdir()
                    .unwrap()
                    .keep(),
                ..Default::default()
            },
            ..Default::default()
        };
        let context = aura_core::context::EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy([215; 32]),
            aura_core::effects::ExecutionMode::Testing,
        );
        let agent = std::sync::Arc::new(
            crate::AgentBuilder::new()
                .with_authority(authority)
                .with_config(config)
                .build_testing_async(&context)
                .await
                .unwrap(),
        );
        AgentRuntimeBridge::new(agent.clone())
            .bootstrap_signing_keys()
            .await
            .unwrap();
        let effects = agent.runtime().effects();
        let expected = require_identity_keys(effects.as_ref(), &authority)
            .await
            .expect("actual encrypted signing_keys envelope is decoded and authenticated");
        let location =
            SecureStorageLocation::with_sub_key("signing_keys", format!("{authority}:0"), "1");
        let caps = [SecureStorageCapability::Read];
        let original = effects.secure_retrieve(&location, &caps).await.unwrap();
        effects
            .secure_store(&location, b"{", &[SecureStorageCapability::Write])
            .await
            .unwrap();
        let error = require_identity_keys(effects.as_ref(), &authority)
            .await
            .expect_err("corrupt primary envelope cannot fall back to participant share");
        let typed = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<RequiredIdentityKeyError>()
            .unwrap();
        assert_eq!(typed.authority, authority);
        assert_eq!(typed.epoch, 0);
        assert_eq!(typed.location, location);
        assert!(matches!(
            typed.stage,
            RequiredIdentityKeyStage::EnvelopeDecode
        ));
        assert!(typed.source.downcast_ref::<serde_json::Error>().is_some());
        effects
            .secure_store(&location, &original, &[SecureStorageCapability::Write])
            .await
            .unwrap();
        effects
            .secure_delete(&location, &[SecureStorageCapability::Delete])
            .await
            .unwrap();
        assert_eq!(
            require_identity_keys(effects.as_ref(), &authority)
                .await
                .unwrap(),
            expected,
            "proven missing solo location selects separately authenticated participant envelope"
        );
        effects
            .secure_store(&location, &original, &[SecureStorageCapability::Write])
            .await
            .unwrap();
    }
}
