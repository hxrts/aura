//! Tree Operation Verification
//!
//! Provides cryptographic verification of attested tree operations against
//! signing keys stored in TreeState. This module implements the two-phase
//! verification model:
//!
//! - **Verification**: Cryptographic signature check against provided key
//! - **Check**: Full verification plus consistency check against TreeState
//!
//! ## Security Model
//!
//! The binding message includes the group public key to prevent signature
//! reuse across different signing groups. This ensures an attacker cannot
//! substitute a different key they control.
//!
//! ## References
//!
//! - `docs/101_accounts_and_commitment_tree.md` - Tree structure
//! - `docs/104_consensus.md` - Threshold signing

use super::{AttestedOp, BranchSigningKey, Epoch, NodeIndex, Policy, TreeHash32};
use crate::crypto::{hash, tree_signing};
use thiserror::Error;

/// Errors that can occur during tree operation verification.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VerificationError {
    /// Missing signing key for the target node
    #[error("Missing signing key for node {0}")]
    MissingSigningKey(NodeIndex),

    /// Insufficient signers for the policy threshold
    #[error("Insufficient signers: required {required}, got {provided}")]
    InsufficientSigners {
        /// Minimum signers required by policy
        required: u16,
        /// Actual number of signers
        provided: u16,
    },

    /// Signature verification failed
    #[error("Signature verification failed: {0}")]
    SignatureFailed(String),

    /// Invalid signature format
    #[error("Invalid signature format: {0}")]
    InvalidSignature(String),

    /// Operation epoch mismatch
    #[error("Epoch mismatch: operation references epoch {op_epoch}, current is {current_epoch}")]
    EpochMismatch {
        /// Epoch referenced in the operation
        op_epoch: Epoch,
        /// Current epoch in TreeState
        current_epoch: Epoch,
    },

    /// Parent commitment mismatch
    #[error("Parent commitment mismatch")]
    ParentCommitmentMismatch,
}

/// Errors that occur during check (verification + consistency).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// Underlying verification failed
    #[error("Verification failed: {0}")]
    VerificationFailed(#[from] VerificationError),

    /// Signing key epoch doesn't match current state
    #[error("Signing key epoch mismatch: key from epoch {key_epoch}, current is {current_epoch}")]
    KeyEpochMismatch {
        /// Epoch when the signing key was established
        key_epoch: Epoch,
        /// Current epoch in TreeState
        current_epoch: Epoch,
    },

    /// Node not found in TreeState
    #[error("Node {0} not found in tree state")]
    NodeNotFound(NodeIndex),

    /// Policy not found for node
    #[error("Policy not found for node {0}")]
    PolicyNotFound(NodeIndex),
}

/// Compute the binding message for an attested operation.
///
/// The binding message includes:
/// - Domain separator ("TREE_OP_VERIFY")
/// - Parent epoch and commitment (replay prevention)
/// - Protocol version
/// - Current epoch
/// - Group public key (prevents key substitution attacks)
/// - Serialized operation content
///
/// ## Security Property
///
/// Including the group public key in the binding ensures that signatures
/// are bound to a specific signing group. An attacker cannot reuse a
/// signature with a different key they control.
pub fn compute_binding_message(
    attested: &AttestedOp,
    current_epoch: Epoch,
    group_public_key: &[u8; 32],
) -> Vec<u8> {
    let mut h = hash::hasher();

    // Domain separator
    h.update(b"TREE_OP_VERIFY");

    // Parent binding (replay prevention)
    h.update(&u64::from(attested.op.parent_epoch).to_le_bytes());
    h.update(&attested.op.parent_commitment);
    h.update(&attested.op.version.to_le_bytes());

    // Current epoch
    h.update(&u64::from(current_epoch).to_le_bytes());

    // Group public key (prevents key substitution)
    h.update(group_public_key);

    // Operation content
    let op_bytes = serialize_op_for_binding(&attested.op.op);
    h.update(&op_bytes);

    h.finalize().to_vec()
}

