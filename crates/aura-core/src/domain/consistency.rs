//! Unified Consistency Types
//!
//! This module provides unified consistency metadata for facts across all
//! operation categories. It combines Agreement, Propagation, and Acknowledgment
//! into a single coherent view.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────┐
//! │                         Consistency                                      │
//! │                                                                          │
//! │  agreement: Agreement::Finalized { ... } ──────────► is_finalized: true  │
//! │                                                                          │
//! │  acknowledgment: Some(Acknowledgment {                                   │
//! │      acked_by: [alice, bob, carol]       ──────────► is_delivered: true  │
//! │  })                                           (if all expected acked)    │
//! │                                                                          │
//! │  propagation: Propagation::Complete      ──────────► (not embedded,      │
//! │                                                       query if needed)   │
//! └─────────────────────────────────────────────────────────────────────────┘
//! ```

use super::acknowledgment::{AckRecord, Acknowledgment};
use super::agreement::Agreement;
use super::propagation::Propagation;
use crate::types::AuthorityId;
use crate::CeremonyId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Operation Category
// ─────────────────────────────────────────────────────────────────────────────

/// Unique identifier for a proposal (Category B operations)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProposalId(pub String);

impl ProposalId {
    /// Create a new proposal ID
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Get the string representation
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProposalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for ProposalId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ProposalId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

/// Operation category from docs/117_operation_categories.md
///
/// Aura classifies operations into three categories based on their
/// coordination requirements:
///
/// - **Category A (Optimistic)**: Immediate local effect, background sync
/// - **Category B (Deferred)**: Requires approval before taking effect
/// - **Category C (Ceremony)**: Blocks until ceremony completes
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationCategory {
    /// Category A: Optimistic, immediate effect.
    ///
    /// Operations apply immediately to local state and are synced
    /// in the background. Examples: send message, update profile.
    Optimistic,

    /// Category B: Deferred until approval.
    ///
    /// Operations create proposals that require approval before
    /// taking effect. Examples: change permissions, remove member.
    Deferred {
        /// The proposal this operation belongs to
        proposal_id: ProposalId,
    },

    /// Category C: Blocked until ceremony completes.
    ///
    /// Operations block until a multi-party ceremony completes.
    /// Examples: add contact, guardian rotation.
    Ceremony {
        /// The ceremony this operation belongs to
        ceremony_id: CeremonyId,
    },
}

impl OperationCategory {
    /// Create an optimistic category
    pub fn optimistic() -> Self {
        Self::Optimistic
    }

    /// Create a deferred category
    pub fn deferred(proposal_id: impl Into<ProposalId>) -> Self {
        Self::Deferred {
            proposal_id: proposal_id.into(),
        }
    }

    /// Create a ceremony category
    pub fn ceremony(ceremony_id: impl Into<CeremonyId>) -> Self {
        Self::Ceremony {
            ceremony_id: ceremony_id.into(),
        }
    }

    /// Check if this is an optimistic operation
    pub fn is_optimistic(&self) -> bool {
        matches!(self, Self::Optimistic)
    }

    /// Check if this is a deferred operation
    pub fn is_deferred(&self) -> bool {
        matches!(self, Self::Deferred { .. })
    }

    /// Check if this is a ceremony operation
    pub fn is_ceremony(&self) -> bool {
        matches!(self, Self::Ceremony { .. })
    }

    /// Get the proposal ID if deferred
    pub fn proposal_id(&self) -> Option<&ProposalId> {
        match self {
            Self::Deferred { proposal_id } => Some(proposal_id),
            _ => None,
        }
    }

    /// Get the ceremony ID if ceremony
    pub fn ceremony_id(&self) -> Option<&CeremonyId> {
        match self {
            Self::Ceremony { ceremony_id } => Some(ceremony_id),
            _ => None,
        }
    }
}

impl std::fmt::Display for OperationCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Optimistic => write!(f, "Optimistic"),
            Self::Deferred { proposal_id } => write!(f, "Deferred({proposal_id})"),
            Self::Ceremony { ceremony_id } => write!(f, "Ceremony({ceremony_id})"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified Consistency
// ─────────────────────────────────────────────────────────────────────────────

/// Unified consistency metadata for any fact.
///
/// Use when you need to handle facts from any category uniformly.
/// Combines the three orthogonal dimensions of consistency:
///
/// - **Agreement**: A1/A2/A3 finalization level
/// - **Propagation**: Anti-entropy sync status
/// - **Acknowledgment**: Per-peer delivery confirmation (opt-in)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consistency {
    /// What category was this operation?
    pub category: OperationCategory,

    /// Agreement level (A1/A2/A3)
    pub agreement: Agreement,

    /// Propagation status
    pub propagation: Propagation,

    /// Acknowledgment (Category A with ack tracking only)
    pub acknowledgment: Option<Acknowledgment>,
}

impl Default for Consistency {
    fn default() -> Self {
        Self {
            category: OperationCategory::Optimistic,
            agreement: Agreement::Provisional,
            propagation: Propagation::Local,
            acknowledgment: None,
        }
    }
}

impl Consistency {
    /// Create new consistency metadata
    pub fn new(category: OperationCategory) -> Self {
        Self {
            category,
            agreement: Agreement::Provisional,
            propagation: Propagation::Local,
            acknowledgment: None,
        }
    }

