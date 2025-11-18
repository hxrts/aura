//! Aura Identity Verification
//!
//! **Layer 2: Specification - WHO (Authentication)**
//!
//! Complete identity verification system combining cryptographic signature verification
//! with organizational device lifecycle management.
//!
//! # Architecture
//!
//! Core Layer 2 domain crate. Implements `aura-core` traits for identity concepts.
//! Used by `aura-authenticate` (Layer 5) and other layers needing signature verification.
//! No effect handlers - pure domain logic and cryptographic verification.
//!
//! # Core Modules
//!
//! - **Cryptographic Verification**: Signature verification (device, guardian, threshold)
//! - **Device Registry**: Device lifecycle management (active, suspended, revoked)
//! - **Session Management**: Session ticket validation
//!
//! # Core Types
//!
//! - **IdentityProof**: WHO signed something (Device, Guardian, or Threshold)
//! - **KeyMaterial**: Public keys for verification (device, guardian, group)
//! - **VerifiedIdentity**: Successful verification result with proof and message hash
//! - **IdentityVerifier**: Device registry and lifecycle management
//! - **DeviceInfo**: Device registration with status tracking
//! - **AuthenticationError**: Signature validation failures

#![allow(missing_docs)]

pub mod device;
pub mod event_validation;
pub mod guardian;
pub mod registry;
pub mod session;
pub mod threshold;

// Re-export commonly used types
pub use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

// Legacy low-level verification functions - prefer SimpleIdentityVerifier instead
#[deprecated(since = "0.2.0", note = "Use SimpleIdentityVerifier::verify_device_signature instead")]
pub use device::verify_device_signature;
#[deprecated(since = "0.2.0", note = "Use SimpleIdentityVerifier::verify_guardian_signature instead")]
pub use guardian::verify_guardian_signature;
pub use session::verify_session_ticket;
#[deprecated(since = "0.2.0", note = "Use SimpleIdentityVerifier::verify_threshold_signature instead")]
pub use threshold::verify_threshold_signature;

// Re-export identity validation functions
pub use event_validation::{
    validate_device_signature, validate_guardian_signature, validate_threshold_signature,
    IdentityValidator,
};

use aura_core::hash::hash;

// Re-export domain types
pub use aura_core::relationships::*;

// Re-export registry types (from merged aura-identity)
pub use registry::{DeviceInfo, DeviceStatus, IdentityVerifier, VerificationResult};

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
/// **Note**: For most use cases, prefer `SimpleIdentityVerifier` which provides
/// a cleaner API. `KeyMaterial` is primarily used for serialization in protocol
/// messages and advanced use cases.
///
/// This provides access to public keys needed for signature verification.
/// No policies or authorization data - pure cryptographic material only.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyMaterial {
    /// Device public keys indexed by DeviceId
    device_keys: std::collections::HashMap<aura_core::DeviceId, aura_core::Ed25519VerifyingKey>,

    /// Guardian public keys indexed by GuardianId
    guardian_keys: std::collections::HashMap<aura_core::GuardianId, aura_core::Ed25519VerifyingKey>,

    /// Group public keys for threshold verification indexed by AccountId
    group_keys: std::collections::HashMap<aura_core::AccountId, aura_core::Ed25519VerifyingKey>,
}

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
        signature: Ed25519Signature,
    },
    /// Guardian identity proof
    Guardian {
        guardian_id: aura_core::GuardianId,
        signature: Ed25519Signature,
    },
    /// Threshold signature proof (M-of-N participants)
    Threshold(ThresholdSig),
}

/// Threshold signature structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThresholdSig {
    /// The aggregated Ed25519 signature
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

/// Simplified identity verifier facade
/// 
/// This hides the complexity of KeyMaterial and provides a clean interface
/// for common verification operations.
pub struct SimpleIdentityVerifier {
    key_material: KeyMaterial,
}

impl SimpleIdentityVerifier {
    /// Create a new identity verifier
    pub fn new() -> Self {
        Self {
            key_material: KeyMaterial::new(),
        }
    }

    /// Create from existing key material
    pub fn from_key_material(key_material: KeyMaterial) -> Self {
        Self { key_material }
    }

    /// Add a device key for verification
    pub fn add_device_key(&mut self, device_id: aura_core::DeviceId, public_key: Ed25519VerifyingKey) {
        self.key_material.add_device_key(device_id, public_key);
    }

    /// Add a guardian key for verification
    pub fn add_guardian_key(&mut self, guardian_id: aura_core::GuardianId, public_key: Ed25519VerifyingKey) {
        self.key_material.add_guardian_key(guardian_id, public_key);
    }

    /// Add a group key for threshold verification
    pub fn add_group_key(&mut self, account_id: aura_core::AccountId, group_key: Ed25519VerifyingKey) {
        self.key_material.add_group_key(account_id, group_key);
    }

    /// Verify a device signature
    pub fn verify_device_signature(&self, proof: &IdentityProof) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Device { device_id, signature } => {
                // For device signatures, we use the device_id as the message
                let message = device_id.0.as_bytes();
                let message_hash = hash(message);
                let public_key = self.key_material.get_device_public_key(device_id)?;
                verify_device_signature(*device_id, message, signature, public_key)?;
                Ok(VerifiedIdentity {
                    proof: proof.clone(),
                    message_hash,
                })
            }
            _ => Err(AuthenticationError::InvalidDeviceSignature(
                "Proof is not a device signature".to_string()
            ))
        }
    }

    /// Verify a threshold signature
    pub fn verify_threshold_signature(&self, proof: &IdentityProof, account_id: aura_core::AccountId) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Threshold(threshold_sig) => {
                // Use account_id as the message context for threshold verification
                let message = account_id.0.as_bytes();
                let message_hash = hash(message);
                let group_key = self.key_material.get_group_public_key(&account_id)?;

                // Calculate minimum signers from the signature shares
                let min_signers = threshold_sig.signers.len().max(1);

                verify_threshold_signature(message, &threshold_sig.signature, group_key, min_signers)?;
                Ok(VerifiedIdentity {
                    proof: proof.clone(),
                    message_hash,
                })
            }
            _ => Err(AuthenticationError::InvalidThresholdSignature(
                "Proof is not a threshold signature".to_string()
            ))
        }
    }

    /// Verify a guardian signature
    pub fn verify_guardian_signature(&self, proof: &IdentityProof, message: &[u8]) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Guardian { guardian_id, signature } => {
                let message_hash = hash(message);
                let public_key = self.key_material.get_guardian_public_key(guardian_id)?;
                verify_guardian_signature(*guardian_id, message, signature, public_key)?;
                Ok(VerifiedIdentity {
                    proof: proof.clone(),
                    message_hash,
                })
            }
            _ => Err(AuthenticationError::InvalidGuardianSignature(
                "Proof is not a guardian signature".to_string()
            ))
        }
    }

    /// Get access to the underlying key material (for advanced use cases)
    pub fn key_material(&self) -> &KeyMaterial {
        &self.key_material
    }
}

impl Default for SimpleIdentityVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Verify an identity proof against a message
///
/// **Deprecated**: Use `SimpleIdentityVerifier` methods instead for better API and complete
/// support for all identity proof types including threshold signatures.
#[deprecated(since = "0.2.0", note = "Use SimpleIdentityVerifier methods instead")]
pub fn verify_identity_proof(
    proof: &IdentityProof,
    message: &[u8],
    key_material: &KeyMaterial,
) -> Result<VerifiedIdentity> {
    let message_hash = hash(message);

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
