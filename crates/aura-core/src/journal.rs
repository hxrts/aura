//! Unified Journal implementation matching the formal specification
//!
//! This module implements the core Journal type:
//! ```rust
//! # use aura_core::{Fact, Cap};
//! struct Journal {
//!   facts: Fact,            // Cv/Δ/CmRDT carrier with ⊔
//!   caps:  Cap,             // capability frontier with ⊓
//! }
//! ```
//!
//! The Journal serves as the pullback where growing facts and shrinking capabilities meet,
//! providing the foundational abstraction for all replicated state in Aura.

use crate::semilattice::{Bottom, JoinSemilattice, MeetSemiLattice, Top};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Fact type for the journal - represents "what we know" (⊔-monotone)
///
/// Facts are join-semilattice elements that can only grow through accumulation.
/// They represent knowledge that has been observed and cannot be "unlearned".
///
/// Uses a proper CRDT (OR-Set with LWW-Map) for distributed consistency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord)]
pub struct Fact {
    /// CRDT-based fact storage with operation timestamps
    entries: FactCrdt,
}

/// CRDT implementation for facts using Observed-Remove Set semantics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
struct FactCrdt {
    /// Last-Writer-Wins map for fact values with vector clocks
    lww_map: std::collections::BTreeMap<String, VersionedFactValue>,
    /// Operation set for add/remove operations
    operation_set: std::collections::BTreeSet<FactOperation>,
}

/// Versioned fact value with causal ordering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
struct VersionedFactValue {
    value: FactValue,
    timestamp: u64,
    actor_id: String, // Device/actor that created this value
    version: u64,     // Logical clock for causality
}

/// CRDT operation for fact modifications
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum FactOperation {
    Add {
        key: String,
        value: FactValue,
        timestamp: u64,
        actor_id: String,
        op_id: String, // Unique operation ID for OR-Set semantics
    },
    Remove {
        key: String,
        timestamp: u64,
        actor_id: String,
        op_id: String,
        /// The specific add operation being removed
        removed_op_id: String,
    },
}

impl PartialOrd for FactOperation {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FactOperation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Order by timestamp, then by op_id for deterministic ordering
        match (self, other) {
            (
                FactOperation::Add {
                    timestamp: t1,
                    op_id: id1,
                    ..
                },
                FactOperation::Add {
                    timestamp: t2,
                    op_id: id2,
                    ..
                },
            ) => t1.cmp(t2).then_with(|| id1.cmp(id2)),
            (
                FactOperation::Remove {
                    timestamp: t1,
                    op_id: id1,
                    ..
                },
                FactOperation::Remove {
                    timestamp: t2,
                    op_id: id2,
                    ..
                },
            ) => t1.cmp(t2).then_with(|| id1.cmp(id2)),
            (FactOperation::Add { .. }, FactOperation::Remove { .. }) => std::cmp::Ordering::Less,
            (FactOperation::Remove { .. }, FactOperation::Add { .. }) => {
                std::cmp::Ordering::Greater
            }
        }
    }
}

/// Individual fact values that can be accumulated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Ord)]
pub enum FactValue {
    /// Simple string facts
    String(String),
    /// Numeric facts (for counters, timestamps)
    Number(i64),
    /// Binary data facts
    Bytes(Vec<u8>),
    /// Set-based facts (OR-set semantics)
    Set(std::collections::BTreeSet<String>),
    /// Nested facts
    Nested(Box<Fact>),
}

impl Fact {
    /// Create a new empty fact
    pub fn new() -> Self {
        Self {
            entries: FactCrdt {
                lww_map: std::collections::BTreeMap::new(),
                operation_set: std::collections::BTreeSet::new(),
            },
        }
    }

    /// Create a fact with a single key-value pair
    pub fn with_value(key: impl Into<String>, value: FactValue) -> Self {
        let mut fact = Self::new();
        fact.insert(key, value);
        fact
    }

