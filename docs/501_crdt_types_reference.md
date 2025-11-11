# CRDT Types Reference

Quick reference for Conflict-free Replicated Data Types (CRDTs) used throughout Aura's distributed system. CRDTs enable conflict-free replication without coordination protocols. All CRDT types follow semilattice laws ensuring convergence across distributed replicas.

Join-semilattices accumulate knowledge through union operations. Meet-semilattices refine authority through intersection operations. Composite CRDTs combine multiple semilattice types for complex state management.

See [CRDT Programming Guide](802_crdt_programming_guide.md) for implementation patterns. See [Journal System](105_journal_system.md) for CRDT integration.

---

## Counter Types

### G-Counter (Grow-Only Counter)

**Purpose**: Monotonic increment counter supporting concurrent increments without conflicts.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GCounter {
    device_counts: BTreeMap<DeviceId, u64>,
}

impl GCounter {
    pub fn new() -> Self {
        Self { device_counts: BTreeMap::new() }
    }
    
    pub fn increment(&mut self, device_id: DeviceId) {
        let current = self.device_counts.get(&device_id).copied().unwrap_or(0);
        self.device_counts.insert(device_id, current + 1);
    }
    
    pub fn value(&self) -> u64 {
        self.device_counts.values().sum()
    }
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

**Usage**: Session counters, vote tallies, usage metrics.

### PN-Counter (Positive-Negative Counter)

**Purpose**: Counter supporting both increment and decrement operations.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PNCounter {
    positive: GCounter,
    negative: GCounter,
}

impl PNCounter {
    pub fn new() -> Self {
        Self {
            positive: GCounter::new(),
            negative: GCounter::new(),
        }
    }
    
    pub fn increment(&mut self, device_id: DeviceId) {
        self.positive.increment(device_id);
    }
    
    pub fn decrement(&mut self, device_id: DeviceId) {
        self.negative.increment(device_id);
    }
    
    pub fn value(&self) -> i64 {
        self.positive.value() as i64 - self.negative.value() as i64
    }
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

**Usage**: Account balances, reputation scores, resource quotas.

## Set Types

### G-Set (Grow-Only Set)

**Purpose**: Set supporting only element addition, never removal.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GSet<T> {
    elements: BTreeSet<T>,
}

impl<T: Ord + Clone> GSet<T> {
    pub fn new() -> Self {
        Self { elements: BTreeSet::new() }
    }
    
    pub fn add(&mut self, element: T) {
        self.elements.insert(element);
    }
    
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains(element)
    }
    
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }
}

impl<T: Ord + Clone> Join for GSet<T> {
    fn join(&self, other: &Self) -> Self {
        Self {
            elements: self.elements.union(&other.elements).cloned().collect()
        }
    }
}
```

**Usage**: Fact accumulation, capability grants, trusted device lists.

### OR-Set (Observed-Remove Set)

**Purpose**: Set supporting both addition and removal with conflict resolution.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ORSet<T> {
    added: BTreeMap<T, BTreeSet<Uuid>>,
    removed: BTreeMap<T, BTreeSet<Uuid>>,
}

impl<T: Ord + Clone> ORSet<T> {
    pub fn new() -> Self {
        Self {
            added: BTreeMap::new(),
            removed: BTreeMap::new(),
        }
    }
    
    pub fn add(&mut self, element: T) -> Uuid {
        let tag = Uuid::new_v4();
        self.added.entry(element).or_default().insert(tag);
        tag
    }
    
    pub fn remove(&mut self, element: &T, tag: Uuid) {
        if let Some(added_tags) = self.added.get(element) {
            if added_tags.contains(&tag) {
                self.removed.entry(element.clone()).or_default().insert(tag);
            }
        }
    }
    
    pub fn contains(&self, element: &T) -> bool {
        let added_tags = self.added.get(element).cloned().unwrap_or_default();
        let removed_tags = self.removed.get(element).cloned().unwrap_or_default();
        
        !added_tags.difference(&removed_tags).collect::<Vec<_>>().is_empty()
    }
    
    pub fn elements(&self) -> Vec<T> {
        self.added.keys()
            .filter(|element| self.contains(element))
            .cloned()
            .collect()
    }
}

impl<T: Ord + Clone> Join for ORSet<T> {
    fn join(&self, other: &Self) -> Self {
        let mut merged_added = self.added.clone();
        let mut merged_removed = self.removed.clone();
        
        // Merge added tags
        for (element, tags) in &other.added {
            merged_added.entry(element.clone())
                .or_default()
                .extend(tags);
        }
        
        // Merge removed tags
        for (element, tags) in &other.removed {
            merged_removed.entry(element.clone())
                .or_default()
                .extend(tags);
        }
        
        Self {
            added: merged_added,
            removed: merged_removed,
        }
    }
}
```

