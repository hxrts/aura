# CRDT Programming Guide

Conflict-free Replicated Data Types enable distributed applications to handle concurrent updates without coordination protocols. Aura's CRDT infrastructure provides automatic conflict resolution using mathematical semilattice properties.

This guide covers CRDT design patterns, semilattice implementation details, conflict resolution strategies, and performance optimization techniques. You will learn to build applications that maintain consistency across distributed devices.

See [Getting Started Guide](800_getting_started_guide.md) for basic concepts. See [Effect System Guide](801_effect_system_guide.md) for integration patterns.

---

## CRDT Design Patterns

**State-Based CRDTs** represent application state as mathematical structures that merge through join operations. All updates produce new states that can be combined with concurrent updates from other devices.

```rust
use aura_journal::semilattice::{Join, PartialOrder};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GCounterState {
    device_counts: BTreeMap<DeviceId, u64>,
}

impl GCounterState {
    pub fn increment(&mut self, device_id: DeviceId, amount: u64) {
        *self.device_counts.entry(device_id).or_insert(0) += amount;
    }

    pub fn value(&self) -> u64 {
        self.device_counts.values().sum()
    }
}
```

State-based CRDTs maintain device-specific state that merges through mathematical operations. Each device tracks its own contributions to avoid conflicts.

**Operation-Based CRDTs** represent changes as operations that commute under concurrent application. Operations can be applied in any order and produce the same final state.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterOp {
    Increment { device_id: DeviceId, amount: u64, timestamp: u64 },
}

impl CounterOp {
    pub fn apply(&self, state: &mut GCounterState) {
        match self {
            CounterOp::Increment { device_id, amount, .. } => {
                state.increment(*device_id, *amount);
            }
        }
    }
}
```

Operation-based CRDTs enable efficient synchronization by transmitting operations rather than full state. Operations include metadata for conflict resolution and duplicate detection.

**Hybrid Approaches** combine state and operation representations for optimal performance. Applications can choose synchronization strategies based on network conditions and data patterns.

```rust
pub struct HybridCounter {
    state: GCounterState,
    pending_ops: Vec<CounterOp>,
    last_sync: u64,
}

impl HybridCounter {
    pub fn sync_strategy(&self) -> SyncStrategy {
        if self.pending_ops.len() < 10 {
            SyncStrategy::Operations(self.pending_ops.clone())
        } else {
            SyncStrategy::FullState(self.state.clone())
        }
    }
}
```

Hybrid approaches optimize bandwidth usage by selecting appropriate synchronization methods. Small change sets use operation synchronization while large change sets use state synchronization.

## Using CRDT Handlers with Builder Pattern

Aura provides builder methods for easily setting up CRDT handlers in choreographies. The `CrdtCoordinator` uses a clean builder pattern that avoids boilerplate while maintaining type safety.

**Simple Setup** uses convenience methods for common cases:

```rust
use aura_protocol::effects::semilattice::CrdtCoordinator;

// Convergent CRDT with initial state
let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_map);

// Delta CRDT with compaction threshold
let coordinator = CrdtCoordinator::with_delta_threshold(device_id, 100);

// Meet-semilattice CRDT for constraints
let coordinator = CrdtCoordinator::with_mv_state(device_id, capability_set);
```

The builder pattern eliminates manual handler registration while providing clear intent. Each convenience method creates the appropriate handler type with sensible defaults.

**Chained Setup** combines multiple handlers for complex scenarios:

```rust
// Multiple CRDT types in one coordinator
let coordinator = CrdtCoordinator::new(device_id)
    .with_cv_handler(CvHandler::new())
    .with_delta_handler(DeltaHandler::with_threshold(50))
    .with_mv_handler(MvHandler::with_state(constraints));
```

Chaining allows selective registration of only the handler types needed by the application. This avoids instantiating unused handlers and reduces memory overhead.

**Integration with Protocols** passes coordinators directly to choreographic implementations:

```rust
use aura_protocol::choreography::protocols::anti_entropy::execute_anti_entropy;

let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_state);

let (result, updated_coordinator) = execute_anti_entropy(
    device_id,
    config,
    is_requester,
    &effect_system,
    coordinator,
).await?;

// Coordinator contains synchronized state after protocol execution
let synchronized_state = updated_coordinator.cv_handler().get_state();
```

Protocols consume and return coordinators, enabling immutable data flow patterns. The returned coordinator contains synchronized state after protocol execution completes.

## Semilattice Implementation

**Join Operations** define how concurrent states merge to produce consistent results. Join operations must be associative, commutative, and idempotent to ensure convergence properties.

```rust
impl Join for GCounterState {
    fn join(&self, other: &Self) -> Self {
        let mut merged_counts = self.device_counts.clone();

        for (device_id, count) in &other.device_counts {
            let current_count = merged_counts.get(device_id).copied().unwrap_or(0);
            merged_counts.insert(*device_id, current_count.max(*count));
        }

        GCounterState {
            device_counts: merged_counts,
        }
    }
}
```

Join operations merge device-specific contributions by taking the maximum value for each device. This ensures that increments from all devices are preserved in the merged state.

**Partial Ordering** defines when one state contains all information from another state. Partial ordering enables optimization by avoiding unnecessary synchronization.

```rust
impl PartialOrder for GCounterState {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let mut self_greater = false;
        let mut other_greater = false;

