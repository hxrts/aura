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
