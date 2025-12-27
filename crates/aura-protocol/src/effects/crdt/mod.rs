//! Layer 4: CRDT Effect Interpreter - Semilattice Law Enforcement
//!
//! Composable effect handlers enforcing CRDT semantic laws (⊔, ⊓) independent of
//! session type communication. Handlers bridge session-type messages with CRDT operations,
//! ensuring mathematical convergence properties (per docs/002_theoretical_model.md, docs/110_state_reduction.md).
//!
//! # Handler Selection Guide
//!
//! Choosing the right handler is **critical for correctness**. Each handler enforces
//! different mathematical properties:
//!
//! ```text
//! Is your data structure...
//! │
//! ├─► Accumulating state over time (counters, sets, logs)?
//! │   └─► Use CvHandler (join semilattice ⊔)
//! │       Examples: G-Counter, G-Set, LWW-Register, OR-Set
//! │
//! ├─► Restricting/constraining permissions or policies?
//! │   └─► Use MvHandler (meet semilattice ⊓)
//! │       Examples: Capability sets, access policies, budget limits
//! │
//! ├─► Operation-based with causal ordering requirements?
//! │   └─► Use CmHandler (commutative operations with causal delivery)
//! │       Examples: Collaborative editing, operation logs, chat messages
//! │
//! └─► State-based but bandwidth-constrained?
//!     └─► Use DeltaHandler (incremental sync with fold threshold)
//!         Examples: Large journals, distributed state with many small updates
//! ```
//!
//! ## Quick Reference
//!
//! | Handler | Lattice | Direction | Use When |
//! |---------|---------|-----------|----------|
//! | `CvHandler` | Join (⊔) | Monotonically increasing | Accumulating data |
//! | `MvHandler` | Meet (⊓) | Monotonically decreasing | Restricting permissions |
//! | `CmHandler` | Operations | Causal ordering | Need operation history |
//! | `DeltaHandler` | Join + Delta | Incremental | Large state, low bandwidth |
//!
//! ## Common Mistakes
//!
//! - **Using CvHandler for permissions**: Join makes sets grow, but permissions
//!   should shrink when restricted. Use MvHandler instead.
//!
//! - **Using MvHandler for counters**: Meet finds minimum, but counters should
//!   find maximum. Use CvHandler instead.
//!
//! - **Using CmHandler when order doesn't matter**: If operations naturally
//!   commute and you don't need causal ordering, CvHandler is simpler.
//!
//! # The `CrdtHandler` Trait
//!
//! All handlers implement the [`CrdtHandler`] trait, providing a unified interface
//! for runtime introspection and generic handler manipulation:
//!
//! ```rust,ignore
//! use aura_protocol::effects::crdt::{
//!     CrdtHandler, CrdtSemantics, CvHandler, MvHandler, DeltaHandler,
//! };
//!
//! // The trait provides runtime type information
//! fn log_handler_info<S>(handler: &impl CrdtHandler<S>) {
//!     let diag = handler.diagnostics();
//!     println!("Semantics: {:?}", diag.semantics);
//!     println!("Pending work: {}", diag.pending_count);
//!     println!("Is idle: {}", diag.is_idle);
//! }
//!
//! // Match on semantics for type-specific behavior
//! fn describe_handler<S>(handler: &impl CrdtHandler<S>) -> &'static str {
//!     match handler.semantics() {
//!         CrdtSemantics::JoinSemilattice => "State-based, monotonically increasing",
//!         CrdtSemantics::MeetSemilattice => "Constraint-based, monotonically decreasing",
//!         CrdtSemantics::OperationBased => "Causal operations with vector clocks",
//!         CrdtSemantics::DeltaBased => "Incremental updates with fold threshold",
//!     }
//! }
//!
//! // Check if handler needs attention
//! fn needs_sync<S>(handler: &impl CrdtHandler<S>) -> bool {
//!     handler.has_pending_work()
//! }
//! ```
//!
//! ## Trait Methods
//!
//! | Method | Returns | Purpose |
//! |--------|---------|---------|
//! | `semantics()` | `CrdtSemantics` | Identify handler type at runtime |
//! | `state()` | `&S` | Read current CRDT state |
//! | `state_mut()` | `&mut S` | Mutate CRDT state directly |
//! | `has_pending_work()` | `bool` | Check for buffered ops/deltas |
//! | `diagnostics()` | `HandlerDiagnostics` | Full runtime metrics |
//!
//! ## Handler-Specific Metrics
//!
//! The [`HandlerDiagnostics`] struct includes [`HandlerMetrics`] with optional
//! fields populated based on handler type:
//!
//! - **CmHandler**: `applied_operations` - count of causally delivered operations
//! - **DeltaHandler**: `fold_threshold` - delta buffer limit before folding
//! - **MvHandler**: `constraints_applied`, `consistency_proofs` - audit trail info
//!
//! # Handler Types
//!
//! - **CvHandler**: State-based CRDTs (join semilattice ⊔, idempotent merge)
//! - **CmHandler**: Operation-based CRDTs (causal ordering, effect commutativity)
//! - **DeltaHandler**: Delta-based CRDTs (incremental state transfer)
//! - **MvHandler**: Value-based CRDTs (meet semilattice ⊓ for policy refinement)
//! - **MultiConstraintHandler**: Manage multiple constraint scopes (per context)
//!
//! # Mathematical Invariants
//!
//! Critical for convergence:
//! - **Associativity**: `(A ⊔ B) ⊔ C = A ⊔ (B ⊔ C)` → allows any merge order
//! - **Commutativity**: `A ⊔ B = B ⊔ A` → order-independent convergence
//! - **Idempotency**: `A ⊔ A = A` → duplicate messages safe
//!
//! **Integration**: Works with choreography layer (aura-mpst) to coordinate multi-party
//! synchronization protocols (anti-entropy, gossip, state reconciliation)
//!
//! # Architecture
//!
//! The effect layer is organized into these components:
//!
//! 1. **Handlers**: Enforce CRDT laws for different CRDT types
//!    - `CvHandler`: State-based CRDTs with join semilattice laws
//!    - `CmHandler`: Operation-based CRDTs with causal ordering
//!    - `DeltaHandler`: Delta-based CRDTs with incremental updates
//!    - `MvHandler`: Meet-based CRDTs for constraints
//!
//! 2. **Delivery Effects**: Provide network guarantees required by CRDTs
//!    - Causal broadcast for CmRDTs
//!    - At-least-once delivery with deduplication
//!    - Gossip protocols for anti-entropy
//!
//! 3. **Execution**: Utilities for integrating handlers with choreographic protocols
//!
//! 4. **Composition**: Utilities for combining multiple handlers
//!
//! 5. **Coordinator**: Bridge between CRDT handlers and choreographic protocols
//!
//! # Usage Patterns
//!
//! ## High-Level: CrdtCoordinator Builder
//!
//! The recommended pattern uses the `CrdtCoordinator` builder for clean setup:
//!
//! ```rust,ignore
//! use aura_protocol::effects::crdt::CrdtCoordinator;
//!
//! // Create coordinator with convergent CRDT handler (builder pattern)
//! let coordinator = CrdtCoordinator::with_cv_state(device_id, journal_map);
//!
//! // Use in choreographic protocols
//! let result = execute_anti_entropy(
//!     device_id,
//!     config,
//!     is_requester,
//!     &effect_system,
//!     coordinator,
//! ).await?;
//! ```
//!
//! ## Direct Handler Usage
//!
//! For cases where you need direct handler control:
//!
//! ```rust,ignore
//! use aura_protocol::effects::crdt::CvHandler;
//! use aura_core::semilattice::StateMsg;
//!
//! // Create handler for state-based CRDT
//! let mut handler = CvHandler::<JournalMap>::new();
//!
//! // Handle incoming session-type message
//! let msg: StateMsg<JournalMap> = receive_from_choreography().await;
//! handler.on_recv(msg); // Enforces join semilattice law automatically
//!
//! // Create message for sending
//! let outgoing = handler.create_state_msg();
//! send_via_choreography(outgoing).await;
//! ```
//!
//! ## Generic Handler Functions
//!
//! Write functions that work with any handler type:
//!
//! ```rust,ignore
//! use aura_protocol::effects::crdt::{CrdtHandler, CrdtSemantics};
//!
//! /// Log sync status for any CRDT handler
//! fn log_sync_status<S: std::fmt::Debug>(handler: &impl CrdtHandler<S>) {
//!     let diag = handler.diagnostics();
//!     tracing::info!(
//!         semantics = ?diag.semantics,
//!         pending = diag.pending_count,
//!         idle = diag.is_idle,
//!         "CRDT handler status"
//!     );
//! }
//!
//! /// Trigger sync if handler has pending work
//! async fn maybe_sync<S>(handler: &impl CrdtHandler<S>) -> bool {
//!     if handler.has_pending_work() {
//!         // Trigger appropriate sync based on semantics
//!         match handler.semantics() {
//!             CrdtSemantics::DeltaBased => { /* delta gossip */ }
//!             CrdtSemantics::OperationBased => { /* causal broadcast */ }
//!             _ => { /* state broadcast */ }
//!         }
//!         true
//!     } else {
//!         false
//!     }
//! }
//! ```

