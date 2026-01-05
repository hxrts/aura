//! Fact model for the journal system
//!
//! This module defines the core fact types used in the fact-based journal.
//! Facts are immutable, ordered entries that represent state changes in the system.
//!
//! # Architecture
//!
//! The fact-based journal implements the new fact-based journal model that replaces
//! the graph-based KeyNode/KeyEdge approach. The journal is a semilattice
//! CRDT using set union for convergence.

use crate::protocol_facts::ProtocolRelationalFact;
pub use aura_core::threshold::{ConvergenceCert, ReversionFact, RotateFact};
use aura_core::{
    domain::{Acknowledgment, Agreement, Consistency, OperationCategory, Propagation},
    identifiers::{AuthorityId, ChannelId, ContextId},
    semilattice::JoinSemilattice,
    time::{OrderTime, PhysicalTime, TimeStamp},
    Hash32, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};


/// Journal namespace for scoping facts
///
/// Facts are scoped to either an authority's namespace or a relational
/// context's namespace, ensuring clean separation of concerns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum JournalNamespace {
    /// Facts belonging to a specific authority
    Authority(AuthorityId),
    /// Facts belonging to a relational context
    Context(ContextId),
}

/// Fact-based journal structure
///
/// The journal is a join-semilattice CRDT that uses set union for merging.
/// Facts are immutable once created and only grow monotonically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// Namespace this journal belongs to
    pub namespace: JournalNamespace,
    /// Set of facts in this journal
    pub facts: BTreeSet<Fact>,
}

impl Journal {
    /// Create a new empty journal for the given namespace
    pub fn new(namespace: JournalNamespace) -> Self {
        Self {
            namespace,
            facts: BTreeSet::new(),
        }
    }

    /// Add a fact to the journal
    pub fn add_fact(&mut self, fact: Fact) -> Result<()> {
        self.facts.insert(fact);
        Ok(())
    }

    /// Get all facts of a specific type
    pub fn facts_of_type(&self, fact_type: FactType) -> Vec<&Fact> {
        self.facts
            .iter()
            .filter(|f| f.content.fact_type() == fact_type)
            .collect()
    }

    /// Get the current size of the journal
    pub fn size(&self) -> usize {
        self.facts.len()
    }

    /// Check if the journal contains a specific fact timestamp
    pub fn contains_timestamp(&self, ts: &TimeStamp) -> bool {
        self.facts.iter().any(|f| &f.timestamp == ts)
    }

    /// Iterate over all facts in the journal
    pub fn iter_facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts.iter()
    }
}

// Implement JoinSemilattice for Journal
impl JoinSemilattice for Journal {
    fn join(&self, other: &Self) -> Self {
        // Journals can only be merged if they're in the same namespace
        assert_eq!(
            self.namespace, other.namespace,
            "Cannot merge journals from different namespaces"
        );

        let mut merged_facts = self.facts.clone();
        merged_facts.extend(other.facts.clone());

        Self {
            namespace: self.namespace.clone(),
            facts: merged_facts,
        }
    }
}

impl Journal {
    /// In-place join operation for efficiency
    pub fn join_assign(&mut self, other: &Self) {
        assert_eq!(
            self.namespace, other.namespace,
            "Cannot merge journals from different namespaces"
        );
        self.facts.extend(other.facts.clone());
    }

    /// Add a fact with options
    ///
    /// This allows configuring metadata like ack tracking when adding facts.
    pub fn add_fact_with_options(&mut self, mut fact: Fact, options: FactOptions) -> Result<()> {
        // Apply options to fact metadata
        fact.ack_tracked = options.request_acks;
        if let Some(agreement) = options.initial_agreement {
            fact.agreement = agreement;
        }
        self.facts.insert(fact);
        Ok(())
    }

    /// Get a fact by its order ID
    pub fn get_fact(&self, order: &OrderTime) -> Option<&Fact> {
        self.facts.iter().find(|f| &f.order == order)
    }

    /// Get a mutable reference to a fact by its order ID
    ///
    /// Note: This removes and re-inserts the fact since BTreeSet doesn't allow
    /// mutable access. Only metadata fields should be modified.
    pub fn get_fact_mut(&mut self, order: &OrderTime) -> Option<Fact> {
        // Find and remove the fact
        let fact = self.facts.iter().find(|f| &f.order == order).cloned();
        if let Some(f) = fact.as_ref() {
            // Clone the fact for comparison (to remove by value)
            self.facts.remove(f);
        }
        fact
    }

    /// Re-insert a fact after modification
    ///
    /// This is used after modifying metadata with get_fact_mut.
    pub fn update_fact(&mut self, fact: Fact) {
        self.facts.insert(fact);
    }

    /// Clear ack tracking for the specified facts.
    ///
    /// This sets `ack_tracked = false` on each fact, indicating that
    /// ack tracking has been garbage collected for these facts.
    pub fn clear_ack_tracking(&mut self, fact_ids: &[OrderTime]) {
        for order in fact_ids {
            if let Some(mut fact) = self.get_fact_mut(order) {
                fact.ack_tracked = false;
                self.update_fact(fact);
            }
        }
    }

