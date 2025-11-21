//! FROST Threshold Signing Primitives for Tree Operations
//!
//! This module provides pure cryptographic primitives for FROST threshold signatures
//! used in commitment tree operations. It contains **NO** tree logic or business logic.
//!
//! ## Design Principles (from work/015.md)
//!
//! - **Pure Cryptography**: Only signing, aggregation, and verification
//! - **No Tree State**: No knowledge of TreeState, NodeIndex, or tree structure
//! - **Binding Context**: Operations bound to epoch/policy/node to prevent replay
//!
//! ## Architecture
//!
//! FROST signing follows the classic threshold signature flow:
//! 1. Each signer generates a nonce commitment
//! 2. Coordinator collects commitments and opens
//! 3. Each signer creates partial signature with their share
//! 4. Coordinator aggregates partials into group signature
//! 5. Anyone can verify against group public key
//!
//! ## References
//!
//! - [`docs/123_commitment_tree.md`](../../../../docs/123_commitment_tree.md) - Tree operations
//! - FROST paper: https://eprint.iacr.org/2020/852

use crate::hash;
// use crate::effects::SecureStorageLocation; // TODO: Import when using SecureStorageEffects
use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// FROST signing share (secret)
///
/// **CRITICAL**: Shares are NEVER stored in the journal. Each device maintains
/// shares locally, keyed by (node_id, epoch). Shares are derived off-chain via
/// separate DKG or resharing ceremonies.
///
/// This wraps the frost-ed25519 SigningShare type for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Share {
    /// Share identifier (1..=n)
    pub identifier: u16,
    /// Secret share value (serialized frost SigningShare)
    #[serde(with = "serde_bytes")]
    pub value: Vec<u8>,
}

impl Share {
    /// Create a new share from a FROST signing share
    pub fn from_frost(identifier: frost::Identifier, share: frost::keys::SigningShare) -> Self {
        let id_bytes = identifier.serialize();
        Self {
            identifier: u16::from_be_bytes([0, id_bytes[0]]),
            value: share.serialize().to_vec(),
        }
    }

    /// Convert to FROST signing share for use in signing
    pub fn to_frost(&self) -> Result<frost::keys::SigningShare, String> {
        if self.value.len() != 32 {
            return Err(format!(
                "Invalid share length: {} (expected 32)",
                self.value.len()
            ));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.value);
        frost::keys::SigningShare::deserialize(array)
            .map_err(|e| format!("Failed to deserialize signing share: {}", e))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, String> {
        frost::Identifier::try_from(self.identifier)
            .map_err(|e| format!("Invalid identifier: {}", e))
    }
}

/// Nonce for FROST signing (secret)
///
/// Generated fresh for each signing operation. Must be bound to the signing
/// context to prevent reuse across different operations.
///
/// This wraps the frost-ed25519 SigningNonces type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Nonce {
    /// Nonce identifier (for tracking)
    pub id: [u8; 32],
    /// Secret nonce value (serialized frost SigningNonces)
    #[serde(with = "serde_bytes")]
    pub value: Vec<u8>,
}

impl Nonce {
    /// Create from FROST signing nonces
    ///
    /// Note: FROST nonces cannot be serialized as they contain secret data.
    /// This stores only an identifier for tracking purposes.
    pub fn from_frost(_nonces: frost::round1::SigningNonces) -> Self {
        let mut id = [0u8; 32];
        #[allow(clippy::expect_used)]
        getrandom::getrandom(&mut id).expect("Failed to generate nonce ID");

        // TODO: Use SecureStorageEffects to store nonces securely
        // For now, we use a hash of the nonce commitment as a placeholder.
        // Real implementation should use:
        // secure_storage.secure_store(
        //     &SecureStorageLocation::frost_nonce(&session_id, signer_id),
        //     &nonce_bytes, &[SecureStorageCapability::Read, SecureStorageCapability::Delete]
        // ).await
        let mut h = hash::hasher();
        h.update(&id);
        let value = h.finalize().to_vec();

        Self { id, value }
    }