// Handler modules
pub mod cm_handler;
pub mod composition;
pub mod crdt_coordinator;
pub mod cv_handler;
pub mod delivery;
pub mod delta_handler;
pub mod execution;
pub mod handler_trait;
pub mod mv_handler;

// Handler re-exports
pub use cm_handler::CmHandler;
pub use crdt_coordinator::{CrdtCoordinator, CrdtCoordinatorError};
pub use cv_handler::CvHandler;
pub use delivery::{DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId};
pub use delta_handler::DeltaHandler;
pub use handler_trait::{CrdtHandler, CrdtSemantics, HandlerDiagnostics, HandlerMetrics};
pub use mv_handler::{ConstraintEvent, ConstraintResult, MultiConstraintHandler, MvHandler};

// Re-export causal context types from aura-journal
pub use aura_journal::{CausalContext, VectorClock};

// Re-export vector clock utilities
pub use crdt_coordinator::{increment_actor, max_counter, merge_vector_clocks};

// Re-export composition utilities
pub use composition::ComposedHandler;

#[cfg(test)]
mod tests {
    use super::*;
    use aura_composition::{EffectRegistry, RegistrableHandler};
    use aura_core::semilattice::{Bottom, CvState, JoinSemilattice};

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
    fn test_composed_handler_registration() {
        let mut registry = EffectRegistry::new(aura_core::effects::ExecutionMode::Testing);

        // Simulate registration via composition layer by boxing the handler as a RegistrableHandler
        let handler = CvHandler::with_state(TestCounter::bottom());
        // Wrap in a shim that implements RegistrableHandler for tests
        struct CvShim<S: CvState>(CvHandler<S>);
        #[async_trait::async_trait]
        impl<S: CvState + Send + Sync + 'static> RegistrableHandler for CvShim<S> {
            async fn execute_operation_bytes(
                &self,
                effect_type: aura_core::EffectType,
                _operation: &str,
                _parameters: &[u8],
                _ctx: &aura_composition::HandlerContext,
            ) -> Result<Vec<u8>, aura_composition::HandlerError> {
                if effect_type == aura_core::EffectType::Choreographic {
                    // Return empty response for test handler
                    Ok(Vec::new())
                } else {
                    Err(aura_composition::HandlerError::UnsupportedEffect { effect_type })
                }
            }
            fn supported_operations(&self, _effect_type: aura_core::EffectType) -> Vec<String> {
                vec![]
            }
            fn supports_effect(&self, effect_type: aura_core::EffectType) -> bool {
                matches!(effect_type, aura_core::EffectType::Choreographic)
            }
            fn execution_mode(&self) -> aura_core::ExecutionMode {
                aura_core::ExecutionMode::Testing
            }
        }

        registry
            .register_handler(
                aura_core::EffectType::Choreographic,
                Box::new(CvShim(handler)),
            )
            .expect("registry registration");

        // Ensure registry holds exactly one handler
        assert_eq!(registry.handlers_len(), 1);
    }
}