    /// Get all facts that have ack tracking enabled
    pub fn ack_tracked_facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts.iter().filter(|f| f.ack_tracked)
    }

    /// Get all facts that are not yet finalized
    pub fn provisional_facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts
            .iter()
            .filter(|f| f.agreement.is_provisional())
    }

    /// Get all facts that are finalized
    pub fn finalized_facts(&self) -> impl Iterator<Item = &Fact> {
        self.facts.iter().filter(|f| f.agreement.is_finalized())
    }

    /// Get agreement level for a fact
    pub fn get_agreement(&self, order: &OrderTime) -> Option<Agreement> {
        self.get_fact(order).map(|f| f.agreement.clone())
    }

    /// Get propagation status for a fact
    pub fn get_propagation(&self, order: &OrderTime) -> Option<Propagation> {
        self.get_fact(order).map(|f| f.propagation.clone())
    }

    /// Check if a fact has ack tracking enabled
    pub fn is_ack_tracked(&self, order: &OrderTime) -> bool {
        self.get_fact(order).map(|f| f.ack_tracked).unwrap_or(false)
    }

    /// Update agreement level for a fact
    pub fn update_agreement(&mut self, order: &OrderTime, agreement: Agreement) -> Result<()> {
        if let Some(mut fact) = self.get_fact_mut(order) {
            fact.agreement = agreement;
            self.update_fact(fact);
            Ok(())
        } else {
            Err(aura_core::AuraError::not_found(format!(
                "Fact with order {order:?} not found"
            )))
        }
    }

    /// Update propagation status for a fact
    pub fn update_propagation(
        &mut self,
        order: &OrderTime,
        propagation: Propagation,
    ) -> Result<()> {
        if let Some(mut fact) = self.get_fact_mut(order) {
            fact.propagation = propagation;
            self.update_fact(fact);
            Ok(())
        } else {
            Err(aura_core::AuraError::not_found(format!(
                "Fact with order {order:?} not found"
            )))
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Ack Storage
// ─────────────────────────────────────────────────────────────────────────────

/// Storage for acknowledgments.
///
/// Acks are stored separately from facts because:
/// 1. Acks are more dynamic (frequently updated)
/// 2. Acks don't participate in CRDT semantics
/// 3. Acks can be garbage collected independently
///
/// This is an in-memory implementation. Production systems should persist
/// acks to a database or storage layer.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AckStorage {
    /// Map from fact OrderTime to acknowledgments
    acks: BTreeMap<OrderTime, Acknowledgment>,
}

impl AckStorage {
    /// Create new empty ack storage
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an acknowledgment from a peer
    pub fn record_ack(
        &mut self,
        fact_id: &OrderTime,
        peer: AuthorityId,
        timestamp: PhysicalTime,
    ) -> Result<()> {
        let ack = self.acks.entry(fact_id.clone()).or_default();
        *ack = ack.clone().add_ack(peer, timestamp);
        Ok(())
    }

    /// Get acknowledgments for a fact
    pub fn get_acks(&self, fact_id: &OrderTime) -> Option<&Acknowledgment> {
        self.acks.get(fact_id)
    }

    /// Check if a peer has acknowledged a fact
    pub fn has_acked(&self, fact_id: &OrderTime, peer: &AuthorityId) -> bool {
        self.acks
            .get(fact_id)
            .map(|a| a.contains(peer))
            .unwrap_or(false)
    }

    /// Get ack count for a fact
    pub fn ack_count(&self, fact_id: &OrderTime) -> usize {
        self.acks.get(fact_id).map(|a| a.count()).unwrap_or(0)
    }

    /// Delete acks for a fact (GC)
    pub fn delete_acks(&mut self, fact_id: &OrderTime) {
        self.acks.remove(fact_id);
    }

    /// Check if all expected peers have acknowledged
    pub fn all_acked(&self, fact_id: &OrderTime, expected: &[AuthorityId]) -> bool {
        self.acks
            .get(fact_id)
            .map(|a| a.all_acked(expected))
            .unwrap_or(false)
    }

    /// Get all fact IDs with acks
    pub fn fact_ids(&self) -> impl Iterator<Item = &OrderTime> {
        self.acks.keys()
    }

    /// Get number of facts with acks
    pub fn len(&self) -> usize {
        self.acks.len()
    }

    /// Check if storage is empty
    pub fn is_empty(&self) -> bool {
        self.acks.is_empty()
    }

    /// Get consistency for a fact, combining journal metadata and ack storage
    pub fn get_consistency(&self, journal: &Journal, fact_id: &OrderTime) -> Option<Consistency> {
        journal.get_fact(fact_id).map(|fact| {
            let mut consistency = Consistency::new(OperationCategory::Optimistic)
                .with_agreement(fact.agreement.clone())
                .with_propagation(fact.propagation.clone());

            if fact.ack_tracked {
                if let Some(ack) = self.get_acks(fact_id) {
                    consistency = consistency.with_acknowledgment(ack.clone());
                } else {
                    consistency = consistency.with_ack_tracking();
                }
            }

            consistency
        })
    }

    /// Merge acks from another storage
    pub fn merge(&mut self, other: &AckStorage) {
        for (fact_id, other_ack) in &other.acks {
            if let Some(self_ack) = self.acks.get_mut(fact_id) {
                *self_ack = self_ack.clone().merge(other_ack);
            } else {
                self.acks.insert(fact_id.clone(), other_ack.clone());
            }
        }
    }

    /// Garbage collect ack tracking for facts that meet policy criteria.
    ///
    /// This method iterates over facts with ack tracking, consults the provided
    /// policy evaluator, and removes ack records for facts where tracking is no
    /// longer needed.
    ///
    /// # Arguments
    ///
    /// * `journal` - The journal containing the facts
    /// * `should_drop` - A closure that takes a fact and its consistency,
    ///   returning true if ack tracking should be dropped for this fact.
    ///
    /// # Returns
    ///
    /// The number of facts whose ack tracking was garbage collected.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Drop tracking for all finalized facts
    /// let gc_count = ack_storage.gc_ack_tracking(&journal, |fact, consistency| {
    ///     consistency.agreement.is_finalized()
    /// });
    /// ```
    pub fn gc_ack_tracking<F>(&mut self, journal: &mut Journal, mut should_drop: F) -> GcResult
    where
        F: FnMut(&Fact, &Consistency) -> bool,
    {
        let mut facts_to_gc: Vec<OrderTime> = Vec::new();

        // First pass: identify facts to GC
        for fact in journal.iter_facts() {
            if !fact.ack_tracked {
                continue;
            }

            if let Some(consistency) = self.get_consistency(journal, fact.id()) {
                if should_drop(fact, &consistency) {
                    facts_to_gc.push(fact.id().clone());
                }
            }
        }

        let count = facts_to_gc.len();

        // Second pass: remove ack records and update facts
        for fact_id in &facts_to_gc {
            self.delete_acks(fact_id);
        }

        // Update fact metadata in journal
        journal.clear_ack_tracking(&facts_to_gc);

        GcResult {
            facts_collected: count,
            facts_remaining: self.len(),
        }
    }

    /// Garbage collect ack tracking using a predicate on consistency only.
    ///
    /// Simpler version of `gc_ack_tracking` when you only need to check
    /// the consistency metadata, not the fact content.
    pub fn gc_by_consistency<F>(&mut self, journal: &mut Journal, mut predicate: F) -> GcResult
    where
        F: FnMut(&Consistency) -> bool,
    {
        self.gc_ack_tracking(journal, |_fact, consistency| predicate(consistency))
    }
}

/// Result of garbage collection operation
#[derive(Debug, Clone, Copy, Default)]
pub struct GcResult {
    /// Number of facts whose ack tracking was collected
    pub facts_collected: usize, // usize ok: internal GC metric, not serialized
    /// Number of facts still being tracked
    pub facts_remaining: usize, // usize ok: internal GC metric, not serialized
}

/// Core fact structure (timestamp-driven identity)
///
/// # Identity vs Metadata
///
/// A fact's identity is determined by its `order`, `timestamp`, and `content` fields.
/// The metadata fields (`agreement`, `propagation`, `ack_tracked`) are mutable and
/// do NOT affect equality or hashing. This allows facts to be stored in BTreeSet
/// while their metadata can be updated without affecting set membership.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fact {
    /// Opaque total order for deterministic merges
    pub order: OrderTime,
    /// Semantic timestamp for ordering/identity
    pub timestamp: TimeStamp,
    /// Content of the fact
    pub content: FactContent,

    // ─────────────────────────────────────────────────────────────────────────
    // Consistency Metadata (mutable, not part of identity)
    // ─────────────────────────────────────────────────────────────────────────
    /// Agreement level (A1/A2/A3)
    /// - Provisional: Usable immediately, may be superseded
    /// - SoftSafe: Bounded divergence with convergence certificate
    /// - Finalized: Consensus-confirmed, durable, non-forkable
    #[serde(default)]
    pub agreement: Agreement,

    /// Propagation status for anti-entropy sync
    /// - Local: Only on this device
    /// - Syncing: Sync in progress to peers
    /// - Complete: Reached all known peers
    /// - Failed: Sync failed, will retry
    #[serde(default)]
    pub propagation: Propagation,

    /// Whether this fact requests acknowledgment tracking
    /// When true, the journal will track per-peer acknowledgments
    #[serde(default)]
    pub ack_tracked: bool,
}