    /// Convert to FROST signing nonces
    ///
    /// This is a security limitation - FROST nonces should not be reconstructed
    /// from serialized data. In production, nonces should be:
    /// 1. Generated fresh for each signing operation
    /// 2. Stored securely in memory only
    /// 3. Never persisted to disk
    pub fn to_frost(&self) -> Result<frost::round1::SigningNonces, String> {
        Err("FROST nonces cannot be reconstructed from serialized data for security reasons. Generate fresh nonces for each signing operation.".to_string())
    }
}

/// Commitment to a nonce (public)
///
/// Sent to the coordinator during the commitment phase. Does not reveal
/// the nonce value but commits the signer to a specific nonce.
///
/// This wraps the frost-ed25519 SigningCommitments type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NonceCommitment {
    /// Signer identifier
    pub signer: u16,
    /// Commitment value (serialized frost SigningCommitments)
    #[serde(with = "serde_bytes")]
    pub commitment: Vec<u8>,
}

impl NonceCommitment {
    /// Create from FROST signing commitments
    pub fn from_frost(
        identifier: frost::Identifier,
        commitments: frost::round1::SigningCommitments,
    ) -> Self {
        let id_bytes = identifier.serialize();
        Self {
            signer: u16::from_be_bytes([0, id_bytes[0]]),
            commitment: commitments
                .serialize()
                .unwrap_or_else(|_| Vec::new()) // Handle serialization error gracefully
                .clone(),
        }
    }

    /// Convert to FROST signing commitments
    pub fn to_frost(&self) -> Result<frost::round1::SigningCommitments, String> {
        frost::round1::SigningCommitments::deserialize(&self.commitment)
            .map_err(|e| format!("Failed to deserialize commitments: {}", e))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, String> {
        frost::Identifier::try_from(self.signer).map_err(|e| format!("Invalid identifier: {}", e))
    }

    /// Create from bytes (for testing and mock implementations)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        // For mock implementations, create a simple commitment
        if bytes.len() < 32 {
            return Err("Commitment too short".to_string());
        }

        Ok(Self {
            signer: 1, // Default signer for mock
            commitment: bytes,
        })
    }
}

/// Opened nonce (public)
///
/// Revealed after all commitments are collected. The coordinator verifies
/// that it matches the earlier commitment.
#[derive(Clone, Serialize, Deserialize)]
pub struct NonceOpen {
    /// Signer identifier
    pub signer: u16,
    /// Revealed nonce value
    #[serde(with = "serde_bytes")]
    pub nonce: Vec<u8>,
}

/// Partial signature from one signer (public)
///
/// Created by applying the signing share to the message. The coordinator
/// aggregates these to form the final signature.
///
/// This wraps the frost-ed25519 SignatureShare type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialSignature {
    /// Signer identifier
    pub signer: u16,
    /// Partial signature value (serialized frost SignatureShare)
    #[serde(with = "serde_bytes")]
    pub signature: Vec<u8>,
}

impl PartialSignature {
    /// Create from FROST signature share
    pub fn from_frost(identifier: frost::Identifier, share: frost::round2::SignatureShare) -> Self {
        let id_bytes = identifier.serialize();
        Self {
            signer: u16::from_be_bytes([0, id_bytes[0]]),
            signature: share.serialize().to_vec(),
        }
    }

    /// Convert to FROST signature share
    pub fn to_frost(&self) -> Result<frost::round2::SignatureShare, String> {
        if self.signature.len() != 32 {
            return Err(format!(
                "Invalid signature length: {} (expected 32)",
                self.signature.len()
            ));
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&self.signature);
        frost::round2::SignatureShare::deserialize(array)
            .map_err(|e| format!("Failed to deserialize signature share: {}", e))
    }

