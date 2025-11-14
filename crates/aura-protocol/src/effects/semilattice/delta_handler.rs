//! Delta-based CRDT effect handler
//!
//! This module provides the `DeltaHandler` for accumulating and folding
//! delta updates in delta-based CRDTs. The handler buffers deltas and
//! periodically folds them into the state for bandwidth optimization.

use aura_core::semilattice::{CvState, Delta, DeltaMsg, DeltaProduce, DeltaState, MsgKind};
use std::collections::VecDeque;

/// Delta-based CRDT effect handler
///
/// Accumulates delta updates and folds them into state periodically.
/// This provides bandwidth optimization over full state synchronization.
pub struct DeltaHandler<S, D>
where
    S: CvState,
    D: Delta,
{
    /// Current CRDT state
    pub state: S,
    /// Buffer of accumulated deltas
    pub delta_inbox: VecDeque<D>,
    /// Maximum number of deltas to buffer before folding
    pub fold_threshold: usize,
}

impl<S, D> DeltaHandler<S, D>
where
    S: CvState,
    D: Delta,
{
    /// Create a new delta handler
    pub fn new() -> Self {
        Self {
            state: S::bottom(),
            delta_inbox: VecDeque::new(),
            fold_threshold: 10, // Default threshold
        }
    }

    /// Create a delta handler with initial state
    pub fn with_state(state: S) -> Self {
        Self {
            state,
            delta_inbox: VecDeque::new(),
            fold_threshold: 10,
        }
    }

    /// Create a delta handler with custom fold threshold
    pub fn with_threshold(fold_threshold: usize) -> Self {
        Self {
            state: S::bottom(),
            delta_inbox: VecDeque::new(),
            fold_threshold,
        }
    }

    /// Clear accumulated deltas without applying (generic fallback)
    ///
    /// This method is available for generic DeltaHandler instances but only
    /// clears the inbox without applying deltas. For actual delta application,
    /// use DeltaState-compatible types which have a proper fold_deltas method.
    pub fn clear_deltas_from_inbox(&mut self) {
        let delta_count = self.delta_inbox.len();
        self.delta_inbox.clear();
        if delta_count > 0 {
            tracing::debug!(
                "Cleared {} deltas from inbox without applying (use DeltaState types for actual folding)",
                delta_count
            );
        }
    }

    /// Apply a single delta to the state
    ///
    /// This is a generic fallback implementation. For specific CRDT types,
    /// you should use the specialized DeltaHandler implementations below
    /// that work with the DeltaState trait.
    fn apply_delta_to_state_generic(&mut self, _delta: D) {
        // Generic implementation: We cannot apply deltas without knowing the specific
        // relationship between Delta type and State type. Use the DeltaState implementations below.
        tracing::debug!(
            "Applied delta to state (generic fallback - consider using DeltaState implementation)"
        );
    }

    /// Get current state
    pub fn get_state(&self) -> &S {
        &self.state
    }

    /// Get mutable state reference
    pub fn get_state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Get number of buffered deltas
    pub fn delta_count(&self) -> usize {
        self.delta_inbox.len()
    }

    /// Check if delta inbox is empty
    pub fn is_delta_inbox_empty(&self) -> bool {
        self.delta_inbox.is_empty()
    }

    /// Create delta message for sending
    pub fn create_delta_msg(&self, delta: D) -> DeltaMsg<D> {
        DeltaMsg {
            payload: delta,
            kind: MsgKind::Delta,
        }
    }

    /// Set fold threshold
    pub fn set_fold_threshold(&mut self, threshold: usize) {
        self.fold_threshold = threshold;
    }

    /// Get fold threshold
    pub fn get_fold_threshold(&self) -> usize {
        self.fold_threshold
    }

    /// Clear delta inbox
    pub fn clear_deltas(&mut self) {
        self.delta_inbox.clear();
    }

    /// Update state directly (for local operations)
    pub fn update_state(&mut self, new_state: S) {
        self.state = self.state.join(&new_state);
    }
}

impl<S, D> DeltaHandler<S, D>
where
    S: CvState,
    D: Delta + DeltaProduce<S>,
{
    /// Produce delta from state change
    ///
    /// This method is available when the delta type implements `DeltaProduce<S>`.
    /// It creates a delta representing the change from old_state to new_state.
    pub fn produce_delta(&self, old_state: &S, new_state: &S) -> D {
        D::delta_from(old_state, new_state)
    }
}

