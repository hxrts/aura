//! Fact-based journal implementation for Aura
//!
//! This module implements the new fact-based journal model that replaces
//! the graph-based KeyNode/KeyEdge approach. The journal is a semilattice
//! CRDT using set union for convergence.

use aura_core::{
    identifiers::{AuthorityId, ContextId},
    semilattice::JoinSemilattice,
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

    /// Check if the journal contains a specific fact
    pub fn contains(&self, fact_id: &FactId) -> bool {
        self.facts.iter().any(|f| &f.fact_id == fact_id)
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

/// Unique identifier for facts
///
/// # Effect System Integration
///
/// FactId should be created using UUIDs from `RandomEffects::random_uuid()` to maintain
/// the effect system boundaries. Direct construction with `new_v4()` bypasses the effect
/// system and prevents deterministic testing.
///
/// # Example
/// ```ignore
/// // Correct: Use RandomEffects
/// let uuid = random_effects.random_uuid().await;
/// let fact_id = FactId::from_uuid(uuid);
///
/// // Incorrect: Direct random generation (disallowed)
/// // let fact_id = FactId::new(); // This method has been removed
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FactId(pub uuid::Uuid);

impl FactId {
    /// Create a FactId from a UUID
    ///
    /// The UUID should be obtained from `RandomEffects::random_uuid()` to maintain
    /// effect system boundaries and enable deterministic testing.
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        Self(uuid)
    }

    /// Create a deterministic FactId from bytes (for testing)
    ///
    /// This should only be used in tests or when you have a deterministic UUID source.
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(uuid::Uuid::from_bytes(bytes))
    }
}

/// Core fact structure
///
/// Facts are immutable entries in the journal that represent
/// state changes or events in the system.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Fact {
    /// Unique identifier for this fact
    pub fact_id: FactId,
    /// Content of the fact
    pub content: FactContent,
}

/// Types of facts that can be stored in the journal
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FactContent {
    /// Attested operation on the commitment tree
    AttestedOp(AttestedOp),
    /// Relational fact for cross-authority coordination
    Relational(RelationalFact),
    /// Snapshot marker for garbage collection
    Snapshot(SnapshotFact),
    /// Flow budget spent counter update
    FlowBudget(FlowBudgetFact),
    /// Rendezvous receipt for tracking message flow
    RendezvousReceipt {
        /// Unique identifier of the envelope
        envelope_id: [u8; 32],
        /// Authority that issued this receipt
        authority_id: AuthorityId,
        /// Timestamp when the receipt was created
        timestamp: u64,
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
            FactContent::FlowBudget(_) => FactType::FlowBudget,
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
    /// Flow budget spent counter update
    FlowBudget,
    /// Rendezvous receipt for tracking message flow
    RendezvousReceipt,
}

/// Attested operation fact
///
/// Represents a threshold-signed operation on the commitment tree.
/// These facts drive the authority's internal state transitions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TreeOpKind {
    /// Add a new device/leaf to the tree
    AddLeaf {
        /// Public key of the new device (opaque bytes)
        public_key: Vec<u8>,
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

/// Relational fact for cross-authority relationships
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationalFact {
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
    /// Generic relational binding for extensibility
    Generic {
        /// Context in which this binding exists
        context_id: ContextId,
        /// Type of binding (domain-specific)
        binding_type: String,
        /// Serialized binding data
        binding_data: Vec<u8>,
    },
}

/// Snapshot fact for garbage collection
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SnapshotFact {
    /// Hash of the state at snapshot time
    pub state_hash: Hash32,
    /// Facts that can be garbage collected
    pub superseded_facts: Vec<FactId>,
    /// Snapshot sequence number
    pub sequence: u64,
}

/// Flow budget fact for tracking spent amounts
///
/// Only spent counters are stored as facts. Limits are computed
/// at runtime from Biscuit token evaluation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FlowBudgetFact {
    /// Relational context where budget was spent
    pub context_id: ContextId,
    /// Source authority that initiated the spending
    pub source: AuthorityId,
    /// Destination authority that received the flow
    pub destination: AuthorityId,
    /// Amount spent in this transaction (incremental counter)
    pub spent_amount: u64,
    /// Epoch number for this spending (for rotation tracking)
    pub epoch: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_journal_creation() {
        let auth_id = AuthorityId::new();
        let namespace = JournalNamespace::Authority(auth_id);
        let journal = Journal::new(namespace.clone());

        assert_eq!(journal.namespace, namespace);
        assert_eq!(journal.size(), 0);
    }

    #[test]
    fn test_journal_merge() {
        let auth_id = AuthorityId::new();
        let namespace = JournalNamespace::Authority(auth_id);

        let mut journal1 = Journal::new(namespace.clone());
        let mut journal2 = Journal::new(namespace.clone());

        // Add different facts to each journal
        let fact1 = Fact {
            fact_id: FactId::from_bytes([1u8; 16]),
            content: FactContent::Snapshot(SnapshotFact {
                state_hash: Hash32::default(),
                superseded_facts: vec![],
                sequence: 1,
            }),
        };

        let fact2 = Fact {
            fact_id: FactId::from_bytes([2u8; 16]),
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
        assert!(merged.contains(&fact1.fact_id));
        assert!(merged.contains(&fact2.fact_id));
    }

    #[test]
    #[should_panic(expected = "Cannot merge journals from different namespaces")]
    fn test_journal_merge_different_namespaces() {
        let namespace1 = JournalNamespace::Authority(AuthorityId::new());
        let namespace2 = JournalNamespace::Authority(AuthorityId::new());

        let journal1 = Journal::new(namespace1);
        let journal2 = Journal::new(namespace2);

        // This should panic
        let _ = journal1.join(&journal2);
    }
}