// Manual PartialEq implementation that excludes metadata from identity
impl PartialEq for Fact {
    fn eq(&self, other: &Self) -> bool {
        // Only compare core identity fields, not metadata
        self.order == other.order
            && self.timestamp == other.timestamp
            && self.content == other.content
    }
}

impl Eq for Fact {}

impl Fact {
    /// Create a new fact with default metadata (Provisional, Local, no ack tracking)
    pub fn new(order: OrderTime, timestamp: TimeStamp, content: FactContent) -> Self {
        Self {
            order,
            timestamp,
            content,
            agreement: Agreement::default(),
            propagation: Propagation::default(),
            ack_tracked: false,
        }
    }

    /// Create a new fact with ack tracking enabled
    pub fn new_with_ack_tracking(
        order: OrderTime,
        timestamp: TimeStamp,
        content: FactContent,
    ) -> Self {
        Self {
            order,
            timestamp,
            content,
            agreement: Agreement::default(),
            propagation: Propagation::default(),
            ack_tracked: true,
        }
    }

    /// Set the agreement level (builder pattern)
    #[must_use]
    pub fn with_agreement(mut self, agreement: Agreement) -> Self {
        self.agreement = agreement;
        self
    }

    /// Set the propagation status (builder pattern)
    #[must_use]
    pub fn with_propagation(mut self, propagation: Propagation) -> Self {
        self.propagation = propagation;
        self
    }

    /// Enable ack tracking (builder pattern)
    #[must_use]
    pub fn with_ack_tracking(mut self) -> Self {
        self.ack_tracked = true;
        self
    }

    /// Get the fact's unique identifier (OrderTime)
    pub fn id(&self) -> &OrderTime {
        &self.order
    }

    /// Check if this fact is finalized (A3)
    pub fn is_finalized(&self) -> bool {
        self.agreement.is_finalized()
    }

    /// Check if this fact is at least safe (A2+)
    pub fn is_safe(&self) -> bool {
        self.agreement.is_safe()
    }

    /// Check if propagation is complete
    pub fn is_propagated(&self) -> bool {
        self.propagation.is_complete()
    }

    /// Get the consistency metadata for this fact.
    ///
    /// Returns a Consistency object constructed from the fact's agreement and
    /// propagation fields. If the fact has ack_tracked=true, the acknowledgment
    /// field will be set to a default (empty) Acknowledgment; callers should
    /// populate this from the journal's ack storage if needed.
    ///
    /// Note: This is a convenience method for cases where you need a Consistency
    /// object. For full consistency tracking including acknowledgments, use the
    /// journal's query methods which populate acknowledgment data.
    pub fn consistency(&self) -> Consistency {
        Consistency {
            category: OperationCategory::Optimistic, // Default category
            agreement: self.agreement.clone(),
            propagation: self.propagation.clone(),
            acknowledgment: if self.ack_tracked {
                Some(Acknowledgment::default())
            } else {
                None
            },
        }
    }
}

