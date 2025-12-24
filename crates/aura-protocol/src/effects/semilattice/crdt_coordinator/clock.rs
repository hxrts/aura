//! Vector clock utilities for CRDT coordination

use aura_core::identifiers::DeviceId;
use aura_core::time::VectorClock;

/// Merge source vector clock into target, taking the maximum for each actor.
pub fn merge_vector_clocks(target: &mut VectorClock, other: &VectorClock) {
    for (actor, time) in other.iter() {
        let current = target.get(actor).copied().unwrap_or(0);
        if *time > current {
            target.insert(*actor, *time);
        }
    }
}

/// Get the maximum counter value from a vector clock (Lamport time).
pub fn max_counter(clock: &VectorClock) -> u64 {
    clock.iter().map(|(_, counter)| *counter).max().unwrap_or(0)
}

/// Increment the counter for a specific actor in the vector clock.
pub fn increment_actor(clock: &mut VectorClock, actor: DeviceId) {
    let current = clock.get(&actor).copied().unwrap_or(0);
    clock.insert(actor, current.saturating_add(1));
}
