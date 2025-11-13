#![allow(clippy::disallowed_methods)]

//! Meet-based CRDT effect handler enforcing meet semi-lattice laws
//!
//! This module provides effect handlers for meet semi-lattices that enable
//! constraint satisfaction and capability restriction through meet operations.

use aura_core::identifiers::DeviceId;
use aura_core::semilattice::{
    ConsistencyProof, ConstraintMsg, ConstraintScope, MeetStateMsg, MvState, Top,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Event recording constraint application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintEvent<S> {
    /// State before constraint application
    pub previous: S,
    /// The constraint that was applied
    pub constraint: S,
    /// State after constraint application (previous ∧ constraint)
    pub result: S,
    /// Timestamp when constraint was applied
    pub timestamp: u64,
}

/// Meet-based CRDT effect handler enforcing meet semi-lattice laws
///
/// This handler manages constraint-based state where operations restrict rather
/// than accumulate. Perfect for capability sets, security policies, and access control.
#[derive(Debug, Clone)]
pub struct MvHandler<S: MvState + Top> {
    /// Current constraint state
    pub state: S,
    /// History of constraint applications for audit trails
    constraint_history: Vec<ConstraintEvent<S>>,
    /// Monotonic counter for message ordering
    message_counter: u64,
    /// Cache of consistency proofs from other participants
    consistency_proofs: HashMap<DeviceId, ConsistencyProof>,
}

impl<S: MvState + Top> MvHandler<S> {
    /// Create a new meet handler with the most permissive state
    pub fn new() -> Self {
        Self {
            state: S::top(), // Start with most permissive state
            constraint_history: Vec::new(),
            message_counter: 0,
            consistency_proofs: HashMap::new(),
        }
    }

    /// Create a meet handler with specific initial state
    pub fn with_state(initial_state: S) -> Self {
        Self {
            state: initial_state,
            constraint_history: Vec::new(),
            message_counter: 0,
            consistency_proofs: HashMap::new(),
        }
    }

    /// Get the current state
    pub fn get_state(&self) -> &S {
        &self.state
    }

    /// Apply constraint through meet operation
    ///
    /// This is the core operation that enforces meet semi-lattice semantics:
    /// new_state = current_state ∧ constraint
    pub fn on_constraint(&mut self, constraint: S) {
        let previous = self.state.clone();
        self.state = self.state.meet(&constraint);

        // Record constraint application for audit trail
        self.constraint_history.push(ConstraintEvent {
            previous,
            constraint,
            result: self.state.clone(),
            timestamp: current_timestamp(),
        });
    }

    /// Receive and process a meet state message
    ///
    /// Updates state through meet operation with received state
    pub fn on_recv(&mut self, msg: MeetStateMsg<S>) -> Result<(), String> {
        // Verify message ordering
        if msg.monotonic_counter <= self.message_counter {
            return Err("Message counter regression detected".to_string());
        }

        self.message_counter = msg.monotonic_counter;
        self.on_constraint(msg.payload);
        Ok(())
    }

    /// Create a state message for sending current state
    pub fn create_state_msg(&mut self) -> MeetStateMsg<S> {
        self.message_counter += 1;
        MeetStateMsg::new(self.state.clone(), self.message_counter)
    }

    /// Verify constraint satisfaction
    ///
    /// Returns true if the current state satisfies the given constraint.
    /// Mathematically: current_state ∧ constraint = current_state
    pub fn satisfies_constraint(&self, constraint: &S) -> bool {
        self.state.meet(constraint) == self.state
    }

    /// Check if the current state is more restrictive than the given state
    ///
    /// Returns true if current ∧ other = current (i.e., current ≤ other in meet order)
    pub fn is_more_restrictive_than(&self, other: &S) -> bool {
        self.state.meet(other) == self.state
    }

    /// Get constraint application history
    pub fn get_constraint_history(&self) -> &[ConstraintEvent<S>] {
        &self.constraint_history
    }

    /// Process a consistency proof from another participant
    pub fn receive_consistency_proof(&mut self, proof: ConsistencyProof) {
        self.consistency_proofs.insert(proof.participant, proof);
    }

    /// Generate consistency proof for current state
    pub fn generate_consistency_proof(&self, participant: DeviceId) -> ConsistencyProof {
        let state_bytes = bincode::serialize(&self.state).unwrap_or_else(|_| Vec::new());
        let constraint_hash: [u8; 32] = aura_core::hash::hash(&state_bytes);

        ConsistencyProof::new(constraint_hash, participant, current_timestamp())
    }

    /// Verify consensus on constraint intersection
    ///
    /// Returns true if all received consistency proofs match our computed state
    pub fn verify_consensus(&self) -> bool {
        if self.consistency_proofs.is_empty() {
            return true;
        }

        let our_hash: [u8; 32] = {
            let state_bytes = bincode::serialize(&self.state).unwrap_or_else(|_| Vec::new());
            aura_core::hash::hash(&state_bytes)
        };

        self.consistency_proofs
            .values()
            .all(|proof| proof.constraint_hash == our_hash)
    }

    /// Clear constraint history (for memory management)
    pub fn clear_history(&mut self) {
        self.constraint_history.clear();
    }

    /// Get number of constraints applied
    pub fn constraint_count(&self) -> usize {
        self.constraint_history.len()
    }
}

impl<S: MvState + Top> Default for MvHandler<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Constraint application result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConstraintResult {
    /// Constraint applied successfully
    Applied,
    /// Constraint had no effect (already satisfied)
    NoEffect,
    /// Constraint application failed
    Failed(String),
}

/// Multi-constraint handler for managing multiple constraint domains
#[derive(Debug, Clone)]
pub struct MultiConstraintHandler<S: MvState + Top> {
    /// Handlers for different constraint scopes
    handlers: HashMap<ConstraintScope, MvHandler<S>>,
}

impl<S: MvState + Top> MultiConstraintHandler<S> {
    /// Create a new multi-constraint handler
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Get or create handler for a specific scope
    pub fn get_or_create_handler(&mut self, scope: ConstraintScope) -> &mut MvHandler<S> {
        self.handlers.entry(scope).or_default()
    }

    /// Apply constraint to specific scope
    pub fn apply_constraint(&mut self, msg: ConstraintMsg<S>) -> ConstraintResult {
        let handler = self.get_or_create_handler(msg.scope);
        handler.on_constraint(msg.constraint);
        ConstraintResult::Applied
    }

    /// Get combined constraint state across all scopes
    pub fn get_combined_state(&self) -> Option<S> {
        if self.handlers.is_empty() {
            return None;
        }

        let mut result = S::top();
        for handler in self.handlers.values() {
            result = result.meet(handler.get_state());
        }
        Some(result)
    }

    /// Check if constraint is satisfied across relevant scopes
    pub fn satisfies_constraint(&self, constraint: &S, scope: &ConstraintScope) -> bool {
        match self.handlers.get(scope) {
            Some(handler) => handler.satisfies_constraint(constraint),
            None => true, // No constraints = everything is satisfied
        }
    }
}

impl<S: MvState + Top> Default for MultiConstraintHandler<S> {
    fn default() -> Self {
        Self::new()
    }
}

/// Get current timestamp in seconds since epoch
fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // Create a local wrapper type for testing
    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct TestSet(BTreeSet<String>);

    // Implement MeetSemiLattice for TestSet
    impl aura_core::semilattice::MeetSemiLattice for TestSet {
        fn meet(&self, other: &Self) -> Self {
            // Set intersection as meet operation
            TestSet(self.0.intersection(&other.0).cloned().collect())
        }
    }

    // Implement Top for TestSet (most permissive = no restrictions = empty set)
    impl Top for TestSet {
        fn top() -> Self {
            TestSet(BTreeSet::new())
        }
    }

    // Mark as MvState
    impl MvState for TestSet {}

    #[test]
    fn test_meet_handler_creation() {
        let handler: MvHandler<TestSet> = MvHandler::new();
        assert_eq!(handler.get_state(), &TestSet(BTreeSet::new()));
        assert_eq!(handler.constraint_count(), 0);
    }

    #[test]
    fn test_constraint_application() {
        let mut handler: MvHandler<TestSet> = MvHandler::new();

        // Start with top (empty set for this implementation)
        let initial_state = handler.get_state().clone();
        assert!(initial_state.0.is_empty());

        // Apply constraint - intersection with set containing "read"
        let mut constraint_set = BTreeSet::new();
        constraint_set.insert("read".to_string());
        let constraint = TestSet(constraint_set);
        handler.on_constraint(constraint);

        // Since initial was empty, intersection is still empty
        assert!(handler.get_state().0.is_empty());
        assert_eq!(handler.constraint_count(), 1);
    }

    #[test]
    fn test_constraint_satisfaction() {
        // Start with an empty set (Top = most permissive)
        let handler: MvHandler<TestSet> = MvHandler::new();

        let mut read_set = BTreeSet::new();
        read_set.insert("read".to_string());
        let read_only = TestSet(read_set);

        let mut read_write_set = BTreeSet::new();
        read_write_set.insert("read".to_string());
        read_write_set.insert("write".to_string());
        let read_write = TestSet(read_write_set);

        // Empty state {} (Top) satisfies any constraint because it's the most permissive
        // {} ∧ {"read"} = {} (state is unchanged, so constraint is satisfied)
        assert!(handler.satisfies_constraint(&read_only));
        assert!(handler.satisfies_constraint(&read_write));

        // Now apply a constraint to make state more restrictive
        let handler2 = MvHandler::with_state(read_write.clone());
        // State: {"read", "write"} (more restrictive)

        // State {"read", "write"} satisfies constraint {"read", "write"}
        // {"read", "write"} ∧ {"read", "write"} = {"read", "write"} ✓
        assert!(handler2.satisfies_constraint(&read_write));

        // State {"read", "write"} satisfies less restrictive constraint {"read"}
        // {"read", "write"} ∧ {"read"} = {"read"} ≠ {"read", "write"} ✗
        assert!(!handler2.satisfies_constraint(&read_only));

        // A less restrictive state should satisfy more restrictive constraints
        let handler3 = MvHandler::with_state(read_only.clone());
        // State: {"read"}

        // State {"read"} satisfies constraint {"read"}
        assert!(handler3.satisfies_constraint(&read_only));

        // State {"read"} satisfies more restrictive constraint {"read", "write"}
        // because {"read"} ∧ {"read", "write"} = {"read"} (state unchanged)
        assert!(handler3.satisfies_constraint(&read_write));
    }

    #[test]
    fn test_state_message_creation() {
        let mut handler: MvHandler<TestSet> = MvHandler::new();

        let msg1 = handler.create_state_msg();
        assert_eq!(msg1.monotonic_counter, 1);

        let msg2 = handler.create_state_msg();
        assert_eq!(msg2.monotonic_counter, 2);
    }

    #[test]
    fn test_consistency_proof() {
        let handler: MvHandler<TestSet> = MvHandler::new();
        let device_id = DeviceId::new();

        let proof = handler.generate_consistency_proof(device_id);
        assert_eq!(proof.participant, device_id);

        // Verify consensus with self
        let mut handler_copy = handler.clone();
        handler_copy.receive_consistency_proof(proof);
        assert!(handler_copy.verify_consensus());
    }

    #[test]
    fn test_multi_constraint_handler() {
        let mut multi_handler: MultiConstraintHandler<TestSet> = MultiConstraintHandler::new();

        let mut read_constraint_set = BTreeSet::new();
        read_constraint_set.insert("read".to_string());
        let read_constraint = TestSet(read_constraint_set);

        let constraint_msg = ConstraintMsg::new(read_constraint, ConstraintScope::Global, 1);

        let result = multi_handler.apply_constraint(constraint_msg);
        assert!(matches!(result, ConstraintResult::Applied));

        let combined = multi_handler.get_combined_state();
        assert!(combined.is_some());
    }
}