impl PartialOrd for Fact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fact {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.order.cmp(&other.order)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fact Options
// ─────────────────────────────────────────────────────────────────────────────

/// Options for creating facts.
///
/// Controls metadata and behavior when adding facts to a journal.
/// This struct uses the builder pattern for ergonomic configuration.
///
/// # Example
///
/// ```ignore
/// use aura_journal::fact::FactOptions;
///
/// let options = FactOptions::default()
///     .with_ack_tracking();  // Enable acknowledgment tracking
///
/// journal.add_fact_with_options(fact, options)?;
/// ```
#[derive(Debug, Clone, Default)]
pub struct FactOptions {
    /// Request acknowledgment tracking for this fact
    ///
    /// When true, the journal will track per-peer acknowledgments
    /// for this fact. This enables delivery confirmation checking.
    pub request_acks: bool,

    /// Initial agreement level (defaults to Provisional)
    ///
    /// Most facts start as Provisional and transition to higher
    /// agreement levels as consensus progresses.
    pub initial_agreement: Option<Agreement>,
}

impl FactOptions {
    /// Create new default options
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable ack tracking for this fact
    #[must_use]
    pub fn with_ack_tracking(mut self) -> Self {
        self.request_acks = true;
        self
    }

    /// Set initial agreement level
    #[must_use]
    pub fn with_agreement(mut self, agreement: Agreement) -> Self {
        self.initial_agreement = Some(agreement);
        self
    }

    /// Check if ack tracking is requested
    pub fn has_ack_tracking(&self) -> bool {
        self.request_acks
    }
}

/// Types of facts that can be stored in the journal
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactContent {
    /// Attested operation on the commitment tree
    AttestedOp(AttestedOp),
    /// Relational fact for cross-authority coordination
    Relational(RelationalFact),
    /// Snapshot marker for garbage collection
    Snapshot(SnapshotFact),
    /// Rendezvous receipt for tracking message flow
    RendezvousReceipt {
        /// Unique identifier of the envelope
        envelope_id: [u8; 32],
        /// Authority that issued this receipt
        authority_id: AuthorityId,
        /// Time when the receipt was created (using unified time system)
        timestamp: TimeStamp,
        /// Signature over the receipt data
        signature: Vec<u8>,
    },
}

impl FactContent {
    /// Get the type of this fact content
    pub fn fact_type(&self) -> FactType {
        match self {
            FactContent::AttestedOp(_) => FactType::AttestedOp,
            FactContent::Relational(_) => FactType::Relational,
            FactContent::Snapshot(_) => FactType::Snapshot,
            FactContent::RendezvousReceipt { .. } => FactType::RendezvousReceipt,
        }
    }
}

/// Enumeration of fact types for filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactType {
    /// Attested operation on the commitment tree
    AttestedOp,
    /// Relational fact for cross-authority coordination
    Relational,
    /// Snapshot marker for garbage collection
    Snapshot,
    /// Rendezvous receipt for tracking message flow
    RendezvousReceipt,
}

/// Attested operation fact
///
/// Represents a threshold-signed operation on the commitment tree.
/// These facts drive the authority's internal state transitions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestedOp {
    /// The tree operation being attested
    pub tree_op: TreeOpKind,
    /// Commitment before the operation
    pub parent_commitment: Hash32,
    /// Commitment after the operation
    pub new_commitment: Hash32,
    /// Number of witnesses that attested
    pub witness_threshold: u16,
    /// Aggregated threshold signature
    pub signature: Vec<u8>,
}

/// Tree operation types that can be attested
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeOpKind {
    /// Add a new device/leaf to the tree
    AddLeaf {
        /// Public key of the new device (opaque bytes)
        public_key: Vec<u8>,
        /// Role of the leaf (device or guardian)
        role: aura_core::tree::LeafRole,
    },
    /// Remove a device/leaf from the tree
    RemoveLeaf {
        /// Index of the leaf to remove
        leaf_index: u32,
    },
    /// Update the threshold policy for the tree
    UpdatePolicy {
        /// New threshold value required for operations
        threshold: u16,
    },
    /// Rotate the epoch (invalidates old key shares)
    RotateEpoch,
}

/// Channel checkpoint anchoring ratchet windows
///
/// Checkpoints define the dual-window envelope for a channel epoch and anchor
/// recovery/GC boundaries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelCheckpoint {
    /// Relational context containing this channel
    pub context: ContextId,
    /// Channel identifier for this checkpoint
    pub channel: ChannelId,
    /// Channel epoch number for this checkpoint
    pub chan_epoch: u64,
    /// Base generation number for the checkpoint window
    pub base_gen: u64,
    /// Checkpoint window size
    pub window: u32,
    /// Commitment hash for this checkpoint
    pub ck_commitment: Hash32,
    /// Optional override for skip window behavior
    pub skip_window_override: Option<u32>,
}

/// Reason for proposing a channel epoch bump
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum ChannelBumpReason {
    /// Routine cadence-based maintenance
    Routine,
    /// Suspicious activity detected (AEAD/MAC failure, ratchet conflict, etc.)
    SuspiciousActivity,
    /// Confirmed compromise requiring immediate PCS
    ConfirmedCompromise,
}

impl ChannelBumpReason {
    /// Whether this reason bypasses routine spacing rules
    pub fn bypass_spacing(self) -> bool {
        matches!(
            self,
            ChannelBumpReason::SuspiciousActivity | ChannelBumpReason::ConfirmedCompromise
        )
    }
}

