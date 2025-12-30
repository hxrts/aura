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
    identifiers::{AuthorityId, ChannelId, ContextId},
    semilattice::JoinSemilattice,
    time::{OrderTime, TimeStamp},
    Hash32, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
}

/// Core fact structure (timestamp-driven identity)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// Opaque total order for deterministic merges
    pub order: OrderTime,
    /// Semantic timestamp for ordering/identity
    pub timestamp: TimeStamp,
    /// Content of the fact
    pub content: FactContent,
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
        let mut journal2 = Journal::new(namespace.clone());

        // Add different facts to each journal
        let fact1 = Fact {
            order: OrderTime([1u8; 32]),
            timestamp: TimeStamp::OrderClock(OrderTime([1u8; 32])),
            content: FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        };

        let fact2 = Fact {
            order: OrderTime([2u8; 32]),
            timestamp: TimeStamp::OrderClock(OrderTime([2u8; 32])),
            content: FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 2,
            }),
        };

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
}