        let all_devices: BTreeSet<_> = self.device_counts.keys()
            .chain(other.device_counts.keys())
            .collect();

        for device_id in all_devices {
            let self_count = self.device_counts.get(device_id).copied().unwrap_or(0);
            let other_count = other.device_counts.get(device_id).copied().unwrap_or(0);

            match self_count.cmp(&other_count) {
                std::cmp::Ordering::Greater => self_greater = true,
                std::cmp::Ordering::Less => other_greater = true,
                std::cmp::Ordering::Equal => {}
            }
        }

        match (self_greater, other_greater) {
            (true, false) => Some(std::cmp::Ordering::Greater),
            (false, true) => Some(std::cmp::Ordering::Less),
            (false, false) => Some(std::cmp::Ordering::Equal),
            (true, true) => None, // Concurrent states
        }
    }
}
```

Partial ordering comparison determines if synchronization is necessary. Devices only need to synchronize when they have concurrent states that cannot be ordered.

**Bottom Elements** represent initial empty states that serve as identity elements for join operations. Bottom elements enable proper CRDT initialization and testing.

```rust
impl Default for GCounterState {
    fn default() -> Self {
        Self {
            device_counts: BTreeMap::new(),
        }
    }
}

impl GCounterState {
    pub fn is_bottom(&self) -> bool {
        self.device_counts.is_empty() ||
        self.device_counts.values().all(|&count| count == 0)
    }
}
```

Bottom elements provide starting points for CRDT operations. All valid states must be reachable from the bottom element through join operations.

## Conflict Resolution Strategies

**Last-Writer-Wins** resolves conflicts by selecting the most recent update based on timestamps. This strategy requires synchronized clocks but provides simple conflict resolution.

```rust
#[derive(Debug, Clone)]
pub struct LWWRegister<T> {
    value: T,
    timestamp: u64,
    device_id: DeviceId,
}

impl<T: Clone> LWWRegister<T> {
    pub fn set(&mut self, value: T, timestamp: u64, device_id: DeviceId) {
        if timestamp > self.timestamp ||
           (timestamp == self.timestamp && device_id > self.device_id) {
            self.value = value;
            self.timestamp = timestamp;
            self.device_id = device_id;
        }
    }
}

impl<T: Clone> Join for LWWRegister<T> {
    fn join(&self, other: &Self) -> Self {
        if other.timestamp > self.timestamp ||
           (other.timestamp == self.timestamp && other.device_id > self.device_id) {
            other.clone()
        } else {
            self.clone()
        }
    }
}
```

Last-Writer-Wins provides deterministic conflict resolution when timestamps are available. Device IDs break ties when timestamps are identical.

**Multi-Value Registers** preserve all concurrent values when conflicts occur. Applications can implement custom resolution logic or present choices to users.

```rust
#[derive(Debug, Clone)]
pub struct MVRegister<T> {
    values: BTreeMap<DeviceId, (T, u64)>, // value, timestamp
}

impl<T: Clone> MVRegister<T> {
    pub fn set(&mut self, value: T, timestamp: u64, device_id: DeviceId) {
        self.values.insert(device_id, (value, timestamp));
    }

    pub fn get_concurrent_values(&self) -> Vec<T> {
        let max_timestamp = self.values.values()
            .map(|(_, ts)| *ts)
            .max()
            .unwrap_or(0);

        self.values.values()
            .filter(|(_, ts)| *ts == max_timestamp)
            .map(|(v, _)| v.clone())
            .collect()
    }
}
```

Multi-Value Registers preserve concurrent updates for application-level conflict resolution. This approach enables sophisticated conflict handling strategies.

**Semantic Resolution** uses domain-specific knowledge to resolve conflicts meaningfully. Custom resolution logic considers the meaning of operations rather than timestamps.

```rust
#[derive(Debug, Clone)]
pub enum BankingOp {
    Deposit { amount: u64, timestamp: u64 },
    Withdraw { amount: u64, timestamp: u64 },
    SetLimit { limit: u64, timestamp: u64 },
}

pub struct BankAccount {
    balance: u64,
    operations: Vec<BankingOp>,
}

impl BankAccount {
    pub fn apply_semantically(&mut self, ops: Vec<BankingOp>) {
        // Sort operations by business rules, not just timestamps
        let mut sorted_ops = ops;
        sorted_ops.sort_by(|a, b| {
            // Deposits can always be applied
            // Withdrawals depend on available balance
            // Limits affect subsequent operations
            self.operation_priority(a).cmp(&self.operation_priority(b))
        });

        for op in sorted_ops {
            self.apply_operation(op);
        }
    }
}
```

Semantic resolution applies domain knowledge to produce meaningful results. Banking operations might prioritize deposits over withdrawals to maintain positive balances.

## Performance Considerations

**State Compaction** reduces memory usage by removing redundant information from CRDT states. Compaction preserves semantics while improving performance.

```rust
impl GCounterState {
    pub fn compact(&mut self, known_devices: &BTreeSet<DeviceId>) {
        // Remove zero counts for known devices
        self.device_counts.retain(|device_id, count| {
            *count > 0 || !known_devices.contains(device_id)
        });
    }