/// Optimistic bump from epoch e to e+1
///
/// Inserted before consensus finalizes the bump; at most one pending bump per epoch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProposedChannelEpochBump {
    /// Relational context containing this channel
    pub context: ContextId,
    /// Channel identifier for this epoch bump
    pub channel: ChannelId,
    /// Current epoch being transitioned from
    pub parent_epoch: u64,
    /// New epoch being transitioned to
    pub new_epoch: u64,
    /// Unique identifier for this bump proposal
    pub bump_id: Hash32,
    /// Reason for proposing this epoch bump
    pub reason: ChannelBumpReason,
}

/// Committed epoch bump, finalized by consensus
///
/// Represents the canonical epoch transition once witnesses have signed off.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CommittedChannelEpochBump {
    /// Relational context containing this channel
    pub context: ContextId,
    /// Channel identifier for this epoch bump
    pub channel: ChannelId,
    /// Current epoch being transitioned from
    pub parent_epoch: u64,
    /// New epoch being transitioned to
    pub new_epoch: u64,
    /// Chosen bump identifier that was finalized
    pub chosen_bump_id: Hash32,
    /// Consensus instance identifier (hash) that finalized this bump
    pub consensus_id: Hash32,
    /// Optional DKG transcript reference for the finalized channel key material
    #[serde(default)]
    pub transcript_ref: Option<Hash32>,
}

/// Finalized DKG transcript commit (consensus-backed)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DkgTranscriptCommit {
    /// Relational context containing this transcript (if applicable)
    pub context: ContextId,
    /// Epoch for which the transcript is valid
    pub epoch: u64,
    /// Membership hash bound into the transcript
    pub membership_hash: Hash32,
    /// Deterministic cutoff (round/height/view)
    pub cutoff: u64,
    /// Number of dealer packages included
    pub package_count: u32,
    /// Hash of the transcript contents
    pub transcript_hash: Hash32,
    /// Optional blob reference for transcript payload
    pub blob_ref: Option<Hash32>,
}

/// Channel-level policy controls (governable)
///
/// Allows overriding skip window per channel via relational context policy facts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelPolicy {
    /// Relational context containing this channel
    pub context: ContextId,
    /// Channel identifier for this policy
    pub channel: ChannelId,
    /// Optional override for skip window behavior
    pub skip_window: Option<u32>,
}

/// Provisional AMP channel bootstrap (dealer key metadata).
///
/// Records the bootstrap key identifier and recipients without exposing
/// the bootstrap key material in the journal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelBootstrap {
    /// Relational context containing this channel
    pub context: ContextId,
    /// Channel identifier for this bootstrap
    pub channel: ChannelId,
    /// Hash identifier for the bootstrap key material
    pub bootstrap_id: Hash32,
    /// Authority acting as the bootstrap dealer
    pub dealer: AuthorityId,
    /// Intended recipients of the bootstrap key
    pub recipients: Vec<AuthorityId>,
    /// Timestamp when bootstrap was created
    pub created_at: aura_core::time::PhysicalTime,
    /// Optional expiration time for the bootstrap key
    pub expires_at: Option<aura_core::time::PhysicalTime>,
}

/// Observer classes for leakage tracking in journals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LeakageObserverClass {
    /// Observer outside the local neighborhood (public/internet visibility).
    External,
    /// Observer within the local neighborhood but outside the group.
    Neighbor,
    /// Observer within the group context.
    InGroup,
}

impl From<LeakageObserverClass> for aura_core::effects::ObserverClass {
    fn from(value: LeakageObserverClass) -> Self {
        match value {
            LeakageObserverClass::External => aura_core::effects::ObserverClass::External,
            LeakageObserverClass::Neighbor => aura_core::effects::ObserverClass::Neighbor,
            LeakageObserverClass::InGroup => aura_core::effects::ObserverClass::InGroup,
        }
    }
}

impl From<aura_core::effects::ObserverClass> for LeakageObserverClass {
    fn from(value: aura_core::effects::ObserverClass) -> Self {
        match value {
            aura_core::effects::ObserverClass::External => LeakageObserverClass::External,
            aura_core::effects::ObserverClass::Neighbor => LeakageObserverClass::Neighbor,
            aura_core::effects::ObserverClass::InGroup => LeakageObserverClass::InGroup,
        }
    }
}

/// Leakage event fact stored in relational journals.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LeakageFact {
    /// Relational context where the leakage occurred.
    pub context_id: ContextId,
    /// Authority that originated the message.
    pub source: AuthorityId,
    /// Authority that received the message.
    pub destination: AuthorityId,
    /// Observer classification for leakage accounting.
    pub observer: LeakageObserverClass,
    /// Amount of leakage budget consumed.
    pub amount: u64,
    /// Operation label for audit and diagnostics.
    pub operation: String,
    /// Timestamp for leakage accounting.
    pub timestamp: aura_core::time::PhysicalTime,
}