    /// Create optimistic consistency with default values
    pub fn optimistic() -> Self {
        Self::new(OperationCategory::Optimistic)
    }

    /// Create deferred consistency
    pub fn deferred(proposal_id: impl Into<ProposalId>) -> Self {
        Self::new(OperationCategory::deferred(proposal_id))
    }

    /// Create ceremony consistency
    pub fn ceremony(ceremony_id: impl Into<CeremonyId>) -> Self {
        Self::new(OperationCategory::ceremony(ceremony_id))
    }

    /// Set the agreement level
    #[must_use]
    pub fn with_agreement(mut self, agreement: Agreement) -> Self {
        self.agreement = agreement;
        self
    }

    /// Set the propagation status
    #[must_use]
    pub fn with_propagation(mut self, propagation: Propagation) -> Self {
        self.propagation = propagation;
        self
    }

    /// Set the acknowledgment
    #[must_use]
    pub fn with_acknowledgment(mut self, acknowledgment: Acknowledgment) -> Self {
        self.acknowledgment = Some(acknowledgment);
        self
    }

    /// Enable ack tracking (empty acknowledgment)
    #[must_use]
    pub fn with_ack_tracking(mut self) -> Self {
        self.acknowledgment = Some(Acknowledgment::new());
        self
    }

    /// Quick check: is this finalized (A3)?
    pub fn is_finalized(&self) -> bool {
        self.agreement.is_finalized()
    }

    /// Quick check: is this at least safe (A2+)?
    pub fn is_safe(&self) -> bool {
        self.agreement.is_safe()
    }

    /// Quick check: is propagation complete?
    pub fn is_propagated(&self) -> bool {
        self.propagation.is_complete()
    }

    /// Quick check: is ack tracking enabled?
    pub fn is_ack_tracked(&self) -> bool {
        self.acknowledgment.is_some()
    }

    /// Check if delivered to all expected peers
    pub fn is_delivered(&self, expected: &[AuthorityId]) -> bool {
        self.acknowledgment
            .as_ref()
            .map(|ack| ack.all_acked(expected))
            .unwrap_or(false)
    }