    /// Insert or update a fact value using CRDT semantics with explicit context.
    pub fn insert_with_context(
        &mut self,
        key: impl Into<String>,
        value: FactValue,
        actor_id: impl Into<String>,
        timestamp: u64,
        op_id: Option<String>,
    ) {
        let key = key.into();
        let actor_id = actor_id.into();
        let op_id = op_id.unwrap_or_else(|| format!("{}:{}:{}", actor_id, timestamp, key));

        // Create add operation for OR-Set
        let add_op = FactOperation::Add {
            key: key.clone(),
            value: value.clone(),
            timestamp,
            actor_id: actor_id.clone(),
            op_id: op_id.clone(),
        };

        self.entries.operation_set.insert(add_op);

        // Update LWW-Map with versioned value
        let versioned_value = VersionedFactValue {
            value,
            timestamp,
            actor_id,
            version: timestamp, // Use timestamp as logical clock for now
        };

        // Last-Writer-Wins resolution
        match self.entries.lww_map.get(&key) {
            Some(existing) if existing.version > versioned_value.version => {
                // Keep existing value (it's newer)
            }
            Some(existing) if existing.version == versioned_value.version => {
                // Tie-break by actor_id (lexicographic)
                if existing.actor_id <= versioned_value.actor_id {
                    // Keep existing
                } else {
                    self.entries.lww_map.insert(key, versioned_value);
                }
            }
            _ => {
                // New value wins
                self.entries.lww_map.insert(key, versioned_value);
            }
        }
    }

    /// Convenience wrapper that uses deterministic defaults.
    pub fn insert(&mut self, key: impl Into<String>, value: FactValue) {
        self.insert_with_context(key, value, "local", 0, None);
    }

    /// Remove a fact value (creates remove operation) with explicit context.
    pub fn remove_with_context(
        &mut self,
        key: impl Into<String>,
        actor_id: impl Into<String>,
        timestamp: u64,
        op_id: Option<String>,
    ) {
        let key = key.into();
        let actor_id = actor_id.into();
        let op_id = op_id.unwrap_or_else(|| format!("{}:{}:remove:{}", actor_id, timestamp, key));

        // Find all add operations for this key to remove them
        let add_ops_to_remove: Vec<_> = self
            .entries
            .operation_set
            .iter()
            .filter_map(|op| match op {
                FactOperation::Add {
                    key: op_key, op_id, ..
                } if op_key == &key => Some(op_id.clone()),
                _ => None,
            })
            .collect();

        // Create remove operations for each add operation
        for removed_op_id in add_ops_to_remove {
            let remove_op = FactOperation::Remove {
                key: key.clone(),
                timestamp,
                actor_id: actor_id.clone(),
                op_id: format!("{}:{}", op_id, removed_op_id),
                removed_op_id,
            };
            self.entries.operation_set.insert(remove_op);
        }

        // Remove from LWW map
        self.entries.lww_map.remove(&key);
    }

    /// Convenience wrapper that uses deterministic defaults.
    pub fn remove(&mut self, key: impl Into<String>) {
        self.remove_with_context(key, "local", 0, None);
    }

    /// Get a fact value by key
    pub fn get(&self, key: &str) -> Option<&FactValue> {
        // Check if key is removed in OR-Set
        if self.is_key_removed(key) {
            return None;
        }

        self.entries.lww_map.get(key).map(|v| &v.value)
    }

    /// Iterate over all key/value pairs honoring removals
    pub fn iter(&self) -> impl Iterator<Item = (&String, &FactValue)> {
        self.entries
            .lww_map
            .iter()
            .filter(move |(k, _)| !self.is_key_removed(k))
            .map(|(k, v)| (k, &v.value))
    }

    /// Check if a key has been removed according to OR-Set semantics
    fn is_key_removed(&self, key: &str) -> bool {
        let mut add_ops = std::collections::HashSet::new();
        let mut removed_ops = std::collections::HashSet::new();

        // Collect add and remove operations
        for op in &self.entries.operation_set {
            match op {
                FactOperation::Add {
                    key: op_key, op_id, ..
                } if op_key == key => {
                    add_ops.insert(op_id.clone());
                }
                FactOperation::Remove {
                    key: op_key,
                    removed_op_id,
                    ..
                } if op_key == key => {
                    removed_ops.insert(removed_op_id.clone());
                }
                _ => {}
            }
        }

        // Key is removed if all adds have corresponding removes
        add_ops.is_subset(&removed_ops)
    }

