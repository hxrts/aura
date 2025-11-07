//! State-based CRDT effect handler
//!
//! This module provides the `CvHandler` for enforcing join semilattice laws
//! in state-based CRDTs. The handler automatically merges received states
//! using the join operation, guaranteeing convergence.

use aura_types::semilattice::{CvState, MsgKind, StateMsg};

/// State-based CRDT effect handler
///
/// Enforces the join semilattice law: `state := state.join(&received_state)`
/// for convergent replicated data types (CvRDTs).
pub struct CvHandler<S: CvState> {
    /// Current CRDT state
    pub state: S,
}

impl<S: CvState> CvHandler<S> {
    /// Create a new handler with the bottom element
    pub fn new() -> Self {
        Self { state: S::bottom() }
    }

    /// Create a handler with initial state
    pub fn with_state(state: S) -> Self {
        Self { state }
    }

    /// Handle received state message - enforces join semilattice law
    ///
    /// This method implements the core CvRDT convergence guarantee:
    /// the state monotonically advances toward the least upper bound.
    pub fn on_recv(&mut self, msg: StateMsg<S>) {
        self.state = self.state.join(&msg.payload);
    }

    /// Get current state (immutable reference)
    pub fn get_state(&self) -> &S {
        &self.state
    }

    /// Get current state (mutable reference)
    pub fn get_state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Create state message for sending
    ///
    /// Wraps the current state in a properly tagged message
    /// for session-type communication.
    pub fn create_state_msg(&self) -> StateMsg<S> {
        StateMsg {
            payload: self.state.clone(),
            kind: MsgKind::FullState,
        }
    }

    /// Update state directly (for local operations)
    ///
    /// Use this for local state changes that don't come from
    /// session-type communication (e.g., user operations).
    pub fn update_state(&mut self, new_state: S) {
        self.state = self.state.join(&new_state);
    }

    /// Reset to bottom element
    pub fn reset(&mut self) {
        self.state = S::bottom();
    }
}

impl<S: CvState> Default for CvHandler<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: CvState + std::fmt::Debug> std::fmt::Debug for CvHandler<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CvHandler")
            .field("state", &self.state)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::semilattice::{Bottom, JoinSemilattice};

    // Test CRDT type
    #[derive(Debug, Clone, PartialEq, Eq)]
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

    #[test]
    fn test_cv_handler_new() {
        let handler = CvHandler::<TestCounter>::new();
        assert_eq!(handler.get_state(), &TestCounter(0));
    }

    #[test]
    fn test_cv_handler_with_state() {
        let handler = CvHandler::with_state(TestCounter(42));
        assert_eq!(handler.get_state(), &TestCounter(42));
    }

    #[test]
    fn test_on_recv_enforces_join() {
        let mut handler = CvHandler::with_state(TestCounter(5));

        let msg = StateMsg::new(TestCounter(3));
        handler.on_recv(msg);
        assert_eq!(handler.get_state(), &TestCounter(5)); // max(5, 3) = 5

        let msg = StateMsg::new(TestCounter(10));
        handler.on_recv(msg);
        assert_eq!(handler.get_state(), &TestCounter(10)); // max(5, 10) = 10
    }

    #[test]
    fn test_create_state_msg() {
        let handler = CvHandler::with_state(TestCounter(42));
        let msg = handler.create_state_msg();

        assert_eq!(msg.payload, TestCounter(42));
        assert_eq!(msg.kind, MsgKind::FullState);
    }

    #[test]
    fn test_update_state() {
        let mut handler = CvHandler::with_state(TestCounter(5));
        handler.update_state(TestCounter(3));
        assert_eq!(handler.get_state(), &TestCounter(5)); // max(5, 3) = 5

        handler.update_state(TestCounter(10));
        assert_eq!(handler.get_state(), &TestCounter(10)); // max(5, 10) = 10
    }

    #[test]
    fn test_reset() {
        let mut handler = CvHandler::with_state(TestCounter(42));
        handler.reset();
        assert_eq!(handler.get_state(), &TestCounter(0));
    }
}