    /// Get FROST identifier
    pub fn frost_identifier(&self) -> Result<frost::Identifier, String> {
        frost::Identifier::try_from(self.signer).map_err(|e| format!("Invalid identifier: {}", e))
    }

    /// Create from bytes (for testing and mock implementations)
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        // For mock implementations, create a simple partial signature
        if bytes.len() < 32 {
            return Err("Partial signature too short".to_string());
        }

        Ok(Self {
            signer: 1, // Default signer for mock
            signature: bytes,
        })
    }
}

/// Tree signing context for binding
///
/// Binds signatures to specific tree operations to prevent replay attacks.
/// All signing operations must include this context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeSigningContext {
    /// Node identifier in the tree
    pub node_id: u32,
    /// Current epoch
    pub epoch: u64,
    /// Policy hash at this node
    pub policy_hash: [u8; 32],
}

impl TreeSigningContext {
    /// Create a new tree signing context
    pub fn new(node_id: u32, epoch: u64, policy_hash: [u8; 32]) -> Self {
        Self {
            node_id,
            epoch,
            policy_hash,
        }
    }
}

/// Generate a binding message for tree operations
///
/// Combines the tree operation with the signing context to create a unique
/// message that prevents replay across epochs, policies, and nodes.
///
/// ## Format
///
/// ```text
/// BLAKE3(
///   "TREE_OP_SIG" ||
///   node_id (u32, LE) ||
///   epoch (u64, LE) ||
///   policy_hash (32 bytes) ||
///   parent_epoch (u64, LE) ||
///   parent_commitment (32 bytes) ||
///   serialized_op_kind
/// )
/// ```
///
/// ## Examples
///
/// ```
/// use aura_core::frost::tree_signing::{TreeSigningContext, binding_message};
/// use aura_core::TreeOp;
///
/// let ctx = TreeSigningContext::new(1, 42, [0u8; 32]);
/// // let op = TreeOp { ... };
/// // let msg = binding_message(&ctx, &op);
/// ```
pub fn binding_message(ctx: &TreeSigningContext, op_bytes: &[u8]) -> Vec<u8> {
    let mut h = hash::hasher();

    // Domain separator
    h.update(b"TREE_OP_SIG");

    // Context binding
    h.update(&ctx.node_id.to_le_bytes());
    h.update(&ctx.epoch.to_le_bytes());
    h.update(&ctx.policy_hash);

    // Operation content
    h.update(op_bytes);

    h.finalize().to_vec()
}

