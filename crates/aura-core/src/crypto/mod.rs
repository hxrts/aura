pub mod amp;
pub mod frost;
pub mod hash;
pub mod key_derivation;
pub mod merkle;
pub mod tree_signing;

use ed25519_dalek::Signer;
use serde::{Deserialize, Serialize};

// Merkle helpers
pub use merkle::{
    build_commitment_tree, build_merkle_root, generate_merkle_proof, verify_merkle_proof,
    SimpleMerkleProof,
};

// Deterministic key derivation types
pub use key_derivation::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};

/// Basic Ed25519 signature wrapper (bytes form for serialization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ed25519Signature(pub Vec<u8>);

impl Ed25519Signature {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn to_bytes(&self) -> [u8; 64] {
        let mut arr = [0u8; 64];
        let len = self.0.len().min(64);
        arr[..len].copy_from_slice(&self.0[..len]);
        arr
    }
}

impl From<[u8; 64]> for Ed25519Signature {
    fn from(value: [u8; 64]) -> Self {
        Self(value.to_vec())
    }
}

/// Basic Ed25519 signing key wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ed25519SigningKey(pub Vec<u8>);

impl Ed25519SigningKey {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.clone()
    }

    pub fn verifying_key(&self) -> Ed25519VerifyingKey {
        let key =
            ed25519_dalek::SigningKey::from_bytes(&self.0.clone().try_into().unwrap_or([0u8; 32]));
        Ed25519VerifyingKey(key.verifying_key().to_bytes().to_vec())
    }

    pub fn sign(&self, message: &[u8]) -> Ed25519Signature {
        let key =
            ed25519_dalek::SigningKey::from_bytes(&self.0.clone().try_into().unwrap_or([0u8; 32]));
        let sig = key.sign(message);
        Ed25519Signature(sig.to_bytes().to_vec())
    }
}

/// Basic Ed25519 verifying key wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Ed25519VerifyingKey(pub Vec<u8>);

impl Ed25519VerifyingKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let arr: [u8; 32] = bytes.try_into().map_err(|_| "invalid key length")?;
        ed25519_dalek::VerifyingKey::from_bytes(&arr)
            .map(|_| Ed25519VerifyingKey(arr.to_vec()))
            .map_err(|e| e.to_string())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Convenience verification helper.
    pub fn verify(
        &self,
        message: &[u8],
        signature: &Ed25519Signature,
    ) -> Result<(), crate::AuraError> {
        if ed25519_verify(message, signature, self)? {
            Ok(())
        } else {
            Err(crate::AuraError::crypto("signature verification failed"))
        }
    }
}

/// HPKE key types (X25519 serialized byte representation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkePublicKey(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkePrivateKey(pub Vec<u8>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HpkeKeyPair {
    pub public: HpkePublicKey,
    pub private: HpkePrivateKey,
}

impl HpkeKeyPair {
    pub fn new(public: Vec<u8>, private: Vec<u8>) -> Self {
        Self {
            public: HpkePublicKey(public),
            private: HpkePrivateKey(private),
        }
    }
}

/// Verify an Ed25519 signature using dalek.
pub fn ed25519_verify(
    message: &[u8],
    signature: &Ed25519Signature,
    public_key: &Ed25519VerifyingKey,
) -> Result<bool, crate::AuraError> {
    let pk_bytes: [u8; 32] = public_key
        .0
        .clone()
        .try_into()
        .map_err(|_| crate::AuraError::crypto("invalid public key length"))?;
    let sig_bytes: [u8; 64] = signature
        .0
        .clone()
        .try_into()
        .map_err(|_| crate::AuraError::crypto("invalid signature length"))?;

    let pk = ed25519_dalek::VerifyingKey::from_bytes(&pk_bytes)
        .map_err(|e| crate::AuraError::crypto(e.to_string()))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    Ok(pk.verify_strict(message, &sig).is_ok())
}

/// Derive verifying key from signing key bytes.
pub fn ed25519_verifying_key(signing_key: &Ed25519SigningKey) -> Ed25519VerifyingKey {
    let arr: [u8; 32] = signing_key.0.clone().try_into().unwrap_or([0u8; 32]);
    let key = ed25519_dalek::SigningKey::from_bytes(&arr);
    Ed25519VerifyingKey(key.verifying_key().to_bytes().to_vec())
}

/// Alias for Merkle proof re-export.
pub type MerkleProof = SimpleMerkleProof;

/// Opaque permission key context alias retained for compatibility.
pub type PermissionKeyContextCompat = PermissionKeyContext;
