//! State-based CRDT effect handler
//!
//! This module provides the `CvHandler` for enforcing join semilattice laws
//! in state-based CRDTs. The handler automatically merges received states
//! using the join operation, guaranteeing convergence.
//!
//! # Overview
//!
//! State-based CRDTs (CvRDTs) achieve eventual consistency through the join
//! semilattice property. The `CvHandler` enforces this mathematical invariant
//! automatically, bridging between session-type choreographic protocols and
//! CRDT semantics.
//!
//! # Mathematical Foundation
//!
//! For any state-based CRDT `S`, the join operation `⊔` must satisfy:
//!
//! - **Commutativity**: `a ⊔ b = b ⊔ a`
//! - **Associativity**: `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)`
//! - **Idempotency**: `a ⊔ a = a`
//!
//! The handler ensures that all state updates preserve these laws, providing
//! the mathematical guarantee that replicas will eventually converge to the
//! same state regardless of message delivery order.
//!
//! # Usage Patterns
//!
//! ## Basic Usage
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{StateMsg, CvState, JoinSemilattice, Bottom};
//!
//! // Define your CRDT type
//! #[derive(Debug, Clone, PartialEq)]
//! struct Counter(u64);
//!
//! impl JoinSemilattice for Counter {
//!     fn join(&self, other: &Self) -> Self {
//!         Counter(self.0.max(other.0))
//!     }
//! }
//!
//! impl Bottom for Counter {
//!     fn bottom() -> Self { Counter(0) }
//! }
//!
//! impl CvState for Counter {}
//!
//! // Use the handler
//! let mut handler = CvHandler::<Counter>::new();
//! assert_eq!(handler.get_state(), &Counter(0));
//!
//! // Local update
//! handler.update_state(Counter(5));
//! assert_eq!(handler.get_state(), &Counter(5));
//!
//! // Receive remote state via choreographic protocol
//! let remote_msg = StateMsg::new(Counter(3));
//! handler.on_recv(remote_msg);
//! // Result: max(5, 3) = 5 (monotonic)
//! assert_eq!(handler.get_state(), &Counter(5));
//!
//! // Send current state to peers
//! let outgoing = handler.create_state_msg();
//! // send_to_peers(outgoing).await;
//! ```
//!
//! ## Integration with Choreographic Protocols
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::{CvHandler, execution};
//! use aura_core::{DeviceId, SessionId};
//!
//! async fn sync_with_peers<S: CvState>(
//!     handler: &mut CvHandler<S>,
//!     peers: Vec<DeviceId>,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let session_id = SessionId::new();
//!
//!     // Execute anti-entropy protocol
//!     execution::execute_cv_sync(handler, peers, session_id).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Complex CRDT Types
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
//! use std::collections::HashMap;
//!
//! // OR-Set (Observed-Remove Set) CRDT
//! #[derive(Debug, Clone, PartialEq)]
//! struct ORSet<T> {
//!     added: HashMap<T, u64>,   // element -> unique add tag
//!     removed: HashMap<T, u64>, // element -> unique remove tag
//! }
//!
//! impl<T: Clone + std::hash::Hash + Eq> JoinSemilattice for ORSet<T> {
//!     fn join(&self, other: &Self) -> Self {
//!         let mut added = self.added.clone();
//!         let mut removed = self.removed.clone();
//!
//!         // Union of add/remove operations
//!         for (elem, tag) in &other.added {
//!             added.entry(elem.clone())
//!                  .and_modify(|t| *t = (*t).max(*tag))
//!                  .or_insert(*tag);
//!         }
//!
//!         for (elem, tag) in &other.removed {
//!             removed.entry(elem.clone())
//!                    .and_modify(|t| *t = (*t).max(*tag))
//!                    .or_insert(*tag);
//!         }
//!
//!         ORSet { added, removed }
//!     }
//! }
//!
//! impl<T> Bottom for ORSet<T> {
//!     fn bottom() -> Self {
//!         ORSet { added: HashMap::new(), removed: HashMap::new() }
//!     }
//! }
//!
//! impl<T: Clone + std::hash::Hash + Eq> CvState for ORSet<T> {}
//!
//! // Usage with complex CRDT
//! let mut handler = CvHandler::<ORSet<String>>::new();
//!
//! // Local operations create new state
//! let mut local_state = ORSet::bottom();
//! local_state.added.insert("alice".to_string(), 1);
//! handler.update_state(local_state);
//!
//! // Remote state is automatically merged
//! let mut remote_state = ORSet::bottom();
//! remote_state.added.insert("bob".to_string(), 2);
//! remote_state.removed.insert("alice".to_string(), 3);
//!
//! handler.on_recv(StateMsg::new(remote_state));
//! // Result contains both operations with proper conflict resolution
//! ```
//!
//! ## Middleware Integration
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{StateMsg, CvState};
//! use std::time::Instant;
//!
//! // Middleware wrapper for CvHandler
//! pub struct InstrumentedCvHandler<S: CvState> {
//!     inner: CvHandler<S>,
//!     metrics: HandlerMetrics,
//! }
//!
//! #[derive(Debug, Default)]
//! pub struct HandlerMetrics {
//!     pub messages_received: u64,
//!     pub join_operations: u64,
//!     pub last_update: Option<Instant>,
//! }
//!
//! impl<S: CvState> InstrumentedCvHandler<S> {
//!     pub fn new(handler: CvHandler<S>) -> Self {
//!         Self {
//!             inner: handler,
//!             metrics: HandlerMetrics::default(),
//!         }
//!     }
//!
//!     pub fn on_recv(&mut self, msg: StateMsg<S>) {
//!         self.metrics.messages_received += 1;
//!         self.metrics.join_operations += 1;
//!         self.metrics.last_update = Some(monotonic_now());
//!
//!         self.inner.on_recv(msg);
//!     }
//!
//!     pub fn get_state(&self) -> &S {
//!         self.inner.get_state()
//!     }
//!
//!     pub fn get_metrics(&self) -> &HandlerMetrics {
//!         &self.metrics
//!     }
//! }
//! ```
//!
//! ## Error Handling and Validation
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{StateMsg, CvState};
//!
//! // Validated CRDT handler
//! pub struct ValidatedCvHandler<S: CvState> {
//!     handler: CvHandler<S>,
//!     validation_enabled: bool,
//! }
//!
//! impl<S: CvState + PartialOrd + Clone> ValidatedCvHandler<S> {
//!     pub fn new_with_validation(handler: CvHandler<S>) -> Self {
//!         Self {
//!             handler,
//!             validation_enabled: true,
//!         }
//!     }
//!
//!     pub fn on_recv_validated(&mut self, msg: StateMsg<S>) -> Result<(), String> {
//!         if self.validation_enabled {
//!             // Verify message doesn't violate monotonicity
//!             let old_state = self.handler.get_state().clone();
//!             let new_state = old_state.join(&msg.payload);
//!
//!             // In a proper CvRDT, new_state should be >= old_state
//!             if new_state < old_state {
//!                 return Err("Join operation violated monotonicity".to_string());
//!             }
//!         }
//!
//!         self.handler.on_recv(msg);
//!         Ok(())
//!     }
//! }
//! ```
//!
//! # Performance Considerations
//!
//! ## State Size Management
//!
//! CvRDTs can grow unboundedly over time. Consider these patterns:
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::CvState;
//!
//! // Implement compaction for large CRDTs
//! trait Compactable: CvState {
//!     fn compact(&self) -> Self;
//!     fn should_compact(&self) -> bool;
//! }
//!
//! impl<S: Compactable> CvHandler<S> {
//!     pub fn compact_if_needed(&mut self) {
//!         if self.state.should_compact() {
//!             self.state = self.state.compact();
//!         }
//!     }
//! }
//! ```
//!
//! ## Batch Processing
//!
//! For high-throughput scenarios:
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{StateMsg, CvState};
//!
//! impl<S: CvState> CvHandler<S> {
//!     /// Process multiple state messages in a batch
//!     pub fn on_recv_batch(&mut self, messages: Vec<StateMsg<S>>) {
//!         for msg in messages {
//!             self.state = self.state.join(&msg.payload);
//!         }
//!     }
//! }
//! ```
//!
//! # Testing
//!
//! ```rust,no_run
//! use aura_protocol::effects::semilattice::CvHandler;
//! use aura_core::semilattice::{StateMsg, CvState, JoinSemilattice, Bottom};
//!
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!
//!     #[test]
//!     fn test_crdt_convergence_property() {
//!         let mut handler1 = CvHandler::<Counter>::new();
//!         let mut handler2 = CvHandler::<Counter>::new();
//!
//!         // Different sequences of operations
//!         handler1.update_state(Counter(5));
//!         handler1.update_state(Counter(3));
//!
//!         handler2.update_state(Counter(3));
//!         handler2.update_state(Counter(5));
//!
//!         // Should converge to same state
//!         assert_eq!(handler1.get_state(), handler2.get_state());
//!     }
//!
//!     #[test]
//!     fn test_idempotency_property() {
//!         let mut handler = CvHandler::with_state(Counter(42));
//!         let original_state = handler.get_state().clone();
//!
//!         // Applying same state multiple times should be idempotent
//!         handler.on_recv(StateMsg::new(Counter(42)));
//!         handler.on_recv(StateMsg::new(Counter(42)));
//!
//!         assert_eq!(handler.get_state(), &original_state);
//!     }
//! }
//! ```

