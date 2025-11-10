//! Journal Effects for Aura
//!
//! This module provides the effect interface for journal operations based on the
//! ratchet tree and CRDT ledger architecture. It provides access to:
//! - Tree state queries
//! - TreeOp operations
//! - Intent pool management
//! - Capability validation
//!
//! ## Architecture
//!
//! The journal system separates:
//! - **Authentication**: Tree membership (who you are)
//! - **Authorization**: Capabilities (what you can do)
//!
//! All operations are performed through this effects interface, following the
//! algebraic effects pattern defined in docs/002_system_architecture.md

use aura_core::{
    identifiers::{DeviceId, GuardianId},
    AuraError,
};
// TODO: These types should be defined in aura-core when implementing the actual journal functionality
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Stub types for compilation - should be moved to aura-core when implementing journal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityRef(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentStatus(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalMap(pub HashMap<String, Vec<u8>>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalStats {
    pub entry_count: u64,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeOpRecord(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commitment(pub Vec<u8>);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeafIndex(pub u32);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RatchetTree(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epoch(pub u64);
use async_trait::async_trait;

/// Journal effects interface
///
/// Provides all operations needed for journal ledger access, tree queries,
/// intent pool management, and capability validation.
///
/// ## Implementation Requirements
///
/// - All trait methods must be implemented in `aura-protocol/src/handlers/journal/`
/// - Business logic should consume this trait via dependency injection
/// - Never implement this trait in business logic crates
///
/// See: docs/002_system_architecture.md for architectural guidelines
#[async_trait]
pub trait JournalEffects: Send + Sync {
    // ===== Journal State Queries =====

    /// Get the complete journal map state
    async fn get_journal_state(&self) -> Result<JournalMap, AuraError>;

    /// Get the current tree state (latest epoch)
    async fn get_current_tree(&self) -> Result<RatchetTree, AuraError>;

    /// Get the tree state at a specific epoch
    async fn get_tree_at_epoch(&self, epoch: Epoch) -> Result<RatchetTree, AuraError>;

    /// Get the current root commitment
    async fn get_current_commitment(&self) -> Result<Commitment, AuraError>;

    /// Get the latest epoch number
    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, AuraError>;

    // ===== TreeOp Operations =====

    /// Append a tree operation to the journal
    ///
    /// This is the authoritative write operation that records a completed TreeSession.
    /// The TreeOp must include a valid threshold signature.
    async fn append_tree_op(&self, op: TreeOpRecord) -> Result<(), AuraError>;

    /// Get a tree operation by epoch
    async fn get_tree_op(&self, epoch: Epoch) -> Result<Option<TreeOpRecord>, AuraError>;

    /// List all tree operations in epoch order
    async fn list_tree_ops(&self) -> Result<Vec<TreeOpRecord>, AuraError>;

    // ===== Intent Pool Operations =====

    /// Submit an intent to the pool
    ///
    /// Intents use observed-remove set semantics for high availability.
    /// Any device can submit an intent while offline; convergence happens via gossip.
    async fn submit_intent(&self, intent: Intent) -> Result<IntentId, AuraError>;

    /// Get an intent by ID
    async fn get_intent(&self, intent_id: IntentId) -> Result<Option<Intent>, AuraError>;

    /// Get the status of an intent
    async fn get_intent_status(&self, intent_id: IntentId) -> Result<IntentStatus, AuraError>;

    /// List all pending intents
    async fn list_pending_intents(&self) -> Result<Vec<Intent>, AuraError>;

    /// Tombstone an intent (mark as completed)
    ///
    /// Called after a TreeSession successfully executes an intent.
    async fn tombstone_intent(&self, intent_id: IntentId) -> Result<(), AuraError>;

    /// Prune stale intents based on snapshot commitment
    async fn prune_stale_intents(&self, current_commitment: Commitment)
        -> Result<usize, AuraError>;

    // ===== Capability Operations =====

    /// Validate a capability reference
    ///
    /// Checks:
    /// - Signature is valid
    /// - Not expired
    /// - Not revoked (no tombstone)
    /// - Issuer has authority (according to tree policy)
    async fn validate_capability(&self, capability: &CapabilityRef) -> Result<bool, AuraError>;

    /// Check if a capability has been revoked
    async fn is_capability_revoked(&self, capability_id: &CapabilityId) -> Result<bool, AuraError>;

    /// List capabilities issued in a specific TreeOp
    async fn list_capabilities_in_op(&self, epoch: Epoch) -> Result<Vec<CapabilityRef>, AuraError>;

    // ===== CRDT Operations =====

    /// Merge another journal map into the local state
    ///
    /// Implements the CRDT join-semilattice merge operation.
    /// Used for anti-entropy and gossip synchronization.
    async fn merge_journal_state(&self, other: JournalMap) -> Result<(), AuraError>;

    /// Get journal statistics
    async fn get_journal_stats(&self) -> Result<JournalStats, AuraError>;

    // ===== Tree Membership Queries =====

    /// Check if a device is currently in the tree
    async fn is_device_member(&self, device_id: DeviceId) -> Result<bool, AuraError>;

    /// Get the leaf index for a device (if it exists)
    async fn get_device_leaf_index(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<LeafIndex>, AuraError>;

    /// List all devices in the current tree
    async fn list_devices(&self) -> Result<Vec<DeviceId>, AuraError>;

    /// List all guardians in the current tree
    async fn list_guardians(&self) -> Result<Vec<GuardianId>, AuraError>;

    // ===== New Ratchet Tree Operations (Phase 2.1f) =====

    /// Append an attested tree operation to the OpLog
    ///
    /// This is the **authoritative write** for tree operations. The operation must:
    /// - Include a valid FROST aggregate signature
    /// - Have correct parent binding (epoch, commitment)
    /// - Be attested by sufficient signers per policy
    ///
    /// ## Behavior (from docs/123_ratchet_tree.md):
    ///
    /// - Stores `AttestedOp` in OpLog CRDT (OR-set)
    /// - Does **NOT** store shares, transcripts, or author identities
    /// - Does **NOT** store TreeState (derived via reduction)
    /// - Returns CID (content identifier) for the operation
    ///
    /// ## CRITICAL Invariants:
    ///
    /// - Journal stores **ONLY** attested operations
    /// - TreeState is **NEVER** persisted (computed on-demand via reduce())
    /// - OpLog is append-only (join-semilattice)
    async fn append_attested_tree_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError>;

    /// Get the current materialized tree state
    ///
    /// This computes TreeState on-demand by reducing the OpLog.
    /// TreeState is **NEVER** stored directly - always derived.
    ///
    /// ## Reduction Process:
    ///
    /// 1. Fetch all operations from OpLog
    /// 2. Build parent-linked DAG
    /// 3. Topologically sort with H(op) tie-breaker
    /// 4. Apply operations in order
    /// 5. Return materialized TreeState
    ///
    /// ## Performance:
    ///
    /// - May cache TreeState for performance (invalidate on append)
    /// - Cache is transparent to callers (implementation detail)
    async fn get_tree_state(&self) -> Result<aura_journal::ratchet_tree::TreeState, AuraError>;

    /// Get the OpLog (all attested operations)
    ///
    /// Returns the complete OpLog CRDT. This is the **source of truth**
    /// for all tree operations.
    ///
    /// Used for:
    /// - Anti-entropy synchronization
    /// - Manual reduction/inspection
    /// - Snapshot creation
    async fn get_op_log(&self) -> Result<aura_journal::semilattice::OpLog, AuraError>;

    /// Merge a remote OpLog into local state
    ///
    /// Implements CRDT join-semilattice merge for anti-entropy.
    /// Used during peer synchronization to converge OpLog state.
    ///
    /// ## Semantics (OR-set):
    ///
    /// - Union of all operations from both logs
    /// - Deduplication by CID (Hash32)
    /// - Deterministic convergence
    async fn merge_op_log(&self, remote: aura_journal::semilattice::OpLog)
        -> Result<(), AuraError>;

    /// Get an attested operation by its CID
    ///
    /// Retrieves a specific operation from the OpLog by content identifier.
    async fn get_attested_op(
        &self,
        cid: &aura_core::Hash32,
    ) -> Result<Option<aura_core::AttestedOp>, AuraError>;

    /// List all attested operations (unordered)
    ///
    /// Returns all operations in the OpLog. Order is not guaranteed.
    /// For ordered traversal, use `get_tree_state()` which applies reduction.
    async fn list_attested_ops(&self) -> Result<Vec<aura_core::AttestedOp>, AuraError>;
}
