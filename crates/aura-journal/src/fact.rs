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

pub use aura_core::threshold::{ConvergenceCert, ReversionFact, RotateFact};
pub use aura_core::types::facts::{FactEncoding, FactEnvelope, FactTypeId};
use aura_core::{
    byzantine::ByzantineSafetyAttestation,
    domain::{Acknowledgment, Agreement, Consistency, OperationCategory, Propagation},
    hash::hash,
    semilattice::JoinSemilattice,
    time::{OrderTime, PhysicalTime, TimeStamp},
    types::identifiers::{AuthorityId, ChannelId, ContextId, SessionId},
    Hash32, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
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
        let _ = self.insert_fact_deduplicated(fact);
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

        let mut merged = Self::new(self.namespace.clone());
        for fact in self.facts.iter().cloned() {
            let _ = merged.insert_fact_deduplicated(fact);
        }
        for fact in other.facts.iter().cloned() {
            let _ = merged.insert_fact_deduplicated(fact);
        }
        merged
    }
}

impl Journal {
    /// In-place join operation that consumes the other journal
    ///
    /// Takes ownership of `other` to avoid cloning facts during merge.
    pub fn join_assign(&mut self, other: Self) {
        assert_eq!(
            self.namespace, other.namespace,
            "Cannot merge journals from different namespaces"
        );
        for fact in other.facts {
            let _ = self.insert_fact_deduplicated(fact);
        }
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
        let _ = self.insert_fact_deduplicated(fact);
        Ok(())
    }

    fn insert_fact_deduplicated(&mut self, fact: Fact) -> bool {
        let identity = fact.deduplication_id();
        if self
            .facts
            .iter()
            .any(|existing| existing.deduplication_id() == identity)
        {
            tracing::debug!(
                namespace = ?self.namespace,
                deduplication_id = %hex::encode(identity),
                "Ignoring duplicate fact submission"
            );
            false
        } else {
            self.facts.insert(fact)
        }
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
        self.facts.iter().filter(|f| f.agreement.is_provisional())
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

    /// Stable identifier for duplicate-submission detection.
    ///
    /// This uses order + timestamp plus canonicalized content bytes so the
    /// journal can reject retransmits even when an envelope payload is encoded
    /// differently but decodes to the same semantic content.
    pub fn deduplication_id(&self) -> Hash32 {
        let mut bytes = aura_core::util::serialization::to_vec(&self.order).unwrap_or_default();
        bytes.extend(aura_core::util::serialization::to_vec(&self.timestamp).unwrap_or_default());
        bytes.extend(self.content.canonical_identity_bytes());
        Hash32(hash(&bytes))
    }
}

impl PartialOrd for Fact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Fact {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // OrderTime is primary sort key. When it collides, use timestamp/content
        // tie-breakers so distinct facts are not dropped by BTreeSet.
        self.order
            .cmp(&other.order)
            .then_with(|| cmp_serialized(&self.timestamp, &other.timestamp))
            .then_with(|| cmp_serialized(&self.content, &other.content))
    }
}

fn cmp_serialized<T: Serialize>(left: &T, right: &T) -> std::cmp::Ordering {
    let left_bytes = aura_core::util::serialization::to_vec(left).unwrap_or_default();
    let right_bytes = aura_core::util::serialization::to_vec(right).unwrap_or_default();
    left_bytes.cmp(&right_bytes)
}

impl FactContent {
    fn canonical_identity_bytes(&self) -> Vec<u8> {
        match self {
            Self::Relational(RelationalFact::Generic {
                context_id,
                envelope,
            }) => aura_core::util::serialization::to_vec(&(
                context_id,
                canonicalize_fact_envelope(envelope),
            ))
            .unwrap_or_default(),
            _ => aura_core::util::serialization::to_vec(self).unwrap_or_default(),
        }
    }
}

fn canonicalize_fact_envelope(envelope: &FactEnvelope) -> FactEnvelope {
    if let Some(payload) = canonicalize_envelope_payload(envelope) {
        FactEnvelope {
            type_id: envelope.type_id.clone(),
            schema_version: envelope.schema_version,
            encoding: FactEncoding::DagCbor,
            payload,
        }
    } else {
        envelope.clone()
    }
}

