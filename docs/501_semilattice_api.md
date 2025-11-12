# Semilattice API

Quick reference for Conflict-free Replicated Data Types (CRDTs) and semilattice operations used in Aura's distributed system.

## Core Semilattice Operations

### Join Semilattices

Join operations accumulate knowledge through union:

```rust
use aura_journal::semilattice::Join;

pub trait Join {
    fn join(&self, other: &Self) -> Self;
}
```

Join operations follow semilattice laws: associativity, commutativity, and idempotency. These properties ensure convergence without coordination.

### Meet Semilattices

Meet operations refine authority through intersection:

```rust
use aura_wot::semilattice::Meet;

pub trait Meet {
    fn meet(&self, other: &Self) -> Self;
}
```

Meet operations provide conservative authorization where capabilities can only be restricted. This ensures secure capability operations.

## Counter Types

### G-Counter (Grow-Only Counter)

Monotonic increment counter supporting concurrent increments without conflicts:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GCounter {
    device_counts: BTreeMap<DeviceId, u64>,
}

impl GCounter {
    pub fn new() -> Self;
    pub fn increment(&mut self, device_id: DeviceId);
    pub fn value(&self) -> u64;
}

impl Join for GCounter {
    fn join(&self, other: &Self) -> Self {
        let mut merged_counts = self.device_counts.clone();
        
        for (device_id, count) in &other.device_counts {
            let current_count = merged_counts.get(device_id).copied().unwrap_or(0);
            merged_counts.insert(*device_id, current_count.max(*count));
        }
        
        GCounter { device_counts: merged_counts }
    }
}
```

G-Counter uses maximum values for conflict resolution. Each device tracks its own increment count independently.

Usage: Session counters, vote tallies, usage metrics.

### PN-Counter (Positive-Negative Counter)

Counter supporting both increment and decrement operations:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PNCounter {
    positive: GCounter,
    negative: GCounter,
}

impl PNCounter {
    pub fn new() -> Self;
    pub fn increment(&mut self, device_id: DeviceId);
    pub fn decrement(&mut self, device_id: DeviceId);
    pub fn value(&self) -> i64;
}

impl Join for PNCounter {
    fn join(&self, other: &Self) -> Self {
        Self {
            positive: self.positive.join(&other.positive),
            negative: self.negative.join(&other.negative),
        }
    }
}
```

PN-Counter composes two G-Counters for increment and decrement operations. The final value is the difference between positive and negative counts.

Usage: Account balances, reputation scores, resource quotas.

## Set Types

### G-Set (Grow-Only Set)

Set supporting only element addition, never removal:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GSet<T> {
    elements: BTreeSet<T>,
}

impl<T: Ord + Clone> GSet<T> {
    pub fn new() -> Self;
    pub fn add(&mut self, element: T);
    pub fn contains(&self, element: &T) -> bool;
    pub fn iter(&self) -> impl Iterator<Item = &T>;
}

impl<T: Ord + Clone> Join for GSet<T> {
    fn join(&self, other: &Self) -> Self {
        Self {
            elements: self.elements.union(&other.elements).cloned().collect()
        }
    }
}
```

G-Set uses set union for merge operations. Elements can only be added and never removed.

Usage: Fact accumulation, capability grants, trusted device lists.

### OR-Set (Observed-Remove Set)

Set supporting both addition and removal with conflict resolution:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ORSet<T> {
    added: BTreeMap<T, BTreeSet<Uuid>>,
    removed: BTreeMap<T, BTreeSet<Uuid>>,
}

impl<T: Ord + Clone> ORSet<T> {
    pub fn new() -> Self;
    pub fn add(&mut self, element: T) -> Uuid;
    pub fn remove(&mut self, element: &T, tag: Uuid);
    pub fn contains(&self, element: &T) -> bool;
    pub fn elements(&self) -> Vec<T>;
}

impl<T: Ord + Clone> Join for ORSet<T> {
    fn join(&self, other: &Self) -> Self {
        // Merge added and removed tags for all elements
        Self {
            added: merge_tag_maps(&self.added, &other.added),
            removed: merge_tag_maps(&self.removed, &other.removed),
        }
    }
}
```

OR-Set uses unique tags for add and remove operations. Elements exist when they have unremoved add tags.

Usage: Group membership, capability revocation, device authorization.

## Map Types

### OR-Map (Observed-Remove Map)

Key-value map with concurrent updates and conflict resolution:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ORMap<K, V> {
    entries: BTreeMap<K, ORSet<V>>,
}

impl<K: Ord + Clone, V: Ord + Clone> ORMap<K, V> {
    pub fn new() -> Self;
    pub fn set(&mut self, key: K, value: V) -> Uuid;
    pub fn get(&self, key: &K) -> Option<Vec<V>>;
    pub fn remove(&mut self, key: &K, value: &V, tag: Uuid);
    pub fn contains_key(&self, key: &K) -> bool;
}
```

OR-Map composes OR-Sets for each key. Multiple values can exist for a single key with conflict resolution.

Usage: Device metadata, configuration settings, trust relationships.

### LWW-Map (Last-Writer-Wins Map)

Key-value map with timestamp-based conflict resolution:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LWWMap<K, V> {
    entries: BTreeMap<K, (V, u64, DeviceId)>,
}

impl<K: Ord + Clone, V: Clone> LWWMap<K, V> {
    pub fn new() -> Self;
    pub fn set(&mut self, key: K, value: V, timestamp: u64, device_id: DeviceId);
    pub fn get(&self, key: &K) -> Option<&V>;
    pub fn remove(&mut self, key: &K, timestamp: u64, device_id: DeviceId);
}

impl<K: Ord + Clone, V: Clone> Join for LWWMap<K, V> {
    fn join(&self, other: &Self) -> Self {
        // Use timestamp and device ID for conflict resolution
        merge_lww_entries(&self.entries, &other.entries)
    }
}
```