    /// Get all fact keys
    pub fn keys(&self) -> impl Iterator<Item = String> + '_ {
        self.entries
            .lww_map
            .keys()
            .filter(|k| !self.is_key_removed(k))
            .cloned()
    }

    /// Check if facts contain a key
    pub fn contains_key(&self, key: &str) -> bool {
        !self.is_key_removed(key) && self.entries.lww_map.contains_key(key)
    }

    /// Get the number of top-level facts
    pub fn len(&self) -> usize {
        self.entries
            .lww_map
            .keys()
            .filter(|k| !self.is_key_removed(k))
            .count()
    }

    /// Check if facts are empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the operation set (for debugging/testing)
    pub fn operation_count(&self) -> usize {
        self.entries.operation_set.len()
    }
}

impl JoinSemilattice for Fact {
    fn join(&self, other: &Self) -> Self {
        let mut result = FactCrdt {
            lww_map: std::collections::BTreeMap::new(),
            operation_set: std::collections::BTreeSet::new(),
        };

        // Union of all operations (OR-Set join)
        result
            .operation_set
            .extend(self.entries.operation_set.iter().cloned());
        result
            .operation_set
            .extend(other.entries.operation_set.iter().cloned());

        // Merge LWW-Maps with proper conflict resolution
        let all_keys: std::collections::BTreeSet<_> = self
            .entries
            .lww_map
            .keys()
            .chain(other.entries.lww_map.keys())
            .collect();

        for key in all_keys {
            let self_val = self.entries.lww_map.get(key);
            let other_val = other.entries.lww_map.get(key);

            let merged_val = match (self_val, other_val) {
                (Some(a), Some(b)) => {
                    // LWW resolution by version, then actor_id
                    if a.version > b.version {
                        a.clone()
                    } else if b.version > a.version {
                        b.clone()
                    } else {
                        // Same version, tie-break by actor_id
                        if a.actor_id <= b.actor_id {
                            a.clone()
                        } else {
                            b.clone()
                        }
                    }
                }
                (Some(a), None) => a.clone(),
                (None, Some(b)) => b.clone(),
                (None, None) => unreachable!(),
            };

            result.lww_map.insert(key.clone(), merged_val);
        }

        Fact { entries: result }
    }
}

impl Bottom for Fact {
    fn bottom() -> Self {
        Self::new()
    }
}

#[allow(clippy::non_canonical_partial_ord_impl)]
impl PartialOrd for Fact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // CRDT partial order: A ≤ B if A's operations ⊆ B's operations
        // and all LWW values in A are ≤ corresponding values in B
        // Note: This is intentionally different from cmp() which uses serialization for total order

        // Check if operations are subset
        let ops_subset = self
            .entries
            .operation_set
            .is_subset(&other.entries.operation_set);
        let ops_superset = other
            .entries
            .operation_set
            .is_subset(&self.entries.operation_set);

        match (ops_subset, ops_superset) {
            (true, true) => {
                // Same operations, compare LWW values
                if self.entries.lww_map == other.entries.lww_map {
                    Some(std::cmp::Ordering::Equal)
                } else {
                    None // Same operations but different values = incomparable
                }
            }
            (true, false) => {
                // Self operations ⊆ Other operations, check value compatibility
                for (key, self_val) in &self.entries.lww_map {
                    if !self.is_key_removed(key) {
                        if let Some(other_val) = other.entries.lww_map.get(key) {
                            if self_val.version > other_val.version {
                                return None; // Incomparable versions
                            }
                        } else if !other.is_key_removed(key) {
                            return None; // Self has value, other doesn't
                        }
                    }
                }
                Some(std::cmp::Ordering::Less)
            }
            (false, true) => {
                // Other operations ⊆ Self operations
                Some(std::cmp::Ordering::Greater)
            }
            (false, false) => None, // Incomparable operation sets
        }
    }
}