fn canonicalize_envelope_payload(envelope: &FactEnvelope) -> Option<Vec<u8>> {
    match envelope.encoding {
        FactEncoding::DagCbor => {
            let value: JsonValue =
                aura_core::util::serialization::from_slice(&envelope.payload).ok()?;
            aura_core::util::serialization::to_vec(&value).ok()
        }
        FactEncoding::Json => {
            let value: JsonValue = serde_json::from_slice(&envelope.payload).ok()?;
            aura_core::util::serialization::to_vec(&value).ok()
        }
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

    /// Stable human-readable type label for journal projections.
    pub fn projection_type_label(&self) -> String {
        match self {
            FactContent::AttestedOp(_) => "AttestedOp".to_string(),
            FactContent::Relational(rel) => rel.projection_type_label(),
            FactContent::Snapshot(_) => "Snapshot".to_string(),
            FactContent::RendezvousReceipt { .. } => "RendezvousReceipt".to_string(),
        }
    }

    /// Human-readable content summary for journal projections.
    pub fn projection_summary(&self) -> String {
        match self {
            FactContent::AttestedOp(op) => format!("{:?} -> {:?}", op.tree_op, op.new_commitment),
            FactContent::Relational(rel) => rel.projection_summary(),
            FactContent::Snapshot(snap) => {
                format!(
                    "seq={}, superseded={}",
                    snap.sequence,
                    snap.superseded_facts.len()
                )
            }
            FactContent::RendezvousReceipt { envelope_id, .. } => {
                format!("envelope={}", hex::encode(&envelope_id[..8]))
            }
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

impl From<ChannelBumpReason> for AmpTransitionPolicy {
    fn from(reason: ChannelBumpReason) -> Self {
        match reason {
            ChannelBumpReason::Routine => Self::NormalTransition,
            ChannelBumpReason::SuspiciousActivity => Self::EmergencyQuarantineTransition,
            ChannelBumpReason::ConfirmedCompromise => Self::EmergencyCryptoshredTransition,
        }
    }
}

/// AMP channel epoch transition policy class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum AmpTransitionPolicy {
    /// Routine transition without special membership removal semantics.
    #[default]
    NormalTransition,
    /// Additive or non-removal transition that may allow bounded receive overlap.
    AdditiveTransition,
    /// Removal or revocation transition with stricter old-epoch acceptance.
    SubtractiveTransition,
    /// Emergency quarantine excluding a suspected compromised participant.
    EmergencyQuarantineTransition,
    /// Emergency transition that destroys ordinary pre-emergency readable state.
    EmergencyCryptoshredTransition,
}

/// Canonical identity for an AMP channel epoch transition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpTransitionIdentity {
    /// Relational context containing this channel.
    pub context: ContextId,
    /// Channel identifier for this transition.
    pub channel: ChannelId,
    /// Parent epoch being transitioned from.
    pub parent_epoch: u64,
    /// Commitment for the parent epoch prestate.
    pub parent_commitment: Hash32,
    /// Successor epoch being transitioned to.
    pub successor_epoch: u64,
    /// Commitment for the successor epoch state.
    pub successor_commitment: Hash32,
    /// Commitment to the successor membership set.
    pub membership_commitment: Hash32,
    /// Policy class governing data-plane and emergency behavior.
    pub transition_policy: AmpTransitionPolicy,
}

impl AmpTransitionIdentity {
    /// Build the transition identity currently derivable from legacy epoch bump fields.
    pub fn for_epoch_bump(
        context: ContextId,
        channel: ChannelId,
        parent_epoch: u64,
        successor_epoch: u64,
        bump_id: Hash32,
        reason: ChannelBumpReason,
    ) -> Self {
        Self {
            context,
            channel,
            parent_epoch,
            parent_commitment: Hash32::default(),
            successor_epoch,
            successor_commitment: bump_id,
            membership_commitment: Hash32::default(),
            transition_policy: reason.into(),
        }
    }

    /// Compute the canonical typed transition id.
    pub fn transition_id(&self) -> Hash32 {
        let bytes = aura_core::util::serialization::to_vec(&("aura.amp.transition.v1", self))
            .unwrap_or_default();
        Hash32(hash(&bytes))
    }
}

/// Witness signature over the canonical AMP transition payload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpTransitionWitnessSignature {
    /// Stable witness authority id from the parent epoch committee.
    pub witness: AuthorityId,
    /// Signature bytes over the canonical witness payload.
    pub signature: Vec<u8>,
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
    /// Commitment for the parent epoch prestate.
    #[serde(default)]
    pub parent_commitment: Hash32,
    /// Commitment for the proposed successor epoch.
    #[serde(default)]
    pub successor_commitment: Hash32,
    /// Commitment to the successor membership set.
    #[serde(default)]
    pub membership_commitment: Hash32,
    /// Transition policy class for this proposal.
    #[serde(default)]
    pub transition_policy: AmpTransitionPolicy,
    /// Canonical transition identity digest.
    #[serde(default)]
    pub transition_id: Hash32,
}

impl ProposedChannelEpochBump {
    /// Build a proposal with a canonical transition identity.
    pub fn new(
        context: ContextId,
        channel: ChannelId,
        parent_epoch: u64,
        new_epoch: u64,
        bump_id: Hash32,
        reason: ChannelBumpReason,
    ) -> Self {
        let identity = AmpTransitionIdentity::for_epoch_bump(
            context,
            channel,
            parent_epoch,
            new_epoch,
            bump_id,
            reason,
        );
        Self {
            context,
            channel,
            parent_epoch,
            new_epoch,
            bump_id,
            reason,
            parent_commitment: identity.parent_commitment,
            successor_commitment: identity.successor_commitment,
            membership_commitment: identity.membership_commitment,
            transition_policy: identity.transition_policy,
            transition_id: identity.transition_id(),
        }
    }

    /// Canonical transition identity bound by this proposal.
    pub fn transition_identity(&self) -> AmpTransitionIdentity {
        AmpTransitionIdentity {
            context: self.context,
            channel: self.channel,
            parent_epoch: self.parent_epoch,
            parent_commitment: self.parent_commitment,
            successor_epoch: self.new_epoch,
            successor_commitment: self.successor_commitment,
            membership_commitment: self.membership_commitment,
            transition_policy: self.transition_policy,
        }
    }
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
    /// Commitment for the parent epoch prestate.
    #[serde(default)]
    pub parent_commitment: Hash32,
    /// Commitment for the finalized successor epoch.
    #[serde(default)]
    pub successor_commitment: Hash32,
    /// Commitment to the successor membership set.
    #[serde(default)]
    pub membership_commitment: Hash32,
    /// Transition policy class for this commit.
    #[serde(default)]
    pub transition_policy: AmpTransitionPolicy,
    /// Canonical transition identity digest.
    #[serde(default)]
    pub transition_id: Hash32,
}

impl CommittedChannelEpochBump {
    /// Build a committed bump from a proposal and consensus evidence.
    pub fn from_proposal(
        proposal: &ProposedChannelEpochBump,
        consensus_id: Hash32,
        transcript_ref: Option<Hash32>,
    ) -> Self {
        Self {
            context: proposal.context,
            channel: proposal.channel,
            parent_epoch: proposal.parent_epoch,
            new_epoch: proposal.new_epoch,
            chosen_bump_id: proposal.bump_id,
            consensus_id,
            transcript_ref,
            parent_commitment: proposal.parent_commitment,
            successor_commitment: proposal.successor_commitment,
            membership_commitment: proposal.membership_commitment,
            transition_policy: proposal.transition_policy,
            transition_id: proposal.transition_id,
        }
    }
}

/// A2 soft-safe AMP channel epoch transition certificate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CertifiedChannelEpochBump {
    /// Canonical transition identity.
    pub identity: AmpTransitionIdentity,
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Digest of the canonical witness payload signed by every witness.
    pub witness_payload_digest: Hash32,
    /// Digest of the witness committee used for this certificate.
    pub committee_digest: Hash32,
    /// Required quorum threshold.
    pub threshold: u16,
    /// Declared Byzantine fault bound for this policy.
    pub fault_bound: u16,
    /// Optional coordinator fencing epoch.
    #[serde(default)]
    pub coord_epoch: Option<u64>,
    /// Inclusive lower generation bound for certificate validity.
    #[serde(default)]
    pub generation_min: Option<u64>,
    /// Inclusive upper generation bound for certificate validity.
    #[serde(default)]
    pub generation_max: Option<u64>,
    /// Witness signatures over the canonical transition payload.
    pub witness_signatures: Vec<AmpTransitionWitnessSignature>,
    /// Optional equivocation evidence references.
    #[serde(default)]
    pub equivocation_refs: BTreeSet<Hash32>,
    /// Authorities explicitly excluded by the successor membership policy.
    #[serde(default)]
    pub excluded_authorities: BTreeSet<AuthorityId>,
    /// Whether this certified transition represents readable-state destruction.
    #[serde(default)]
    pub readable_state_destroyed: bool,
}