    pub fn memory_usage(&self) -> usize {
        std::mem::size_of::<Self>() +
        self.device_counts.len() * (
            std::mem::size_of::<DeviceId>() +
            std::mem::size_of::<u64>()
        )
    }
}
```

Compaction removes unnecessary state information while preserving CRDT properties. Applications should balance memory usage with synchronization efficiency.

**Delta Synchronization** transmits only changed state portions rather than complete CRDT states. Delta synchronization reduces bandwidth requirements for large CRDTs.

```rust
#[derive(Debug, Clone)]
pub struct DeltaGCounter {
    base_state: GCounterState,
    delta_state: GCounterState,
}

impl DeltaGCounter {
    pub fn increment(&mut self, device_id: DeviceId, amount: u64) {
        self.delta_state.increment(device_id, amount);
    }

    pub fn get_delta(&self) -> GCounterState {
        self.delta_state.clone()
    }

    pub fn apply_delta(&mut self, delta: GCounterState) {
        self.base_state = self.base_state.join(&delta);
        self.delta_state = GCounterState::default();
    }
}
```

Delta CRDTs separate local changes from base state. Synchronization transmits deltas rather than full states to improve bandwidth efficiency.

**Lazy Evaluation** defers expensive computations until results are needed. Lazy evaluation improves performance for CRDTs with complex derived values.

```rust
pub struct LazySetUnion<T> {
    sets: Vec<BTreeSet<T>>,
    cached_union: Option<BTreeSet<T>>,
}

impl<T: Clone + Ord> LazySetUnion<T> {
    pub fn add_set(&mut self, set: BTreeSet<T>) {
        self.sets.push(set);
        self.cached_union = None; // Invalidate cache
    }

    pub fn get_union(&mut self) -> &BTreeSet<T> {
        if self.cached_union.is_none() {
            let mut union = BTreeSet::new();
            for set in &self.sets {
                union = union.union(set).cloned().collect();
            }
            self.cached_union = Some(union);
        }

        self.cached_union.as_ref().unwrap()
    }
}
```

Lazy evaluation caches expensive computations and invalidates caches when state changes. This approach improves performance for read-heavy workloads.

## Advanced CRDT Patterns

**Causal Consistency** ensures that causally related operations appear in consistent order across all devices. Vector clocks track causal relationships between operations.

```rust
#[derive(Debug, Clone)]
pub struct VectorClock {
    clocks: BTreeMap<DeviceId, u64>,
}

impl VectorClock {
    pub fn increment(&mut self, device_id: DeviceId) {
        *self.clocks.entry(device_id).or_insert(0) += 1;
    }

    pub fn happens_before(&self, other: &VectorClock) -> bool {
        self.clocks.iter().all(|(device, clock)| {
            other.clocks.get(device).map_or(false, |other_clock| clock <= other_clock)
        }) && self != other
    }

    pub fn concurrent_with(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }
}
```

Vector clocks enable applications to detect concurrent operations and apply causal ordering constraints. This ensures that dependent operations execute in correct order.

**Composite CRDTs** combine multiple CRDT types to create complex data structures. Composite CRDTs maintain individual CRDT properties while enabling sophisticated applications.

```rust
#[derive(Debug, Clone)]
pub struct DocumentCRDT {
    content: LWWRegister<String>,
    word_count: GCounterState,
    collaborators: GSetCRDT<DeviceId>,
    last_modified: LWWRegister<u64>,
}

impl Join for DocumentCRDT {
    fn join(&self, other: &Self) -> Self {
        DocumentCRDT {
            content: self.content.join(&other.content),
            word_count: self.word_count.join(&other.word_count),
            collaborators: self.collaborators.join(&other.collaborators),
            last_modified: self.last_modified.join(&other.last_modified),
        }
    }
}
```

Composite CRDTs apply join operations to each component independently. This enables building complex applications while maintaining CRDT convergence properties.

**CRDT Garbage Collection** removes obsolete information that no longer affects application behavior. Garbage collection improves performance and reduces memory usage.

```rust
pub struct GCGCounter {
    state: GCounterState,
    gc_watermark: BTreeMap<DeviceId, u64>,
}

impl GCGCounter {
    pub fn garbage_collect(&mut self, global_watermark: &BTreeMap<DeviceId, u64>) {
        for (device_id, watermark) in global_watermark {
            if let Some(current_count) = self.state.device_counts.get_mut(device_id) {
                if *current_count <= *watermark {
                    self.state.device_counts.remove(device_id);
                }
            }
        }
    }

    pub fn can_gc_device(&self, device_id: DeviceId, watermark: u64) -> bool {
        self.state.device_counts.get(&device_id)
            .map_or(true, |count| *count <= watermark)
    }
}
```

Garbage collection requires global knowledge about device states to safely remove information. Distributed garbage collection protocols coordinate removal decisions across devices.