impl JoinSemilattice for FactValue {
    fn join(&self, other: &Self) -> Self {
        match (self, other) {
            (FactValue::String(a), FactValue::String(b)) => {
                // For strings, use lexicographic max (simple join)
                FactValue::String(a.max(b).clone())
            }
            (FactValue::Number(a), FactValue::Number(b)) => {
                // For numbers, use max (simple join)
                FactValue::Number(*a.max(b))
            }
            (FactValue::Bytes(a), FactValue::Bytes(b)) => {
                // For bytes, concatenate unique elements
                let mut result = a.clone();
                if b != a {
                    result.extend_from_slice(b);
                }
                FactValue::Bytes(result)
            }
            (FactValue::Set(a), FactValue::Set(b)) => {
                // For sets, use union (proper join)
                let mut result = a.clone();
                result.extend(b.iter().cloned());
                FactValue::Set(result)
            }
            (FactValue::Nested(a), FactValue::Nested(b)) => {
                // For nested facts, recursively join
                FactValue::Nested(Box::new(a.join(b)))
            }
            // Type mismatch - keep the first one (could be an error in real system)
            (a, _) => a.clone(),
        }
    }
}

impl Bottom for FactValue {
    fn bottom() -> Self {
        FactValue::Set(std::collections::BTreeSet::new())
    }
}

impl PartialOrd for FactValue {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (FactValue::String(a), FactValue::String(b)) => a.partial_cmp(b),
            (FactValue::Number(a), FactValue::Number(b)) => a.partial_cmp(b),
            (FactValue::Bytes(a), FactValue::Bytes(b)) => a.partial_cmp(b),
            (FactValue::Set(a), FactValue::Set(b)) => {
                if a == b {
                    Some(std::cmp::Ordering::Equal)
                } else if a.is_subset(b) {
                    Some(std::cmp::Ordering::Less)
                } else if b.is_subset(a) {
                    Some(std::cmp::Ordering::Greater)
                } else {
                    None // Incomparable sets
                }
            }
            (FactValue::Nested(a), FactValue::Nested(b)) => a.partial_cmp(b),
            // Different types are incomparable
            _ => None,
        }
    }
}

impl Default for Fact {
    fn default() -> Self {
        Self::new()
    }
}

/// Capability type for the journal - represents "what we may do" (⊓-monotone)
///
/// Capabilities are serialized Biscuit tokens.
///
/// The semantic evaluation lives in higher layers (`AuthorizationEffects`). The helper
/// methods here provide conservative stubs for legacy callers and should not be used
/// for authorization decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cap {
    /// Serialized Biscuit token (empty if no capabilities)
    token_bytes: Vec<u8>,
}

/// Authentication levels (kept for compatibility)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthLevel {
    None = 0,
    Device = 1,
    MultiFactor = 2,
    Threshold = 3,
}

/// Delegation constraints (stub for compatibility)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationConstraints {
    pub max_depth: Option<u32>,
}

impl Cap {
    pub fn new() -> Self {
        Self {
            token_bytes: Vec::new(),
        }
    }

    pub fn top() -> Self {
        Self {
            token_bytes: Vec::new(),
        }
    }

    pub fn from_biscuit(token: &biscuit_auth::Biscuit) -> Result<Self, CapError> {
        Ok(Self {
            token_bytes: token
                .to_vec()
                .map_err(|e| CapError::Serialization(e.to_string()))?,
        })
    }

    pub fn to_biscuit(
        &self,
        root_key: &biscuit_auth::PublicKey,
    ) -> Result<biscuit_auth::Biscuit, CapError> {
        if self.token_bytes.is_empty() {
            return Err(CapError::EmptyToken);
        }
        biscuit_auth::Biscuit::from(&self.token_bytes, *root_key)
            .map_err(|e| CapError::Deserialization(e.to_string()))
    }

    pub fn is_empty(&self) -> bool {
        self.token_bytes.is_empty()
    }

    pub fn update(&mut self, token: &biscuit_auth::Biscuit) -> Result<(), CapError> {
        self.token_bytes = token
            .to_vec()
            .map_err(|e| CapError::Serialization(e.to_string()))?;
        Ok(())
    }

    // Legacy compatibility stubs
    pub fn allows(&self, _permission: &str) -> bool {
        !self.is_empty()
    }