use aura_core::semilattice::{CvState, MsgKind, StateMsg};

/// State-based CRDT effect handler
///
/// Enforces the join semilattice law: `state := state.join(&received_state)`
/// for convergent replicated data types (CvRDTs).
///
/// # Type Parameters
///
/// - `S`: The CRDT state type that implements [`CvState`]. Must satisfy join semilattice laws.
///
/// # Mathematical Properties
///
/// The handler maintains these invariants for all operations:
///
/// 1. **Monotonicity**: `old_state ≤ new_state` after any operation
/// 2. **Convergence**: All replicas with same operations converge to same state
/// 3. **Commutativity**: Order of `on_recv` calls doesn't affect final state
/// 4. **Idempotency**: Repeated application of same state has no effect
///
/// # Thread Safety
///
/// This handler is `Send` and `Sync` when the state type `S` is `Send` and `Sync`.
/// For concurrent access, wrap in `Arc<Mutex<CvHandler<S>>>`.
///
/// # Memory Usage
///
/// The handler stores the full CRDT state in memory. For large CRDTs, consider:
/// - Implementing compaction in your CRDT type
/// - Using delta-based synchronization with [`DeltaHandler`]
/// - Periodic state snapshots with reset
///
/// # Performance
///
/// - **Time Complexity**: O(join(S)) per message, depends on CRDT implementation
/// - **Space Complexity**: O(|S|), size of current CRDT state
/// - **Network Overhead**: Full state sent per message (use deltas for efficiency)
///
/// # Examples
///
/// See module-level documentation for comprehensive usage examples.
pub struct CvHandler<S: CvState> {
    /// Current CRDT state
    ///
    /// This field maintains the current state of the CRDT. It evolves
    /// monotonically through join operations, ensuring convergence.
    pub state: S,
}