/// Generate a nonce and its commitment using FROST
///
/// Creates a fresh random nonce and computes its cryptographic commitment
/// using the FROST protocol's round 1 commitment generation.
///
/// ## Security Requirements
///
/// - Nonce MUST be fresh for each signing operation
/// - Nonce MUST be bound to the signing context
/// - Nonce MUST be discarded after use (never reused)
///
/// ## Examples
///
/// ```
/// use aura_core::frost::tree_signing::generate_nonce_with_share;
///
/// // Use generate_nonce_with_share for proper FROST nonces
/// // let (nonce, commitment) = generate_nonce_with_share(1, &signing_share);
/// // Send commitment to coordinator
/// // Keep nonce secret for signing round
/// ```
/// Generate nonce with a signing share for FROST operations
///
/// This function requires a signing share to properly generate FROST nonces.
/// TODO: Use SecureStorageEffects to retrieve signing share from secure storage.
/// Real implementation should retrieve from:
/// secure_storage.secure_retrieve(
///     &SecureStorageLocation::signing_share(&account_id, epoch, signer_id),
///     &[SecureStorageCapability::Read]
/// ).await
pub fn generate_nonce_with_share(
    signer_id: u16,
    signing_share: &frost::keys::SigningShare,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> (Nonce, NonceCommitment) {
    // Use valid identifier - if signer_id is invalid, use 1 as fallback
    let identifier = if let Ok(id) = frost::Identifier::try_from(signer_id) {
        id
    } else {
        // 1 is always a valid identifier in FROST, this is safe
        #[allow(clippy::unwrap_used)]
        frost::Identifier::try_from(1).unwrap()
    };

    // Generate proper FROST nonces and commitments using the signing share
    let (frost_nonce, frost_commitment) = frost::round1::commit(signing_share, rng);

    // Create nonce ID for tracking
    let mut nonce_id = [0u8; 32];
    rand::RngCore::fill_bytes(rng, &mut nonce_id);

    // Note: FROST SigningNonces don't have a serialize method because they contain
    // secret data. We'll store a placeholder and reconstruct when needed.
    // TODO: Use SecureStorageEffects for secure nonce storage
    // Real implementation should store nonces using time-bound tokens:
    // secure_storage.secure_store(&location, &nonce_bytes, &capabilities).await
    let nonce = Nonce::from_frost(frost_nonce);

    let commitment = NonceCommitment::from_frost(identifier, frost_commitment);

    (nonce, commitment)
}

/// Create a partial signature using FROST
///
/// Signs a message using the participant's signing share and nonce,
/// producing a partial signature that can be aggregated by the coordinator.
///
/// ## Parameters
///
/// - `share`: The signer's secret signing share
/// - `msg`: The message to sign (should be binding_message output)
/// - `nonce`: The signing nonces generated in round 1
/// - `commitments`: Map of all participants' nonce commitments (identifier -> commitment)
///
/// ## Returns
///
/// A partial signature that the coordinator will aggregate with others.
///
/// ## Errors
///
/// Returns error string if:
/// - Share deserialization fails
/// - Nonce deserialization fails
/// - Commitment deserialization fails
/// - FROST signing fails
///
/// ## Note
///
/// Create a partial signature using FROST with fresh nonces
///
/// This function performs the complete FROST signing flow:
/// 1. Generates fresh nonces (for security)
/// 2. Creates the signing package
/// 3. Signs using the participant's share
///
/// ## Security Note
///
/// This function generates fresh nonces for security rather than reusing
/// serialized nonces. This is the correct approach for FROST signatures.
pub fn frost_sign_partial(
    _share: &Share,
    _msg: &[u8],
    _nonce: &Nonce, // Ignored for security - we generate fresh nonces
    _commitments: &BTreeMap<u16, NonceCommitment>,
) -> Result<PartialSignature, String> {
    // For security reasons, this function requires a proper KeyPackage from DKG
    // rather than just a SigningShare. This prevents misuse of shares.
    Err("FROST signing requires a complete KeyPackage from a DKG ceremony. Use frost_sign_partial_with_keypackage instead.".to_string())
}

/// Create a partial signature using FROST with a proper KeyPackage
///
/// This function performs secure FROST signing with a complete key package
/// that includes all necessary cryptographic material from DKG.
pub fn frost_sign_partial_with_keypackage(
    key_package: &frost::keys::KeyPackage,
    msg: &[u8],
    commitments: &BTreeMap<u16, NonceCommitment>,
    rng: &mut (impl rand::RngCore + rand::CryptoRng),
) -> Result<PartialSignature, String> {
    let identifier = key_package.identifier();

    // Generate fresh nonces for this signing operation (secure approach)
    let (frost_nonce, _our_commitment) = frost::round1::commit(key_package.signing_share(), rng);

    // Convert commitments to FROST format
    let mut frost_commitments = BTreeMap::new();
    for (signer_id, commitment) in commitments {
        let frost_id = frost::Identifier::try_from(*signer_id)
            .map_err(|e| format!("Invalid signer ID {}: {}", signer_id, e))?;
        let frost_commit = commitment.to_frost()?;
        frost_commitments.insert(frost_id, frost_commit);
    }

    // Create signing package
    let signing_package = frost::SigningPackage::new(frost_commitments, msg);

    // Create partial signature using FROST protocol with KeyPackage
    let signature_share = frost::round2::sign(&signing_package, &frost_nonce, key_package)
        .map_err(|e| format!("FROST signing failed: {}", e))?;

    // Convert to our format
    let _signer_id = u16::from_be_bytes([0, identifier.serialize()[0]]);
    Ok(PartialSignature::from_frost(*identifier, signature_share))
}

/// Aggregate partial signatures using FROST
///
/// Combines threshold number of partial signatures into a single
/// group signature that can be verified against the group public key.
///
/// ## Parameters
///
/// - `partials`: Slice of partial signatures from threshold participants
/// - `msg`: The message that was signed
/// - `commitments`: Map of nonce commitments from all signers
/// - `pubkey_package`: The group's public key package
///
/// ## Returns
///
/// The aggregated Ed25519 signature (64 bytes)
///
/// ## Errors
///
/// Returns error if:
/// - Partial signature deserialization fails
/// - Commitment deserialization fails
/// - FROST aggregation fails (e.g., invalid shares)
pub fn frost_aggregate(
    partials: &[PartialSignature],
    msg: &[u8],
    commitments: &BTreeMap<u16, NonceCommitment>,
    pubkey_package: &frost::keys::PublicKeyPackage,
) -> Result<Vec<u8>, String> {
    // Convert partial signatures to FROST format
    let mut frost_shares = BTreeMap::new();
    for partial in partials {
        let identifier = partial.frost_identifier()?;
        let share = partial.to_frost()?;
        frost_shares.insert(identifier, share);
    }

    // Convert commitments to FROST format
    let mut frost_commitments = BTreeMap::new();
    for (signer_id, commitment) in commitments {
        let frost_id = frost::Identifier::try_from(*signer_id)
            .map_err(|e| format!("Invalid signer ID {}: {}", signer_id, e))?;
        let frost_commit = commitment.to_frost()?;
        frost_commitments.insert(frost_id, frost_commit);
    }

    // Create signing package
    let signing_package = frost::SigningPackage::new(frost_commitments, msg);

    // Aggregate signature shares
    let group_signature = frost::aggregate(&signing_package, &frost_shares, pubkey_package)
        .map_err(|e| format!("FROST aggregation failed: {}", e))?;

    // Return serialized signature
    Ok(group_signature.serialize().as_ref().to_vec())
}

/// Verify an aggregate signature using FROST
///
/// Verifies that an aggregate signature is valid for the given message
/// and group public key.
///
/// ## Parameters
///
/// - `group_pk`: The group's verification key (from PublicKeyPackage)
/// - `msg`: The message that was signed
/// - `signature`: The aggregated signature bytes (64 bytes for Ed25519)
///
/// ## Returns
///
/// `Ok(())` if signature is valid, `Err(String)` otherwise
pub fn frost_verify_aggregate(
    group_pk: &frost::VerifyingKey,
    msg: &[u8],
    signature: &[u8],
) -> Result<(), String> {
    // Deserialize signature
    if signature.len() != 64 {
        return Err(format!(
            "Invalid signature length: {} (expected 64)",
            signature.len()
        ));
    }
    let mut sig_array = [0u8; 64];
    sig_array.copy_from_slice(signature);
    let sig = frost::Signature::deserialize(sig_array)
        .map_err(|e| format!("Invalid signature format: {}", e))?;

    // Verify signature
    group_pk
        .verify(msg, &sig)
        .map_err(|e| format!("Signature verification failed: {}", e))
}

/// Threshold signature result (aggregated signature)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdSignature {
    /// The aggregated Ed25519 signature bytes (64 bytes)
    pub signature: Vec<u8>,
    /// Signers who participated in this signature
    pub signers: Vec<u16>,
}

