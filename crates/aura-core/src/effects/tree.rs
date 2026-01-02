//! Tree Operations Effects for Commitment Tree Operations
//!
//! This module provides the effect interface for tree operations following
//! the algebraic effects pattern. It defines what tree operations can be
//! performed without specifying how they are implemented.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-protocol` (Layer 4)
//! - **Usage**: Commitment tree operations (add/remove leaves, policy changes, snapshots)
//!
//! This is an application effect implemented in orchestration layer by composing
//! infrastructure effects (crypto, storage) with commitment tree logic.
//!
//! ## Architecture
//!
//! - **Effect Traits**: Define operations (this module)
//! - **Handlers**: Implement operations (in aura-protocol)
//! - **Business Logic**: Consumes effects via dependency injection
//!
//! ## Design Principles
//!
//! - **No Business Logic**: Trait contains only interface definitions
//! - **Clean Separation**: Effects don't know about handlers
//! - **Algebraic Composition**: Effects compose via trait bounds

use crate::{AttestedOp, AuraError, Epoch, Hash32, LeafId, LeafNode, NodeIndex, Policy};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub const MAX_TREE_SIGNATURE_SHARE_BYTES: usize = 1024;
pub const MAX_TREE_STATE_BYTES: usize = 262_144;
pub const MAX_TREE_AGGREGATE_SIGNATURE_BYTES: usize = 2048;

// Re-export canonical ProposalId from tree module
pub use crate::tree::ProposalId;

// Snapshot-related types for effect interface (simplified versions)
/// Defines a cut point for snapshotting (effect interface version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cut {
    pub epoch: Epoch,
    pub commitment: Hash32,
    pub cid: Hash32,
}

/// Partial signature share for snapshot approval (effect interface version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Partial {
    pub signature_share: Vec<u8>,
    pub participant_id: crate::types::identifiers::DeviceId,
}

/// Immutable snapshot containing compacted tree state (effect interface version)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub cut: Cut,
    pub tree_state: Vec<u8>, // Serialized TreeState to avoid circular dependency
    pub aggregate_signature: Vec<u8>,
}

/// Tree operations effect interface
///
/// Provides all operations needed for commitment tree management:
/// - State queries (current tree, commitments)
/// - Operation application (append attested ops)
/// - Signature verification (FROST aggregates)
/// - Operation proposal (create TreeOpKind before attestation)
///
/// ## Implementation Requirements
///
/// - All trait methods must be implemented by handlers
/// - Business logic should consume this trait via dependency injection
/// - Never implement this trait in business logic crates
///
/// ## Operation Flow
///
/// 1. Propose operation → `add_leaf()`, `remove_leaf()`, etc. → `TreeOpKind`
/// 2. Run threshold ceremony (separate) → Collect signatures → `AttestedOp`
/// 3. Apply operation → `apply_attested_op()` → Updates tree state
#[async_trait]
pub trait TreeOperationEffects: Send + Sync {
    // ===== State Queries =====

    /// Get the current tree state
    ///
    /// Returns the materialized tree state at the latest epoch. This is
    /// computed on-demand via reduction from the OpLog - it is never stored.
    ///
    /// ## Critical Invariants
    ///
    /// - TreeState is **derived**, never persisted
    /// - Computed via `reduce(oplog)` from OpLog CRDT
    /// - Deterministic across all replicas
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// let state = tree_effects.get_current_state().await?;
    /// println!("Current epoch: {}", state.current_epoch());
    /// println!("Leaves: {}", state.num_leaves());
    /// ```
    async fn get_current_state(&self) -> Result<Vec<u8>, AuraError>; // Serialized TreeState

    /// Get the current root commitment
    ///
    /// Returns the commitment that binds the entire tree structure.
    /// All new operations must reference this as their parent commitment.
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// let commitment = tree_effects.get_current_commitment().await?;
    /// // Use this as parent_commitment in new TreeOp
    /// ```
    async fn get_current_commitment(&self) -> Result<Hash32, AuraError>;

    /// Get the current epoch
    ///
    /// Returns the epoch number of the current tree state. Increments
    /// after `RotateEpoch` operations.
    async fn get_current_epoch(&self) -> Result<Epoch, AuraError>;

    // ===== Operation Application =====