/// A3 finalized AMP channel epoch transition commit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FinalizedChannelEpochBump {
    /// Canonical transition identity.
    pub identity: AmpTransitionIdentity,
    /// Canonical transition identity digest.
    pub transition_id: Hash32,
    /// Consensus instance identifier that finalized the transition.
    pub consensus_id: Hash32,
    /// Optional DKG transcript reference for finalized channel key material.
    #[serde(default)]
    pub transcript_ref: Option<Hash32>,
    /// Authorities explicitly excluded by the finalized successor membership.
    #[serde(default)]
    pub excluded_authorities: BTreeSet<AuthorityId>,
    /// Whether this finalized transition represents readable-state destruction.
    #[serde(default)]
    pub readable_state_destroyed: bool,
}

/// Scope affected by abort, conflict, or supersession evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum AmpTransitionSuppressionScope {
    /// Suppress only A2 live exposure.
    A2LiveOnly,
    /// Suppress both A2 live exposure and later A3 finalization.
    A2AndA3,
}

/// Explicit abort evidence for an AMP transition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpTransitionAbort {
    /// Relational context containing this channel.
    pub context: ContextId,
    /// Parent transition group this abort affects.
    pub channel: ChannelId,
    /// Parent epoch being transitioned from.
    pub parent_epoch: u64,
    /// Parent prestate commitment.
    pub parent_commitment: Hash32,
    /// Transition being invalidated.
    pub transition_id: Hash32,
    /// Authority or certificate evidence authorizing the abort.
    pub evidence_id: Hash32,
    /// Suppression scope.
    pub scope: AmpTransitionSuppressionScope,
}