    /// Get ack count
    pub fn ack_count(&self) -> usize {
        self.acknowledgment
            .as_ref()
            .map(|a| a.count())
            .unwrap_or(0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Consistency Map
// ─────────────────────────────────────────────────────────────────────────────

/// Map from item ID to consistency state.
///
/// Used in query results to provide consistency metadata for each returned item.
///
/// # Example
///
/// ```ignore
/// let result = effects.query(&ChannelsQuery::default()).await?;
/// for channel in &result.items {
///     if result.consistency.is_finalized(&channel.id) {
///         println!("{} is finalized", channel.name);
///     }
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConsistencyMap {
    entries: HashMap<String, Consistency>,
}

impl ConsistencyMap {
    /// Create an empty consistency map
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert consistency for an item
    pub fn insert(&mut self, id: impl Into<String>, consistency: Consistency) {
        self.entries.insert(id.into(), consistency);
    }

    /// Get consistency for an item
    pub fn get(&self, id: &str) -> Option<&Consistency> {
        self.entries.get(id)
    }

    /// Check if an item is finalized
    pub fn is_finalized(&self, id: &str) -> bool {
        self.get(id)
            .map(|c| c.agreement.is_finalized())
            .unwrap_or(false)
    }

    /// Check if an item is safe (A2+)
    pub fn is_safe(&self, id: &str) -> bool {
        self.get(id)
            .map(|c| c.agreement.is_safe())
            .unwrap_or(false)
    }

    /// Get the ack records for an item
    pub fn acked_by(&self, id: &str) -> Option<&[AckRecord]> {
        self.get(id)
            .and_then(|c| c.acknowledgment.as_ref())
            .map(|a| a.acked_by.as_slice())
    }

    /// Get the propagation status for an item
    pub fn propagation(&self, id: &str) -> Option<&Propagation> {
        self.get(id).map(|c| &c.propagation)
    }

    /// Get the agreement level for an item
    pub fn agreement(&self, id: &str) -> Option<&Agreement> {
        self.get(id).map(|c| &c.agreement)
    }

    /// Get the operation category for an item
    pub fn category(&self, id: &str) -> Option<&OperationCategory> {
        self.get(id).map(|c| &c.category)
    }

    /// Check if the map contains an entry for an item
    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    /// Get the number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all entries
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Consistency)> {
        self.entries.iter()
    }

    /// Get all item IDs
    pub fn ids(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    /// Merge another map into this one
    pub fn merge(&mut self, other: ConsistencyMap) {
        self.entries.extend(other.entries);
    }
}

impl FromIterator<(String, Consistency)> for ConsistencyMap {
    fn from_iter<T: IntoIterator<Item = (String, Consistency)>>(iter: T) -> Self {
        Self {
            entries: iter.into_iter().collect(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::ConsensusId;
    use crate::time::PhysicalTime;
    use uuid::Uuid;

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    fn test_time(millis: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: millis,
            uncertainty: None,
        }
    }

    #[test]
    fn test_operation_category() {
        let opt = OperationCategory::optimistic();
        assert!(opt.is_optimistic());
        assert!(!opt.is_deferred());
        assert!(!opt.is_ceremony());

        let def = OperationCategory::deferred("prop-1");
        assert!(def.is_deferred());
        assert_eq!(def.proposal_id().map(|p| p.as_str()), Some("prop-1"));

        let cer = OperationCategory::ceremony(CeremonyId::new("cer-1"));
        assert!(cer.is_ceremony());
    }

    #[test]
    fn test_consistency_new() {
        let c = Consistency::optimistic();
        assert!(c.category.is_optimistic());
        assert!(!c.is_finalized());
        assert!(!c.is_ack_tracked());
    }

    #[test]
    fn test_consistency_with_builders() {
        let consensus_id = ConsensusId::new([1; 32]);

        let c = Consistency::optimistic()
            .with_agreement(Agreement::finalized(consensus_id))
            .with_propagation(Propagation::complete())
            .with_ack_tracking();

        assert!(c.is_finalized());
        assert!(c.is_propagated());
        assert!(c.is_ack_tracked());
    }

    #[test]
    fn test_consistency_is_delivered() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);

        let ack = Acknowledgment::new()
            .add_ack(peer1, test_time(1000));

        let c = Consistency::optimistic().with_acknowledgment(ack);

        // Delivered to peer1 only
        assert!(c.is_delivered(&[peer1]));
        assert!(!c.is_delivered(&[peer1, peer2]));
    }

    #[test]
    fn test_consistency_map() {
        let mut map = ConsistencyMap::new();

        let c1 = Consistency::optimistic()
            .with_agreement(Agreement::finalized(ConsensusId::new([1; 32])));
        let c2 = Consistency::optimistic();

        map.insert("item-1", c1);
        map.insert("item-2", c2);

        assert!(map.is_finalized("item-1"));
        assert!(!map.is_finalized("item-2"));
        assert!(!map.is_finalized("item-3")); // doesn't exist

        assert!(map.contains("item-1"));
        assert!(!map.contains("item-3"));
        assert_eq!(map.len(), 2);
    }

    #[test]
    fn test_consistency_map_accessors() {
        let mut map = ConsistencyMap::new();

        let c = Consistency::optimistic()
            .with_propagation(Propagation::syncing(3, 5))
            .with_agreement(Agreement::soft_safe());

        map.insert("item-1", c);

        assert!(map.is_safe("item-1"));
        assert!(matches!(
            map.propagation("item-1"),
            Some(Propagation::Syncing { .. })
        ));
        assert!(matches!(
            map.agreement("item-1"),
            Some(Agreement::SoftSafe { .. })
        ));
        assert!(matches!(
            map.category("item-1"),
            Some(OperationCategory::Optimistic)
        ));
    }

    #[test]
    fn test_consistency_map_merge() {
        let mut map1 = ConsistencyMap::new();
        map1.insert("a", Consistency::optimistic());

        let mut map2 = ConsistencyMap::new();
        map2.insert("b", Consistency::optimistic());

        map1.merge(map2);

        assert!(map1.contains("a"));
        assert!(map1.contains("b"));
        assert_eq!(map1.len(), 2);
    }

    #[test]
    fn test_proposal_id() {
        let id = ProposalId::new("test-proposal");
        assert_eq!(id.as_str(), "test-proposal");
        assert_eq!(id.to_string(), "test-proposal");

        let id2: ProposalId = "another".into();
        assert_eq!(id2.as_str(), "another");
    }
}
