//! Unified Journal implementation matching the formal specification
//!
//! This module implements the core Journal type from the whole system model:
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// The underlying data structure storing accumulated facts
    /// Using a simple map TODO fix - For now, but this would be backed by a proper CRDT
    data: std::collections::BTreeMap<String, FactValue>,
}

/// Individual fact values that can be accumulated
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            data: std::collections::BTreeMap::new(),
        }
    }

    /// Create a fact with a single key-value pair
    pub fn with_value(key: impl Into<String>, value: FactValue) -> Self {
        let mut fact = Self::new();
        fact.data.insert(key.into(), value);
        fact
    }

    /// Insert or update a fact value
    pub fn insert(&mut self, key: impl Into<String>, value: FactValue) {
        let key = key.into();

        match (self.data.get(&key), &value) {
            // Join existing fact with new fact
            (Some(existing), new_value) => {
                let joined = existing.join(new_value);
                self.data.insert(key, joined);
            }
            // No existing fact, just insert
            (None, _) => {
                self.data.insert(key, value);
            }
        }
    }

    /// Get a fact value by key
    pub fn get(&self, key: &str) -> Option<&FactValue> {
        self.data.get(key)
    }

    /// Get all fact keys
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.data.keys()
    }

    /// Check if facts contain a key
    pub fn contains_key(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }

    /// Get the number of top-level facts
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if facts are empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl JoinSemilattice for Fact {
    fn join(&self, other: &Self) -> Self {
        let mut result = self.clone();

        for (key, other_value) in &other.data {
            match result.data.get(key) {
                Some(existing_value) => {
                    let joined = existing_value.join(other_value);
                    result.data.insert(key.clone(), joined);
                }
                None => {
                    result.data.insert(key.clone(), other_value.clone());
                }
            }
        }

        result
    }
}

impl Bottom for Fact {
    fn bottom() -> Self {
        Self::new()
    }
}

impl PartialOrd for Fact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Fact A ≤ Fact B if all facts in A are ≤ corresponding facts in B
        let all_leq = true;
        let mut any_lt = false;

        for (key, self_value) in &self.data {
            match other.data.get(key) {
                Some(other_value) => match self_value.partial_cmp(other_value)? {
                    std::cmp::Ordering::Greater => return None,
                    std::cmp::Ordering::Less => any_lt = true,
                    std::cmp::Ordering::Equal => {}
                },
                None => {
                    // Self has a fact that other doesn't have
                    return None;
                }
            }
        }

        // Check if other has facts that self doesn't have
        for key in other.data.keys() {
            if !self.data.contains_key(key) {
                any_lt = true;
            }
        }

        match (all_leq, any_lt) {
            (true, true) => Some(std::cmp::Ordering::Less),
            (true, false) => Some(std::cmp::Ordering::Equal),
            (false, _) => None,
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
/// Capabilities are meet-semilattice elements that can only shrink through refinement.
/// They represent authority that can be attenuated but never amplified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cap {
    /// The underlying capability set
    /// Using a simple set TODO fix - For now, but this would be a proper capability lattice
    permissions: std::collections::BTreeSet<String>,
    /// Resource constraints (what resources these capabilities apply to)
    resources: std::collections::BTreeSet<String>,
    /// Time constraints (when these capabilities are valid)
    valid_until: Option<u64>, // Unix timestamp
}

impl Cap {
    /// Create a new capability set
    pub fn new() -> Self {
        Self {
            permissions: std::collections::BTreeSet::new(),
            resources: std::collections::BTreeSet::new(),
            valid_until: None,
        }
    }

    /// Create the top capability (all permissions)
    pub fn top() -> Self {
        let mut cap = Self::new();
        cap.permissions.insert("*".to_string()); // Wildcard permission
        cap.resources.insert("*".to_string()); // All resources
        cap
    }

    /// Create a capability with specific permissions
    pub fn with_permissions<I>(permissions: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut cap = Self::new();
        cap.permissions.extend(permissions);
        cap.resources.insert("*".to_string()); // Default to all resources
        cap
    }

    /// Add a permission to this capability
    pub fn add_permission(&mut self, permission: impl Into<String>) {
        self.permissions.insert(permission.into());
    }

    /// Add a resource constraint
    pub fn add_resource(&mut self, resource: impl Into<String>) {
        self.resources.insert(resource.into());
    }

    /// Set time constraint
    pub fn set_valid_until(&mut self, timestamp: u64) {
        self.valid_until = Some(timestamp);
    }

    /// Check if this capability allows a permission
    pub fn allows(&self, permission: &str) -> bool {
        self.permissions.contains(permission) || self.permissions.contains("*")
    }

    /// Check if this capability applies to a resource
    pub fn applies_to(&self, resource: &str) -> bool {
        self.resources.is_empty() || // No resource constraints means applies to all
        self.resources.contains(resource) ||
        self.resources.contains("*")
    }

    /// Check if this capability is currently valid
    pub fn is_valid_at(&self, timestamp: u64) -> bool {
        match self.valid_until {
            Some(valid_until) => timestamp <= valid_until,
            None => true, // No time constraint
        }
    }

    /// Get all permissions
    pub fn permissions(&self) -> &std::collections::BTreeSet<String> {
        &self.permissions
    }

    /// Get all resources
    pub fn resources(&self) -> &std::collections::BTreeSet<String> {
        &self.resources
    }
}