/// Serialize a tree operation for binding message computation.
fn serialize_op_for_binding(op: &super::TreeOpKind) -> Vec<u8> {
    use super::TreeOpKind;

    let mut buffer = Vec::new();
    match op {
        TreeOpKind::AddLeaf { leaf, under } => {
            buffer.extend_from_slice(b"AddLeaf");
            buffer.extend_from_slice(&leaf.leaf_id.0.to_le_bytes());
            buffer.extend_from_slice(&under.0.to_le_bytes());
            buffer.extend_from_slice(&leaf.public_key);
        }
        TreeOpKind::RemoveLeaf { leaf, reason } => {
            buffer.extend_from_slice(b"RemoveLeaf");
            buffer.extend_from_slice(&leaf.0.to_le_bytes());
            buffer.push(*reason);
        }
        TreeOpKind::ChangePolicy { node, new_policy } => {
            buffer.extend_from_slice(b"ChangePolicy");
            buffer.extend_from_slice(&node.0.to_le_bytes());
            // Include policy in binding
            buffer.extend_from_slice(&super::commitment::policy_hash(new_policy));
        }
        TreeOpKind::RotateEpoch { affected } => {
            buffer.extend_from_slice(b"RotateEpoch");
            buffer.extend_from_slice(&(affected.len() as u32).to_le_bytes());
            for node in affected {
                buffer.extend_from_slice(&node.0.to_le_bytes());
            }
        }
    }
    buffer
}

/// Verify an attested operation against a provided signing key.
///
/// This performs cryptographic verification only:
/// 1. Check signer count against threshold
/// 2. Compute binding message (includes group key)
/// 3. Verify aggregate signature
///
/// This function does NOT check consistency with TreeState. Use `check_attested_op`
/// for full verification including state consistency.
///
/// ## Arguments
///
/// * `attested` - The attested operation to verify
/// * `signing_key` - The branch signing key to verify against
/// * `threshold` - Required number of signers (from Policy::required_signers)
/// * `current_epoch` - Current epoch for binding message
///
/// ## Returns
///
/// `Ok(())` if verification succeeds, `Err(VerificationError)` otherwise.
pub fn verify_attested_op(
    attested: &AttestedOp,
    signing_key: &BranchSigningKey,
    threshold: u16,
    current_epoch: Epoch,
) -> Result<(), VerificationError> {
    // 1. Check signer count against threshold
    if attested.signer_count < threshold {
        return Err(VerificationError::InsufficientSigners {
            required: threshold,
            provided: attested.signer_count,
        });
    }

    // 2. Compute binding message (includes group key for security)
    let binding = compute_binding_message(attested, current_epoch, signing_key.group_key());

    // 3. Verify aggregate signature using FROST
    verify_frost_signature(signing_key.group_key(), &binding, &attested.agg_sig)
}

/// Verify a FROST aggregate signature.
fn verify_frost_signature(
    group_public_key: &[u8; 32],
    message: &[u8],
    signature: &[u8],
) -> Result<(), VerificationError> {
    // Deserialize the group verifying key
    let verifying_key = frost_ed25519::VerifyingKey::deserialize(*group_public_key)
        .map_err(|e| VerificationError::InvalidSignature(format!("Invalid group key: {e}")))?;

    // Use the tree_signing module's verification
    tree_signing::frost_verify_aggregate(&verifying_key, message, signature)
        .map_err(VerificationError::SignatureFailed)
}

/// Signing witness extracted from TreeState for verification.
///
/// Contains all information needed to verify an operation against a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningWitness {
    /// Group public key for signature verification
    pub group_public_key: [u8; 32],
    /// Required number of signers
    pub threshold: u16,
    /// Epoch when the signing key was established
    pub key_epoch: Epoch,
}