impl<S: CvState> CvHandler<S> {
    /// Create a new handler with the bottom element
    ///
    /// Initializes the handler with the identity element of the join semilattice.
    /// The bottom element has the property that `bottom.join(x) = x` for any `x`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use aura_protocol::effects::semilattice::CvHandler;
    /// use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    ///
    /// #[derive(Debug, Clone, PartialEq)]
    /// struct Counter(u64);
    /// impl JoinSemilattice for Counter {
    ///     fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) }
    /// }
    /// impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// impl CvState for Counter {}
    ///
    /// let handler = CvHandler::<Counter>::new();
    /// assert_eq!(handler.get_state(), &Counter(0)); // bottom element
    /// ```
    pub fn new() -> Self {
        Self { state: S::bottom() }
    }

    /// Create a handler with initial state
    ///
    /// Useful when you want to start with a specific state rather than bottom.
    /// The provided state becomes the initial state of the CRDT.
    ///
    /// # Parameters
    ///
    /// - `state`: Initial state for the CRDT handler
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let handler = CvHandler::with_state(Counter(42));
    /// assert_eq!(handler.get_state(), &Counter(42));
    /// ```
    pub fn with_state(state: S) -> Self {
        Self { state }
    }

    /// Handle received state message - enforces join semilattice law
    ///
    /// This method implements the core CvRDT convergence guarantee by applying
    /// the join operation: `self.state := self.state.join(received.state)`.
    ///
    /// The state monotonically advances toward the least upper bound of all
    /// received states, ensuring eventual convergence across all replicas.
    ///
    /// # Mathematical Properties
    ///
    /// This operation preserves all semilattice laws:
    /// - **Monotonicity**: `old_state ≤ new_state` after join
    /// - **Commutativity**: Order of received messages doesn't matter
    /// - **Associativity**: Multiple joins can be grouped arbitrarily
    /// - **Idempotency**: Receiving the same message multiple times has no effect
    ///
    /// # Parameters
    ///
    /// - `msg`: State message containing the remote CRDT state to merge
    ///
    /// # Performance
    ///
    /// Time complexity is O(join(S)), which depends on the CRDT implementation.
    /// For complex CRDTs, consider implementing efficient join algorithms.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{StateMsg, CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let mut handler = CvHandler::with_state(Counter(5));
    ///
    /// // Receive lower value - state remains the same (monotonic)
    /// handler.on_recv(StateMsg::new(Counter(3)));
    /// assert_eq!(handler.get_state(), &Counter(5));
    ///
    /// // Receive higher value - state advances
    /// handler.on_recv(StateMsg::new(Counter(10)));
    /// assert_eq!(handler.get_state(), &Counter(10));
    /// ```
    pub fn on_recv(&mut self, msg: StateMsg<S>) {
        self.state = self.state.join(&msg.payload);
    }