**Usage**: Group membership, capability revocation, device authorization.

## Map Types

### OR-Map (Observed-Remove Map)

**Purpose**: Key-value map with concurrent updates and conflict resolution.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ORMap<K, V> {
    entries: BTreeMap<K, ORSet<V>>,
}

impl<K: Ord + Clone, V: Ord + Clone> ORMap<K, V> {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }
    
    pub fn set(&mut self, key: K, value: V) -> Uuid {
        self.entries.entry(key).or_default().add(value)
    }
    
    pub fn get(&self, key: &K) -> Option<Vec<V>> {
        self.entries.get(key).map(|or_set| or_set.elements())
    }
    
    pub fn remove(&mut self, key: &K, value: &V, tag: Uuid) {
        if let Some(or_set) = self.entries.get_mut(key) {
            or_set.remove(value, tag);
        }
    }
    
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key) && 
        self.entries.get(key).unwrap().elements().len() > 0
    }
}

impl<K: Ord + Clone, V: Ord + Clone> Join for ORMap<K, V> {
    fn join(&self, other: &Self) -> Self {
        let mut merged_entries = self.entries.clone();
        
        for (key, other_set) in &other.entries {
            match merged_entries.get_mut(key) {
                Some(existing_set) => {
                    *existing_set = existing_set.join(other_set);
                }
                None => {
                    merged_entries.insert(key.clone(), other_set.clone());
                }
            }
        }
        
        Self { entries: merged_entries }
    }
}
```

**Usage**: Device metadata, configuration settings, trust relationships.

### LWW-Map (Last-Writer-Wins Map)

**Purpose**: Key-value map with timestamp-based conflict resolution.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LWWMap<K, V> {
    entries: BTreeMap<K, (V, u64, DeviceId)>, // (value, timestamp, device_id)
}

impl<K: Ord + Clone, V: Clone> LWWMap<K, V> {
    pub fn new() -> Self {
        Self { entries: BTreeMap::new() }
    }
    
    pub fn set(&mut self, key: K, value: V, timestamp: u64, device_id: DeviceId) {
        match self.entries.get(&key) {
            Some((_, existing_timestamp, existing_device)) => {
                if timestamp > *existing_timestamp || 
                   (timestamp == *existing_timestamp && device_id > *existing_device) {
                    self.entries.insert(key, (value, timestamp, device_id));
                }
            }
            None => {
                self.entries.insert(key, (value, timestamp, device_id));
            }
        }
    }
    
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key).map(|(value, _, _)| value)
    }
    
    pub fn remove(&mut self, key: &K, timestamp: u64, device_id: DeviceId) {
        if let Some((_, existing_timestamp, existing_device)) = self.entries.get(key) {
            if timestamp > *existing_timestamp || 
               (timestamp == *existing_timestamp && device_id > *existing_device) {
                self.entries.remove(key);
            }
        }
    }
}

impl<K: Ord + Clone, V: Clone> Join for LWWMap<K, V> {
    fn join(&self, other: &Self) -> Self {
        let mut merged = self.clone();
        
        for (key, (value, timestamp, device_id)) in &other.entries {
            merged.set(key.clone(), value.clone(), *timestamp, *device_id);
        }
        
        merged
    }
}
```

**Usage**: Configuration values, user preferences, system settings.

## Aura-Specific Types

### FactSet

**Purpose**: Journal fact accumulation with cryptographic verification.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactSet {
    facts: BTreeMap<FactId, Fact>,
    fact_hashes: BTreeSet<Hash>,
}

impl FactSet {
    pub fn new() -> Self {
        Self {
            facts: BTreeMap::new(),
            fact_hashes: BTreeSet::new(),
        }
    }
    
    pub fn add_fact(&mut self, fact: Fact) -> Result<(), FactError> {
        // Verify fact signature
        if !fact.verify_signature()? {
            return Err(FactError::InvalidSignature);
        }
        
        let fact_hash = fact.compute_hash();
        
        // Prevent duplicate facts
        if self.fact_hashes.contains(&fact_hash) {
            return Ok(());
        }
        
        self.facts.insert(fact.fact_id, fact);
        self.fact_hashes.insert(fact_hash);
        
        Ok(())
    }
    
    pub fn get_fact(&self, fact_id: &FactId) -> Option<&Fact> {
        self.facts.get(fact_id)
    }
    
    pub fn facts_by_device(&self, device_id: DeviceId) -> Vec<&Fact> {
        self.facts.values()
            .filter(|fact| fact.device_id == device_id)
            .collect()
    }
    
    pub fn all_facts(&self) -> Vec<&Fact> {
        self.facts.values().collect()
    }
}