LWW-Map uses timestamps with device ID tiebreakers for conflict resolution. The most recent update wins.

Usage: Configuration values, user preferences, system settings.

## Aura-Specific Types

### FactSet

Journal fact accumulation with cryptographic verification:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactSet {
    facts: BTreeMap<FactId, Fact>,
    fact_hashes: BTreeSet<Hash>,
}

impl FactSet {
    pub fn new() -> Self;
    pub fn add_fact(&mut self, fact: Fact) -> Result<(), FactError>;
    pub fn get_fact(&self, fact_id: &FactId) -> Option<&Fact>;
    pub fn facts_by_device(&self, device_id: DeviceId) -> Vec<&Fact>;
    pub fn all_facts(&self) -> Vec<&Fact>;
}

impl Join for FactSet {
    fn join(&self, other: &Self) -> Self {
        // Merge facts with signature verification
        let mut merged = self.clone();
        for fact in other.facts.values() {
            merged.add_fact(fact.clone()).ok(); // Log errors but continue
        }
        merged
    }
}
```

FactSet accumulates verified facts with hash-based deduplication. Cryptographic verification prevents invalid fact injection.

### CapabilitySet

Capability-based authorization with meet-semilattice restriction:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
    constraints: BTreeMap<String, ConstraintValue>,
}

impl CapabilitySet {
    pub fn universal() -> Self;
    pub fn empty() -> Self;
    pub fn add_capability(&mut self, capability: Capability);
    pub fn contains(&self, capability: &Capability) -> bool;
    pub fn contains_all(&self, required: &CapabilitySet) -> bool;
}

impl Meet for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        let intersection = self.capabilities.intersection(&other.capabilities).cloned().collect();
        
        Self {
            capabilities: intersection,
            constraints: merge_constraint_intersection(&self.constraints, &other.constraints),
        }
    }
}
```

CapabilitySet uses intersection for conservative authorization. Capabilities can only be restricted through meet operations.

### TrustSet

Trust relationship accumulation with weight computation:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct TrustSet {
    relationships: BTreeMap<(DeviceId, DeviceId), TrustRelationship>,
}

impl TrustSet {
    pub fn new() -> Self;
    pub fn add_relationship(&mut self, relationship: TrustRelationship);
    pub fn get_relationship(&self, truster: DeviceId, trustee: DeviceId) -> Option<&TrustRelationship>;
    pub fn compute_trust_weight(&self, source: DeviceId, target: DeviceId) -> f64;
    pub fn trusted_devices(&self, device_id: DeviceId, min_weight: f64) -> Vec<DeviceId>;
}

impl Join for TrustSet {
    fn join(&self, other: &Self) -> Self {
        // Use timestamps for conflict resolution on relationships
        merge_trust_relationships(&self.relationships, &other.relationships)
    }
}
```

TrustSet accumulates trust relationships with transitive trust computation. Most recent relationships take precedence for conflicts.

## Property Verification

### Semilattice Laws

Verification functions for CRDT implementations:

```rust
pub fn verify_join_laws<T: Join + PartialEq + Clone>(a: &T, b: &T, c: &T) -> bool {
    // Associativity: (a ∨ b) ∨ c = a ∨ (b ∨ c)
    let left_assoc = a.join(b).join(c);
    let right_assoc = a.join(&b.join(c));
    
    // Commutativity: a ∨ b = b ∨ a
    let commutative = a.join(b) == b.join(a);
    
    // Idempotency: a ∨ a = a
    let idempotent = a.join(a) == *a;
    
    left_assoc == right_assoc && commutative && idempotent
}

pub fn verify_meet_laws<T: Meet + PartialEq + Clone>(a: &T, b: &T, c: &T) -> bool {
    // Same properties for meet operations
    let left_assoc = a.meet(b).meet(c);
    let right_assoc = a.meet(&b.meet(c));
    let commutative = a.meet(b) == b.meet(a);
    let idempotent = a.meet(a) == *a;
    
    left_assoc == right_assoc && commutative && idempotent
}
```

These verification functions validate CRDT implementations for correctness. All CRDT types must satisfy semilattice laws for convergence guarantees.

## Usage Patterns

### State Synchronization

Merging CRDT state from multiple sources:

```rust
pub fn synchronize_device_state(
    local_state: &mut DeviceState,
    remote_states: Vec<DeviceState>,
) -> SyncResult {
    for remote_state in remote_states {
        local_state.facts = local_state.facts.join(&remote_state.facts);
        local_state.capabilities = local_state.capabilities.meet(&remote_state.capabilities);
        local_state.trust_relationships = local_state.trust_relationships.join(&remote_state.trust_relationships);
    }
    
    SyncResult::Success
}
```

Facts accumulate through join operations while capabilities refine through meet operations. Trust relationships merge with timestamp resolution.

### Delta Synchronization

Efficient incremental updates for large CRDT structures:

```rust
pub struct DeltaState<T> {
    pub base_version: u64,
    pub delta_operations: Vec<DeltaOperation<T>>,
}

impl<T: Join> DeltaState<T> {
    pub fn apply_deltas(&self, base_state: &T) -> T {
        self.delta_operations.iter().fold(
            base_state.clone(),
            |acc, delta_op| acc.join(&delta_op.apply())
        )
    }
}
```

Delta synchronization reduces bandwidth by sending only state changes. Delta operations compose through join operations for efficient merging.

See [Coordination Systems Guide](803_coordination_systems_guide.md) for CRDT integration patterns. See [Simulation and Testing Guide](805_simulation_and_testing_guide.md) for CRDT testing approaches.