//! Operation-based CRDT effect handler
//!
//! This module provides the `CmHandler` for enforcing causal ordering and
//! deduplication laws in operation-based CRDTs. The handler buffers operations
//! until causal dependencies are satisfied, then applies them idempotently.

use aura_core::semilattice::{CausalOp, CmApply, Dedup, OpWithCtx};
use aura_journal::{CausalContext, OperationId, VectorClock, VectorClockExt};
use std::collections::{HashMap, VecDeque};

/// Operation-based CRDT effect handler with proper causal ordering
///
/// Enforces causal ordering and deduplication for commutative
/// replicated data types (CmRDTs). Uses vector clocks to ensure
/// causal delivery guarantees.
pub struct CmHandler<S, Op, Id>
where
    S: CmApply<Op> + Dedup<Id>,
    Op: CausalOp<Id = Id, Ctx = CausalContext>,
    Id: Clone + PartialEq,
{
    /// Current CRDT state
    pub state: S,
    /// Buffer for operations waiting for causal dependencies
    pub buffer: VecDeque<OpWithCtx<Op, CausalContext>>,
    /// Current vector clock representing our causal knowledge
    pub current_clock: VectorClock,
    /// Applied operations for dependency checking
    pub applied_operations: HashMap<OperationId, bool>,
}