impl Join for FactSet {
    fn join(&self, other: &Self) -> Self {
        let mut merged = self.clone();
        
        for fact in other.facts.values() {
            if let Err(e) = merged.add_fact(fact.clone()) {
                // Log error but continue merging
                eprintln!("Failed to merge fact: {}", e);
            }
        }
        
        merged
    }
}
```

### CapabilitySet

**Purpose**: Capability-based authorization with meet-semilattice restriction.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySet {
    capabilities: BTreeSet<Capability>,
    constraints: BTreeMap<String, ConstraintValue>,
}

impl CapabilitySet {
    pub fn universal() -> Self {
        Self {
            capabilities: Capability::all_capabilities().into_iter().collect(),
            constraints: BTreeMap::new(),
        }
    }
    
    pub fn empty() -> Self {
        Self {
            capabilities: BTreeSet::new(),
            constraints: BTreeMap::new(),
        }
    }
    
    pub fn add_capability(&mut self, capability: Capability) {
        self.capabilities.insert(capability);
    }
    
    pub fn add_constraint(&mut self, name: String, value: ConstraintValue) {
        self.constraints.insert(name, value);
    }
    
    pub fn contains(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }
    
    pub fn contains_all(&self, required: &CapabilitySet) -> bool {
        required.capabilities.is_subset(&self.capabilities) &&
        required.constraints.iter().all(|(name, value)| {
            self.constraints.get(name)
                .map(|our_value| our_value.satisfies(value))
                .unwrap_or(false)
        })
    }
    
    pub fn difference(&self, other: &CapabilitySet) -> CapabilitySet {
        Self {
            capabilities: self.capabilities.difference(&other.capabilities).cloned().collect(),
            constraints: self.constraints.clone(), // Constraints don't subtract
        }
    }
}

impl Meet for CapabilitySet {
    fn meet(&self, other: &Self) -> Self {
        let intersection = self.capabilities.intersection(&other.capabilities).cloned().collect();
        
        let mut merged_constraints = self.constraints.clone();
        for (name, other_value) in &other.constraints {
            match merged_constraints.get(name) {
                Some(our_value) => {
                    merged_constraints.insert(name.clone(), our_value.intersect(other_value));
                }
                None => {
                    merged_constraints.insert(name.clone(), other_value.clone());
                }
            }
        }
        
        Self {
            capabilities: intersection,
            constraints: merged_constraints,
        }
    }
}
```

### TrustSet

**Purpose**: Trust relationship accumulation with weight computation.

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct TrustSet {
    relationships: BTreeMap<(DeviceId, DeviceId), TrustRelationship>,
}

impl TrustSet {
    pub fn new() -> Self {
        Self { relationships: BTreeMap::new() }
    }
    
    pub fn add_relationship(&mut self, relationship: TrustRelationship) {
        let key = (relationship.truster, relationship.trustee);
        self.relationships.insert(key, relationship);
    }
    
    pub fn get_relationship(&self, truster: DeviceId, trustee: DeviceId) -> Option<&TrustRelationship> {
        self.relationships.get(&(truster, trustee))
    }
    
    pub fn compute_trust_weight(&self, source: DeviceId, target: DeviceId) -> f64 {
        if source == target {
            return 1.0; // Self-trust
        }
        
        // Direct trust
        if let Some(relationship) = self.get_relationship(source, target) {
            return relationship.trust_weight;
        }
        
        // Transitive trust computation (simplified)
        self.compute_transitive_trust(source, target, &mut BTreeSet::new())
    }
    
    fn compute_transitive_trust(&self, source: DeviceId, target: DeviceId, visited: &mut BTreeSet<DeviceId>) -> f64 {
        if visited.contains(&source) {
            return 0.0; // Prevent cycles
        }
        
        visited.insert(source);
        
        let mut max_trust = 0.0;
        
        for ((truster, intermediate), relationship) in &self.relationships {
            if *truster == source && !visited.contains(intermediate) {
                let intermediate_trust = relationship.trust_weight;
                let onward_trust = self.compute_transitive_trust(*intermediate, target, visited);
                let total_trust = intermediate_trust * onward_trust * 0.8; // Decay factor
                max_trust = max_trust.max(total_trust);
            }
        }
        
        visited.remove(&source);
        max_trust
    }
    
    pub fn trusted_devices(&self, device_id: DeviceId, min_weight: f64) -> Vec<DeviceId> {
        self.relationships.keys()
            .map(|(_, trustee)| *trustee)
            .filter(|&trustee| self.compute_trust_weight(device_id, trustee) >= min_weight)
            .collect()
    }
}