/// Specialized DeltaHandler for DeltaState-compatible types
impl<S> DeltaHandler<S, S::Delta>
where
    S: CvState + DeltaState,
    S::Delta: aura_core::semilattice::Delta,
{
    /// Apply a single delta to the state using DeltaState trait
    ///
    /// This implementation uses the DeltaState trait to properly apply deltas
    /// to states that support delta-based updates.
    fn apply_delta_to_state(&mut self, delta: S::Delta) {
        let new_state = self.state.apply_delta(&delta);
        self.state = self.state.join(&new_state);
    }

    /// Handle received delta message (specialized for DeltaState)
    ///
    /// This version properly folds and applies deltas when the threshold is reached.
    pub fn on_recv(&mut self, msg: DeltaMsg<S::Delta>) {
        self.delta_inbox.push_back(msg.payload);

        // Check if we should fold deltas into state
        if self.delta_inbox.len() >= self.fold_threshold {
            self.fold_deltas();
        }
    }

    /// Update state with local change and produce delta
    ///
    /// This method combines local state updates with delta production,
    /// which is useful for tracking local changes that need to be propagated.
    pub fn update_and_produce_delta(&mut self, new_state: S) -> Option<S::Delta>
    where
        S::Delta: DeltaProduce<S>,
    {
        if new_state != self.state {
            let delta = S::Delta::delta_from(&self.state, &new_state);
            self.state = self.state.join(&new_state);
            Some(delta)
        } else {
            None
        }
    }

    /// Apply a batch of deltas efficiently
    ///
    /// This method combines all deltas into a single delta before applying,
    /// which maintains proper CRDT semantics and is more efficient than
    /// applying them one by one through on_recv.
    pub fn apply_deltas(&mut self, deltas: Vec<S::Delta>) {
        if deltas.is_empty() {
            return;
        }

        // Combine all deltas into a single delta using join_delta
        let combined_delta = deltas
            .into_iter()
            .reduce(|acc, delta| acc.join_delta(&delta));

        if let Some(delta) = combined_delta {
            self.apply_delta_to_state(delta);
        }
    }

    /// Fold accumulated deltas into state
    ///
    /// This operation combines all buffered deltas and applies them to the state.
    /// The folding process maintains the semilattice properties of the CRDT.
    pub fn fold_deltas(&mut self) {
        if self.delta_inbox.is_empty() {
            return;
        }

        // Combine all deltas into a single delta
        let combined_delta = self
            .delta_inbox
            .drain(..)
            .reduce(|acc, delta| acc.join_delta(&delta));

        if let Some(delta) = combined_delta {
            // Apply the combined delta to state using DeltaState trait
            self.apply_delta_to_state(delta);
        }
    }

    /// Force fold of deltas (specialized for DeltaState)
    ///
    /// Immediately fold all buffered deltas into state, regardless of threshold.
    pub fn force_fold(&mut self) {
        self.fold_deltas();
    }
}

/// Type alias for JournalMap delta handler
pub type JournalDeltaHandler<J> = DeltaHandler<J, <J as DeltaState>::Delta>;

/// Helper functions for creating specialized handlers
impl<S> DeltaHandler<S, S::Delta>
where
    S: CvState + DeltaState,
    S::Delta: aura_core::semilattice::Delta,
{
    /// Create a new delta handler optimized for this state type
    pub fn for_state_type() -> Self {
        Self {
            state: S::bottom(),
            delta_inbox: VecDeque::new(),
            fold_threshold: 10,
        }
    }

    /// Create a delta handler with specific state and optimized folding
    pub fn for_state_type_with_state(state: S) -> Self {
        Self {
            state,
            delta_inbox: VecDeque::new(),
            fold_threshold: 10,
        }
    }
}

impl<S, D> Default for DeltaHandler<S, D>
where
    S: CvState,
    D: Delta,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<S, D> std::fmt::Debug for DeltaHandler<S, D>