    /// Apply an attested tree operation
    ///
    /// Appends a fully-attested operation to the OpLog. The operation must:
    /// - Have valid aggregate signature
    /// - Have correct parent binding (epoch + commitment)
    /// - Be signed by sufficient threshold
    ///
    /// ## Behavior
    ///
    /// - Stores `AttestedOp` in OpLog CRDT
    /// - Does **NOT** store shares, transcripts, or author identities
    /// - Does **NOT** immediately update TreeState (recomputed on query)
    /// - Returns CID (content identifier) for the operation
    ///
    /// ## Verification Steps
    ///
    /// 1. Verify aggregate signature
    /// 2. Verify parent binding
    /// 3. Append to OpLog
    /// 4. Return CID
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// let attested_op = AttestedOp { /* ... */ };
    /// let cid = tree_effects.apply_attested_op(attested_op).await?;
    /// ```
    async fn apply_attested_op(&self, op: AttestedOp) -> Result<Hash32, AuraError>;

    /// Verify an aggregate signature
    ///
    /// Verifies that an operation's aggregate signature is valid for the
    /// given tree state. This checks that the signature was created by
    /// the threshold policy at the relevant node.
    ///
    /// ## Parameters
    ///
    /// - `op`: The attested operation to verify
    /// - `state`: Serialized tree state at parent epoch (for group key lookup)
    ///
    /// ## Returns
    ///
    /// - `Ok(true)` if signature is valid
    /// - `Ok(false)` if signature is invalid
    /// - `Err(_)` if verification cannot be performed
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// let valid = tree_effects.verify_aggregate_sig(&op, &state).await?;
    /// if !valid {
    ///     return Err(AuraError::crypto_verification_failed("Invalid signature"));
    /// }
    /// ```
    async fn verify_aggregate_sig(
        &self,
        op: &AttestedOp,
        state: &[u8], // Serialized TreeState
    ) -> Result<bool, AuraError>;

    // ===== Operation Proposals =====
    //
    // These methods create TreeOpKind proposals that will later be attested
    // via threshold ceremonies. They do NOT produce AttestedOp directly -
    // that happens after signature collection.

    /// Propose adding a leaf to the tree
    ///
    /// Creates a `TreeOpKind::AddLeaf` proposal. The proposal must then go
    /// through a threshold ceremony to collect signatures before becoming
    /// an `AttestedOp`.
    ///
    /// ## Parameters
    ///
    /// - `leaf`: The leaf node to add (device or guardian)
    /// - `under`: The parent node index to add the leaf under
    ///
    /// ## Returns
    ///
    /// A serialized `TreeOpKind::AddLeaf` variant ready for threshold signing.
    ///
    /// ## Authorization
    ///
    /// Caller must have `CanProposeAddLeaf` capability (checked by handler).
    ///
    /// ## Examples
    ///
    /// ```ignore
    /// let leaf = LeafNode::new_device(/* leaf_id */, /* device_id */, /* public_key */)?;
    /// let op_kind = tree_effects.add_leaf(leaf, NodeIndex(0)).await?;
    /// // Now run threshold ceremony to attest op_kind
    /// ```
    async fn add_leaf(&self, leaf: LeafNode, under: NodeIndex) -> Result<Vec<u8>, AuraError>; // Serialized TreeOpKind

    /// Propose removing a leaf from the tree
    ///
    /// Creates a `TreeOpKind::RemoveLeaf` proposal. Used for:
    /// - Device removal (lost, compromised, etc.)
    /// - Guardian removal (revocation)
    ///
    /// ## Parameters
    ///
    /// - `leaf_id`: The leaf to remove
    /// - `reason`: Reason code (0 = voluntary, 1 = compromised, etc.)
    ///
    /// ## Authorization
    ///
    /// Caller must have `CanProposeRemoveLeaf` capability.
    async fn remove_leaf(&self, leaf_id: LeafId, reason: u8) -> Result<Vec<u8>, AuraError>; // Serialized TreeOpKind