/// Relational fact for cross-authority relationships
///
/// # Protocol-Level vs Domain-Level Facts
///
/// This enum contains two categories of facts:
///
/// ## Protocol-Level Facts (stay in aura-journal)
///
/// These facts are core protocol constructs with complex reduction logic in
/// `aura-journal/src/reduction.rs`. They participate directly in state derivation
/// and have interdependencies that require specialized handling:
///
/// - `Protocol(GuardianBinding)` - Core guardian relationship protocol
/// - `Protocol(RecoveryGrant)` - Core recovery protocol
/// - `Protocol(Consensus)` - Aura Consensus results
/// - `Protocol(AmpChannelCheckpoint)` - AMP ratchet window anchoring
/// - `Protocol(AmpProposedChannelEpochBump)` - Optimistic epoch transitions
/// - `Protocol(AmpCommittedChannelEpochBump)` - Finalized epoch transitions
/// - `Protocol(AmpChannelPolicy)` - Channel-level policy overrides
/// - `Protocol(AmpChannelBootstrap)` - Channel bootstrap key metadata
/// - `Protocol(DkgTranscriptCommit)` - Consensus-finalized DKG transcript
/// - `Protocol(ConvergenceCert)` - Soft-safe convergence certificate
/// - `Protocol(ReversionFact)` - Soft-safe explicit reversion
/// - `Protocol(RotateFact)` - Lifecycle rotation/upgrade marker
///
/// New protocol facts must be added to `protocol_facts.rs` and documented
/// in `docs/102_journal.md` (criteria + reduction rules).
///
/// ## Domain-Level Facts (via Generic + FactRegistry)
///
/// Application-specific facts use `Generic` and are reduced by registered
/// `FactReducer` implementations in their respective domain crates:
///
/// - `aura-chat`: ChatFact (channels, messages)
/// - `aura-invitation`: InvitationFact (invitation lifecycle)
/// - `aura-relational`: ContactFact (contact management)
/// - `aura-social/moderation`: Home/Mute/Ban/Kick facts
///
/// Domain crates implement `DomainFact` trait and register reducers in
/// `aura-agent/src/fact_registry.rs`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationalFact {
    // ========================================================================
    // Protocol-Level Facts (core protocol, complex reduction logic)
    // These facts have specialized handling in reduce_context() and should
    // NOT be migrated to domain crates.
    // ========================================================================
    /// Protocol-level facts that participate in core reduction semantics.
    Protocol(ProtocolRelationalFact),

    // ========================================================================
    // Domain-Level Facts (extensibility point for application facts)
    // Domain crates define their own fact types and register reducers in
    // aura-agent/src/fact_registry.rs. Facts are stored via to_generic().
    // ========================================================================
    /// Generic relational binding for extensibility
    ///
    /// This is the extensibility mechanism for domain-specific fact types.
    /// Higher-level crates define their own fact types implementing `DomainFact`
    /// and store them via this variant using `DomainFact::to_generic()`.
    ///
    /// # Domain Fact Crates
    ///
    /// - `aura_chat::ChatFact` - Channel/message facts (ChannelCreated, MessageSentSealed, etc.)
    /// - `aura_invitation::InvitationFact` - Invitation lifecycle facts
    /// - `aura_relational::ContactFact` - Contact management facts
    ///
    /// # Example
    ///
    /// ```ignore
    /// use aura_chat::ChatFact;
    /// use aura_journal::DomainFact;
    ///
    /// let chat_fact = ChatFact::message_sent_sealed_ms(
    ///     /* context_id */ todo!(),
    ///     /* channel_id */ todo!(),
    ///     "msg-123".to_string(),
    ///     /* sender_id */ todo!(),
    ///     "Alice".to_string(),
    ///     b"opaque bytes".to_vec(),
    ///     /* sent_at_ms */ 0,
    ///     None,
    /// );
    /// let generic = chat_fact.to_generic(); // Returns RelationalFact::Generic
    /// ```
    Generic {
        /// Context in which this binding exists
        context_id: ContextId,
        /// Type of binding (domain-specific, e.g., "chat", "invitation", "contact")
        binding_type: String,
        /// Serialized binding data (deserialize with `DomainFact::from_bytes`)
        binding_data: Vec<u8>,
    },
}

/// Typed key for protocol-level relational facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolFactKey {
    /// Guardian binding between an account and guardian authority.
    GuardianBinding {
        /// The account being protected.
        account_id: AuthorityId,
        /// The guardian authority.
        guardian_id: AuthorityId,
        /// Hash of the binding commitment.
        binding_hash: Hash32,
    },
    /// Recovery grant issued by a guardian.
    RecoveryGrant {
        /// The account being recovered.
        account_id: AuthorityId,
        /// The guardian issuing the grant.
        guardian_id: AuthorityId,
        /// Hash of the grant commitment.
        grant_hash: Hash32,
    },
    /// Consensus result tying a consensus instance to an operation.
    Consensus {
        /// Unique identifier for the consensus instance.
        consensus_id: Hash32,
        /// Hash of the operation being decided.
        operation_hash: Hash32,
    },
    /// AMP channel checkpoint anchor.
    AmpChannelCheckpoint {
        /// The channel identifier.
        channel: ChannelId,
        /// The channel epoch at checkpoint.
        chan_epoch: u64,
        /// Commitment to the checkpoint state.
        ck_commitment: Hash32,
    },
    /// Proposed AMP channel epoch bump.
    AmpProposedChannelEpochBump {
        /// The channel identifier.
        channel: ChannelId,
        /// The epoch being bumped from.
        parent_epoch: u64,
        /// The proposed new epoch.
        new_epoch: u64,
        /// Unique identifier for this bump proposal.
        bump_id: Hash32,
    },
    /// Committed AMP channel epoch bump.
    AmpCommittedChannelEpochBump {
        /// The channel identifier.
        channel: ChannelId,
        /// The epoch being bumped from.
        parent_epoch: u64,
        /// The committed new epoch.
        new_epoch: u64,
        /// The chosen bump proposal identifier.
        chosen_bump_id: Hash32,
    },
    /// Channel policy override.
    AmpChannelPolicy {
        /// The channel identifier.
        channel: ChannelId,
    },
    /// AMP channel bootstrap metadata.
    AmpChannelBootstrap {
        /// The channel identifier.
        channel: ChannelId,
        /// Bootstrap key identifier.
        bootstrap_id: Hash32,
    },
    /// Leakage accounting event.
    LeakageEvent {
        /// Source authority of the leakage.
        source: AuthorityId,
        /// Destination authority of the leakage.
        destination: AuthorityId,
        /// When the leakage occurred.
        timestamp: aura_core::time::PhysicalTime,
    },
    /// Finalized DKG transcript commit.
    DkgTranscriptCommit {
        /// Hash of the committed transcript.
        transcript_hash: Hash32,
    },
    /// Coordinator convergence certificate.
    ConvergenceCert {
        /// Operation identifier for convergence.
        op_id: Hash32,
    },
    /// Explicit reversion fact.
    ReversionFact {
        /// Operation identifier being reverted.
        op_id: Hash32,
    },
    /// Lifecycle rotation marker.
    RotateFact {
        /// Hash of the target state.
        to_state: Hash32,
    },
}

