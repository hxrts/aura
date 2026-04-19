//! # Aura Verify - Layer 2: Specification (Domain Crate)
//!
//! **Purpose**: Define identity semantics and signature verification logic.
//!
//! Complete identity verification system combining cryptographic signature verification
//! with authority lifecycle management.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - YES Identity semantics and signature verification logic
//! - YES Authority lifecycle management (active, suspended, revoked)
//! - YES Session management and validation
//! - YES Pure domain logic for authentication checks
//! - YES Fact-based authority lifecycle state changes
//! - NO cryptographic signing/verification operations (use CryptoEffects from aura-effects)
//! - NO handler composition (that's aura-composition)
//! - NO multi-party protocol logic (that's aura-protocol)
//!
//! # Authority Model
//!
//! Protocol participants and session issuers are identified by `AuthorityId` rather than
//! device-level identifiers. This aligns with the authority-centric identity model where
//! authorities hide their internal device structure from external parties.
//!
//! # Core Modules
//!
//! - **Cryptographic Verification**: Signature verification (authority, guardian, threshold)
//! - **Authority Registry**: Authority lifecycle management (active, suspended, revoked)
//! - **Session Management**: Session ticket validation
//! - **Facts**: Pure fact types for authority lifecycle state changes
//!
//! # Core Types
//!
//! - **IdentityProof**: WHO signed something (Guardian, Authority, or Threshold)
//! - **KeyMaterial**: Public keys for verification (authority, guardian, group)
//! - **VerifiedIdentity**: Successful verification result with proof and message hash
//! - **AuthorityRegistry**: Authority registry and lifecycle management
//! - **VerifyFact**: Fact types for authority lifecycle events
//! - **AuthenticationError**: Signature validation failures

#![allow(missing_docs)]

pub(crate) mod authority;
pub mod event_validation;
pub mod facts;
pub mod guardian;
pub mod messages;
pub(crate) mod registry;
pub mod session;
pub mod threshold;
mod verification_common;

// Re-export commonly used types
pub use aura_core::{Ed25519Signature, Ed25519VerifyingKey};

// Re-export session verification
pub use session::verify_session_ticket;

// Re-export identity validation functions
pub use event_validation::{
    validate_authority_signature, validate_guardian_signature, validate_threshold_signature,
    IdentityValidator,
};

use aura_core::hash::hash;
use aura_macros::aura_error_types;
use std::collections::HashMap;
use std::hash::Hash;

// Internal imports for SimpleIdentityVerifier implementation
use authority::verify_authority_signature;
use event_validation::validate_threshold_signature as validate_threshold_signature_event;
use guardian::verify_guardian_signature;

// Re-export domain types
pub use aura_core::types::relationships::*;

// Re-export registry types (from merged aura-identity)
pub use registry::{AuthorityRegistry, AuthorityStatus, VerificationResult};

// Re-export fact types
pub use facts::{
    derive_device_naming_context, device_naming_fact_type_id, verify_fact_type_id, Confidence,
    DeviceNamingFact, PublicKeyBytes, RevocationReason, VerificationType, VerifyFact,
    VerifyFactDelta, VerifyFactReducer, DEVICE_NAMING_FACT_TYPE_ID, DEVICE_NAMING_SCHEMA_VERSION,
    NICKNAME_SUGGESTION_BYTES_MAX, VERIFY_FACT_TYPE_ID,
};

// Re-export crypto message types (now consolidated in messages.rs)
pub use messages::{
    AbortResharingMessage, AcknowledgeSubShareMessage, CryptoMessage, CryptoPayload,
    DistributeSubShareMessage, EncryptedShare, FinalizeResharingMessage, InitiateResharingMessage,
    ParticipantResharingVerification, ResharingAbortReason, ResharingMessage,
    ResharingProtocolResult, ResharingVerification, RollbackResharingMessage,
};

// Convenience functions
pub use authority::verify_signature;

