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
/// 
/// Uses a proper CRDT (OR-Set with LWW-Map) for distributed consistency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    /// CRDT-based fact storage with operation timestamps
    entries: FactCrdt,
}

/// CRDT implementation for facts using Observed-Remove Set semantics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FactCrdt {
    /// Last-Writer-Wins map for fact values with vector clocks
    lww_map: std::collections::BTreeMap<String, VersionedFactValue>,
    /// Operation set for add/remove operations
    operation_set: std::collections::BTreeSet<FactOperation>,
}

/// Versioned fact value with causal ordering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct VersionedFactValue {
    value: FactValue,
    timestamp: u64,
    actor_id: String, // Device/actor that created this value
    version: u64,     // Logical clock for causality
}

/// CRDT operation for fact modifications
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
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

impl Ord for FactOperation {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Order by timestamp, then by op_id for deterministic ordering
        match (self, other) {
            (FactOperation::Add { timestamp: t1, op_id: id1, .. }, FactOperation::Add { timestamp: t2, op_id: id2, .. }) => {
                t1.cmp(t2).then_with(|| id1.cmp(id2))
            }
            (FactOperation::Remove { timestamp: t1, op_id: id1, .. }, FactOperation::Remove { timestamp: t2, op_id: id2, .. }) => {
                t1.cmp(t2).then_with(|| id1.cmp(id2))
            }
            (FactOperation::Add { .. }, FactOperation::Remove { .. }) => std::cmp::Ordering::Less,
            (FactOperation::Remove { .. }, FactOperation::Add { .. }) => std::cmp::Ordering::Greater,
        }
    }
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

    /// Insert or update a fact value using CRDT semantics
    pub fn insert(&mut self, key: impl Into<String>, value: FactValue) {
        let key = key.into();
        let timestamp = crate::current_unix_timestamp();
        let actor_id = std::env::var("AURA_DEVICE_ID")
            .unwrap_or_else(|_| "localhost".to_string()); // Device ID from environment or localhost default
        let op_id = format!("{}:{}:{}", actor_id, timestamp, key);
        
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
    
    /// Remove a fact value (creates remove operation)
    pub fn remove(&mut self, key: impl Into<String>) {
        let key = key.into();
        let timestamp = crate::current_unix_timestamp();
        let actor_id = "local".to_string();
        let op_id = format!("{}:{}:remove:{}", actor_id, timestamp, key);
        
        // Find all add operations for this key to remove them
        let add_ops_to_remove: Vec<_> = self.entries.operation_set
            .iter()
            .filter_map(|op| match op {
                FactOperation::Add { key: op_key, op_id, .. } if op_key == &key => Some(op_id.clone()),
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

    /// Get a fact value by key
    pub fn get(&self, key: &str) -> Option<&FactValue> {
        // Check if key is removed in OR-Set
        if self.is_key_removed(key) {
            return None;
        }
        
        self.entries.lww_map.get(key).map(|v| &v.value)
    }
    
    /// Check if a key has been removed according to OR-Set semantics
    fn is_key_removed(&self, key: &str) -> bool {
        let mut add_ops = std::collections::HashSet::new();
        let mut removed_ops = std::collections::HashSet::new();
        
        // Collect add and remove operations
        for op in &self.entries.operation_set {
            match op {
                FactOperation::Add { key: op_key, op_id, .. } if op_key == key => {
                    add_ops.insert(op_id.clone());
                }
                FactOperation::Remove { key: op_key, removed_op_id, .. } if op_key == key => {
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
        self.entries.lww_map
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
        self.entries.lww_map
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
        result.operation_set.extend(self.entries.operation_set.iter().cloned());
        result.operation_set.extend(other.entries.operation_set.iter().cloned());
        
        // Merge LWW-Maps with proper conflict resolution
        let all_keys: std::collections::BTreeSet<_> = self.entries.lww_map
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

impl PartialOrd for Fact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // CRDT partial order: A ≤ B if A's operations ⊆ B's operations
        // and all LWW values in A are ≤ corresponding values in B
        
        // Check if operations are subset
        let ops_subset = self.entries.operation_set.is_subset(&other.entries.operation_set);
        let ops_superset = other.entries.operation_set.is_subset(&self.entries.operation_set);
        
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
/// Capabilities are meet-semilattice elements that can only shrink through refinement.
/// They represent authority that can be attenuated but never amplified.
/// 
/// Uses a proper capability lattice with hierarchical permissions and delegation chains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cap {
    /// Hierarchical capability lattice with delegation chains
    lattice: CapabilityLattice,
}

/// Capability lattice implementation with proper meet-semilattice properties
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CapabilityLattice {
    /// Permission hierarchy with delegation chains
    permissions: PermissionHierarchy,
    /// Resource constraints with scoping
    resources: ResourceScope,
    /// Temporal constraints with delegation periods
    temporal: TemporalConstraints,
    /// Delegation chain for capability provenance
    delegation_chain: Vec<DelegationEntry>,
}

/// Hierarchical permission structure
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PermissionHierarchy {
    /// Core permissions (leaf nodes in hierarchy)
    atomic_permissions: std::collections::BTreeSet<String>,
    /// Derived permissions (internal nodes)
    derived_permissions: std::collections::BTreeMap<String, Vec<String>>,
    /// Wildcard permissions (root nodes)
    wildcards: std::collections::BTreeSet<String>,
}

/// Resource scope with hierarchical constraints
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ResourceScope {
    /// Allowed resource patterns (e.g., "journal:*", "storage:chunk:123")
    allowed_patterns: std::collections::BTreeSet<ResourcePattern>,
    /// Explicit resource exclusions
    excluded_patterns: std::collections::BTreeSet<ResourcePattern>,
}

/// Resource pattern for capability scoping
#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
struct ResourcePattern {
    /// Pattern string (supports wildcards)
    pattern: String,
    /// Pattern type (exact, prefix, wildcard)
    pattern_type: PatternType,
}

#[derive(Debug, Clone, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
enum PatternType {
    Exact,
    Prefix,
    Wildcard,
}

/// Temporal constraints for capability validity
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TemporalConstraints {
    /// Not valid before this timestamp
    valid_from: Option<u64>,
    /// Not valid after this timestamp
    valid_until: Option<u64>,
    /// Usage limits (number of operations)
    usage_limit: Option<u64>,
    /// Current usage count
    usage_count: u64,
}

/// Delegation entry for capability provenance
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DelegationEntry {
    /// Who delegated this capability
    delegator: String,
    /// Who received the delegation
    delegate: String,
    /// When the delegation occurred
    delegated_at: u64,
    /// Optional delegation constraints
    constraints: Option<DelegationConstraints>,
}

/// Constraints that can be applied during delegation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationConstraints {
    /// Maximum delegation depth
    max_depth: Option<u32>,
    /// Required endorsements for delegation
    required_endorsements: Vec<String>,
    /// Delegation-specific temporal limits
    delegation_temporal: Option<TemporalConstraints>,
}

/// Authentication levels for capability requirements
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum AuthLevel {
    /// No authentication required
    None = 0,
    /// Basic device authentication
    Device = 1,
    /// Multi-factor authentication
    MultiFactor = 2,
    /// Threshold signature required
    Threshold = 3,
}

impl Cap {
    /// Create a new empty capability
    pub fn new() -> Self {
        Self {
            lattice: CapabilityLattice {
                permissions: PermissionHierarchy {
                    atomic_permissions: std::collections::BTreeSet::new(),
                    derived_permissions: std::collections::BTreeMap::new(),
                    wildcards: std::collections::BTreeSet::new(),
                },
                resources: ResourceScope {
                    allowed_patterns: std::collections::BTreeSet::new(),
                    excluded_patterns: std::collections::BTreeSet::new(),
                },
                temporal: TemporalConstraints {
                    valid_from: None,
                    valid_until: None,
                    usage_limit: None,
                    usage_count: 0,
                },
                delegation_chain: Vec::new(),
            },
        }
    }

    /// Create the top capability (all permissions)
    pub fn top() -> Self {
        let mut cap = Self::new();
        cap.lattice.permissions.wildcards.insert("*".to_string());
        cap.lattice.resources.allowed_patterns.insert(ResourcePattern {
            pattern: "*".to_string(),
            pattern_type: PatternType::Wildcard,
        });
        cap
    }

    /// Create a capability with specific permissions
    pub fn with_permissions<I>(permissions: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        let mut cap = Self::new();
        for permission in permissions {
            if permission == "*" {
                cap.lattice.permissions.wildcards.insert(permission);
            } else {
                cap.lattice.permissions.atomic_permissions.insert(permission);
            }
        }
        // Default to all resources
        cap.lattice.resources.allowed_patterns.insert(ResourcePattern {
            pattern: "*".to_string(),
            pattern_type: PatternType::Wildcard,
        });
        cap
    }
    
    /// Add resource constraints to the capability
    pub fn with_resources(mut self, resources: Vec<String>) -> Self {
        self.lattice.resources.allowed_patterns.clear(); // Clear default
        for resource in resources {
            let pattern_type = if resource.contains('*') {
                PatternType::Wildcard
            } else if resource.ends_with(':') {
                PatternType::Prefix
            } else {
                PatternType::Exact
            };
            
            self.lattice.resources.allowed_patterns.insert(ResourcePattern {
                pattern: resource,
                pattern_type,
            });
        }
        self
    }

    /// Add a permission to this capability
    pub fn add_permission(&mut self, permission: impl Into<String>) {
        let permission = permission.into();
        if permission == "*" {
            self.lattice.permissions.wildcards.insert(permission);
        } else {
            self.lattice.permissions.atomic_permissions.insert(permission);
        }
    }

    /// Add a resource constraint
    pub fn add_resource(&mut self, resource: impl Into<String>) {
        let resource = resource.into();
        let pattern_type = if resource.contains('*') {
            PatternType::Wildcard
        } else if resource.ends_with(':') {
            PatternType::Prefix
        } else {
            PatternType::Exact
        };
        
        self.lattice.resources.allowed_patterns.insert(ResourcePattern {
            pattern: resource,
            pattern_type,
        });
    }

    /// Set time constraint
    pub fn set_valid_until(&mut self, timestamp: u64) {
        self.lattice.temporal.valid_until = Some(timestamp);
    }
    
    /// Set usage limit
    pub fn set_usage_limit(&mut self, limit: u64) {
        self.lattice.temporal.usage_limit = Some(limit);
    }
    
    /// Increment usage count
    pub fn increment_usage(&mut self) {
        self.lattice.temporal.usage_count += 1;
    }

    /// Check if this capability allows a permission
    pub fn allows(&self, permission: &str) -> bool {
        // Check wildcards first
        if self.lattice.permissions.wildcards.contains("*") {
            return true;
        }
        
        // Check atomic permissions
        if self.lattice.permissions.atomic_permissions.contains(permission) {
            return true;
        }
        
        // Check derived permissions (hierarchical)
        for (derived, atomics) in &self.lattice.permissions.derived_permissions {
            if atomics.contains(&permission.to_string()) &&
               self.lattice.permissions.atomic_permissions.contains(derived) {
                return true;
            }
        }
        
        false
    }

    /// Check if this capability applies to a resource
    pub fn applies_to(&self, resource: &str) -> bool {
        // Check if explicitly excluded
        for pattern in &self.lattice.resources.excluded_patterns {
            if self.matches_pattern(&pattern.pattern, resource, &pattern.pattern_type) {
                return false;
            }
        }
        
        // Check allowed patterns
        for pattern in &self.lattice.resources.allowed_patterns {
            if self.matches_pattern(&pattern.pattern, resource, &pattern.pattern_type) {
                return true;
            }
        }
        
        false
    }
    
    /// Helper to match resource patterns
    fn matches_pattern(&self, pattern: &str, resource: &str, pattern_type: &PatternType) -> bool {
        match pattern_type {
            PatternType::Exact => pattern == resource,
            PatternType::Prefix => resource.starts_with(&pattern[..pattern.len()-1]),
            PatternType::Wildcard => {
                if pattern == "*" {
                    true
                } else {
                    // Simple wildcard matching (could be more sophisticated)
                    pattern.trim_end_matches('*')
                        .split('*')
                        .all(|part| resource.contains(part))
                }
            }
        }
    }

    /// Check if this capability is currently valid
    pub fn is_valid_at(&self, timestamp: u64) -> bool {
        // Check temporal constraints
        if let Some(valid_from) = self.lattice.temporal.valid_from {
            if timestamp < valid_from {
                return false;
            }
        }
        
        if let Some(valid_until) = self.lattice.temporal.valid_until {
            if timestamp > valid_until {
                return false;
            }
        }
        
        // Check usage limits
        if let Some(usage_limit) = self.lattice.temporal.usage_limit {
            if self.lattice.temporal.usage_count >= usage_limit {
                return false;
            }
        }
        
        true
    }
    
    /// Get the authentication level based on capability complexity
    pub fn auth_level(&self) -> AuthLevel {
        // Determine auth level based on capability lattice complexity
        if self.lattice.permissions.wildcards.contains("*") {
            AuthLevel::Threshold
        } else if self.lattice.delegation_chain.len() > 1 {
            AuthLevel::MultiFactor
        } else {
            AuthLevel::Device
        }
    }

    /// Get all permissions (flattened view)
    pub fn permissions(&self) -> std::collections::BTreeSet<String> {
        let mut permissions = self.lattice.permissions.atomic_permissions.clone();
        permissions.extend(self.lattice.permissions.wildcards.iter().cloned());
        permissions.extend(self.lattice.permissions.derived_permissions.keys().cloned());
        permissions
    }

    /// Get all resource patterns (flattened view)
    pub fn resources(&self) -> std::collections::BTreeSet<String> {
        self.lattice.resources.allowed_patterns
            .iter()
            .map(|p| p.pattern.clone())
            .collect()
    }
    
    /// Add a delegation entry
    pub fn add_delegation(&mut self, delegator: String, delegate: String, constraints: Option<DelegationConstraints>) {
        let entry = DelegationEntry {
            delegator,
            delegate,
            delegated_at: crate::current_unix_timestamp(),
            constraints,
        };
        self.lattice.delegation_chain.push(entry);
    }
}

impl MeetSemiLattice for Cap {
    fn meet(&self, other: &Self) -> Self {
        let mut result_lattice = self.lattice.clone();
        
        // Meet permissions (intersection with proper lattice semantics)
        let permissions = PermissionHierarchy {
            // Intersect atomic permissions
            atomic_permissions: self.lattice.permissions.atomic_permissions
                .intersection(&other.lattice.permissions.atomic_permissions)
                .cloned()
                .collect(),
            // For derived permissions, take those common to both
            derived_permissions: self.lattice.permissions.derived_permissions
                .iter()
                .filter_map(|(k, v)| {
                    other.lattice.permissions.derived_permissions.get(k)
                        .map(|other_v| {
                            // Intersection of derived permission sets
                            let intersection: Vec<_> = v.iter()
                                .filter(|perm| other_v.contains(perm))
                                .cloned()
                                .collect();
                            (k.clone(), intersection)
                        })
                })
                .collect(),
            // Wildcards - only keep if both have them
            wildcards: self.lattice.permissions.wildcards
                .intersection(&other.lattice.permissions.wildcards)
                .cloned()
                .collect(),
        };
        
        // Meet resources (intersection of allowed, union of excluded)
        let resources = ResourceScope {
            // Intersection of allowed patterns (more restrictive)
            allowed_patterns: self.lattice.resources.allowed_patterns
                .intersection(&other.lattice.resources.allowed_patterns)
                .cloned()
                .collect(),
            // Union of excluded patterns (more restrictive)
            excluded_patterns: self.lattice.resources.excluded_patterns
                .union(&other.lattice.resources.excluded_patterns)
                .cloned()
                .collect(),
        };
        
        // Meet temporal constraints (most restrictive)
        let temporal = TemporalConstraints {
            valid_from: match (self.lattice.temporal.valid_from, other.lattice.temporal.valid_from) {
                (Some(a), Some(b)) => Some(a.max(b)), // Latest start time
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            valid_until: match (self.lattice.temporal.valid_until, other.lattice.temporal.valid_until) {
                (Some(a), Some(b)) => Some(a.min(b)), // Earliest end time
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            usage_limit: match (self.lattice.temporal.usage_limit, other.lattice.temporal.usage_limit) {
                (Some(a), Some(b)) => Some(a.min(b)), // Minimum usage limit
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            },
            usage_count: self.lattice.temporal.usage_count.max(other.lattice.temporal.usage_count),
        };
        
        // Merge delegation chains (preserve full provenance)
        let mut delegation_chain = self.lattice.delegation_chain.clone();
        delegation_chain.extend(other.lattice.delegation_chain.iter().cloned());
        // Deduplicate while preserving order
        delegation_chain.sort_by_key(|entry| entry.delegated_at);
        delegation_chain.dedup();
        
        result_lattice.permissions = permissions;
        result_lattice.resources = resources;
        result_lattice.temporal = temporal;
        result_lattice.delegation_chain = delegation_chain;
        
        Cap { lattice: result_lattice }
    }
}

impl Top for Cap {
    fn top() -> Self {
        Self::top()
    }
}

impl PartialOrd for Cap {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        // Capability partial order: A ≤ B if A is more restrictive than B
        // in all dimensions (permissions, resources, temporal constraints)
        
        // Compare permission lattices
        let perm_cmp = self.compare_permissions(&other.lattice.permissions)?;
        
        // Compare resource scopes
        let resource_cmp = self.compare_resources(&other.lattice.resources)?;
        
        // Compare temporal constraints
        let temporal_cmp = self.compare_temporal(&other.lattice.temporal)?;
        
        // All dimensions must agree for total ordering
        match (perm_cmp, resource_cmp, temporal_cmp) {
            (std::cmp::Ordering::Equal, std::cmp::Ordering::Equal, std::cmp::Ordering::Equal) => {
                Some(std::cmp::Ordering::Equal)
            }
            (std::cmp::Ordering::Less, b, c) if b != std::cmp::Ordering::Greater && c != std::cmp::Ordering::Greater => {
                Some(std::cmp::Ordering::Less)
            }
            (a, std::cmp::Ordering::Less, c) if a != std::cmp::Ordering::Greater && c != std::cmp::Ordering::Greater => {
                Some(std::cmp::Ordering::Less)
            }
            (a, b, std::cmp::Ordering::Less) if a != std::cmp::Ordering::Greater && b != std::cmp::Ordering::Greater => {
                Some(std::cmp::Ordering::Less)
            }
            (std::cmp::Ordering::Greater, b, c) if b != std::cmp::Ordering::Less && c != std::cmp::Ordering::Less => {
                Some(std::cmp::Ordering::Greater)
            }
            (a, std::cmp::Ordering::Greater, c) if a != std::cmp::Ordering::Less && c != std::cmp::Ordering::Less => {
                Some(std::cmp::Ordering::Greater)
            }
            (a, b, std::cmp::Ordering::Greater) if a != std::cmp::Ordering::Less && b != std::cmp::Ordering::Less => {
                Some(std::cmp::Ordering::Greater)
            }
            _ => None, // Incomparable dimensions
        }
    }
}

// Helper methods for comparing capability dimensions
impl Cap {
    fn compare_permissions(&self, other: &PermissionHierarchy) -> Option<std::cmp::Ordering> {
        // Compare atomic permissions
        let atomic_is_subset = self.lattice.permissions.atomic_permissions.is_subset(&other.atomic_permissions);
        let atomic_is_superset = other.atomic_permissions.is_subset(&self.lattice.permissions.atomic_permissions);
        
        // Compare wildcards
        let wildcards_is_subset = self.lattice.permissions.wildcards.is_subset(&other.wildcards);
        let wildcards_is_superset = other.wildcards.is_subset(&self.lattice.permissions.wildcards);
        
        match (atomic_is_subset, atomic_is_superset, wildcards_is_subset, wildcards_is_superset) {
            (true, true, true, true) => Some(std::cmp::Ordering::Equal),
            (true, false, true, false) => Some(std::cmp::Ordering::Less),  // More restrictive
            (false, true, false, true) => Some(std::cmp::Ordering::Greater), // Less restrictive
            _ => None, // Incomparable
        }
    }
    
    fn compare_resources(&self, other: &ResourceScope) -> Option<std::cmp::Ordering> {
        let allowed_is_subset = self.lattice.resources.allowed_patterns.is_subset(&other.allowed_patterns);
        let allowed_is_superset = other.allowed_patterns.is_subset(&self.lattice.resources.allowed_patterns);
        
        let excluded_is_subset = self.lattice.resources.excluded_patterns.is_subset(&other.excluded_patterns);
        let excluded_is_superset = other.excluded_patterns.is_subset(&self.lattice.resources.excluded_patterns);
        
        // More restrictive = fewer allowed, more excluded
        match (allowed_is_subset, excluded_is_superset) {
            (true, true) if !allowed_is_superset || !excluded_is_subset => Some(std::cmp::Ordering::Less),
            (true, true) if allowed_is_superset && excluded_is_subset => Some(std::cmp::Ordering::Equal),
            _ if allowed_is_superset && excluded_is_subset => Some(std::cmp::Ordering::Greater),
            _ => None,
        }
    }
    
    fn compare_temporal(&self, other: &TemporalConstraints) -> Option<std::cmp::Ordering> {
        let mut cmp_factors = vec![];
        
        // Compare valid_from (later start = more restrictive)
        match (self.lattice.temporal.valid_from, other.valid_from) {
            (Some(a), Some(b)) => cmp_factors.push(a.cmp(&b)),
            (Some(_), None) => cmp_factors.push(std::cmp::Ordering::Greater),
            (None, Some(_)) => cmp_factors.push(std::cmp::Ordering::Less),
            (None, None) => cmp_factors.push(std::cmp::Ordering::Equal),
        }
        
        // Compare valid_until (earlier end = more restrictive)
        match (self.lattice.temporal.valid_until, other.valid_until) {
            (Some(a), Some(b)) => cmp_factors.push(a.cmp(&b).reverse()),
            (Some(_), None) => cmp_factors.push(std::cmp::Ordering::Greater),
            (None, Some(_)) => cmp_factors.push(std::cmp::Ordering::Less),
            (None, None) => cmp_factors.push(std::cmp::Ordering::Equal),
        }
        
        // Compare usage_limit (lower limit = more restrictive)
        match (self.lattice.temporal.usage_limit, other.usage_limit) {
            (Some(a), Some(b)) => cmp_factors.push(a.cmp(&b).reverse()),
            (Some(_), None) => cmp_factors.push(std::cmp::Ordering::Greater),
            (None, Some(_)) => cmp_factors.push(std::cmp::Ordering::Less),
            (None, None) => cmp_factors.push(std::cmp::Ordering::Equal),
        }
        
        // All factors must agree
        let first = cmp_factors[0];
        if cmp_factors.iter().all(|&cmp| cmp == first || cmp == std::cmp::Ordering::Equal) {
            Some(first)
        } else {
            None // Incomparable
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
        let rejoined = fact1.join(&fact2);
        // Result depends on operation timestamps and OR-Set semantics
        
        assert!(fact1.operation_count() > 0); // Should have operations recorded
    }
    
    #[test]
    fn test_hierarchical_capabilities() {
        let mut cap = Cap::with_permissions(vec!["journal:read".to_string(), "journal:write".to_string()])
            .with_resources(vec!["journal:*".to_string()]);
            
        cap.set_usage_limit(10);
        cap.add_delegation("alice".to_string(), "bob".to_string(), None);
        
        assert!(cap.allows("journal:read"));
        assert!(cap.allows("journal:write"));
        assert!(!cap.allows("storage:read")); // Outside scope
        
        assert!(cap.applies_to("journal:facts"));
        assert!(cap.applies_to("journal:capabilities"));
        assert!(!cap.applies_to("storage:chunk:123")); // Outside scope
        
        // Test usage limits
        assert!(cap.is_valid_at(crate::current_unix_timestamp()));
        for _ in 0..10 {
            cap.increment_usage();
        }
        assert!(!cap.is_valid_at(crate::current_unix_timestamp())); // Exceeded limit
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
        let timestamp = crate::current_unix_timestamp();
        assert!(journal.is_authorized("read", "*", timestamp));
        assert!(journal.is_authorized("write", "*", timestamp));
        assert!(!journal.is_authorized("delete", "*", timestamp));
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
