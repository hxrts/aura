//! Proof-of-storage challenge/response protocol
//!
//! This module implements a challenge-response system for proving that storage
//! providers actually possess the data they claim to store. It uses cryptographic
//! challenges combined with device signatures to verify data integrity and availability.

use crate::{Result, StorageError, StoreErrorBuilder};
use aura_journal::{Cid, SessionEpoch};
use aura_types::{DeviceId, DeviceIdExt};
use blake3;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Replica tag for tracking storage replicas
///
/// A unique identifier for each storage replica across devices in the network.
/// Used to distinguish between different copies of the same data chunk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplicaTag(pub Uuid);

impl ReplicaTag {
    /// Create a new replica tag using injected effects (for production/testing)
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        ReplicaTag(effects.gen_uuid())
    }
}

/// Proof-of-storage challenge
///
/// A cryptographic challenge sent to storage providers to verify they possess
/// specific data chunks. Contains a random nonce to prevent replay attacks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    /// Content identifier of the chunk being challenged
    pub chunk_cid: Cid,
    /// Random nonce to prevent replay attacks
    pub nonce: [u8; 32],
    /// Device ID of the challenger for accountability
    pub challenger_id: DeviceId,
}

impl Challenge {
    /// Create a new challenge using injected effects (for production/testing)
    pub fn new_with_effects(
        chunk_cid: Cid,
        challenger_id: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Self {
        let nonce = effects.random_bytes::<32>();

        Challenge {
            chunk_cid,
            nonce,
            challenger_id,
        }
    }
}

/// Proof-of-storage response
///
/// Response to a proof-of-storage challenge, containing cryptographic proof
/// that the responder possesses the challenged data chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofResponse {
    /// Replica tag identifying this specific storage replica
    pub replica_tag: ReplicaTag,
    /// Session epoch when the proof was generated
    pub session_epoch: u64,
    /// Digest of the presence ticket (for authentication)
    pub ticket_digest: [u8; 32],
    /// BLAKE3 hash proving possession of the data
    pub proof_hash: [u8; 32],
    /// Ed25519 signature over the proof hash
    #[serde(with = "signature_serde")]
    pub signature: Signature,
}

mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&sig.to_bytes())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        Signature::from_slice(&bytes).map_err(serde::de::Error::custom)
    }
}

/// Generate proof response
///
/// Creates a cryptographic proof-of-storage response by:
/// 1. Computing BLAKE3(chunk || replica_tag || nonce || session_epoch)
/// 2. Signing the resulting hash with the device's signing key
///
/// # Arguments
/// * `chunk_data` - The actual data chunk being proven
/// * `replica_tag` - Unique identifier for this replica
/// * `challenge` - The challenge being responded to
/// * `session_epoch` - Current session epoch for freshness
/// * `signing_key` - Device signing key for authentication
pub fn generate_proof(
    chunk_data: &[u8],
    replica_tag: &ReplicaTag,
    challenge: &Challenge,
    session_epoch: SessionEpoch,
    signing_key: &SigningKey,
) -> Result<ProofResponse> {
    // Compute proof hash: BLAKE3(chunk || replica_tag || nonce || session_epoch)
    let mut hasher = blake3::Hasher::new();
    hasher.update(chunk_data);
    hasher.update(replica_tag.0.as_bytes());
    hasher.update(&challenge.nonce);
    hasher.update(&session_epoch.0.to_le_bytes());
    let proof_hash = *hasher.finalize().as_bytes();

    // Sign the proof hash
    let signature = signing_key.sign(&proof_hash);

    // Compute ticket digest (mock for MVP)
    let ticket_digest = [0u8; 32];

    Ok(ProofResponse {
        replica_tag: replica_tag.clone(),
        session_epoch: session_epoch.0,
        ticket_digest,
        proof_hash,
        signature,
    })
}