aura_error_types! {
    /// Authentication errors
    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    #[allow(missing_docs)]
    pub enum AuthenticationError {
        #[category = "authorization"]
        InvalidAuthoritySignature { details: String } =>
            "Invalid authority signature: {details}",

        #[category = "authorization"]
        InvalidThresholdSignature { details: String } =>
            "Invalid threshold signature: {details}",

        #[category = "authorization"]
        InvalidGuardianSignature { details: String } =>
            "Invalid guardian signature: {details}",

        #[category = "authorization"]
        InvalidSessionTicket { details: String } =>
            "Invalid session ticket: {details}",

        #[category = "crypto"]
        CryptoError { details: String } =>
            "Crypto error: {details}",
    }
}

pub type Result<T> = std::result::Result<T, AuthenticationError>;

fn get_key<'a, K, V>(
    map: &'a HashMap<K, V>,
    key: &K,
    missing_error: impl FnOnce() -> AuthenticationError,
) -> Result<&'a V>
where
    K: Eq + Hash,
{
    map.get(key).ok_or_else(missing_error)
}

fn insert_key<K, V>(map: &mut HashMap<K, V>, key: K, value: V)
where
    K: Eq + Hash,
{
    map.insert(key, value);
}

fn verified_identity(proof: &IdentityProof, message: &[u8]) -> VerifiedIdentity {
    VerifiedIdentity {
        proof: proof.clone(),
        message_hash: hash(message),
    }
}

fn threshold_signer_requirement(threshold_sig: &ThresholdSig) -> Result<u16> {
    let signer_count = u16::try_from(threshold_sig.signers.len()).map_err(|_| {
        AuthenticationError::InvalidThresholdSignature {
            details: "threshold signer set exceeds supported size".to_string(),
        }
    })?;

    if signer_count == 0 {
        Err(AuthenticationError::InvalidThresholdSignature {
            details: "threshold signature requires at least one signer".to_string(),
        })
    } else {
        Ok(signer_count)
    }
}

/// Key material for identity verification
///
/// **Note**: For most use cases, prefer `SimpleIdentityVerifier` which provides
/// a cleaner API. `KeyMaterial` is primarily used for serialization in protocol
/// messages and advanced use cases.
///
/// This provides access to public keys needed for signature verification.
/// No policies or authorization data - pure cryptographic material only.
///
/// **Authority Model Note**: Authority keys are indexed by `AuthorityId` and
/// used for cross-authority protocol messages.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KeyMaterial {
    /// Authority public keys indexed by AuthorityId
    authority_keys:
        std::collections::HashMap<aura_core::AuthorityId, aura_core::Ed25519VerifyingKey>,

    /// Guardian public keys indexed by GuardianId
    guardian_keys: std::collections::HashMap<aura_core::GuardianId, aura_core::Ed25519VerifyingKey>,

    /// Group public keys for threshold verification indexed by AccountId
    group_keys: std::collections::HashMap<aura_core::AccountId, aura_core::Ed25519VerifyingKey>,
}

impl KeyMaterial {
    /// Create new key material store
    pub fn new() -> Self {
        Self {
            authority_keys: std::collections::HashMap::new(),
            guardian_keys: std::collections::HashMap::new(),
            group_keys: std::collections::HashMap::new(),
        }
    }

    /// Get the public key for an authority
    pub fn get_authority_public_key(
        &self,
        authority_id: &aura_core::AuthorityId,
    ) -> Result<&Ed25519VerifyingKey> {
        get_key(&self.authority_keys, authority_id, || {
            AuthenticationError::InvalidAuthoritySignature {
                details: format!("Unknown authority: {authority_id}"),
            }
        })
    }

    /// Add an authority public key
    pub fn add_authority_key(
        &mut self,
        authority_id: aura_core::AuthorityId,
        public_key: Ed25519VerifyingKey,
    ) {
        insert_key(&mut self.authority_keys, authority_id, public_key);
    }

    /// Get the guardian public key
    pub fn get_guardian_public_key(
        &self,
        guardian_id: &aura_core::GuardianId,
    ) -> Result<&Ed25519VerifyingKey> {
        get_key(&self.guardian_keys, guardian_id, || {
            AuthenticationError::InvalidGuardianSignature {
                details: format!("Unknown guardian: {guardian_id}"),
            }
        })
    }