impl ProtocolFactKey {
    /// Stable subtype identifier for reducer keys.
    pub fn sub_type(&self) -> &'static str {
        match self {
            ProtocolFactKey::GuardianBinding { .. } => "guardian-binding",
            ProtocolFactKey::RecoveryGrant { .. } => "recovery-grant",
            ProtocolFactKey::Consensus { .. } => "consensus",
            ProtocolFactKey::AmpChannelCheckpoint { .. } => "amp-channel-checkpoint",
            ProtocolFactKey::AmpProposedChannelEpochBump { .. } => "amp-proposed-epoch-bump",
            ProtocolFactKey::AmpCommittedChannelEpochBump { .. } => "amp-committed-epoch-bump",
            ProtocolFactKey::AmpChannelPolicy { .. } => "amp-channel-policy",
            ProtocolFactKey::AmpChannelBootstrap { .. } => "amp-channel-bootstrap",
            ProtocolFactKey::LeakageEvent { .. } => "leakage-event",
            ProtocolFactKey::DkgTranscriptCommit { .. } => "dkg-transcript-commit",
            ProtocolFactKey::ConvergenceCert { .. } => "convergence-cert",
            ProtocolFactKey::ReversionFact { .. } => "reversion-fact",
            ProtocolFactKey::RotateFact { .. } => "rotate-fact",
        }
    }

    /// Opaque key payload for reducer indexing.
    pub fn data(&self) -> Vec<u8> {
        match self {
            ProtocolFactKey::GuardianBinding { binding_hash, .. } => {
                binding_hash.as_bytes().to_vec()
            }
            ProtocolFactKey::RecoveryGrant { grant_hash, .. } => grant_hash.as_bytes().to_vec(),
            ProtocolFactKey::Consensus {
                consensus_id,
                operation_hash,
            } => {
                let mut data = Vec::with_capacity(64);
                data.extend_from_slice(consensus_id.as_bytes());
                data.extend_from_slice(operation_hash.as_bytes());
                data
            }
            ProtocolFactKey::AmpChannelCheckpoint {
                channel,
                chan_epoch,
                ck_commitment,
            } => aura_core::util::serialization::to_vec(&(channel, chan_epoch, ck_commitment))
                .unwrap_or_default(),
            ProtocolFactKey::AmpProposedChannelEpochBump {
                channel,
                parent_epoch,
                new_epoch,
                bump_id,
            } => {
                aura_core::util::serialization::to_vec(&(channel, parent_epoch, new_epoch, bump_id))
                    .unwrap_or_default()
            }
            ProtocolFactKey::AmpCommittedChannelEpochBump {
                channel,
                parent_epoch,
                new_epoch,
                chosen_bump_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                new_epoch,
                chosen_bump_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpChannelPolicy { channel } => {
                aura_core::util::serialization::to_vec(channel).unwrap_or_default()
            }
            ProtocolFactKey::AmpChannelBootstrap {
                channel,
                bootstrap_id,
            } => aura_core::util::serialization::to_vec(&(channel, bootstrap_id))
                .unwrap_or_default(),
            ProtocolFactKey::LeakageEvent {
                source,
                destination,
                timestamp,
            } => aura_core::util::serialization::to_vec(&(source, destination, timestamp))
                .unwrap_or_default(),
            ProtocolFactKey::DkgTranscriptCommit { transcript_hash } => {
                transcript_hash.as_bytes().to_vec()
            }
            ProtocolFactKey::ConvergenceCert { op_id } => op_id.as_bytes().to_vec(),
            ProtocolFactKey::ReversionFact { op_id } => op_id.as_bytes().to_vec(),
            ProtocolFactKey::RotateFact { to_state } => to_state.as_bytes().to_vec(),
        }
    }
}