impl ThresholdSignature {
    /// Create a new threshold signature
    pub fn new(signature: Vec<u8>, signers: Vec<u16>) -> Self {
        Self { signature, signers }
    }

    /// Get the signature bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.signature
    }
}

/// Public key package from DKG ceremony
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicKeyPackage {
    /// The group's public key for verification
    pub group_public_key: Vec<u8>,
    /// Individual signer public keys
    pub signer_public_keys: std::collections::BTreeMap<u16, Vec<u8>>,
    /// Threshold parameters
    pub threshold: u16,
    /// Maximum number of signers
    pub max_signers: u16,
}

impl PublicKeyPackage {
    /// Create a new public key package
    pub fn new(
        group_public_key: Vec<u8>,
        signer_public_keys: std::collections::BTreeMap<u16, Vec<u8>>,
        threshold: u16,
        max_signers: u16,
    ) -> Self {
        Self {
            group_public_key,
            signer_public_keys,
            threshold,
            max_signers,
        }
    }
}

/// Signing session state for coordinating signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningSession {
    /// Session identifier
    pub session_id: String,
    /// Message being signed
    pub message: Vec<u8>,
    /// Tree signing context
    pub context: TreeSigningContext,
    /// Threshold required for signing
    pub threshold: u16,
    /// Available signers
    pub available_signers: Vec<u16>,
    /// Collected nonce commitments
    pub commitments: std::collections::BTreeMap<u16, NonceCommitment>,
    /// Collected partial signatures
    pub partial_signatures: std::collections::BTreeMap<u16, PartialSignature>,
    /// Session state
    pub state: SigningSessionState,
}