    /// Get current state (immutable reference)
    ///
    /// Returns a read-only reference to the current CRDT state. Use this
    /// for querying state without modifying it.
    ///
    /// # Returns
    ///
    /// Immutable reference to the current CRDT state
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let handler = CvHandler::with_state(Counter(42));
    /// let current_state = handler.get_state();
    /// assert_eq!(current_state.0, 42);
    /// ```
    pub fn get_state(&self) -> &S {
        &self.state
    }

    /// Get current state (mutable reference)
    ///
    /// Returns a mutable reference to the current CRDT state. Use with caution
    /// as direct mutations may violate CRDT invariants. Prefer `update_state()`
    /// for safe local modifications.
    ///
    /// # Warning
    ///
    /// Direct mutation of the state should preserve semilattice properties.
    /// Incorrect modifications may break convergence guarantees.
    ///
    /// # Returns
    ///
    /// Mutable reference to the current CRDT state
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let mut handler = CvHandler::with_state(Counter(42));
    ///
    /// // Direct mutation (use carefully!)
    /// let state_mut = handler.get_state_mut();
    /// *state_mut = Counter(state_mut.0.max(50)); // Preserve monotonicity
    ///
    /// assert_eq!(handler.get_state(), &Counter(50));
    /// ```
    pub fn get_state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    /// Create state message for sending
    ///
    /// Wraps the current state in a properly tagged [`StateMsg`] for transmission
    /// via choreographic protocols. The message includes metadata for session-type
    /// communication and CRDT synchronization.
    ///
    /// # Returns
    ///
    /// A [`StateMsg`] containing the current state and appropriate metadata
    ///
    /// # Message Format
    ///
    /// The returned message has:
    /// - `payload`: Clone of current CRDT state
    /// - `kind`: [`MsgKind::FullState`] indicating complete state transfer
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{StateMsg, MsgKind, CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let handler = CvHandler::with_state(Counter(42));
    /// let msg = handler.create_state_msg();
    ///
    /// assert_eq!(msg.payload, Counter(42));
    /// assert_eq!(msg.kind, MsgKind::FullState);
    ///
    /// // Message ready for transmission to peers
    /// // send_to_choreography(msg).await;
    /// ```
    pub fn create_state_msg(&self) -> StateMsg<S> {
        StateMsg {
            payload: self.state.clone(),
            kind: MsgKind::FullState,
        }
    }

    /// Update state directly (for local operations)
    ///
    /// Applies a local state change by joining with the provided state.
    /// Use this for local operations that don't originate from network
    /// messages (e.g., user interactions, local computations).
    ///
    /// The join operation ensures that the update respects semilattice laws
    /// and maintains convergence properties.
    ///
    /// # Parameters
    ///
    /// - `new_state`: State to join with current state
    ///
    /// # Equivalence
    ///
    /// ```text
    /// handler.update_state(s) ≡ handler.on_recv(StateMsg::new(s))
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// // Obtain a handler from a composition/registry (in tests, construct directly)
    /// let mut handler = CvHandler::with_state(Counter::bottom());
    ///
    /// // Local user operation: increment counter
    /// handler.update_state(Counter(1));
    /// assert_eq!(handler.get_state(), &Counter(1));
    ///
    /// // Another local operation
    /// handler.update_state(Counter(3));
    /// assert_eq!(handler.get_state(), &Counter(3)); // max(1, 3) = 3
    /// ```
    pub fn update_state(&mut self, new_state: S) {
        self.state = self.state.join(&new_state);
    }

    /// Reset to bottom element
    ///
    /// Resets the handler state to the bottom element of the semilattice.
    /// This operation effectively "clears" the CRDT state.
    ///
    /// # Use Cases
    ///
    /// - **Testing**: Reset between test cases
    /// - **Cleanup**: Clear accumulated state
    /// - **Restart**: Begin fresh synchronization
    ///
    /// # Warning
    ///
    /// In a distributed setting, resetting one replica while others maintain
    /// their state may lead to the reset replica quickly rejoining the current
    /// state through anti-entropy protocols.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use aura_protocol::effects::semilattice::CvHandler;
    /// # use aura_core::semilattice::{CvState, JoinSemilattice, Bottom};
    /// # #[derive(Debug, Clone, PartialEq)]
    /// # struct Counter(u64);
    /// # impl JoinSemilattice for Counter { fn join(&self, other: &Self) -> Self { Counter(self.0.max(other.0)) } }
    /// # impl Bottom for Counter { fn bottom() -> Self { Counter(0) } }
    /// # impl CvState for Counter {}
    ///
    /// let mut handler = CvHandler::with_state(Counter(42));
    /// assert_eq!(handler.get_state(), &Counter(42));
    ///
    /// handler.reset();
    /// assert_eq!(handler.get_state(), &Counter(0)); // Back to bottom
    /// ```
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
    use aura_core::semilattice::{Bottom, JoinSemilattice};

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