/// Equivocation or conflict evidence for AMP transition certificates.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpTransitionConflict {
    /// Relational context containing this channel.
    pub context: ContextId,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Parent epoch being transitioned from.
    pub parent_epoch: u64,
    /// Parent prestate commitment.
    pub parent_commitment: Hash32,
    /// First conflicting transition id.
    pub first_transition_id: Hash32,
    /// Second conflicting transition id.
    pub second_transition_id: Hash32,
    /// Witness accused of duplicate-signing, if known.
    #[serde(default)]
    pub equivocating_witness: Option<AuthorityId>,
    /// Evidence digest for the conflict proof.
    pub evidence_id: Hash32,
}

/// Authorized supersession from one AMP transition path to another.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpTransitionSupersession {
    /// Relational context containing this channel.
    pub context: ContextId,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Parent epoch being transitioned from.
    pub parent_epoch: u64,
    /// Parent prestate commitment.
    pub parent_commitment: Hash32,
    /// Transition being replaced.
    pub superseded_transition_id: Hash32,
    /// Transition replacing it.
    pub superseding_transition_id: Hash32,
    /// Authority or certificate evidence authorizing supersession.
    pub evidence_id: Hash32,
    /// Suppression scope for the superseded transition.
    pub scope: AmpTransitionSuppressionScope,
}

/// Informational emergency alarm for an AMP channel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AmpEmergencyAlarm {
    /// Relational context containing this channel.
    pub context: ContextId,
    /// Channel identifier.
    pub channel: ChannelId,
    /// Parent epoch where suspicion was raised.
    pub parent_epoch: u64,
    /// Parent prestate commitment.
    pub parent_commitment: Hash32,
    /// Authority suspected of compromise.
    pub suspect: AuthorityId,
    /// Authority raising the alarm.
    pub raised_by: AuthorityId,
    /// Alarm evidence digest.
    pub evidence_id: Hash32,
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
    /// Optional Byzantine safety attestation captured for this transcript finalization.
    #[serde(default)]
    pub byzantine_attestation: Option<ByzantineSafetyAttestation>,
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

/// Session delegation event fact for reconfiguration audit trails.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionDelegationFact {
    /// Context in which delegation was performed.
    pub context_id: ContextId,
    /// Delegated session identifier.
    pub session_id: SessionId,
    /// Authority transferring ownership.
    pub from_authority: AuthorityId,
    /// Authority receiving delegated ownership.
    pub to_authority: AuthorityId,
    /// Optional composed bundle id.
    pub bundle_id: Option<String>,
    /// Physical delegation timestamp.
    pub timestamp: aura_core::time::PhysicalTime,
}

/// Protocol-level relational facts that must remain in `aura-journal`.
///
/// These facts are owned by `aura-journal` because they participate directly in
/// reduction semantics and cross-domain invariants. Domain facts must use
/// `RelationalFact::Generic` + `FactRegistry` instead.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProtocolRelationalFact {
    /// Guardian binding established between two authorities
    GuardianBinding {
        /// Account being bound to a guardian
        account_id: AuthorityId,
        /// Guardian authority
        guardian_id: AuthorityId,
        /// Hash of the binding agreement
        binding_hash: Hash32,
    },
    /// Recovery grant issued by a guardian
    RecoveryGrant {
        /// Account that can be recovered
        account_id: AuthorityId,
        /// Guardian granting recovery capability
        guardian_id: AuthorityId,
        /// Hash of the grant details
        grant_hash: Hash32,
    },
    /// Consensus result from Aura Consensus
    Consensus {
        /// Consensus operation identifier (as Hash32 to avoid circular dependency)
        consensus_id: Hash32,
        /// Hash of the operation being consensus'd
        operation_hash: Hash32,
        /// Whether consensus threshold was met
        threshold_met: bool,
        /// Number of participants in the consensus
        participant_count: u16,
    },
    /// AMP channel checkpoint anchoring ratchet windows
    AmpChannelCheckpoint(ChannelCheckpoint),
    /// Proposed channel epoch bump (optimistic)
    AmpProposedChannelEpochBump(ProposedChannelEpochBump),
    /// A2-certified AMP channel epoch bump (live, non-durable)
    AmpCertifiedChannelEpochBump(CertifiedChannelEpochBump),
    /// Committed channel epoch bump (final)
    AmpCommittedChannelEpochBump(CommittedChannelEpochBump),
    /// A3-finalized AMP channel epoch bump
    AmpFinalizedChannelEpochBump(FinalizedChannelEpochBump),
    /// Explicit AMP transition abort evidence
    AmpTransitionAbort(AmpTransitionAbort),
    /// AMP transition conflict or equivocation evidence
    AmpTransitionConflict(AmpTransitionConflict),
    /// Authorized AMP transition supersession
    AmpTransitionSupersession(AmpTransitionSupersession),
    /// Informational AMP emergency alarm
    AmpEmergencyAlarm(AmpEmergencyAlarm),
    /// Channel policy overrides
    AmpChannelPolicy(ChannelPolicy),
    /// AMP channel bootstrap metadata (dealer key)
    AmpChannelBootstrap(ChannelBootstrap),
    /// Leakage tracking event (privacy budget accounting)
    LeakageEvent(LeakageFact),
    /// Session delegation event for reconfiguration/migration.
    SessionDelegation(SessionDelegationFact),
    /// Finalized DKG transcript commit
    DkgTranscriptCommit(DkgTranscriptCommit),
    /// Coordinator convergence certificate (soft-safe)
    ConvergenceCert(ConvergenceCert),
    /// Explicit reversion fact (soft-safe)
    ReversionFact(ReversionFact),
    /// Rotation/upgrade marker for lifecycle transitions
    RotateFact(RotateFact),
}