/// Verify proof response
///
/// Verifies a proof-of-storage response by:
/// 1. Checking the session epoch is current
/// 2. Recomputing the expected proof hash
/// 3. Verifying the signature over the proof hash
///
/// # Arguments
/// * `chunk_data` - The actual data chunk being verified
/// * `challenge` - The original challenge
/// * `response` - The proof response to verify
/// * `current_epoch` - Current session epoch
/// * `verifying_key` - Public key for signature verification
pub fn verify_proof(
    chunk_data: &[u8],
    challenge: &Challenge,
    response: &ProofResponse,
    current_epoch: SessionEpoch,
    verifying_key: &VerifyingKey,
) -> Result<bool> {
    // Check epoch
    if response.session_epoch != current_epoch.0 {
        return Err(StoreErrorBuilder::invalid_protocol_state(format!(
            "Stale epoch: {} != {}",
            response.session_epoch, current_epoch.0
        )));
    }

    // Recompute proof hash
    let mut hasher = blake3::Hasher::new();
    hasher.update(chunk_data);
    hasher.update(response.replica_tag.0.as_bytes());
    hasher.update(&challenge.nonce);
    hasher.update(&response.session_epoch.to_le_bytes());
    let expected_hash = *hasher.finalize().as_bytes();

    // Verify hash matches
    if expected_hash != response.proof_hash {
        return Ok(false);
    }

    // Verify signature
    verifying_key
        .verify(&response.proof_hash, &response.signature)
        .map_err(|_| StoreErrorBuilder::integrity_check_failed("Invalid signature"))?;

    Ok(true)
}

/// Replica metadata
///
/// Metadata about a storage replica including tracking information
/// for challenge scheduling and verification history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaMetadata {
    /// Unique identifier for this replica
    pub replica_tag: ReplicaTag,
    /// Device storing this replica
    pub device_id: DeviceId,
    /// Content identifier of the stored chunk
    pub chunk_cid: Cid,
    /// Unix timestamp when replica was created
    pub created_at: u64,
    /// Unix timestamp of last successful verification
    pub last_verified: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use rand::rngs::OsRng;

    #[test]
    fn test_proof_generation_and_verification() {
        let effects = aura_crypto::Effects::for_test("test_proof_generation_and_verification");
        let chunk_data = b"test chunk data";
        let replica_tag = ReplicaTag::new_with_effects(&effects);
        let challenge = Challenge::new_with_effects(
            Cid("test_chunk".to_string()),
            DeviceId::new_with_effects(&effects),
            &effects,
        );
        let session_epoch = SessionEpoch::initial();

        let random_bytes: [u8; 32] = effects.random_bytes();
        let signing_key = SigningKey::from_bytes(&random_bytes);
        let verifying_key = signing_key.verifying_key();

        let proof = generate_proof(
            chunk_data,
            &replica_tag,
            &challenge,
            session_epoch,
            &signing_key,
        )
        .unwrap();

        let valid = verify_proof(
            chunk_data,
            &challenge,
            &proof,
            session_epoch,
            &verifying_key,
        )
        .unwrap();

        assert!(valid);
    }

    #[test]
    fn test_proof_fails_with_wrong_data() {
        let effects = aura_crypto::Effects::for_test("test_proof_fails_with_wrong_data");
        let chunk_data = b"test chunk data";
        let wrong_data = b"wrong chunk data";
        let replica_tag = ReplicaTag::new_with_effects(&effects);
        let challenge = Challenge::new_with_effects(
            Cid("test_chunk".to_string()),
            DeviceId::new_with_effects(&effects),
            &effects,
        );
        let session_epoch = SessionEpoch::initial();

        let random_bytes: [u8; 32] = effects.random_bytes();
        let signing_key = SigningKey::from_bytes(&random_bytes);
        let verifying_key = signing_key.verifying_key();

        let proof = generate_proof(
            chunk_data,
            &replica_tag,
            &challenge,
            session_epoch,
            &signing_key,
        )
        .unwrap();

        let valid = verify_proof(
            wrong_data,
            &challenge,
            &proof,
            session_epoch,
            &verifying_key,
        )
        .unwrap();

        assert!(!valid);
    }
}