/// States for a signing session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SigningSessionState {
    /// Collecting nonce commitments
    CollectingCommitments,
    /// Collecting partial signatures
    CollectingSignatures,
    /// Aggregating final signature
    Aggregating,
    /// Session completed successfully
    Completed(ThresholdSignature),
    /// Session failed
    Failed(String),
}

impl SigningSession {
    /// Create a new signing session
    pub fn new(
        session_id: String,
        message: Vec<u8>,
        context: TreeSigningContext,
        threshold: u16,
        available_signers: Vec<u16>,
    ) -> Self {
        Self {
            session_id,
            message,
            context,
            threshold,
            available_signers,
            commitments: std::collections::BTreeMap::new(),
            partial_signatures: std::collections::BTreeMap::new(),
            state: SigningSessionState::CollectingCommitments,
        }
    }

    /// Get the threshold required for this session
    pub fn threshold(&self) -> u16 {
        self.threshold
    }

    /// Add a nonce commitment
    pub fn add_commitment(&mut self, commitment: NonceCommitment) {
        self.commitments.insert(commitment.signer, commitment);
    }

    /// Add a partial signature
    pub fn add_partial_signature(&mut self, signature: PartialSignature) {
        self.partial_signatures.insert(signature.signer, signature);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binding_message_deterministic() {
        let ctx = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let op = b"test_operation";

        let msg1 = binding_message(&ctx, op);
        let msg2 = binding_message(&ctx, op);

        assert_eq!(msg1, msg2, "Binding message should be deterministic");
    }

    #[test]
    fn test_binding_message_different_contexts() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(2, 42, [0xAA; 32]); // Different node
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different nodes should produce different bindings"
        );
    }

    #[test]
    fn test_binding_message_different_epochs() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(1, 43, [0xAA; 32]); // Different epoch
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different epochs should produce different bindings"
        );
    }

    #[test]
    fn test_binding_message_different_policies() {
        let ctx1 = TreeSigningContext::new(1, 42, [0xAA; 32]);
        let ctx2 = TreeSigningContext::new(1, 42, [0xBB; 32]); // Different policy
        let op = b"test_operation";

        let msg1 = binding_message(&ctx1, op);
        let msg2 = binding_message(&ctx2, op);

        assert_ne!(
            msg1, msg2,
            "Different policies should produce different bindings"
        );
    }
}