impl SigningWitness {
    /// Create a new signing witness
    pub fn new(group_public_key: [u8; 32], threshold: u16, key_epoch: Epoch) -> Self {
        Self {
            group_public_key,
            threshold,
            key_epoch,
        }
    }

    /// Create from a BranchSigningKey and threshold
    pub fn from_signing_key(key: &BranchSigningKey, threshold: u16) -> Self {
        Self {
            group_public_key: key.group_public_key,
            threshold,
            key_epoch: key.key_epoch,
        }
    }
}

/// Context needed for check_attested_op.
///
/// This trait abstracts over TreeState to allow verification without
/// a direct dependency on the journal crate.
pub trait TreeStateView {
    /// Get the signing key for a branch node
    fn get_signing_key(&self, node: NodeIndex) -> Option<&BranchSigningKey>;

    /// Get the policy for a node
    fn get_policy(&self, node: NodeIndex) -> Option<&Policy>;

    /// Get the number of children for a node (for Policy::All threshold)
    fn child_count(&self, node: NodeIndex) -> usize;

    /// Get the current epoch
    fn current_epoch(&self) -> Epoch;

    /// Get the current root commitment
    fn current_commitment(&self) -> TreeHash32;
}

/// Check an attested operation against TreeState.
///
/// This performs full verification plus consistency checks:
/// 1. Verify the operation (cryptographic check)
/// 2. Check signing key exists for target node
/// 3. Check operation epoch matches state
/// 4. Check parent commitment matches state
///
/// ## Arguments
///
/// * `state` - TreeState view to check against
/// * `attested` - The attested operation to check
/// * `target_node` - The node this operation targets
///
/// ## Returns
///
/// `Ok(())` if check passes, `Err(CheckError)` otherwise.
pub fn check_attested_op<S: TreeStateView>(
    state: &S,
    attested: &AttestedOp,
    target_node: NodeIndex,
) -> Result<(), CheckError> {
    // 1. Get signing key from state
    let signing_key = state
        .get_signing_key(target_node)
        .ok_or(CheckError::VerificationFailed(
            VerificationError::MissingSigningKey(target_node),
        ))?;

    // 2. Get policy and compute threshold
    let policy = state
        .get_policy(target_node)
        .ok_or(CheckError::PolicyNotFound(target_node))?;
    let child_count = state.child_count(target_node);
    let threshold = policy.required_signers(child_count);

    // 3. Get current epoch
    let current_epoch = state.current_epoch();

    // 4. Verify the operation cryptographically
    verify_attested_op(attested, signing_key, threshold, current_epoch)?;

    // 5. Check parent epoch (must reference current or recent epoch)
    // Note: We allow parent_epoch == current_epoch for operations in the current epoch
    if attested.op.parent_epoch > current_epoch {
        return Err(CheckError::VerificationFailed(
            VerificationError::EpochMismatch {
                op_epoch: attested.op.parent_epoch,
                current_epoch,
            },
        ));
    }

    // 6. Check parent commitment (for current epoch operations)
    if attested.op.parent_epoch == current_epoch {
        let current_commitment = state.current_commitment();
        if attested.op.parent_commitment != current_commitment {
            return Err(CheckError::VerificationFailed(
                VerificationError::ParentCommitmentMismatch,
            ));
        }
    }

    Ok(())
}

