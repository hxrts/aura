//! Causal Context Types for CRDT Operations
//!
//! This module provides vector clock and causal dependency tracking
//! for implementing proper causal ordering in CmRDT handlers.
//!
//! This module now uses the unified time system from aura-core for all time-related types.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// Import unified time types from aura-core
use aura_core::hash;
use aura_core::identifiers::DeviceId;
use aura_core::time::{LogicalTime, VectorClock};

/// Device/Actor identifier for vector clocks
pub type ActorId = DeviceId;

/// Extension trait providing vector clock helpers for the unified `VectorClock` type.
pub trait VectorClockExt {
    /// Get the clock value for an actor (0 if absent)
    fn get_time(&self, actor: &ActorId) -> u64;
    /// Set the clock value for an actor
    fn set_time(&mut self, actor: ActorId, time: u64);
    /// Increment the clock value for an actor
    fn increment(&mut self, actor: ActorId);
    /// Merge with another clock, taking element-wise maxima
    fn update(&mut self, other: &Self);
    /// Merge with another clock (alias for update)
    fn merge(&mut self, other: &Self) {
        self.update(other);
    }
    /// True if self happens-before other (strict)
    fn happens_before(&self, other: &Self) -> bool;
    /// True if clocks are concurrent (no happens-before relation)
    fn concurrent_with(&self, other: &Self) -> bool;
    /// True if clocks are concurrent (alias for concurrent_with)
    fn is_concurrent_with(&self, other: &Self) -> bool {
        self.concurrent_with(other)
    }
    /// True if this clock dominates other (other happens-before self or equal)
    fn dominates(&self, other: &Self) -> bool
    where
        Self: PartialEq,
    {
        other.happens_before(self) || self == other
    }
}

impl VectorClockExt for VectorClock {
    fn get_time(&self, actor: &ActorId) -> u64 {
        match self {
            VectorClock::Single { device, counter } => {
                if device == actor {
                    *counter
                } else {
                    0
                }
            }
            VectorClock::Multiple(map) => map.get(actor).copied().unwrap_or(0),
        }
    }

    fn set_time(&mut self, actor: ActorId, time: u64) {
        match self {
            VectorClock::Single { device, counter } => {
                if device == &actor {
                    *counter = time;
                } else {
                    // Need to convert to Multiple variant
                    let mut map = BTreeMap::new();
                    map.insert(*device, *counter);
                    map.insert(actor, time);
                    *self = VectorClock::Multiple(map);
                }
            }
            VectorClock::Multiple(map) => {
                map.insert(actor, time);
            }
        }
    }

    fn increment(&mut self, actor: ActorId) {
        let current = self.get_time(&actor);
        self.set_time(actor, current + 1);
    }

    fn update(&mut self, other: &Self) {
        match other {
            VectorClock::Single { device, counter } => {
                let current_time = self.get_time(device);
                if *counter > current_time {
                    self.set_time(*device, *counter);
                }
            }
            VectorClock::Multiple(other_map) => {
                for (actor, other_time) in other_map {
                    let current_time = self.get_time(actor);
                    if *other_time > current_time {
                        self.set_time(*actor, *other_time);
                    }
                }
            }
        }
    }

    fn happens_before(&self, other: &Self) -> bool {
        let mut all_leq = true;
        let mut some_less = false;

        let mut all_actors = BTreeSet::new();

        // Collect all actors from both clocks
        match self {
            VectorClock::Single { device, .. } => {
                all_actors.insert(device);
            }
            VectorClock::Multiple(map) => {
                all_actors.extend(map.keys());
            }
        }

        match other {
            VectorClock::Single { device, .. } => {
                all_actors.insert(device);
            }
            VectorClock::Multiple(map) => {
                all_actors.extend(map.keys());
            }
        }

        for actor in all_actors {
            let self_time = self.get_time(actor);
            let other_time = other.get_time(actor);

            if self_time > other_time {
                all_leq = false;
                break;
            }
            if self_time < other_time {
                some_less = true;
            }
        }

        all_leq && some_less
    }

    fn concurrent_with(&self, other: &Self) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }
}

/// Causal context containing logical time and dependency information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalContext {
    /// Logical time representing the causal time (vector clock + lamport clock)
    pub logical_time: LogicalTime,
    /// Optional explicit dependencies this operation requires
    pub dependencies: BTreeSet<OperationId>,
    /// The actor that created this context
    pub actor: ActorId,
}

impl CausalContext {
    /// Create an empty causal context with no actor
    /// Useful for testing or initializing contexts that will be merged later
    pub fn empty() -> Self {
        Self {
            logical_time: LogicalTime {
                vector: VectorClock::new(),
                lamport: 0,
            },
            dependencies: BTreeSet::new(),
            actor: DeviceId::placeholder(),
        }
    }

    /// Create a new causal context for an actor
    pub fn new(actor: ActorId) -> Self {
        let mut vector = VectorClock::new();
        vector.set_time(actor, 1);
        Self {
            logical_time: LogicalTime { vector, lamport: 1 },
            dependencies: BTreeSet::new(),
            actor,
        }
    }

    /// Create a context that happens after another context
    pub fn after(actor: ActorId, previous: &CausalContext) -> Self {
        let mut vector = previous.logical_time.vector.clone();
        vector.increment(actor);
        let lamport = previous.logical_time.lamport + 1;
        Self {
            logical_time: LogicalTime { vector, lamport },
            dependencies: BTreeSet::new(),
            actor,
        }
    }

    /// Add an explicit dependency
    pub fn with_dependency(mut self, dep: OperationId) -> Self {
        self.dependencies.insert(dep);
        self
    }