impl<S, Op, Id> CmHandler<S, Op, Id>
where
    S: CmApply<Op> + Dedup<Id>,
    Op: CausalOp<Id = Id, Ctx = CausalContext>,
    Id: Clone + PartialEq,
{
    /// Create a new handler with initial state
    pub fn new(state: S) -> Self {
        Self {
            state,
            buffer: VecDeque::new(),
            current_clock: VectorClock::new(),
            applied_operations: HashMap::new(),
        }
    }

    /// Handle received operation - enforces causal ordering and deduplication
    ///
    /// This method implements the core CmRDT guarantees:
    /// 1. Operations are applied only when causally ready
    /// 2. Duplicate operations are detected and ignored
    /// 3. Operations commute under proper causal delivery
    pub fn on_recv(&mut self, msg: OpWithCtx<Op, CausalContext>) {
        if self.is_causal_ready(&msg.ctx) && !self.state.seen(&msg.op.id()) {
            // Apply operation immediately
            self.apply_operation(msg.op, msg.ctx);

            // Check if any buffered operations are now ready
            self.process_buffered();
        } else if !self.state.seen(&msg.op.id()) {
            // Buffer operation for later
            self.buffer.push_back(msg);
        }
        // If already seen, ignore (deduplication)
    }

    /// Apply an operation and update our causal state
    fn apply_operation(&mut self, op: Op, ctx: CausalContext) {
        // Mark as seen for deduplication
        self.state.mark_seen(op.id());

        // Create operation ID for dependency tracking
        let op_id = OperationId::new(ctx.actor, ctx.logical_time.vector.get_time(&ctx.actor));
        self.applied_operations.insert(op_id, true);

        // Update our vector clock
        self.current_clock.update(&ctx.logical_time.vector);

        // Apply the operation to the state
        self.state.apply(op);
    }

    /// Check if operation is ready for delivery based on causal context
    ///
    /// Uses vector clocks and explicit dependencies to determine if an operation
    /// can be safely delivered without violating causal ordering guarantees.
    fn is_causal_ready(&self, ctx: &CausalContext) -> bool {
        ctx.is_ready(
            |op_id| self.applied_operations.contains_key(op_id),
            &aura_core::time::LogicalTime {
                vector: self.current_clock.clone(),
                lamport: 0,
            },
        )
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
            self.apply_operation(msg.op, msg.ctx);
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
    pub fn create_op_msg(&self, op: Op, ctx: CausalContext) -> OpWithCtx<Op, CausalContext> {
        OpWithCtx::new(op, ctx)
    }

    /// Get current vector clock
    pub fn current_clock(&self) -> &VectorClock {
        &self.current_clock
    }

    /// Get applied operations (for debugging/inspection)
    pub fn applied_operations(&self) -> &HashMap<OperationId, bool> {
        &self.applied_operations
    }
}

impl<S, Op, Id> std::fmt::Debug for CmHandler<S, Op, Id>
where
    S: CmApply<Op> + Dedup<Id> + std::fmt::Debug,
    Op: CausalOp<Id = Id, Ctx = CausalContext> + std::fmt::Debug,
    Id: Clone + PartialEq,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CmHandler")
            .field("state", &self.state)
            .field("buffer_len", &self.buffer.len())
            .field("current_clock", &self.current_clock)
            .field("applied_operations_count", &self.applied_operations.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::identifiers::DeviceId;
    use std::collections::HashSet;

    // Test operation type
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestOp {
        id: u64,
        value: i32,
        causal_ctx: CausalContext,
    }

    impl CausalOp for TestOp {
        type Id = u64;
        type Ctx = CausalContext;

        fn id(&self) -> Self::Id {
            self.id
        }

        fn ctx(&self) -> &Self::Ctx {
            &self.causal_ctx
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

    fn create_test_op(id: u64, value: i32, actor: DeviceId) -> TestOp {
        TestOp {
            id,
            value,
            causal_ctx: CausalContext::new(actor),
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
        let actor = DeviceId::deterministic_test_id();

        let op = create_test_op(1, 5, actor);
        let msg = OpWithCtx::new(op.clone(), op.causal_ctx.clone());

        handler.on_recv(msg);
        assert_eq!(handler.get_state().sum, 5);
        assert!(handler.get_state().seen(&1));
    }

    #[test]
    fn test_on_recv_deduplicates() {
        let mut handler = CmHandler::new(TestState::new());
        let actor = DeviceId::deterministic_test_id();

        let op1 = create_test_op(1, 5, actor);
        let op2 = create_test_op(1, 10, actor); // Same ID, different value

        handler.on_recv(OpWithCtx::new(op1.clone(), op1.causal_ctx.clone()));
        handler.on_recv(OpWithCtx::new(op2.clone(), op2.causal_ctx.clone()));

        // Should only apply first operation
        assert_eq!(handler.get_state().sum, 5);
    }

    #[test]
    fn test_multiple_operations() {
        let mut handler = CmHandler::new(TestState::new());
        let actor = DeviceId::deterministic_test_id();

        let op1 = create_test_op(1, 5, actor);
        let op2 = create_test_op(2, 3, actor);
        let op3 = create_test_op(3, -2, actor);

        handler.on_recv(OpWithCtx::new(op1.clone(), op1.causal_ctx.clone()));
        handler.on_recv(OpWithCtx::new(op2.clone(), op2.causal_ctx.clone()));
        handler.on_recv(OpWithCtx::new(op3.clone(), op3.causal_ctx.clone()));

        assert_eq!(handler.get_state().sum, 6); // 5 + 3 - 2
        assert!(handler.get_state().seen(&1));
        assert!(handler.get_state().seen(&2));
        assert!(handler.get_state().seen(&3));
    }

    #[test]
    fn test_causal_ordering() {
        let mut handler = CmHandler::new(TestState::new());
        let actor = DeviceId::deterministic_test_id();

        // Create operations with causal dependencies
        let op1_ctx = CausalContext::new(actor);
        let op1 = TestOp {
            id: 1,
            value: 5,
            causal_ctx: op1_ctx.clone(),
        };

        // op2 depends on op1 - use explicit dependency
        let op1_id = OperationId::new(actor, 1);
        let op2_ctx = CausalContext::after(actor, &op1_ctx).with_dependency(op1_id);
        let op2 = TestOp {
            id: 2,
            value: 3,
            causal_ctx: op2_ctx.clone(),
        };

        // Send op2 first (out of order)
        handler.on_recv(OpWithCtx::new(op2.clone(), op2.causal_ctx.clone()));
        assert_eq!(handler.get_state().sum, 0); // Should be buffered
        assert_eq!(handler.buffer_len(), 1);

        // Send op1
        handler.on_recv(OpWithCtx::new(op1.clone(), op1.causal_ctx.clone()));
        assert_eq!(handler.get_state().sum, 8); // Should apply both: 5 + 3
        assert_eq!(handler.buffer_len(), 0); // Buffer should be empty
    }

    #[test]
    fn test_create_op_msg() {
        let handler = CmHandler::new(TestState::new());
        let actor = DeviceId::deterministic_test_id();
        let op = create_test_op(1, 5, actor);
        let msg = handler.create_op_msg(op.clone(), op.causal_ctx.clone());

        assert_eq!(msg.op.id, 1);
        assert_eq!(msg.op.value, 5);
    }
}