/// Snapshot fact for garbage collection
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SnapshotFact {
    /// Hash of the state at snapshot time
    pub state_hash: Hash32,
    /// Facts that can be garbage collected
    pub superseded_facts: Vec<OrderTime>,
    /// Snapshot sequence number
    pub sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_creation() {
        let auth_id = AuthorityId::new_from_entropy([9u8; 32]);
        let namespace = JournalNamespace::Authority(auth_id);
        let journal = Journal::new(namespace.clone());

        assert_eq!(journal.namespace, namespace);
        assert_eq!(journal.size(), 0);
    }

    #[test]
    fn test_journal_merge() {
        let auth_id = AuthorityId::new_from_entropy([10u8; 32]);
        let namespace = JournalNamespace::Authority(auth_id);

        let mut journal1 = Journal::new(namespace.clone());
        let mut journal2 = Journal::new(namespace);

        // Add different facts to each journal
        let fact1 = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );

        let fact2 = Fact::new(
            OrderTime([2u8; 32]),
            TimeStamp::OrderClock(OrderTime([2u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 2,
            }),
        );

        journal1.add_fact(fact1.clone()).unwrap();
        journal2.add_fact(fact2.clone()).unwrap();

        // Merge journals
        let merged = journal1.join(&journal2);

        assert_eq!(merged.size(), 2);
        assert!(merged.contains_timestamp(&fact1.timestamp));
        assert!(merged.contains_timestamp(&fact2.timestamp));
    }

    #[test]
    #[should_panic(expected = "Cannot merge journals from different namespaces")]
    fn test_journal_merge_different_namespaces() {
        let namespace1 = JournalNamespace::Authority(AuthorityId::new_from_entropy([11u8; 32]));
        let namespace2 = JournalNamespace::Authority(AuthorityId::new_from_entropy([12u8; 32]));

        let journal1 = Journal::new(namespace1);
        let journal2 = Journal::new(namespace2);

        // This should panic
        let _ = journal1.join(&journal2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Fact Metadata Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_fact_default_metadata() {
        let fact = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );

        // Default metadata
        assert!(fact.agreement.is_provisional());
        assert!(fact.propagation.is_local());
        assert!(!fact.ack_tracked);
        assert!(!fact.is_finalized());
        assert!(!fact.is_propagated());
    }

    #[test]
    fn test_fact_with_ack_tracking() {
        let fact = Fact::new_with_ack_tracking(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );

        assert!(fact.ack_tracked);
    }

    #[test]
    fn test_fact_builder_pattern() {
        use aura_core::query::ConsensusId;

        let consensus_id = ConsensusId::new([1u8; 32]);
        let fact = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        )
        .with_agreement(Agreement::finalized(consensus_id))
        .with_propagation(Propagation::complete())
        .with_ack_tracking();

        assert!(fact.is_finalized());
        assert!(fact.is_propagated());
        assert!(fact.ack_tracked);
    }

    #[test]
    fn test_fact_equality_ignores_metadata() {
        use aura_core::query::ConsensusId;

        // Create two facts with same identity but different metadata
        let fact1 = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );

        let fact2 = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        )
        .with_agreement(Agreement::finalized(ConsensusId::new([2u8; 32])))
        .with_propagation(Propagation::complete())
        .with_ack_tracking();

        // Facts should be equal despite different metadata
        assert_eq!(fact1, fact2);
    }

    #[test]
    fn test_fact_ordering_ignores_metadata() {
        // Create facts with same order but different metadata
        let fact1 = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        );

        let fact2 = Fact::new(
            OrderTime([2u8; 32]),
            TimeStamp::OrderClock(OrderTime([2u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 2,
            }),
        )
        .with_propagation(Propagation::complete());

        // Ordering should be based on order field only
        assert!(fact1 < fact2);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Garbage Collection Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_gc_ack_tracking_basic() {
        use aura_core::query::ConsensusId;

        let auth_id = AuthorityId::new_from_entropy([20u8; 32]);
        let namespace = JournalNamespace::Authority(auth_id);
        let mut journal = Journal::new(namespace);
        let mut ack_storage = AckStorage::new();

        // Create a fact with ack tracking
        let fact = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        )
        .with_ack_tracking()
        .with_agreement(Agreement::Finalized {
            consensus_id: ConsensusId([1u8; 32]),
        });

        journal.add_fact(fact.clone()).unwrap();

        // Record an ack
        let peer = AuthorityId::new_from_entropy([21u8; 32]);
        ack_storage
            .record_ack(
                &fact.order,
                peer,
                PhysicalTime {
                    ts_ms: 1000,
                    uncertainty: None,
                },
            )
            .unwrap();

        assert_eq!(ack_storage.len(), 1);
        assert!(journal.ack_tracked_facts().count() == 1);

        // GC based on finalization - should drop tracking for finalized facts
        let result = ack_storage.gc_by_consistency(&mut journal, |c| c.agreement.is_finalized());

        assert_eq!(result.facts_collected, 1);
        assert_eq!(result.facts_remaining, 0);
        assert!(ack_storage.is_empty());
        assert_eq!(journal.ack_tracked_facts().count(), 0);
    }

    #[test]
    fn test_gc_ack_tracking_partial() {
        use aura_core::query::ConsensusId;

        let auth_id = AuthorityId::new_from_entropy([22u8; 32]);
        let namespace = JournalNamespace::Authority(auth_id);
        let mut journal = Journal::new(namespace);
        let mut ack_storage = AckStorage::new();

        // Create two facts - one finalized, one provisional
        let fact1 = Fact::new(
            OrderTime([1u8; 32]),
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        )
        .with_ack_tracking()
        .with_agreement(Agreement::Finalized {
            consensus_id: ConsensusId([1u8; 32]),
        });

        let fact2 = Fact::new(
            OrderTime([2u8; 32]),
            TimeStamp::OrderClock(OrderTime([2u8; 32])),
            FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 2,
            }),
        )
        .with_ack_tracking()
        .with_agreement(Agreement::Provisional);

        journal.add_fact(fact1.clone()).unwrap();
        journal.add_fact(fact2.clone()).unwrap();

        // Record acks for both
        let peer = AuthorityId::new_from_entropy([23u8; 32]);
        ack_storage
            .record_ack(
                &fact1.order,
                peer,
                PhysicalTime {
                    ts_ms: 1000,
                    uncertainty: None,
                },
            )
            .unwrap();
        ack_storage
            .record_ack(
                &fact2.order,
                peer,
                PhysicalTime {
                    ts_ms: 2000,
                    uncertainty: None,
                },
            )
            .unwrap();

        assert_eq!(ack_storage.len(), 2);
        assert_eq!(journal.ack_tracked_facts().count(), 2);

        // GC only finalized facts
        let result = ack_storage.gc_by_consistency(&mut journal, |c| c.agreement.is_finalized());

        assert_eq!(result.facts_collected, 1);
        assert_eq!(result.facts_remaining, 1);
        assert_eq!(ack_storage.len(), 1);
        assert_eq!(journal.ack_tracked_facts().count(), 1);

        // Provisional fact should still have ack tracking
        let remaining_fact = journal.get_fact(&fact2.order).unwrap();
        assert!(remaining_fact.ack_tracked);
    }

}
