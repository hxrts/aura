// Proof-of-storage challenge/response protocol

use crate::{Result, StorageError};
use aura_journal::{Cid, DeviceId, SessionEpoch};
use blake3;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Replica tag for tracking storage replicas
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReplicaTag(pub Uuid);

impl ReplicaTag {
    pub fn new() -> Self {
        ReplicaTag(Uuid::new_v4())
    }
}

impl Default for ReplicaTag {
    fn default() -> Self {
        Self::new()
    }
}

/// Proof-of-storage challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub chunk_cid: Cid,
    pub nonce: [u8; 32],
    pub challenger_id: DeviceId,
}

impl Challenge {
    pub fn new(chunk_cid: Cid, challenger_id: DeviceId) -> Self {
        let mut nonce = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
        
        Challenge {
            chunk_cid,
            nonce,
            challenger_id,
        }
    }
}

/// Proof-of-storage response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofResponse {
    pub replica_tag: ReplicaTag,
    pub session_epoch: u64,
    pub ticket_digest: [u8; 32],
    pub proof_hash: [u8; 32],
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
pub fn verify_proof(
    chunk_data: &[u8],
    challenge: &Challenge,
    response: &ProofResponse,
    current_epoch: SessionEpoch,
    verifying_key: &VerifyingKey,
) -> Result<bool> {
    // Check epoch
    if response.session_epoch != current_epoch.0 {
        return Err(StorageError::Storage(format!(
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
        .map_err(|_| StorageError::Storage("Invalid signature".to_string()))?;
    
    Ok(true)
}

/// Replica metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaMetadata {
    pub replica_tag: ReplicaTag,
    pub device_id: DeviceId,
    pub chunk_cid: Cid,
    pub created_at: u64,
    pub last_verified: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use rand::rngs::OsRng;

    #[test]
    fn test_proof_generation_and_verification() {
        let chunk_data = b"test chunk data";
        let replica_tag = ReplicaTag::new();
        let challenge = Challenge::new(
            Cid("test_chunk".to_string()),
            DeviceId::new(),
        );
        let session_epoch = SessionEpoch::initial();
        
        let signing_key = SigningKey::from_bytes(&rand::random());
        let verifying_key = signing_key.verifying_key();
        
        let proof = generate_proof(
            chunk_data,
            &replica_tag,
            &challenge,
            session_epoch,
            &signing_key,
        ).unwrap();
        
        let valid = verify_proof(
            chunk_data,
            &challenge,
            &proof,
            session_epoch,
            &verifying_key,
        ).unwrap();
        
        assert!(valid);
    }

    #[test]
    fn test_proof_fails_with_wrong_data() {
        let chunk_data = b"test chunk data";
        let wrong_data = b"wrong chunk data";
        let replica_tag = ReplicaTag::new();
        let challenge = Challenge::new(
            Cid("test_chunk".to_string()),
            DeviceId::new(),
        );
        let session_epoch = SessionEpoch::initial();
        
        let signing_key = SigningKey::from_bytes(&rand::random());
        let verifying_key = signing_key.verifying_key();
        
        let proof = generate_proof(
            chunk_data,
            &replica_tag,
            &challenge,
            session_epoch,
            &signing_key,
        ).unwrap();
        
        let valid = verify_proof(
            wrong_data,
            &challenge,
            &proof,
            session_epoch,
            &verifying_key,
        ).unwrap();
        
        assert!(!valid);
    }
}