fn derive_protocol_context_id(label: &[u8], parts: &[&[u8]]) -> ContextId {
    let mut input = Vec::new();
    input.extend_from_slice(label);
    for part in parts {
        input.extend_from_slice(part);
    }
    ContextId::new_from_entropy(hash(&input))
}

impl ProtocolRelationalFact {
    /// Stable reducer key for this protocol fact.
    pub fn binding_key(&self) -> ProtocolFactKey {
        match self {
            ProtocolRelationalFact::GuardianBinding {
                account_id,
                guardian_id,
                binding_hash,
            } => ProtocolFactKey::GuardianBinding {
                account_id: *account_id,
                guardian_id: *guardian_id,
                binding_hash: *binding_hash,
            },
            ProtocolRelationalFact::RecoveryGrant {
                account_id,
                guardian_id,
                grant_hash,
            } => ProtocolFactKey::RecoveryGrant {
                account_id: *account_id,
                guardian_id: *guardian_id,
                grant_hash: *grant_hash,
            },
            ProtocolRelationalFact::Consensus {
                consensus_id,
                operation_hash,
                ..
            } => ProtocolFactKey::Consensus {
                consensus_id: *consensus_id,
                operation_hash: *operation_hash,
            },
            ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint) => {
                ProtocolFactKey::AmpChannelCheckpoint {
                    channel: checkpoint.channel,
                    chan_epoch: checkpoint.chan_epoch,
                    ck_commitment: checkpoint.ck_commitment,
                }
            }
            ProtocolRelationalFact::AmpProposedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpProposedChannelEpochBump {
                    channel: bump.channel,
                    parent_epoch: bump.parent_epoch,
                    new_epoch: bump.new_epoch,
                    transition_id: bump.transition_id,
                }
            }
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpCertifiedChannelEpochBump {
                    channel: bump.identity.channel,
                    parent_epoch: bump.identity.parent_epoch,
                    successor_epoch: bump.identity.successor_epoch,
                    transition_id: bump.transition_id,
                }
            }
            ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpCommittedChannelEpochBump {
                    channel: bump.channel,
                    parent_epoch: bump.parent_epoch,
                    new_epoch: bump.new_epoch,
                    transition_id: bump.transition_id,
                }
            }
            ProtocolRelationalFact::AmpFinalizedChannelEpochBump(bump) => {
                ProtocolFactKey::AmpFinalizedChannelEpochBump {
                    channel: bump.identity.channel,
                    parent_epoch: bump.identity.parent_epoch,
                    successor_epoch: bump.identity.successor_epoch,
                    transition_id: bump.transition_id,
                }
            }
            ProtocolRelationalFact::AmpTransitionAbort(abort) => {
                ProtocolFactKey::AmpTransitionAbort {
                    channel: abort.channel,
                    parent_epoch: abort.parent_epoch,
                    transition_id: abort.transition_id,
                }
            }
            ProtocolRelationalFact::AmpTransitionConflict(conflict) => {
                ProtocolFactKey::AmpTransitionConflict {
                    channel: conflict.channel,
                    parent_epoch: conflict.parent_epoch,
                    first_transition_id: conflict.first_transition_id,
                    second_transition_id: conflict.second_transition_id,
                    evidence_id: conflict.evidence_id,
                }
            }
            ProtocolRelationalFact::AmpTransitionSupersession(supersession) => {
                ProtocolFactKey::AmpTransitionSupersession {
                    channel: supersession.channel,
                    parent_epoch: supersession.parent_epoch,
                    superseded_transition_id: supersession.superseded_transition_id,
                    superseding_transition_id: supersession.superseding_transition_id,
                }
            }
            ProtocolRelationalFact::AmpEmergencyAlarm(alarm) => {
                ProtocolFactKey::AmpEmergencyAlarm {
                    channel: alarm.channel,
                    parent_epoch: alarm.parent_epoch,
                    suspect: alarm.suspect,
                    evidence_id: alarm.evidence_id,
                }
            }
            ProtocolRelationalFact::AmpChannelPolicy(policy) => ProtocolFactKey::AmpChannelPolicy {
                channel: policy.channel,
            },
            ProtocolRelationalFact::AmpChannelBootstrap(bootstrap) => {
                ProtocolFactKey::AmpChannelBootstrap {
                    channel: bootstrap.channel,
                    bootstrap_id: bootstrap.bootstrap_id,
                }
            }
            ProtocolRelationalFact::LeakageEvent(event) => ProtocolFactKey::LeakageEvent {
                source: event.source,
                destination: event.destination,
                timestamp: event.timestamp.clone(),
            },
            ProtocolRelationalFact::SessionDelegation(event) => {
                ProtocolFactKey::SessionDelegation {
                    session_id: event.session_id,
                    from_authority: event.from_authority,
                    to_authority: event.to_authority,
                }
            }
            ProtocolRelationalFact::DkgTranscriptCommit(commit) => {
                ProtocolFactKey::DkgTranscriptCommit {
                    transcript_hash: commit.transcript_hash,
                }
            }
            ProtocolRelationalFact::ConvergenceCert(cert) => {
                ProtocolFactKey::ConvergenceCert { op_id: cert.op_id }
            }
            ProtocolRelationalFact::ReversionFact(reversion) => ProtocolFactKey::ReversionFact {
                op_id: reversion.op_id,
            },
            ProtocolRelationalFact::RotateFact(rotate) => ProtocolFactKey::RotateFact {
                to_state: rotate.to_state,
            },
        }
    }

    /// Relational context scope for this protocol fact.
    pub fn context_id(&self) -> ContextId {
        match self {
            ProtocolRelationalFact::GuardianBinding {
                account_id,
                guardian_id,
                ..
            } => derive_protocol_context_id(
                b"guardian-binding",
                &[&account_id.to_bytes(), &guardian_id.to_bytes()],
            ),
            ProtocolRelationalFact::RecoveryGrant {
                account_id,
                guardian_id,
                grant_hash,
            } => derive_protocol_context_id(
                b"recovery-grant",
                &[
                    &account_id.to_bytes(),
                    &guardian_id.to_bytes(),
                    grant_hash.as_bytes(),
                ],
            ),
            ProtocolRelationalFact::Consensus {
                consensus_id,
                operation_hash,
                ..
            } => derive_protocol_context_id(
                b"consensus",
                &[consensus_id.as_bytes(), operation_hash.as_bytes()],
            ),
            ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint) => checkpoint.context,
            ProtocolRelationalFact::AmpProposedChannelEpochBump(bump) => bump.context,
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(bump) => bump.identity.context,
            ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump) => bump.context,
            ProtocolRelationalFact::AmpFinalizedChannelEpochBump(bump) => bump.identity.context,
            ProtocolRelationalFact::AmpTransitionAbort(abort) => abort.context,
            ProtocolRelationalFact::AmpTransitionConflict(conflict) => conflict.context,
            ProtocolRelationalFact::AmpTransitionSupersession(supersession) => supersession.context,
            ProtocolRelationalFact::AmpEmergencyAlarm(alarm) => alarm.context,
            ProtocolRelationalFact::AmpChannelPolicy(policy) => policy.context,
            ProtocolRelationalFact::AmpChannelBootstrap(bootstrap) => bootstrap.context,
            ProtocolRelationalFact::LeakageEvent(event) => event.context_id,
            ProtocolRelationalFact::SessionDelegation(event) => event.context_id,
            ProtocolRelationalFact::DkgTranscriptCommit(commit) => commit.context,
            ProtocolRelationalFact::ConvergenceCert(cert) => cert.context,
            ProtocolRelationalFact::ReversionFact(reversion) => reversion.context,
            ProtocolRelationalFact::RotateFact(rotate) => rotate.context,
        }
    }

    /// Stable human-readable type label for journal projections.
    pub fn projection_type_label(&self) -> &'static str {
        match self {
            ProtocolRelationalFact::GuardianBinding { .. } => "GuardianBinding",
            ProtocolRelationalFact::RecoveryGrant { .. } => "RecoveryGrant",
            ProtocolRelationalFact::Consensus { .. } => "Consensus",
            ProtocolRelationalFact::AmpChannelCheckpoint(..) => "AmpChannelCheckpoint",
            ProtocolRelationalFact::AmpProposedChannelEpochBump(..) => {
                "AmpProposedChannelEpochBump"
            }
            ProtocolRelationalFact::AmpCertifiedChannelEpochBump(..) => {
                "AmpCertifiedChannelEpochBump"
            }
            ProtocolRelationalFact::AmpCommittedChannelEpochBump(..) => {
                "AmpCommittedChannelEpochBump"
            }
            ProtocolRelationalFact::AmpFinalizedChannelEpochBump(..) => {
                "AmpFinalizedChannelEpochBump"
            }
            ProtocolRelationalFact::AmpTransitionAbort(..) => "AmpTransitionAbort",
            ProtocolRelationalFact::AmpTransitionConflict(..) => "AmpTransitionConflict",
            ProtocolRelationalFact::AmpTransitionSupersession(..) => "AmpTransitionSupersession",
            ProtocolRelationalFact::AmpEmergencyAlarm(..) => "AmpEmergencyAlarm",
            ProtocolRelationalFact::AmpChannelPolicy(..) => "AmpChannelPolicy",
            ProtocolRelationalFact::AmpChannelBootstrap(..) => "AmpChannelBootstrap",
            ProtocolRelationalFact::LeakageEvent(..) => "LeakageEvent",
            ProtocolRelationalFact::SessionDelegation(..) => "SessionDelegation",
            ProtocolRelationalFact::DkgTranscriptCommit(..) => "DkgTranscriptCommit",
            ProtocolRelationalFact::ConvergenceCert(..) => "ConvergenceCert",
            ProtocolRelationalFact::ReversionFact(..) => "ReversionFact",
            ProtocolRelationalFact::RotateFact(..) => "RotateFact",
        }
    }
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
/// New protocol facts must be added to the `ProtocolRelationalFact` enum and documented
/// in `docs/105_journal.md` (criteria + reduction rules).
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
#[allow(clippy::large_enum_variant)]
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
        /// Typed fact envelope (contains type_id, schema_version, encoding, payload)
        ///
        /// This replaces the stringly-typed `binding_type: String, binding_data: Vec<u8>`
        /// pattern to eliminate double serialization and enable type-safe access.
        envelope: FactEnvelope,
    },
}