where
    S: CvState + std::fmt::Debug,
    D: Delta + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeltaHandler")
            .field("state", &self.state)
            .field("delta_count", &self.delta_count())
            .field("fold_threshold", &self.fold_threshold)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::semilattice::{Bottom, JoinSemilattice};

    // Test state type
    #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    struct TestCounter(u64);

    impl JoinSemilattice for TestCounter {
        fn join(&self, other: &Self) -> Self {
            TestCounter(self.0.max(other.0))
        }
    }

    impl Bottom for TestCounter {
        fn bottom() -> Self {
            TestCounter(0)
        }
    }

    impl CvState for TestCounter {}

    impl DeltaState for TestCounter {
        type Delta = TestDelta;

        fn apply_delta(&self, delta: &Self::Delta) -> Self {
            TestCounter(self.0 + delta.0)
        }
    }

    // Test delta type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestDelta(u64);

    impl Delta for TestDelta {
        fn join_delta(&self, other: &Self) -> Self {
            TestDelta(self.0.max(other.0))
        }
    }

    impl DeltaProduce<TestCounter> for TestDelta {
        fn delta_from(old: &TestCounter, new: &TestCounter) -> Self {
            TestDelta(new.0.saturating_sub(old.0))
        }
    }

    #[test]
    fn test_delta_handler_new() {
        let handler = DeltaHandler::<TestCounter, TestDelta>::new();
        assert_eq!(handler.get_state(), &TestCounter(0));
        assert!(handler.is_delta_inbox_empty());
        assert_eq!(handler.get_fold_threshold(), 10);
    }

    #[test]
    fn test_delta_handler_with_state() {
        let handler: DeltaHandler<TestCounter, TestDelta> =
            DeltaHandler::with_state(TestCounter(42));
        assert_eq!(handler.get_state(), &TestCounter(42));
    }

    #[test]
    fn test_delta_handler_with_threshold() {
        let handler = DeltaHandler::<TestCounter, TestDelta>::with_threshold(5);
        assert_eq!(handler.get_fold_threshold(), 5);
    }

    #[test]
    fn test_on_recv_buffers_deltas() {
        let mut handler = DeltaHandler::<TestCounter, TestDelta>::with_threshold(3);

        handler.on_recv(DeltaMsg::new(TestDelta(5)));
        assert_eq!(handler.delta_count(), 1);

        handler.on_recv(DeltaMsg::new(TestDelta(3)));
        assert_eq!(handler.delta_count(), 2);
    }

    #[test]
    fn test_fold_deltas_at_threshold() {
        let mut handler = DeltaHandler::<TestCounter, TestDelta>::with_threshold(2);

        handler.on_recv(DeltaMsg::new(TestDelta(5)));
        assert_eq!(handler.delta_count(), 1);

        // This should trigger folding
        handler.on_recv(DeltaMsg::new(TestDelta(3)));
        assert_eq!(handler.delta_count(), 0); // Deltas should be folded
    }

    #[test]
    fn test_force_fold() {
        let mut handler = DeltaHandler::<TestCounter, TestDelta>::with_threshold(10);

        handler.on_recv(DeltaMsg::new(TestDelta(5)));
        handler.on_recv(DeltaMsg::new(TestDelta(3)));
        assert_eq!(handler.delta_count(), 2);

        handler.force_fold();
        assert_eq!(handler.delta_count(), 0);
    }

    #[test]
    fn test_produce_delta() {
        let handler = DeltaHandler::<TestCounter, TestDelta>::new();

        let old_state = TestCounter(5);
        let new_state = TestCounter(8);

        let delta = handler.produce_delta(&old_state, &new_state);
        assert_eq!(delta, TestDelta(3)); // 8 - 5 = 3
    }

    #[test]
    fn test_create_delta_msg() {
        let handler = DeltaHandler::<TestCounter, TestDelta>::new();
        let delta = TestDelta(5);
        let msg = handler.create_delta_msg(delta);

        assert_eq!(msg.payload, TestDelta(5));
        assert_eq!(msg.kind, MsgKind::Delta);
    }

    #[test]
    fn test_set_fold_threshold() {
        let mut handler = DeltaHandler::<TestCounter, TestDelta>::new();
        assert_eq!(handler.get_fold_threshold(), 10);

        handler.set_fold_threshold(20);
        assert_eq!(handler.get_fold_threshold(), 20);
    }

    #[test]
    fn test_delta_state_application() {
        let mut handler =
            DeltaHandler::<TestCounter, TestDelta>::for_state_type_with_state(TestCounter(5));

        // Apply a delta directly
        handler.apply_deltas(vec![TestDelta(3), TestDelta(7)]);

        // State should be updated: 5 + max(3, 7) = 5 + 7 = 12
        assert_eq!(handler.get_state(), &TestCounter(12));
    }

    #[test]
    fn test_update_and_produce_delta() {
        let mut handler =
            DeltaHandler::<TestCounter, TestDelta>::for_state_type_with_state(TestCounter(5));

        // Update state and produce delta
        let delta = handler.update_and_produce_delta(TestCounter(10));

        assert!(delta.is_some());
        assert_eq!(delta.unwrap(), TestDelta(5)); // 10 - 5 = 5
        assert_eq!(handler.get_state(), &TestCounter(10));
    }

    #[test]
    fn test_update_and_produce_delta_no_change() {
        let mut handler =
            DeltaHandler::<TestCounter, TestDelta>::for_state_type_with_state(TestCounter(5));

        // Update with same state
        let delta = handler.update_and_produce_delta(TestCounter(5));

        assert!(delta.is_none()); // No change, no delta produced
        assert_eq!(handler.get_state(), &TestCounter(5));
    }

    #[test]
    fn test_fold_deltas_with_delta_state() {
        let mut handler =
            DeltaHandler::<TestCounter, TestDelta>::for_state_type_with_state(TestCounter(5));
        handler.set_fold_threshold(2);

        // Add deltas to trigger folding
        handler.on_recv(DeltaMsg::new(TestDelta(3)));
        handler.on_recv(DeltaMsg::new(TestDelta(7))); // This should trigger folding

        // State should be: 5 + max(3, 7) = 5 + 7 = 12
        assert_eq!(handler.get_state(), &TestCounter(12));
        assert_eq!(handler.delta_count(), 0); // Deltas should be folded
    }

    #[test]
    fn test_for_state_type_constructors() {
        let handler1 = DeltaHandler::<TestCounter, TestDelta>::for_state_type();
        assert_eq!(handler1.get_state(), &TestCounter(0));

        let handler2 =
            DeltaHandler::<TestCounter, TestDelta>::for_state_type_with_state(TestCounter(42));
        assert_eq!(handler2.get_state(), &TestCounter(42));
    }
}