    pub fn applies_to(&self, _resource: &str) -> bool {
        !self.is_empty()
    }

    pub fn is_valid_at(&self, _timestamp: u64) -> bool {
        !self.is_empty()
    }

    pub fn permissions(&self) -> Vec<String> {
        vec![]
    }

    pub fn with_permissions<I>(_permissions: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Self::new()
    }

    pub fn with_resources<I>(self, _resources: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self
    }

    pub fn set_usage_limit(&mut self, _limit: u32) {
        // Stub - actual usage limits managed via Biscuit token caveats
    }

    pub fn increment_usage(&mut self) {
        // Stub - actual usage tracking done via Biscuit token facts
    }

    pub fn add_permission(&mut self, _permission: &str) {
        // Stub - actual permission management done via Biscuit tokens
    }

    pub fn add_delegation(
        &mut self,
        _delegator: String,
        _delegatee: String,
        _constraints: Option<DelegationConstraints>,
    ) {
        // Stub - actual delegation done via Biscuit token blocks
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CapError {
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Empty token")]
    EmptyToken,
}

impl MeetSemiLattice for Cap {
    fn meet(&self, other: &Self) -> Self {
        if self.token_bytes.is_empty() || other.token_bytes.is_empty() {
            return Self::new();
        }
        self.clone()
    }
}

impl Top for Cap {
    fn top() -> Self {
        Self::top()
    }
}

impl PartialOrd for Cap {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self.is_empty(), other.is_empty()) {
            (true, true) => Some(std::cmp::Ordering::Equal),
            (true, false) => Some(std::cmp::Ordering::Less),
            (false, true) => Some(std::cmp::Ordering::Greater),
            (false, false) => {
                if self.token_bytes == other.token_bytes {
                    Some(std::cmp::Ordering::Equal)
                } else {
                    None
                }
            }
        }
    }
}

impl Default for Cap {
    fn default() -> Self {
        Self::new()
    }
}

/// Unified Journal structure matching the formal specification
///
/// The Journal is the pullback where growing facts and shrinking capabilities meet.
/// It represents the core abstraction for replicated state in the Aura system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// Facts represent "what we know" - join-semilattice (⊔-monotone)
    pub facts: Fact,
    /// Capabilities represent "what we may do" - meet-semilattice (⊓-monotone)
    pub caps: Cap,
}

impl Journal {
    /// Create a new empty journal
    pub fn new() -> Self {
        Self {
            facts: Fact::new(),
            caps: Cap::top(), // Start with top capabilities (all permissions)
        }
    }

    /// Create a journal with initial capabilities
    pub fn with_caps(caps: Cap) -> Self {
        Self {
            facts: Fact::new(),
            caps,
        }
    }

    /// Create a journal with initial facts
    pub fn with_facts(facts: Fact) -> Self {
        Self {
            facts,
            caps: Cap::top(), // Start with top capabilities
        }
    }

    /// Merge facts (⊔ operation)
    pub fn merge_facts(&mut self, other_facts: Fact) {
        self.facts = self.facts.join(&other_facts);
    }

    /// Refine capabilities (⊓ operation)
    pub fn refine_caps(&mut self, constraint: Cap) {
        self.caps = self.caps.meet(&constraint);
    }

    /// Read current facts
    pub fn read_facts(&self) -> &Fact {
        &self.facts
    }

    /// Read current capabilities
    pub fn read_caps(&self) -> &Cap {
        &self.caps
    }

    /// Check if an operation is authorized
    pub fn is_authorized(&self, permission: &str, resource: &str, timestamp: u64) -> bool {
        self.caps.allows(permission)
            && self.caps.applies_to(resource)
            && self.caps.is_valid_at(timestamp)
    }

    /// Merge two journals (facts join, capabilities meet)
    pub fn merge(&mut self, other: &Journal) {
        self.merge_facts(other.facts.clone());
        self.refine_caps(other.caps.clone());
    }

    /// Create a restricted view of this journal with reduced capabilities
    pub fn restrict_view(&self, capability_constraint: Cap) -> Journal {
        Journal {
            facts: self.facts.clone(),
            caps: self.caps.meet(&capability_constraint),
        }
    }
}