/// Extract the target node from a TreeOpKind.
///
/// Different operations target different nodes:
/// - AddLeaf: targets the parent branch (under)
/// - RemoveLeaf: targets the leaf's parent branch
/// - ChangePolicy: targets the specified node
/// - RotateEpoch: targets the root (or first affected node)
pub fn extract_target_node(op: &super::TreeOpKind) -> Option<NodeIndex> {
    use super::TreeOpKind;

    match op {
        TreeOpKind::AddLeaf { under, .. } => Some(*under),
        TreeOpKind::RemoveLeaf { .. } => None, // Need TreeState to find parent
        TreeOpKind::ChangePolicy { node, .. } => Some(*node),
        TreeOpKind::RotateEpoch { affected } => affected.first().copied(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{LeafId, LeafNode, LeafRole, TreeOp, TreeOpKind};

    fn test_signing_key() -> BranchSigningKey {
        BranchSigningKey::new([0xAA; 32], Epoch::new(1))
    }

    fn test_attested_op() -> AttestedOp {
        let leaf = LeafNode::new_device(
            LeafId(1),
            crate::DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            vec![0u8; 32],
        )
        .expect("leaf should build");

        AttestedOp {
            op: TreeOp {
                parent_epoch: Epoch::new(1),
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf,
                    under: NodeIndex(0),
                },
                version: 1,
            },
            agg_sig: vec![0u8; 64],
            signer_count: 2,
        }
    }

    #[test]
    fn test_binding_message_deterministic() {
        let attested = test_attested_op();
        let group_key = [0xAA; 32];

        let msg1 = compute_binding_message(&attested, Epoch::new(1), &group_key);
        let msg2 = compute_binding_message(&attested, Epoch::new(1), &group_key);

        assert_eq!(msg1, msg2, "Binding message should be deterministic");
    }

    #[test]
    fn test_binding_message_changes_with_key() {
        let attested = test_attested_op();
        let key1 = [0xAA; 32];
        let key2 = [0xBB; 32];

        let msg1 = compute_binding_message(&attested, Epoch::new(1), &key1);
        let msg2 = compute_binding_message(&attested, Epoch::new(1), &key2);

        assert_ne!(
            msg1, msg2,
            "Different keys should produce different bindings"
        );
    }

    #[test]
    fn test_binding_message_changes_with_epoch() {
        let attested = test_attested_op();
        let group_key = [0xAA; 32];

        let msg1 = compute_binding_message(&attested, Epoch::new(1), &group_key);
        let msg2 = compute_binding_message(&attested, Epoch::new(2), &group_key);

        assert_ne!(
            msg1, msg2,
            "Different epochs should produce different bindings"
        );
    }

    #[test]
    fn test_insufficient_signers() {
        let attested = AttestedOp {
            op: test_attested_op().op,
            agg_sig: vec![0u8; 64],
            signer_count: 1, // Only 1 signer
        };

        let key = test_signing_key();
        let result = verify_attested_op(&attested, &key, 2, Epoch::new(1)); // Requires 2

        assert!(matches!(
            result,
            Err(VerificationError::InsufficientSigners {
                required: 2,
                provided: 1
            })
        ));
    }

    #[test]
    fn test_signing_witness_from_key() {
        let key = BranchSigningKey::new([0xCC; 32], Epoch::new(5));
        let witness = SigningWitness::from_signing_key(&key, 2);

        assert_eq!(witness.group_public_key, [0xCC; 32]);
        assert_eq!(witness.threshold, 2);
        assert_eq!(witness.key_epoch, Epoch::new(5));
    }

    #[test]
    fn test_extract_target_node_add_leaf() {
        let leaf = LeafNode::new_device(
            LeafId(1),
            crate::DeviceId(uuid::Uuid::from_bytes([0u8; 16])),
            vec![],
        )
        .expect("leaf should build");

        let op = TreeOpKind::AddLeaf {
            leaf,
            under: NodeIndex(5),
        };

        assert_eq!(extract_target_node(&op), Some(NodeIndex(5)));
    }

    #[test]
    fn test_extract_target_node_change_policy() {
        let op = TreeOpKind::ChangePolicy {
            node: NodeIndex(3),
            new_policy: Policy::All,
        };

        assert_eq!(extract_target_node(&op), Some(NodeIndex(3)));
    }

    #[test]
    fn test_extract_target_node_rotate_epoch() {
        let op = TreeOpKind::RotateEpoch {
            affected: vec![NodeIndex(0), NodeIndex(1)],
        };

        assert_eq!(extract_target_node(&op), Some(NodeIndex(0)));
    }
}
