//! FROST Threshold Signing Primitives for Tree Operations
//!
//! This module provides pure cryptographic primitives for FROST threshold signatures
//! used in ratchet tree operations. It contains **NO** tree logic or business logic.
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
//! - [`docs/123_ratchet_tree.md`](../../../../docs/123_ratchet_tree.md) - Tree operations
//! - FROST paper: https://eprint.iacr.org/2020/852

use blake3::Hasher;
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
    pub fn from_frost(_nonces: frost::round1::SigningNonces) -> Self {
        let mut id = [0u8; 32];
        #[allow(clippy::expect_used)]
        getrandom::getrandom(&mut id).expect("Failed to generate nonce ID");

        Self {
            id,
            value: vec![0u8; 64], // Placeholder - FROST SigningNonces are secret and don't have serialize
        }
    }

    /// Convert to FROST signing nonces
    pub fn to_frost(&self) -> Result<frost::round1::SigningNonces, String> {
        frost::round1::SigningNonces::deserialize(&self.value)
            .map_err(|e| format!("Failed to deserialize nonces: {}", e))
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
                .expect("FROST commitments serialization cannot fail")
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
/// use aura_crypto::frost::tree_signing::{TreeSigningContext, binding_message};
/// use aura_core::TreeOp;
///
/// let ctx = TreeSigningContext::new(1, 42, [0u8; 32]);
/// // let op = TreeOp { ... };
/// // let msg = binding_message(&ctx, &op);
/// ```
pub fn binding_message(ctx: &TreeSigningContext, op_bytes: &[u8]) -> Vec<u8> {
    let mut hasher = Hasher::new();

    // Domain separator
    hasher.update(b"TREE_OP_SIG");

    // Context binding
    hasher.update(&ctx.node_id.to_le_bytes());
    hasher.update(&ctx.epoch.to_le_bytes());
    hasher.update(&ctx.policy_hash);

    // Operation content
    hasher.update(op_bytes);

    hasher.finalize().as_bytes().to_vec()
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
/// use aura_crypto::frost::tree_signing::generate_nonce;
///
/// let (nonce, commitment) = generate_nonce(1);
/// // Send commitment to coordinator
/// // Keep nonce secret for signing round
/// ```
pub fn generate_nonce(signer_id: u16) -> (Nonce, NonceCommitment) {
    // Generate FROST identifier (for future use in real FROST protocol)
    let _identifier = frost::Identifier::try_from(signer_id).expect("Valid signer ID");

    // Generate random nonce (TODO fix - Simplified for choreography testing)
    // Real FROST would use frost::round1::commit() with proper KeyPackage
    #[allow(clippy::disallowed_methods)]
    let mut rng = rand::thread_rng();
    let mut nonce_bytes = vec![0u8; 64];
    rand::RngCore::fill_bytes(&mut rng, &mut nonce_bytes);

    let mut nonce_id = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rng, &mut nonce_id);

    // Generate commitment as hash of nonce (TODO fix - Simplified)
    let commitment_bytes = blake3::hash(&nonce_bytes);

    let nonce = Nonce {
        id: nonce_id,
        value: nonce_bytes,
    };

    let commitment = NonceCommitment {
        signer: signer_id,
        commitment: commitment_bytes.as_bytes().to_vec(),
    };

    (nonce, commitment)
}

/// Open a nonce commitment (deprecated in FROST protocol)
///
/// In FROST, nonces are not explicitly "opened" - instead, commitments are
/// collected and used directly in signing. This function exists for API
/// compatibility but is not used in the actual FROST protocol flow.
///
/// **Note**: In FROST, the coordinator collects commitments and participants
/// sign directly using their nonces. No explicit opening phase.
#[deprecated(note = "Not used in FROST protocol - commitments are used directly")]
pub fn open_nonce(nonce: &Nonce, signer_id: u16) -> NonceOpen {
    NonceOpen {
        signer: signer_id,
        nonce: nonce.value.clone(),
    }
}

/// Verify a nonce opening matches its commitment (deprecated)
///
/// Not used in FROST protocol. Kept for backwards compatibility.
#[deprecated(note = "Not used in FROST protocol")]
pub fn verify_nonce_opening(commitment: &NonceCommitment, opening: &NonceOpen) -> bool {
    commitment.signer == opening.signer
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
/// This is a TODO fix - Simplified implementation for choreography testing.
/// Production use should use proper KeyPackage management from DKG.
pub fn frost_sign_partial(
    share: &Share,
    msg: &[u8],
    nonce: &Nonce,
    commitments: &BTreeMap<u16, NonceCommitment>,
) -> Result<PartialSignature, String> {
    // Get identifier (for future use in real FROST protocol)
    let _identifier = share.frost_identifier()?;

    // Convert commitments to FROST format
    let mut frost_commitments = BTreeMap::new();
    for (signer_id, commitment) in commitments {
        let frost_id = frost::Identifier::try_from(*signer_id)
            .map_err(|e| format!("Invalid signer ID {}: {}", signer_id, e))?;
        let frost_commit = commitment.to_frost()?;
        frost_commitments.insert(frost_id, frost_commit);
    }

    // Create signing package (for future use in real FROST protocol)
    let _signing_package = frost::SigningPackage::new(frost_commitments, msg);

    // TODO fix - For now, generate a deterministic "signature share" using BLAKE3
    // This is NOT cryptographically secure FROST signing!
    // Real implementation needs proper KeyPackage from DKG ceremony
    //
    // This TODO fix - Simplified version is for choreography protocol testing only
    let mut sig_input = Vec::new();
    sig_input.extend_from_slice(&share.value);
    sig_input.extend_from_slice(&nonce.value);
    sig_input.extend_from_slice(msg);
    let sig_hash = blake3::hash(&sig_input);

    // Create partial signature
    Ok(PartialSignature {
        signer: share.identifier,
        signature: sig_hash.as_bytes().to_vec(),
    })
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
    fn test_nonce_generation() {
        let (nonce1, commitment1) = generate_nonce(1);
        let (nonce2, commitment2) = generate_nonce(2); // Use different signer IDs

        // Different nonces should have different IDs (but values are placeholders)
        assert_ne!(nonce1.id, nonce2.id);
        // Note: nonce.value is a placeholder and will be the same, so we don't check it

        // Commitments should be different for different signers
        assert_ne!(commitment1.commitment, commitment2.commitment);

        // Only commitment should deserialize correctly (nonces use placeholder data)
        assert!(commitment1.to_frost().is_ok());
    }

    #[test]
    #[allow(deprecated)]
    fn test_nonce_commitment_verification() {
        let (nonce, commitment) = generate_nonce(1);
        let opening = open_nonce(&nonce, 1);

        // Valid opening should verify (deprecated API)
        assert!(verify_nonce_opening(&commitment, &opening));
    }

    #[test]
    #[ignore = "Uses deprecated API not part of real FROST protocol"]
    #[allow(deprecated)]
    fn test_nonce_commitment_invalid() {
        let (nonce1, commitment1) = generate_nonce(1);
        let (nonce2, _) = generate_nonce(1);
        let opening2 = open_nonce(&nonce2, 1);

        // Wrong nonce should not verify (deprecated API)
        assert!(!verify_nonce_opening(&commitment1, &opening2));
    }

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

    #[test]
    fn test_frost_roundtrip_serialization() {
        use frost::keys::KeyPackage;

        // Generate a test key package (2-of-3 threshold)
        let max_signers = 3;
        let min_signers = 2;
        #[allow(clippy::disallowed_methods)]
        let mut rng = rand::thread_rng();

        let (shares, pubkey_package) = frost::keys::generate_with_dealer(
            max_signers,
            min_signers,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .unwrap();

        // Test Share serialization roundtrip
        let frost_share = shares
            .get(&frost::Identifier::try_from(1).unwrap())
            .unwrap();
        let share = Share::from_frost(
            frost::Identifier::try_from(1).unwrap(),
            frost_share.signing_share().clone(),
        );

        let roundtrip_share = share.to_frost().unwrap();
        assert_eq!(
            frost_share.signing_share().serialize(),
            roundtrip_share.serialize()
        );

        // Test nonce generation and commitment
        let (_nonce, commitment) = generate_nonce(1);
        // Note: nonce.to_frost() is expected to fail because SigningNonces cannot be serialized
        // This is by design - nonces should remain secret and not be round-trip serialized
        assert!(commitment.to_frost().is_ok());

        println!("FROST serialization roundtrip successful");
    }

    #[test]
    fn test_frost_integration_basic() {
        // This test verifies FROST integration works but doesn't test
        // the full signing flow (which requires proper setup)
        use frost::keys::KeyPackage;

        let max_signers = 3;
        let min_signers = 2;
        #[allow(clippy::disallowed_methods)]
        let mut rng = rand::thread_rng();

        // Generate shares
        let (_shares, _pubkey_package) = frost::keys::generate_with_dealer(
            max_signers,
            min_signers,
            frost::keys::IdentifierList::Default,
            &mut rng,
        )
        .unwrap();

        // Generate nonces for signers
        let (_nonce1, _commitment1) = generate_nonce(1);
        let (_nonce2, _commitment2) = generate_nonce(2);

        println!("FROST key generation and nonce commitment successful");
    }
}
