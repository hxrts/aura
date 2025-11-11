//! Aura Authentication
//!
//! Layer 2 of the Aura security model: Identity verification and proof of who signed something.
//!
//! This crate handles proving WHO signed something (identity verification):
//! - "This signature proves DeviceId X signed this message"
//! - "This threshold signature proves M-of-N devices signed this"
//! - "This guardian signature proves GuardianId Y signed this"
//!
//! Authentication is stateless - it verifies signatures against public keys without
//! requiring knowledge of permissions, capabilities, or policies.

#![allow(missing_docs)]

pub mod device;
pub mod event_validation;
pub mod guardian;
pub mod session;
pub mod threshold;

// Pure authentication - no authorization types

// Phase 6: Pure identity verification tests
#[cfg(test)]
mod identity_verification_tests;

// Re-export commonly used types
pub use aura_crypto::{Ed25519Signature, Ed25519VerifyingKey};
pub use device::verify_device_signature;
pub use guardian::verify_guardian_signature;
pub use session::verify_session_ticket;
pub use threshold::verify_threshold_signature;

// Re-export identity validation functions
pub use event_validation::{
    validate_device_signature, validate_guardian_signature, validate_threshold_signature,
    IdentityValidator,
};

// New identity verification function (defined below)

// Re-export domain types
pub use aura_core::relationships::*;

// IdentityProof and ThresholdSig are defined in this module and exported by default

// Convenience functions
pub use device::verify_signature;

/// Authentication errors
#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Invalid device signature: {0}")]
    InvalidDeviceSignature(String),

    #[error("Invalid threshold signature: {0}")]
    InvalidThresholdSignature(String),

    #[error("Invalid guardian signature: {0}")]
    InvalidGuardianSignature(String),

    #[error("Invalid session ticket: {0}")]
    InvalidSessionTicket(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),
}

pub type Result<T> = std::result::Result<T, AuthenticationError>;

/// Key material for identity verification
///
/// This provides access to public keys needed for signature verification.
/// No policies or authorization data - pure cryptographic material only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyMaterial {
    /// Device public keys indexed by DeviceId
    device_keys: std::collections::HashMap<aura_core::DeviceId, aura_crypto::Ed25519VerifyingKey>,

    /// Guardian public keys indexed by GuardianId
    guardian_keys:
        std::collections::HashMap<aura_core::GuardianId, aura_crypto::Ed25519VerifyingKey>,

    /// Group public keys for threshold verification indexed by AccountId
    group_keys: std::collections::HashMap<aura_core::AccountId, aura_crypto::Ed25519VerifyingKey>,
}

// Re-export types for convenience

impl KeyMaterial {
    /// Create new key material store
    pub fn new() -> Self {
        Self {
            device_keys: std::collections::HashMap::new(),
            guardian_keys: std::collections::HashMap::new(),
            group_keys: std::collections::HashMap::new(),
        }
    }

    /// Get the public key for a device
    pub fn get_device_public_key(
        &self,
        device_id: &aura_core::DeviceId,
    ) -> Result<&Ed25519VerifyingKey> {
        self.device_keys.get(device_id).ok_or_else(|| {
            AuthenticationError::InvalidDeviceSignature(format!("Unknown device: {}", device_id))
        })
    }

    /// Add a device public key
    pub fn add_device_key(
        &mut self,
        device_id: aura_core::DeviceId,
        public_key: Ed25519VerifyingKey,
    ) {
        self.device_keys.insert(device_id, public_key);
    }

    /// Get the guardian public key
    pub fn get_guardian_public_key(
        &self,
        guardian_id: &aura_core::GuardianId,
    ) -> Result<&Ed25519VerifyingKey> {
        self.guardian_keys.get(guardian_id).ok_or_else(|| {
            AuthenticationError::InvalidGuardianSignature(format!(
                "Unknown guardian: {}",
                guardian_id
            ))
        })
    }

    /// Add a guardian public key
    pub fn add_guardian_key(
        &mut self,
        guardian_id: aura_core::GuardianId,
        public_key: Ed25519VerifyingKey,
    ) {
        self.guardian_keys.insert(guardian_id, public_key);
    }

    /// Get the group public key for threshold verification
    pub fn get_group_public_key(
        &self,
        account_id: &aura_core::AccountId,
    ) -> Result<&Ed25519VerifyingKey> {
        self.group_keys.get(account_id).ok_or_else(|| {
            AuthenticationError::InvalidThresholdSignature(format!(
                "No group key for account: {}",
                account_id
            ))
        })
    }

    /// Add a group public key for threshold verification
    pub fn add_group_key(
        &mut self,
        account_id: aura_core::AccountId,
        group_key: Ed25519VerifyingKey,
    ) {
        self.group_keys.insert(account_id, group_key);
    }
}

impl Default for KeyMaterial {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure identity proof that proves WHO signed something
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IdentityProof {
    /// Single device identity proof
    Device {
        device_id: aura_core::DeviceId,
        #[serde(with = "aura_crypto::middleware::serde_utils::signature_serde")]
        signature: Ed25519Signature,
    },
    /// Guardian identity proof
    Guardian {
        guardian_id: aura_core::GuardianId,
        #[serde(with = "aura_crypto::middleware::serde_utils::signature_serde")]
        signature: Ed25519Signature,
    },
    /// Threshold signature proof (M-of-N participants)
    Threshold(ThresholdSig),
}

/// Threshold signature structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThresholdSig {
    /// The aggregated Ed25519 signature
    #[serde(with = "aura_crypto::middleware::serde_utils::signature_serde")]
    pub signature: Ed25519Signature,
    /// Indices of devices that participated in signing
    pub signers: Vec<u8>,
    /// Individual signature shares (for auditing)
    pub signature_shares: Vec<Vec<u8>>,
}

/// Verified identity after successful authentication
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VerifiedIdentity {
    /// The identity that was verified
    pub proof: IdentityProof,
    /// Message that was authenticated
    pub message_hash: [u8; 32],
}

/// Verify an identity proof against a message
pub fn verify_identity_proof(
    proof: &IdentityProof,
    message: &[u8],
    key_material: &KeyMaterial,
) -> Result<VerifiedIdentity> {
    let message_hash = *blake3::hash(message).as_bytes();

    match proof {
        IdentityProof::Device {
            device_id,
            signature,
        } => {
            let public_key = key_material.get_device_public_key(device_id)?;
            verify_device_signature(*device_id, message, signature, public_key)?;
        }
        IdentityProof::Guardian {
            guardian_id,
            signature,
        } => {
            let public_key = key_material.get_guardian_public_key(guardian_id)?;
            verify_guardian_signature(*guardian_id, message, signature, public_key)?;
        }
        IdentityProof::Threshold(_threshold_sig) => {
            // Note: This needs the group public key to be derivable from the message context
            // TODO fix - For now, we'll need an account_id to look up the group key
            return Err(AuthenticationError::InvalidThresholdSignature(
                "Threshold verification requires account context".to_string(),
            ));
        }
    }

    Ok(VerifiedIdentity {
        proof: proof.clone(),
        message_hash,
    })
}