impl Join for TrustSet {
    fn join(&self, other: &Self) -> Self {
        let mut merged = self.clone();
        
        for ((truster, trustee), other_rel) in &other.relationships {
            match merged.relationships.get(&(*truster, *trustee)) {
                Some(existing_rel) => {
                    // Take relationship with higher timestamp
                    if other_rel.timestamp > existing_rel.timestamp {
                        merged.relationships.insert((*truster, *trustee), other_rel.clone());
                    }
                }
                None => {
                    merged.relationships.insert((*truster, *trustee), other_rel.clone());
                }
            }
        }
        
        merged
    }
}
```

## CRDT Laws and Properties

### Semilattice Laws

**Join-Semilattice Properties**:
- **Associativity**: `a ∨ (b ∨ c) = (a ∨ b) ∨ c`
- **Commutativity**: `a ∨ b = b ∨ a` 
- **Idempotency**: `a ∨ a = a`

**Meet-Semilattice Properties**:
- **Associativity**: `a ∧ (b ∧ c) = (a ∧ b) ∧ c`
- **Commutativity**: `a ∧ b = b ∧ a`
- **Idempotency**: `a ∧ a = a`

### Verification Functions

**Property testing** for CRDT implementations.

```rust
pub fn verify_join_laws<T: Join + PartialEq + Clone>(a: &T, b: &T, c: &T) -> bool {
    // Associativity: (a ∨ b) ∨ c = a ∨ (b ∨ c)
    let left_assoc = a.join(b).join(c);
    let right_assoc = a.join(&b.join(c));
    if left_assoc != right_assoc {
        return false;
    }
    
    // Commutativity: a ∨ b = b ∨ a
    if a.join(b) != b.join(a) {
        return false;
    }
    
    // Idempotency: a ∨ a = a
    if a.join(a) != *a {
        return false;
    }
    
    true
}

pub fn verify_meet_laws<T: Meet + PartialEq + Clone>(a: &T, b: &T, c: &T) -> bool {
    // Associativity: (a ∧ b) ∧ c = a ∧ (b ∧ c)
    let left_assoc = a.meet(b).meet(c);
    let right_assoc = a.meet(&b.meet(c));
    if left_assoc != right_assoc {
        return false;
    }
    
    // Commutativity: a ∧ b = b ∧ a
    if a.meet(b) != b.meet(a) {
        return false;
    }
    
    // Idempotency: a ∧ a = a
    if a.meet(a) != *a {
        return false;
    }
    
    true
}
```

## Usage Patterns

### State Synchronization

**Merging CRDT state** from multiple sources.

```rust
pub fn synchronize_device_state(
    local_state: &mut DeviceState,
    remote_states: Vec<DeviceState>,
) -> SyncResult {
    let mut merged_facts = local_state.facts.clone();
    let mut merged_capabilities = local_state.capabilities.clone();
    let mut merged_trust = local_state.trust_relationships.clone();
    
    for remote_state in remote_states {
        merged_facts = merged_facts.join(&remote_state.facts);
        merged_capabilities = merged_capabilities.meet(&remote_state.capabilities);
        merged_trust = merged_trust.join(&remote_state.trust_relationships);
    }
    
    local_state.facts = merged_facts;
    local_state.capabilities = merged_capabilities;
    local_state.trust_relationships = merged_trust;
    
    SyncResult::Success
}
```

### Conflict Detection

**Identifying conflicting updates** for manual resolution.

```rust
pub fn detect_conflicts<T: CRDT>(
    base: &T,
    update_a: &T,
    update_b: &T,
) -> ConflictStatus {
    let merged_a = base.join(update_a);
    let merged_b = base.join(update_b);
    let final_merge = merged_a.join(&merged_b);
    
    if merged_a == merged_b {
        ConflictStatus::NoConflict
    } else if final_merge.is_deterministic() {
        ConflictStatus::AutoResolved
    } else {
        ConflictStatus::RequiresManualResolution {
            option_a: merged_a,
            option_b: merged_b,
        }
    }
}
```

### Delta Synchronization

**Efficient incremental updates** for large CRDT structures.

```rust
pub struct DeltaState<T> {
    pub base_version: u64,
    pub delta_operations: Vec<DeltaOperation<T>>,
}

impl<T: CRDT> DeltaState<T> {
    pub fn apply_deltas(&self, base_state: &T) -> T {
        let mut current = base_state.clone();
        
        for delta_op in &self.delta_operations {
            current = current.join(&delta_op.apply());
        }
        
        current
    }
    
    pub fn compress_deltas(&mut self) {
        // Merge consecutive deltas to reduce bandwidth
        if self.delta_operations.len() > 10 {
            let compressed = self.delta_operations.iter()
                .fold(DeltaOperation::identity(), |acc, op| acc.compose(op));
            
            self.delta_operations = vec![compressed];
        }
    }
}
```