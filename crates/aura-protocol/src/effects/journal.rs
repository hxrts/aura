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
//! algebraic effects pattern defined in docs/400_effect_system.md

use aura_types::identifiers::DeviceId;
use aura_journal::ledger::{CapabilityId, CapabilityRef, Intent, IntentId, IntentStatus, JournalError, JournalMap, JournalStats, TreeOpRecord};
use aura_journal::tree::state::Epoch;
use aura_journal::tree::{Commitment, LeafIndex, RatchetTree};
use aura_types::ProtocolError;
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
/// See: docs/400_effect_system.md for architectural guidelines
#[async_trait]
pub trait JournalEffects: Send + Sync {
    // ===== Journal State Queries =====

    /// Get the complete journal map state
    async fn get_journal_state(&self) -> Result<JournalMap, JournalError>;

    /// Get the current tree state (latest epoch)
    async fn get_current_tree(&self) -> Result<RatchetTree, JournalError>;

    /// Get the tree state at a specific epoch
    async fn get_tree_at_epoch(&self, epoch: Epoch) -> Result<RatchetTree, JournalError>;

    /// Get the current root commitment
    async fn get_current_commitment(&self) -> Result<Commitment, JournalError>;

    /// Get the latest epoch number
    async fn get_latest_epoch(&self) -> Result<Option<Epoch>, JournalError>;

    // ===== TreeOp Operations =====

    /// Append a tree operation to the journal
    ///
    /// This is the authoritative write operation that records a completed TreeSession.
    /// The TreeOp must include a valid threshold signature.
    async fn append_tree_op(&self, op: TreeOpRecord) -> Result<(), JournalError>;

    /// Get a tree operation by epoch
    async fn get_tree_op(&self, epoch: Epoch) -> Result<Option<TreeOpRecord>, JournalError>;

    /// List all tree operations in epoch order
    async fn list_tree_ops(&self) -> Result<Vec<TreeOpRecord>, JournalError>;

    // ===== Intent Pool Operations =====

    /// Submit an intent to the pool
    ///
    /// Intents use observed-remove set semantics for high availability.
    /// Any device can submit an intent while offline; convergence happens via gossip.
    async fn submit_intent(&self, intent: Intent) -> Result<IntentId, JournalError>;

    /// Get an intent by ID
    async fn get_intent(&self, intent_id: IntentId) -> Result<Option<Intent>, JournalError>;

    /// Get the status of an intent
    async fn get_intent_status(&self, intent_id: IntentId) -> Result<IntentStatus, JournalError>;

    /// List all pending intents
    async fn list_pending_intents(&self) -> Result<Vec<Intent>, JournalError>;

    /// Tombstone an intent (mark as completed)
    ///
    /// Called after a TreeSession successfully executes an intent.
    async fn tombstone_intent(&self, intent_id: IntentId) -> Result<(), JournalError>;

    /// Prune stale intents based on snapshot commitment
    async fn prune_stale_intents(
        &self,
        current_commitment: Commitment,
    ) -> Result<usize, JournalError>;

    // ===== Capability Operations =====

    /// Validate a capability reference
    ///
    /// Checks:
    /// - Signature is valid
    /// - Not expired
    /// - Not revoked (no tombstone)
    /// - Issuer has authority (according to tree policy)
    async fn validate_capability(&self, capability: &CapabilityRef) -> Result<bool, JournalError>;

    /// Check if a capability has been revoked
    async fn is_capability_revoked(
        &self,
        capability_id: &CapabilityId,
    ) -> Result<bool, JournalError>;

    /// List capabilities issued in a specific TreeOp
    async fn list_capabilities_in_op(
        &self,
        epoch: Epoch,
    ) -> Result<Vec<CapabilityRef>, JournalError>;

    // ===== CRDT Operations =====

    /// Merge another journal map into the local state
    ///
    /// Implements the CRDT join-semilattice merge operation.
    /// Used for anti-entropy and gossip synchronization.
    async fn merge_journal_state(&self, other: JournalMap) -> Result<(), JournalError>;

    /// Get journal statistics
    async fn get_journal_stats(&self) -> Result<JournalStats, JournalError>;

    // ===== Tree Membership Queries =====

    /// Check if a device is currently in the tree
    async fn is_device_member(&self, device_id: DeviceId) -> Result<bool, JournalError>;

    /// Get the leaf index for a device (if it exists)
    async fn get_device_leaf_index(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<LeafIndex>, JournalError>;

    /// List all devices in the current tree
    async fn list_devices(&self) -> Result<Vec<DeviceId>, JournalError>;

    /// List all guardians in the current tree
    async fn list_guardians(&self) -> Result<Vec<aura_types::identifiers::GuardianId>, JournalError>;
}
