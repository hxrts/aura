//! Seal a payload to another device of the same authority.
//!
//! The sender uses an ephemeral X25519 key against the recipient device's leaf
//! public key (converted from Ed25519), derives an AES-256-GCM key over a
//! transcript naming the authority, recipient and purpose, and binds the same
//! transcript as associated data. Only the recipient device's local
//! key-agreement secret opens it, and any change to the transcript fails.

use aura_core::effects::CryptoEffects;
use aura_core::{AuraError, AuthorityId, DeviceId};
use curve25519_dalek::{montgomery::MontgomeryPoint, scalar::Scalar};
use serde::{Deserialize, Serialize};

const DEVICE_SEALED_PROTOCOL_VERSION: u8 = 1;
const DEVICE_SEALED_KDF_DOMAIN: &[u8] = b"aura.sync.device-sealed.v1";

/// A payload only `recipient_device_id` of `authority` can open. Deserialized
/// identity and public keys are untrusted key material until `open_for_device`
/// checks the recipient against the caller's local device key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSealedPayload {
    /// Untrusted key material in this wire value is checked against the
    /// caller's local device identity before decryption.
    pub protocol_version: u8,
    pub recipient_device_id: DeviceId,
    pub recipient_public_key: Vec<u8>,
    pub ephemeral_public_key: Vec<u8>,
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

#[derive(Serialize)]
struct DeviceSealedTranscript<'a> {
    protocol_version: u8,
    purpose: &'a str,
    authority: AuthorityId,
    recipient_device_id: DeviceId,
    recipient_public_key: &'a [u8],
    ephemeral_public_key: &'a [u8],
}

fn transcript_bytes(
    purpose: &str,
    authority: AuthorityId,
    recipient_device_id: DeviceId,
    recipient_public_key: &[u8],
    ephemeral_public_key: &[u8],
) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(&DeviceSealedTranscript {
        protocol_version: DEVICE_SEALED_PROTOCOL_VERSION,
        purpose,
        authority,
        recipient_device_id,
        recipient_public_key,
        ephemeral_public_key,
    })
    .map_err(|error| AuraError::internal(format!("encode device seal transcript: {error}")))
}

fn x25519_shared_secret(private_key: &[u8; 32], public_key: &[u8; 32]) -> [u8; 32] {
    (Scalar::from_bytes_mod_order(*private_key) * MontgomeryPoint(*public_key)).to_bytes()
}

async fn derive_key<E>(
    effects: &E,
    shared_secret: &[u8; 32],
    transcript: &[u8],
) -> Result<[u8; 32], AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let key = effects
        .kdf_derive(shared_secret, DEVICE_SEALED_KDF_DOMAIN, transcript, 32)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal key derivation: {error}")))?;
    key.as_slice()
        .try_into()
        .map_err(|_| AuraError::crypto("device seal key must be 32 bytes"))
}

/// Seal `plaintext` for `recipient_device_id`, whose tree leaf public key
/// (Ed25519) is `recipient_public_key`. `purpose` separates uses.
pub async fn seal_for_device<E>(
    effects: &E,
    purpose: &str,
    authority: AuthorityId,
    recipient_device_id: DeviceId,
    recipient_public_key: &[u8],
    plaintext: &[u8],
) -> Result<DeviceSealedPayload, AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    let recipient_x25519 = effects
        .convert_ed25519_to_x25519_public(recipient_public_key)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal recipient key: {error}")))?;
    let (ephemeral_private, ephemeral_ed25519_public) = effects
        .ed25519_generate_keypair()
        .await
        .map_err(|error| AuraError::crypto(format!("device seal ephemeral key: {error}")))?;
    let ephemeral_x25519_private = effects
        .convert_ed25519_to_x25519_private(&ephemeral_private)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal ephemeral key: {error}")))?;
    let ephemeral_x25519_public = effects
        .convert_ed25519_to_x25519_public(&ephemeral_ed25519_public)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal ephemeral key: {error}")))?;
    let transcript = transcript_bytes(
        purpose,
        authority,
        recipient_device_id,
        recipient_public_key,
        &ephemeral_x25519_public,
    )?;
    let key = derive_key(
        effects,
        &x25519_shared_secret(&ephemeral_x25519_private, &recipient_x25519),
        &transcript,
    )
    .await?;
    let nonce: [u8; 12] = effects
        .random_bytes(12)
        .await
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::crypto("device seal nonce must be 12 bytes"))?;
    let ciphertext = effects
        .aes_gcm_encrypt_with_aad(plaintext, &key, &nonce, &transcript)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal encryption: {error}")))?;
    Ok(DeviceSealedPayload {
        protocol_version: DEVICE_SEALED_PROTOCOL_VERSION,
        recipient_device_id,
        recipient_public_key: recipient_public_key.to_vec(),
        ephemeral_public_key: ephemeral_x25519_public.to_vec(),
        nonce,
        ciphertext,
    })
}

/// Open a payload sealed to this device. `recipient_public_key` is this
/// device's tree leaf public key and `recipient_private_key` its local
/// key-agreement secret (X25519).
pub async fn open_for_device<E>(
    effects: &E,
    purpose: &str,
    authority: AuthorityId,
    recipient_device_id: DeviceId,
    recipient_public_key: &[u8],
    recipient_private_key: &[u8; 32],
    sealed: &DeviceSealedPayload,
) -> Result<Vec<u8>, AuraError>
where
    E: CryptoEffects + Send + Sync + ?Sized,
{
    if sealed.protocol_version != DEVICE_SEALED_PROTOCOL_VERSION {
        return Err(AuraError::invalid(format!(
            "unsupported device seal version {}",
            sealed.protocol_version
        )));
    }
    if sealed.recipient_device_id != recipient_device_id
        || sealed.recipient_public_key != recipient_public_key
    {
        return Err(AuraError::invalid(
            "device-sealed payload is addressed to another device".to_string(),
        ));
    }
    let ephemeral_public: [u8; 32] = sealed
        .ephemeral_public_key
        .as_slice()
        .try_into()
        .map_err(|_| AuraError::invalid("device seal ephemeral key must be 32 bytes"))?;
    let transcript = transcript_bytes(
        purpose,
        authority,
        recipient_device_id,
        recipient_public_key,
        &sealed.ephemeral_public_key,
    )?;
    let key = derive_key(
        effects,
        &x25519_shared_secret(recipient_private_key, &ephemeral_public),
        &transcript,
    )
    .await?;
    effects
        .aes_gcm_decrypt_with_aad(&sealed.ciphertext, &key, &sealed.nonce, &transcript)
        .await
        .map_err(|error| AuraError::crypto(format!("device seal decryption: {error}")))
}