    /// Increment clock for a device
    pub fn increment(&mut self, device: DeviceId) {
        self.logical_time.vector.increment(device);
        self.logical_time.lamport += 1;
    }

    /// Merge with another context (take maximum of all clocks)
    pub fn merge(&mut self, other: &CausalContext) {
        self.logical_time.vector.merge(&other.logical_time.vector);
        self.logical_time.lamport = self.logical_time.lamport.max(other.logical_time.lamport) + 1;
        self.dependencies.extend(other.dependencies.iter().cloned());
    }

    /// Check if this context happens before another (delegates to vector clock)
    pub fn happens_before(&self, other: &CausalContext) -> bool {
        self.logical_time
            .vector
            .happens_before(&other.logical_time.vector)
    }

    /// Check if contexts are concurrent (neither happens before the other)
    pub fn is_concurrent_with(&self, other: &CausalContext) -> bool {
        self.logical_time
            .vector
            .is_concurrent_with(&other.logical_time.vector)
    }

    /// Check if this context is ready given the current logical time
    /// A context is ready if:
    /// 1. All explicit dependencies have been satisfied
    /// 2. The vector clock dependencies are satisfied
    pub fn is_ready<F>(&self, dependency_check: F, current_logical_time: &LogicalTime) -> bool
    where
        F: Fn(&OperationId) -> bool,
    {
        // Check explicit dependencies
        for dep in &self.dependencies {
            if !dependency_check(dep) {
                return false;
            }
        }

        // Check vector clock causality
        // We can deliver an operation with clock C if our current clock dominates
        // all entries in C except for the sender's entry
        // Check if we've seen enough from each actor to satisfy causal dependencies
        for (actor, required_time) in self.logical_time.vector.iter() {
            // Skip the sender's entry since they can have advanced beyond what we've seen
            if actor == &self.actor {
                continue;
            }

            if current_logical_time.vector.get_time(actor) < *required_time {
                return false;
            }
        }

        true
    }
}

/// Operation identifier for dependency tracking
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OperationId {
    /// The actor that created the operation
    pub actor: ActorId,
    /// Sequence number within that actor's operations
    pub sequence: u64,
}

impl OperationId {
    /// Create a new operation ID
    pub fn new(actor: ActorId, sequence: u64) -> Self {
        Self { actor, sequence }
    }

    /// Get the UUID representation of this operation ID
    pub fn uuid(&self) -> uuid::Uuid {
        // Create a deterministic UUID from actor ID and sequence using hash
        let mut h = hash::hasher();

        // Add actor UUID bytes
        h.update(self.actor.0.as_bytes());
        // Add sequence bytes
        h.update(&self.sequence.to_be_bytes());

        // Take first 16 bytes of hash as UUID
        let hash_bytes = h.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&hash_bytes[0..16]);
        uuid::Uuid::from_bytes(uuid_bytes)
    }

    /// Create an operation ID from a UUID
    /// Note: This is not a perfect round-trip since UUID has less info than OperationId
    pub fn from_uuid(uuid: uuid::Uuid) -> Self {
        // Create a DeviceId from the UUID
        let actor = DeviceId(uuid);
        // Set sequence to 0 since we can't recover it from UUID
        Self::new(actor, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_actor() -> ActorId {
        DeviceId::new()
    }

    #[test]
    fn test_vector_clock_basic() {
        let mut vc1 = VectorClock::new();
        let actor = test_actor();

        assert_eq!(vc1.get_time(&actor), 0);

        vc1.increment(actor);
        assert_eq!(vc1.get_time(&actor), 1);

        vc1.set_time(actor, 5);
        assert_eq!(vc1.get_time(&actor), 5);
    }

    #[test]
    fn test_vector_clock_happens_before() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let mut vc1 = VectorClock::new();
        vc1.set_time(actor1, 1);
        vc1.set_time(actor2, 2);

        let mut vc2 = VectorClock::new();
        vc2.set_time(actor1, 2);
        vc2.set_time(actor2, 3);

        assert!(vc1.happens_before(&vc2));
        assert!(!vc2.happens_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let mut vc1 = VectorClock::new();
        vc1.set_time(actor1, 2);
        vc1.set_time(actor2, 1);

        let mut vc2 = VectorClock::new();
        vc2.set_time(actor1, 1);
        vc2.set_time(actor2, 2);

        assert!(vc1.concurrent_with(&vc2));
        assert!(vc2.concurrent_with(&vc1));
    }

    #[test]
    fn test_causal_context_ready() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let ctx = CausalContext::new(actor1);
        let mut current_vector = VectorClock::new();
        current_vector.set_time(actor1, 1);
        current_vector.set_time(actor2, 1);
        let current_logical_time = LogicalTime {
            vector: current_vector,
            lamport: 2,
        };

        // Should be ready when we have no dependencies and current time dominates
        assert!(ctx.is_ready(|_| true, &current_logical_time));
    }

    #[test]
    fn test_causal_context_with_dependencies() {
        let actor = test_actor();
        let dep_id = OperationId::new(actor, 42);

        let ctx = CausalContext::new(actor).with_dependency(dep_id.clone());
        let current_logical_time = LogicalTime {
            vector: VectorClock::new(),
            lamport: 0,
        };

        // Should not be ready when dependency is not satisfied
        assert!(!ctx.is_ready(|id| id != &dep_id, &current_logical_time));

        // Should be ready when dependency is satisfied
        assert!(ctx.is_ready(|id| id == &dep_id, &current_logical_time));
    }
}