    /// Propose changing a node's policy
    ///
    /// Creates a `TreeOpKind::ChangePolicy` proposal. The new policy must
    /// be stricter-or-equal to the old policy (meet-semilattice property).
    ///
    /// ## Parameters
    ///
    /// - `node`: The node to update
    /// - `new_policy`: The new policy (must be stricter or equal)
    ///
    /// ## Policy Lattice
    ///
    /// Policies form a meet-semilattice where "more restrictive is smaller":
    /// - `Any ≥ Threshold{m,n} ≥ All`
    /// - Can change from `Threshold{2,3}` to `Threshold{3,3}` (stricter)
    /// - Cannot change from `Threshold{3,3}` to `Threshold{2,3}` (weaker)
    ///
    /// ## Authorization
    ///
    /// Caller must have `CanProposeChangePolicy` capability.
    async fn change_policy(
        &self,
        node: NodeIndex,
        new_policy: Policy,
    ) -> Result<Vec<u8>, AuraError>; // Serialized TreeOpKind

    /// Propose rotating the epoch
    ///
    /// Creates a `TreeOpKind::RotateEpoch` proposal. This increments the
    /// epoch counter and invalidates old signing shares.
    ///
    /// ## Parameters
    ///
    /// - `affected`: Hint of affected nodes (for share refresh planning)
    ///
    /// ## Behavior
    ///
    /// - Increments epoch counter
    /// - Old shares become invalid
    /// - New shares must be derived off-chain (DKG ceremony)
    /// - Does NOT record shares in journal
    ///
    /// ## Authorization
    ///
    /// Caller must have `CanProposeRotateEpoch` capability.
    async fn rotate_epoch(&self, affected: Vec<NodeIndex>) -> Result<Vec<u8>, AuraError>; // Serialized TreeOpKind

    // ========================================================================
    // Snapshot Operations (Phase 5.4)
    // ========================================================================

    /// Propose a snapshot at a specific cut point
    ///
    /// Initiates a threshold ceremony to create a snapshot of the tree state.
    /// The snapshot allows pruning OpLog history while preserving merge semantics.
    ///
    /// ## Parameters
    ///
    /// - `cut`: Defines the epoch, commitment, and CID at which to snapshot
    ///
    /// ## Behavior
    ///
    /// - Creates a proposal that requires threshold approval
    /// - Returns a ProposalId for tracking approval progress
    /// - Approval ceremony uses FROST threshold signatures
    ///
    /// ## Authorization
    ///
    /// Caller must have `CanProposeSnapshot` capability.
    async fn propose_snapshot(&self, cut: Cut) -> Result<ProposalId, AuraError>;

    /// Add partial approval to a snapshot proposal
    ///
    /// Provides a FROST signature share approving the snapshot proposal.
    ///
    /// ## Parameters
    ///
    /// - `proposal_id`: The proposal being approved
    ///
    /// ## Returns
    ///
    /// A `Partial` containing the caller's signature share.
    ///
    /// ## Authorization
    ///
    /// Caller must have signing authority in the current threshold policy.
    async fn approve_snapshot(&self, proposal_id: ProposalId) -> Result<Partial, AuraError>;

    /// Finalize a snapshot after threshold approval
    ///
    /// Aggregates partial signatures and creates the snapshot if threshold is met.
    ///
    /// ## Parameters
    ///
    /// - `proposal_id`: The proposal to finalize
    ///
    /// ## Returns
    ///
    /// A `Snapshot` containing the compacted tree state.
    ///
    /// ## Behavior
    ///
    /// - Verifies threshold of partial signatures collected
    /// - Aggregates signatures using FROST
    /// - Creates immutable snapshot
    /// - Does NOT automatically prune OpLog (separate operation)
    ///
    /// ## Authorization
    ///
    /// Any device can attempt finalization; verification is cryptographic.
    async fn finalize_snapshot(&self, proposal_id: ProposalId) -> Result<Snapshot, AuraError>;

    /// Apply a snapshot to local state
    ///
    /// Installs a snapshot, allowing operations after the cut point to be
    /// applied without the full OpLog history.
    ///
    /// ## Parameters
    ///
    /// - `snapshot`: The snapshot to apply
    ///
    /// ## Behavior
    ///
    /// - Verifies snapshot signature and validity
    /// - Replaces local state up to snapshot epoch
    /// - Retains operations after snapshot cut
    /// - Updates current commitment and epoch
    ///
    /// ## Compatibility
    ///
    /// Old clients that don't understand snapshots can refuse to apply them
    /// but MUST continue to merge operations (forward compatibility).
    ///
    /// ## Authorization
    ///
    /// Snapshot must have valid threshold signature; no additional auth required.
    async fn apply_snapshot(&self, snapshot: &Snapshot) -> Result<(), AuraError>;
}