    /// Add a guardian public key
    pub fn add_guardian_key(
        &mut self,
        guardian_id: aura_core::GuardianId,
        public_key: Ed25519VerifyingKey,
    ) {
        insert_key(&mut self.guardian_keys, guardian_id, public_key);
    }

    /// Get the group public key for threshold verification
    pub fn get_group_public_key(
        &self,
        account_id: &aura_core::AccountId,
    ) -> Result<&Ed25519VerifyingKey> {
        get_key(&self.group_keys, account_id, || {
            AuthenticationError::InvalidThresholdSignature {
                details: format!("No group key for account: {account_id}"),
            }
        })
    }

    /// Add a group public key for threshold verification
    pub fn add_group_key(
        &mut self,
        account_id: aura_core::AccountId,
        group_key: Ed25519VerifyingKey,
    ) {
        insert_key(&mut self.group_keys, account_id, group_key);
    }
}

impl Default for KeyMaterial {
    fn default() -> Self {
        Self::new()
    }
}

/// Pure identity proof that proves WHO signed something.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum IdentityProof {
    /// Guardian identity proof
    Guardian {
        guardian_id: aura_core::GuardianId,
        signature: Ed25519Signature,
    },
    /// Authority identity proof (for authority-level authentication)
    ///
    /// **Preferred**: Use this variant for cross-authority communication
    /// where the internal device structure should be hidden.
    Authority {
        authority_id: aura_core::AuthorityId,
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
    /// Indices of participants that participated in signing
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

    /// Add an authority key for verification
    pub fn add_authority_key(
        &mut self,
        authority_id: aura_core::AuthorityId,
        public_key: Ed25519VerifyingKey,
    ) {
        self.key_material
            .add_authority_key(authority_id, public_key);
    }

    /// Add a guardian key for verification
    pub fn add_guardian_key(
        &mut self,
        guardian_id: aura_core::GuardianId,
        public_key: Ed25519VerifyingKey,
    ) {
        self.key_material.add_guardian_key(guardian_id, public_key);
    }

    /// Add a group key for threshold verification
    pub fn add_group_key(
        &mut self,
        account_id: aura_core::AccountId,
        group_key: Ed25519VerifyingKey,
    ) {
        self.key_material.add_group_key(account_id, group_key);
    }

    /// Verify an authority signature
    pub fn verify_authority_signature(&self, proof: &IdentityProof) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Authority {
                authority_id,
                signature,
            } => {
                let message = authority_id.0.as_bytes();
                let public_key = self.key_material.get_authority_public_key(authority_id)?;
                verify_authority_signature(*authority_id, message, signature, public_key)?;
                Ok(verified_identity(proof, message))
            }
            _ => Err(AuthenticationError::InvalidAuthoritySignature {
                details: "Proof is not an authority signature".to_string(),
            }),
        }
    }

    /// Verify a threshold signature
    pub fn verify_threshold_signature(
        &self,
        proof: &IdentityProof,
        account_id: aura_core::AccountId,
    ) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Threshold(threshold_sig) => {
                let message = account_id.0.as_bytes();
                let group_key = self.key_material.get_group_public_key(&account_id)?;
                let required_threshold = threshold_signer_requirement(threshold_sig)?;

                validate_threshold_signature_event(
                    threshold_sig,
                    message,
                    group_key,
                    required_threshold,
                )?;
                Ok(verified_identity(proof, message))
            }
            _ => Err(AuthenticationError::InvalidThresholdSignature {
                details: "Proof is not a threshold signature".to_string(),
            }),
        }
    }

    /// Verify a guardian signature
    pub fn verify_guardian_signature(
        &self,
        proof: &IdentityProof,
        message: &[u8],
    ) -> Result<VerifiedIdentity> {
        match proof {
            IdentityProof::Guardian {
                guardian_id,
                signature,
            } => {
                let public_key = self.key_material.get_guardian_public_key(guardian_id)?;
                verify_guardian_signature(*guardian_id, message, signature, public_key)?;
                Ok(verified_identity(proof, message))
            }
            _ => Err(AuthenticationError::InvalidGuardianSignature {
                details: "Proof is not a guardian signature".to_string(),
            }),
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

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use aura_core::crypto::ed25519::Ed25519SigningKey;

    fn authority_id(seed: u8) -> aura_core::AuthorityId {
        aura_core::AuthorityId::new_from_entropy([seed; 32])
    }

    fn guardian_id(seed: u8) -> aura_core::GuardianId {
        aura_core::GuardianId::new_from_entropy([seed; 32])
    }

    fn account_id(seed: u8) -> aura_core::AccountId {
        aura_core::AccountId::new_from_entropy([seed; 32])
    }

    fn signing_material(seed: u8, message: &[u8]) -> (Ed25519Signature, Ed25519VerifyingKey) {
        let signing_key = Ed25519SigningKey::from_bytes([seed; 32]);
        let verifying_key = signing_key.verifying_key().unwrap();
        let signature = signing_key.sign(message).unwrap();
        (signature, verifying_key)
    }

    #[test]
    fn unknown_authority_key_lookup_fails() {
        let verifier = SimpleIdentityVerifier::new();
        let authority_id = authority_id(1);
        let (signature, _) = signing_material(9, authority_id.0.as_bytes());
        let proof = IdentityProof::Authority {
            authority_id,
            signature,
        };

        let result = verifier.verify_authority_signature(&proof);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidAuthoritySignature { .. }
        ));
    }

    #[test]
    fn authority_signature_verification_hashes_authority_context() {
        let mut verifier = SimpleIdentityVerifier::new();
        let authority_id = authority_id(2);
        let message = authority_id.0.as_bytes();
        let (signature, verifying_key) = signing_material(10, message);
        verifier.add_authority_key(authority_id, verifying_key);
        let proof = IdentityProof::Authority {
            authority_id,
            signature,
        };

        let verified = verifier.verify_authority_signature(&proof).unwrap();

        assert_eq!(verified.message_hash, hash(message));
    }

    #[test]
    fn guardian_proof_type_mismatch_is_rejected() {
        let mut verifier = SimpleIdentityVerifier::new();
        let authority_id = authority_id(3);
        let (signature, verifying_key) = signing_material(20, authority_id.0.as_bytes());
        verifier.add_authority_key(authority_id, verifying_key);
        let proof = IdentityProof::Authority {
            authority_id,
            signature,
        };

        let result = verifier.verify_guardian_signature(&proof, b"guardian-message");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidGuardianSignature { .. }
        ));
    }

    #[test]
    fn threshold_signature_requires_nonempty_signers() {
        let mut verifier = SimpleIdentityVerifier::new();
        let account_id = account_id(4);
        let message = account_id.0.as_bytes();
        let (signature, verifying_key) = signing_material(21, message);
        verifier.add_group_key(account_id, verifying_key);
        let proof = IdentityProof::Threshold(ThresholdSig {
            signature,
            signers: Vec::new(),
            signature_shares: Vec::new(),
        });

        let result = verifier.verify_threshold_signature(&proof, account_id);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AuthenticationError::InvalidThresholdSignature { .. }
        ));
    }

    #[test]
    fn threshold_signature_verification_hashes_account_context() {
        let mut verifier = SimpleIdentityVerifier::new();
        let account_id = account_id(5);
        let message = account_id.0.as_bytes();
        let (signature, verifying_key) = signing_material(22, message);
        verifier.add_group_key(account_id, verifying_key);
        let proof = IdentityProof::Threshold(ThresholdSig {
            signature,
            signers: vec![1],
            signature_shares: Vec::new(),
        });

        let verified = verifier
            .verify_threshold_signature(&proof, account_id)
            .unwrap();

        assert_eq!(verified.message_hash, hash(message));
    }

    #[test]
    fn guardian_signature_verification_hashes_message_context() {
        let mut verifier = SimpleIdentityVerifier::new();
        let guardian_id = guardian_id(6);
        let message = b"guardian context";
        let (signature, verifying_key) = signing_material(23, message);
        verifier.add_guardian_key(guardian_id, verifying_key);
        let proof = IdentityProof::Guardian {
            guardian_id,
            signature,
        };

        let verified = verifier.verify_guardian_signature(&proof, message).unwrap();

        assert_eq!(verified.message_hash, hash(message));
    }
}