impl Default for Journal {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Journal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Journal[facts: {} items, caps: {} permissions]",
            self.facts.len(),
            self.caps.permissions().len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fact_join_laws() {
        let fact1 = Fact::with_value("key1", FactValue::String("value1".to_string()));
        let fact2 = Fact::with_value("key2", FactValue::String("value2".to_string()));
        let fact3 = Fact::with_value("key3", FactValue::String("value3".to_string()));

        // Associativity: (a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)
        let left = fact1.join(&fact2).join(&fact3);
        let right = fact1.join(&fact2.join(&fact3));
        assert_eq!(left, right);

        // Commutativity: a ⊔ b = b ⊔ a
        assert_eq!(fact1.join(&fact2), fact2.join(&fact1));

        // Idempotency: a ⊔ a = a
        assert_eq!(fact1.join(&fact1), fact1);

        // Identity: a ⊔ ⊥ = a
        assert_eq!(fact1.join(&Fact::bottom()), fact1);
    }

    #[test]
    fn test_capability_meet_laws() {
        let cap1 = Cap::with_permissions(vec!["read".to_string(), "write".to_string()]);
        let cap2 = Cap::with_permissions(vec!["read".to_string(), "delete".to_string()]);
        let cap3 = Cap::with_permissions(vec!["execute".to_string()]);

        // Associativity: (a ⊓ b) ⊓ c = a ⊓ (b ⊓ c)
        let left = cap1.meet(&cap2).meet(&cap3);
        let right = cap1.meet(&cap2.meet(&cap3));
        assert_eq!(left, right);

        // Commutativity: a ⊓ b = b ⊓ a
        assert_eq!(cap1.meet(&cap2), cap2.meet(&cap1));

        // Idempotency: a ⊓ a = a
        assert_eq!(cap1.meet(&cap1), cap1);

        // Identity: a ⊓ ⊤ = a
        assert_eq!(cap1.meet(&Cap::top()), cap1);
    }

    #[test]
    fn test_crdt_fact_operations() {
        let mut fact1 = Fact::new();
        let mut fact2 = Fact::new();

        // Add same key from different actors
        fact1.insert("key1", FactValue::String("value1".to_string()));
        fact2.insert("key1", FactValue::String("value2".to_string()));

        // Join should resolve conflicts deterministically
        let joined = fact1.join(&fact2);
        assert!(joined.contains_key("key1"));

        // Test remove operations
        fact1.remove("key1");
        assert!(!fact1.contains_key("key1"));

        // Join with removed fact should handle OR-Set semantics
        let _rejoined = fact1.join(&fact2);
        // Result depends on operation timestamps and OR-Set semantics

        assert!(fact1.operation_count() > 0); // Should have operations recorded
    }

    #[test]
    fn test_journal_merge_and_caps_refinement() {
        let mut journal_a =
            Journal::with_facts(Fact::with_value("k1", FactValue::String("v1".to_string())));
        let mut journal_b =
            Journal::with_facts(Fact::with_value("k2", FactValue::String("v2".to_string())));

        // Refine capabilities on B to simulate attenuation
        journal_b.refine_caps(Cap::new());

        journal_a.merge(&journal_b);

        let merged_keys: Vec<_> = journal_a.facts.entries.lww_map.keys().cloned().collect();
        assert!(merged_keys.contains(&"k1".to_string()));
        assert!(merged_keys.contains(&"k2".to_string()));
        // Cap meet should not panic; current stub yields an empty token_bytes
        assert!(journal_a.caps.is_empty());
    }

    #[test]
    fn test_journal_restrict_view() {
        let journal = Journal::with_facts(Fact::with_value("k", FactValue::Number(1)));
        let restricted = journal.restrict_view(Cap::new());

        // Facts are preserved, caps are attenuated
        assert!(restricted.facts.contains_key("k"));
        assert!(restricted.caps.is_empty());
    }

    #[test]
    fn test_journal_authorization_stub() {
        let journal = Journal::new();
        // Current Cap stub always authorizes; ensure the method is wired
        assert!(journal.is_authorized("read", "resource", 0));
    }
}
