//! Operation-based CRDT effect handler
//!
//! This module provides the `CmHandler` for enforcing causal ordering and
//! deduplication laws in operation-based CRDTs. The handler buffers operations
//! until causal dependencies are satisfied, then applies them idempotently.

use aura_types::semilattice::{CausalOp, CmApply, Dedup, OpWithCtx};
use std::collections::VecDeque;

/// Operation-based CRDT effect handler
///
/// Enforces causal ordering and deduplication for commutative
/// replicated data types (CmRDTs).
pub struct CmHandler<S, Op, Id, Ctx>
where
    S: CmApply<Op> + Dedup<Id>,
    Op: CausalOp<Id = Id, Ctx = Ctx>,
    Id: Clone,
{
    /// Current CRDT state
    pub state: S,
    /// Buffer for operations waiting for causal dependencies
    pub buffer: VecDeque<OpWithCtx<Op, Ctx>>,
}

impl<S, Op, Id, Ctx> CmHandler<S, Op, Id, Ctx>
where
    S: CmApply<Op> + Dedup<Id>,
    Op: CausalOp<Id = Id, Ctx = Ctx>,
    Id: Clone,
{
    /// Create a new handler with initial state
    pub fn new(state: S) -> Self {
        Self {
            state,
            buffer: VecDeque::new(),
        }
    }

    /// Handle received operation - enforces causal ordering and deduplication
    ///
    /// This method implements the core CmRDT guarantees:
    /// 1. Operations are applied only when causally ready
    /// 2. Duplicate operations are detected and ignored
    /// 3. Operations commute under proper causal delivery
    pub fn on_recv(&mut self, msg: OpWithCtx<Op, Ctx>) {
        if self.is_causal_ready(&msg.ctx) && !self.state.seen(&msg.op.id()) {
            // Apply operation immediately
            self.state.mark_seen(msg.op.id());
            self.state.apply(msg.op);

            // Check if any buffered operations are now ready
            self.process_buffered();
        } else if !self.state.seen(&msg.op.id()) {
            // Buffer operation for later
            self.buffer.push_back(msg);
        }
        // If already seen, ignore (deduplication)
    }

    /// Check if operation is ready for delivery based on causal context
    ///
    /// This is a placeholder implementation. Real implementations would:
    /// - Check vector clocks for causal dependencies
    /// - Verify dependency sets are satisfied
    /// - Use Lamport timestamps or other ordering mechanisms
    fn is_causal_ready(&self, _ctx: &Ctx) -> bool {
        // For now, assume all operations are ready
        // TODO: Implement proper causal ordering based on context type
        true
    }

    /// Process buffered operations that are now ready
    ///
    /// This method should be called after applying any operation
    /// to check if buffered operations have become causally ready.
    pub fn process_buffered(&mut self) {
        let mut ready_ops = Vec::new();

        // Find operations that are now ready
        let mut i = 0;
        while i < self.buffer.len() {
            if self.is_causal_ready(&self.buffer[i].ctx)
                && !self.state.seen(&self.buffer[i].op.id())
            {
                ready_ops.push(self.buffer.remove(i).unwrap());
            } else {
                i += 1;
            }
        }

        // Apply ready operations
        for msg in ready_ops {
            self.state.mark_seen(msg.op.id());
            self.state.apply(msg.op);
        }
    }

    /// Get current state (immutable reference)
    pub fn get_state(&self) -> &S {
        &self.state
    }

    /// Get current state (mutable reference)
    pub fn get_state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Get number of buffered operations
    pub fn buffer_len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if buffer is empty
    pub fn buffer_is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Clear the buffer (use with caution)
    ///
    /// This should only be used in exceptional cases like
    /// session reset or recovery scenarios.
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    /// Create operation message for broadcasting
    ///
    /// This is a utility method for creating properly formatted
    /// operation messages for session-type communication.
    pub fn create_op_msg(&self, op: Op, ctx: Ctx) -> OpWithCtx<Op, Ctx> {
        OpWithCtx::new(op, ctx)
    }
}

impl<S, Op, Id, Ctx> std::fmt::Debug for CmHandler<S, Op, Id, Ctx>
where
    S: CmApply<Op> + Dedup<Id> + std::fmt::Debug,
    Op: CausalOp<Id = Id, Ctx = Ctx> + std::fmt::Debug,
    Ctx: std::fmt::Debug,
    Id: Clone,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CmHandler")
            .field("state", &self.state)
            .field("buffer_len", &self.buffer.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Test operation type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestOp {
        id: u64,
        value: i32,
    }

    impl CausalOp for TestOp {
        type Id = u64;
        type Ctx = (); // Simple context for testing

        fn id(&self) -> Self::Id {
            self.id
        }

        fn ctx(&self) -> &Self::Ctx {
            &()
        }
    }

    // Test state type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestState {
        sum: i32,
        seen_ops: HashSet<u64>,
    }

    impl TestState {
        fn new() -> Self {
            Self {
                sum: 0,
                seen_ops: HashSet::new(),
            }
        }
    }

    impl CmApply<TestOp> for TestState {
        fn apply(&mut self, op: TestOp) {
            self.sum += op.value;
        }
    }

    impl Dedup<u64> for TestState {
        fn seen(&self, id: &u64) -> bool {
            self.seen_ops.contains(id)
        }

        fn mark_seen(&mut self, id: u64) {
            self.seen_ops.insert(id);
        }
    }

    #[test]
    fn test_cm_handler_new() {
        let state = TestState::new();
        let handler = CmHandler::new(state);
        assert_eq!(handler.get_state().sum, 0);
        assert!(handler.buffer_is_empty());
    }

    #[test]
    fn test_on_recv_applies_operation() {
        let mut handler = CmHandler::new(TestState::new());

        let op = TestOp { id: 1, value: 5 };
        let msg = OpWithCtx::new(op, ());

        handler.on_recv(msg);
        assert_eq!(handler.get_state().sum, 5);
        assert!(handler.get_state().seen(&1));
    }

    #[test]
    fn test_on_recv_deduplicates() {
        let mut handler = CmHandler::new(TestState::new());

        let op1 = TestOp { id: 1, value: 5 };
        let op2 = TestOp { id: 1, value: 10 }; // Same ID, different value

        handler.on_recv(OpWithCtx::new(op1, ()));
        handler.on_recv(OpWithCtx::new(op2, ()));

        // Should only apply first operation
        assert_eq!(handler.get_state().sum, 5);
    }

    #[test]
    fn test_multiple_operations() {
        let mut handler = CmHandler::new(TestState::new());

        handler.on_recv(OpWithCtx::new(TestOp { id: 1, value: 5 }, ()));
        handler.on_recv(OpWithCtx::new(TestOp { id: 2, value: 3 }, ()));
        handler.on_recv(OpWithCtx::new(TestOp { id: 3, value: -2 }, ()));

        assert_eq!(handler.get_state().sum, 6); // 5 + 3 - 2
        assert!(handler.get_state().seen(&1));
        assert!(handler.get_state().seen(&2));
        assert!(handler.get_state().seen(&3));
    }

    #[test]
    fn test_create_op_msg() {
        let handler = CmHandler::new(TestState::new());
        let op = TestOp { id: 1, value: 5 };
        let msg = handler.create_op_msg(op, ());

        assert_eq!(msg.op.id, 1);
        assert_eq!(msg.op.value, 5);
    }
}