impl MeetSemiLattice for Cap {
    fn meet(&self, other: &Self) -> Self {
        // Meet is intersection of permissions and constraints
        // Special handling for wildcard permissions "*" (identity element)
        let permissions = if self.permissions.contains("*") && other.permissions.contains("*") {
            // Both wildcards - result is wildcard
            let mut set = std::collections::BTreeSet::new();
            set.insert("*".to_string());
            set
        } else if self.permissions.contains("*") {
            // Self is wildcard, result is other's permissions
            other.permissions.clone()
        } else if other.permissions.contains("*") {
            // Other is wildcard, result is self's permissions
            self.permissions.clone()
        } else {
            // Neither is wildcard, normal intersection
            self.permissions
                .intersection(&other.permissions)
                .cloned()
                .collect()
        };

        // For resources, intersection with empty set or wildcards
        let resources = if self.resources.is_empty() || other.resources.is_empty() {
            // Empty means "no constraint" = all resources, so take the non-empty one
            if self.resources.is_empty() && other.resources.is_empty() {
                std::collections::BTreeSet::new() // Both unconstrained = unconstrained
            } else if self.resources.is_empty() {
                other.resources.clone()
            } else {
                self.resources.clone()
            }
        } else {
            // Both have constraints, intersect them
            self.resources
                .intersection(&other.resources)
                .cloned()
                .collect()
        };

        let valid_until = match (self.valid_until, other.valid_until) {
            (Some(a), Some(b)) => Some(a.min(b)), // Take earliest expiration
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        Cap {
            permissions,
            resources,
            valid_until,
        }
    }
}

impl Top for Cap {
    fn top() -> Self {
        Self::top()
    }
}

impl PartialOrd for Cap {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Cap A ≤ Cap B if A is more restrictive than B

        // Check permissions
        let perm_cmp = if self.permissions.is_subset(&other.permissions) {
            if self.permissions == other.permissions {
                std::cmp::Ordering::Equal
            } else {
                std::cmp::Ordering::Less // Fewer permissions = more restrictive
            }
        } else if other.permissions.is_subset(&self.permissions) {
            std::cmp::Ordering::Greater // More permissions = less restrictive
        } else {
            return None; // Incomparable
        };

        // Check time constraints
        let time_cmp = match (self.valid_until, other.valid_until) {
            (Some(a), Some(b)) => a.cmp(&b).reverse(), // Earlier expiry = more restrictive
            (Some(_), None) => std::cmp::Ordering::Less, // Has expiry = more restrictive
            (None, Some(_)) => std::cmp::Ordering::Greater, // No expiry = less restrictive
            (None, None) => std::cmp::Ordering::Equal,
        };

        // Combine orderings
        match (perm_cmp, time_cmp) {
            (std::cmp::Ordering::Equal, other_ord) => Some(other_ord),
            (perm_ord, std::cmp::Ordering::Equal) => Some(perm_ord),
            (std::cmp::Ordering::Less, std::cmp::Ordering::Less) => Some(std::cmp::Ordering::Less),
            (std::cmp::Ordering::Greater, std::cmp::Ordering::Greater) => {
                Some(std::cmp::Ordering::Greater)
            }
            _ => None, // Incomparable
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
    fn test_journal_operations() {
        let mut journal = Journal::new();

        // Add some facts
        let fact1 = Fact::with_value("event1", FactValue::String("occurred".to_string()));
        journal.merge_facts(fact1);

        // Add capabilities
        let caps = Cap::with_permissions(vec!["read".to_string(), "write".to_string()]);
        journal.refine_caps(caps);

        // Check authorization
        assert!(journal.is_authorized("read", "*", u64::MAX));
        assert!(journal.is_authorized("write", "*", u64::MAX));
        assert!(!journal.is_authorized("delete", "*", u64::MAX));
    }

    #[test]
    fn test_journal_merge() {
        let fact1 = Fact::with_value("key1", FactValue::String("value1".to_string()));
        let cap1 = Cap::with_permissions(vec!["read".to_string(), "write".to_string()]);
        let journal1 = Journal {
            facts: fact1,
            caps: cap1,
        };

        let fact2 = Fact::with_value("key2", FactValue::String("value2".to_string()));
        let cap2 = Cap::with_permissions(vec!["read".to_string(), "delete".to_string()]);
        let journal2 = Journal {
            facts: fact2,
            caps: cap2,
        };

        let mut merged = journal1.clone();
        merged.merge(&journal2);

        // Facts should join (both keys present)
        assert!(merged.facts.contains_key("key1"));
        assert!(merged.facts.contains_key("key2"));

        // Capabilities should meet (only common permissions)
        assert!(merged.caps.allows("read"));
        assert!(!merged.caps.allows("write"));
        assert!(!merged.caps.allows("delete"));
    }

    #[test]
    fn test_capability_restriction() {
        let fact = Fact::with_value("data", FactValue::String("sensitive".to_string()));
        let cap = Cap::with_permissions(vec![
            "read".to_string(),
            "write".to_string(),
            "delete".to_string(),
        ]);
        let journal = Journal {
            facts: fact,
            caps: cap,
        };

        // Restrict to read-only
        let readonly_constraint = Cap::with_permissions(vec!["read".to_string()]);
        let restricted = journal.restrict_view(readonly_constraint);

        // Should still have same facts but reduced capabilities
        assert_eq!(restricted.facts, journal.facts);
        assert!(restricted.caps.allows("read"));
        assert!(!restricted.caps.allows("write"));
        assert!(!restricted.caps.allows("delete"));
    }
}
