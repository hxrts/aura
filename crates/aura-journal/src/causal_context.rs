//! Causal Context Types for CRDT Operations
//!
//! This module provides vector clock and causal dependency tracking
//! for implementing proper causal ordering in CmRDT handlers.
//!
//! This is the unified implementation combining functionality from both
//! aura-core and aura-protocol's CausalContext types.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// Import from aura-core for types and hash
use aura_core::{hash, DeviceId};

/// Device/Actor identifier for vector clocks
pub type ActorId = DeviceId;

/// Vector clock for causal ordering
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorClock {
    /// Clock values for each known actor
    pub clocks: BTreeMap<ActorId, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock
    pub fn new() -> Self {
        Self {
            clocks: BTreeMap::new(),
        }
    }

    /// Create a vector clock with a single actor
    pub fn single(actor: ActorId, time: u64) -> Self {
        let mut clocks = BTreeMap::new();
        clocks.insert(actor, time);
        Self { clocks }
    }

    /// Get the clock value for an actor
    pub fn get(&self, actor: &ActorId) -> u64 {
        self.clocks.get(actor).copied().unwrap_or(0)
    }

    /// Set the clock value for an actor
    pub fn set(&mut self, actor: ActorId, time: u64) {
        self.clocks.insert(actor, time);
    }

    /// Increment the clock for an actor
    pub fn increment(&mut self, actor: ActorId) {
        let current = self.get(&actor);
        self.clocks.insert(actor, current + 1);
    }

    /// Update this clock with another clock (taking maximum of each actor)
    pub fn update(&mut self, other: &VectorClock) {
        for (actor, other_time) in &other.clocks {
            let current_time = self.get(actor);
            if *other_time > current_time {
                self.clocks.insert(*actor, *other_time);
            }
        }
    }

    /// Merge with another clock (alias for update for compatibility)
    pub fn merge(&mut self, other: &VectorClock) {
        self.update(other);
    }

    /// Check if this clock happens-before another clock
    pub fn happens_before(&self, other: &VectorClock) -> bool {
        // VC1 < VC2 if VC1[i] <= VC2[i] for all i AND VC1 != VC2
        let mut all_leq = true;
        let mut some_less = false;

        // Check all actors in both clocks
        let mut all_actors = BTreeSet::new();
        all_actors.extend(self.clocks.keys());
        all_actors.extend(other.clocks.keys());

        for actor in all_actors {
            let self_time = self.get(actor);
            let other_time = other.get(actor);

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

    /// Check if this clock is concurrent with another clock
    pub fn concurrent_with(&self, other: &VectorClock) -> bool {
        !self.happens_before(other) && !other.happens_before(self) && self != other
    }

    /// Check if this clock is concurrent with another clock (alias for compatibility)
    pub fn is_concurrent_with(&self, other: &VectorClock) -> bool {
        self.concurrent_with(other)
    }

    /// Check if this clock dominates another (happens-after or equal)
    pub fn dominates(&self, other: &VectorClock) -> bool {
        other.happens_before(self) || self == other
    }
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

/// Causal context containing vector clock and dependency information
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalContext {
    /// Vector clock representing the causal time
    pub clock: VectorClock,
    /// Optional explicit dependencies this operation requires
    pub dependencies: BTreeSet<OperationId>,
    /// The actor that created this context
    pub actor: ActorId,
}

impl CausalContext {
    /// Create a new causal context for an actor
    pub fn new(actor: ActorId) -> Self {
        Self {
            clock: VectorClock::single(actor, 1),
            dependencies: BTreeSet::new(),
            actor,
        }
    }

    /// Create a context that happens after another context
    pub fn after(actor: ActorId, previous: &CausalContext) -> Self {
        let mut clock = previous.clock.clone();
        clock.increment(actor);
        Self {
            clock,
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
        self.clock.increment(device);
    }

    /// Merge with another context (take maximum of all clocks)
    pub fn merge(&mut self, other: &CausalContext) {
        self.clock.merge(&other.clock);
        self.dependencies.extend(other.dependencies.iter().cloned());
    }

    /// Check if this context happens before another (delegates to vector clock)
    pub fn happens_before(&self, other: &CausalContext) -> bool {
        self.clock.happens_before(&other.clock)
    }

    /// Check if contexts are concurrent (neither happens before the other)
    pub fn is_concurrent_with(&self, other: &CausalContext) -> bool {
        self.clock.is_concurrent_with(&other.clock)
    }

    /// Check if this context is ready given the current state
    /// A context is ready if:
    /// 1. All explicit dependencies have been satisfied
    /// 2. The vector clock dependencies are satisfied
    pub fn is_ready<F>(&self, dependency_check: F, current_clock: &VectorClock) -> bool
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
        let mut required_clock = self.clock.clone();
        // Remove the sender's entry since they can have advanced beyond what we've seen
        required_clock.clocks.remove(&self.actor);

        // Check if we've seen enough from each actor to satisfy causal dependencies
        for (actor, required_time) in &required_clock.clocks {
            if current_clock.get(actor) < *required_time {
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

        assert_eq!(vc1.get(&actor), 0);

        vc1.increment(actor);
        assert_eq!(vc1.get(&actor), 1);

        vc1.set(actor, 5);
        assert_eq!(vc1.get(&actor), 5);
    }

    #[test]
    fn test_vector_clock_happens_before() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let mut vc1 = VectorClock::new();
        vc1.set(actor1, 1);
        vc1.set(actor2, 2);

        let mut vc2 = VectorClock::new();
        vc2.set(actor1, 2);
        vc2.set(actor2, 3);

        assert!(vc1.happens_before(&vc2));
        assert!(!vc2.happens_before(&vc1));
    }

    #[test]
    fn test_vector_clock_concurrent() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let mut vc1 = VectorClock::new();
        vc1.set(actor1, 2);
        vc1.set(actor2, 1);

        let mut vc2 = VectorClock::new();
        vc2.set(actor1, 1);
        vc2.set(actor2, 2);

        assert!(vc1.concurrent_with(&vc2));
        assert!(vc2.concurrent_with(&vc1));
    }

    #[test]
    fn test_causal_context_ready() {
        let actor1 = test_actor();
        let actor2 = test_actor();

        let ctx = CausalContext::new(actor1);
        let mut current_clock = VectorClock::new();

        // Should be ready when we have no dependencies
        assert!(ctx.is_ready(|_| true, &current_clock));

        // Should be ready when current clock dominates required clock
        current_clock.set(actor1, 1);
        current_clock.set(actor2, 1);
        assert!(ctx.is_ready(|_| true, &current_clock));
    }

    #[test]
    fn test_causal_context_with_dependencies() {
        let actor = test_actor();
        let dep_id = OperationId::new(actor, 42);

        let ctx = CausalContext::new(actor).with_dependency(dep_id.clone());
        let current_clock = VectorClock::new();

        // Should not be ready when dependency is not satisfied
        assert!(!ctx.is_ready(|id| id != &dep_id, &current_clock));

        // Should be ready when dependency is satisfied
        assert!(ctx.is_ready(|id| id == &dep_id, &current_clock));
    }
}