impl RelationalFact {
    /// Relational context scope for this fact.
    pub fn context_id(&self) -> ContextId {
        match self {
            RelationalFact::Protocol(protocol) => protocol.context_id(),
            RelationalFact::Generic { context_id, .. } => *context_id,
        }
    }

    /// Stable human-readable type label for journal projections.
    pub fn projection_type_label(&self) -> String {
        match self {
            RelationalFact::Protocol(protocol) => protocol.projection_type_label().to_string(),
            RelationalFact::Generic { envelope, .. } => {
                format!("Generic:{}", envelope.type_id.as_str())
            }
        }
    }

    /// Human-readable content summary for journal projections.
    pub fn projection_summary(&self) -> String {
        match self {
            RelationalFact::Protocol(_) => format!("{self:?}"),
            RelationalFact::Generic { envelope, .. } => String::from_utf8(envelope.payload.clone())
                .unwrap_or_else(|_| format!("{} bytes", envelope.payload.len())),
        }
    }
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
        /// Canonical transition identifier.
        transition_id: Hash32,
    },
    /// A2-certified AMP channel epoch bump.
    AmpCertifiedChannelEpochBump {
        /// The channel identifier.
        channel: ChannelId,
        /// The epoch being bumped from.
        parent_epoch: u64,
        /// The certified successor epoch.
        successor_epoch: u64,
        /// Canonical transition identifier.
        transition_id: Hash32,
    },
    /// Committed AMP channel epoch bump.
    AmpCommittedChannelEpochBump {
        /// The channel identifier.
        channel: ChannelId,
        /// The epoch being bumped from.
        parent_epoch: u64,
        /// The committed new epoch.
        new_epoch: u64,
        /// Canonical transition identifier.
        transition_id: Hash32,
    },
    /// A3-finalized AMP channel epoch bump.
    AmpFinalizedChannelEpochBump {
        /// The channel identifier.
        channel: ChannelId,
        /// The epoch being bumped from.
        parent_epoch: u64,
        /// The finalized successor epoch.
        successor_epoch: u64,
        /// Canonical transition identifier.
        transition_id: Hash32,
    },
    /// AMP transition abort evidence.
    AmpTransitionAbort {
        /// The channel identifier.
        channel: ChannelId,
        /// Parent epoch affected by the abort.
        parent_epoch: u64,
        /// Transition identifier being aborted.
        transition_id: Hash32,
    },
    /// AMP transition conflict evidence.
    AmpTransitionConflict {
        /// The channel identifier.
        channel: ChannelId,
        /// Parent epoch affected by the conflict.
        parent_epoch: u64,
        /// First conflicting transition id.
        first_transition_id: Hash32,
        /// Second conflicting transition id.
        second_transition_id: Hash32,
        /// Evidence digest.
        evidence_id: Hash32,
    },
    /// AMP transition supersession evidence.
    AmpTransitionSupersession {
        /// The channel identifier.
        channel: ChannelId,
        /// Parent epoch affected by supersession.
        parent_epoch: u64,
        /// Transition being replaced.
        superseded_transition_id: Hash32,
        /// Replacement transition.
        superseding_transition_id: Hash32,
    },
    /// AMP emergency alarm.
    AmpEmergencyAlarm {
        /// The channel identifier.
        channel: ChannelId,
        /// Parent epoch affected by alarm.
        parent_epoch: u64,
        /// Suspect authority.
        suspect: AuthorityId,
        /// Alarm evidence digest.
        evidence_id: Hash32,
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
    /// Session delegation event.
    SessionDelegation {
        /// Delegated session identifier.
        session_id: SessionId,
        /// Authority transferring ownership.
        from_authority: AuthorityId,
        /// Authority receiving ownership.
        to_authority: AuthorityId,
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
            ProtocolFactKey::AmpCertifiedChannelEpochBump { .. } => "amp-certified-epoch-bump",
            ProtocolFactKey::AmpCommittedChannelEpochBump { .. } => "amp-committed-epoch-bump",
            ProtocolFactKey::AmpFinalizedChannelEpochBump { .. } => "amp-finalized-epoch-bump",
            ProtocolFactKey::AmpTransitionAbort { .. } => "amp-transition-abort",
            ProtocolFactKey::AmpTransitionConflict { .. } => "amp-transition-conflict",
            ProtocolFactKey::AmpTransitionSupersession { .. } => "amp-transition-supersession",
            ProtocolFactKey::AmpEmergencyAlarm { .. } => "amp-emergency-alarm",
            ProtocolFactKey::AmpChannelPolicy { .. } => "amp-channel-policy",
            ProtocolFactKey::AmpChannelBootstrap { .. } => "amp-channel-bootstrap",
            ProtocolFactKey::LeakageEvent { .. } => "leakage-event",
            ProtocolFactKey::SessionDelegation { .. } => "session-delegation",
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
                transition_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                new_epoch,
                transition_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpCertifiedChannelEpochBump {
                channel,
                parent_epoch,
                successor_epoch,
                transition_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                successor_epoch,
                transition_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpCommittedChannelEpochBump {
                channel,
                parent_epoch,
                new_epoch,
                transition_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                new_epoch,
                transition_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpFinalizedChannelEpochBump {
                channel,
                parent_epoch,
                successor_epoch,
                transition_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                successor_epoch,
                transition_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpTransitionAbort {
                channel,
                parent_epoch,
                transition_id,
            } => aura_core::util::serialization::to_vec(&(channel, parent_epoch, transition_id))
                .unwrap_or_default(),
            ProtocolFactKey::AmpTransitionConflict {
                channel,
                parent_epoch,
                first_transition_id,
                second_transition_id,
                evidence_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                first_transition_id,
                second_transition_id,
                evidence_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpTransitionSupersession {
                channel,
                parent_epoch,
                superseded_transition_id,
                superseding_transition_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                superseded_transition_id,
                superseding_transition_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpEmergencyAlarm {
                channel,
                parent_epoch,
                suspect,
                evidence_id,
            } => aura_core::util::serialization::to_vec(&(
                channel,
                parent_epoch,
                suspect,
                evidence_id,
            ))
            .unwrap_or_default(),
            ProtocolFactKey::AmpChannelPolicy { channel } => {
                aura_core::util::serialization::to_vec(channel).unwrap_or_default()
            }
            ProtocolFactKey::AmpChannelBootstrap {
                channel,
                bootstrap_id,
            } => {
                aura_core::util::serialization::to_vec(&(channel, bootstrap_id)).unwrap_or_default()
            }
            ProtocolFactKey::LeakageEvent {
                source,
                destination,
                timestamp,
            } => aura_core::util::serialization::to_vec(&(source, destination, timestamp))
                .unwrap_or_default(),
            ProtocolFactKey::SessionDelegation {
                session_id,
                from_authority,
                to_authority,
            } => {
                aura_core::util::serialization::to_vec(&(session_id, from_authority, to_authority))
                    .unwrap_or_default()
            }
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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn json_generic_fact(payload: &[u8]) -> Fact {
        Fact::new(
            OrderTime([7u8; 32]),
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 42,
                uncertainty: Some(0),
            }),
            FactContent::Relational(RelationalFact::Generic {
                context_id: ContextId::new_from_entropy([9u8; 32]),
                envelope: FactEnvelope {
                    type_id: FactTypeId::from("test/v1"),
                    schema_version: 1,
                    encoding: FactEncoding::Json,
                    payload: payload.to_vec(),
                },
            }),
        )
    }

    #[test]
    fn journal_rejects_duplicate_json_facts_with_noncanonical_payload_bytes() {
        let namespace = JournalNamespace::Context(ContextId::new_from_entropy([1u8; 32]));
        let mut journal = Journal::new(namespace);

        journal
            .add_fact(json_generic_fact(br#"{"channel":"alpha","epoch":1}"#))
            .expect("first insert succeeds");
        journal
            .add_fact(json_generic_fact(
                br#"{ "channel" : "alpha", "epoch" : 1 }"#,
            ))
            .expect("duplicate semantic insert is ignored");

        assert_eq!(journal.size(), 1);
    }

    #[test]
    fn journal_join_deduplicates_equivalent_fact_retransmits() {
        let namespace = JournalNamespace::Context(ContextId::new_from_entropy([2u8; 32]));
        let mut left = Journal::new(namespace.clone());
        let mut right = Journal::new(namespace);

        left.add_fact(json_generic_fact(br#"{"channel":"alpha","epoch":1}"#))
            .expect("left insert succeeds");
        right
            .add_fact(json_generic_fact(
                br#"{ "channel" : "alpha", "epoch" : 1 }"#,
            ))
            .expect("right duplicate insert succeeds");

        left.join_assign(right);

        assert_eq!(left.size(), 1);
    }
}
