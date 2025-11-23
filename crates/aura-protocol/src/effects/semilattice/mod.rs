//! Layer 4: CRDT Effect Interpreter - Semilattice Law Enforcement
//!
//! Composable effect handlers enforcing CRDT semantic laws (⊔, ⊓) independent of
//! session type communication. Handlers bridge session-type messages with CRDT operations,
//! ensuring mathematical convergence properties (per docs/002_theoretical_model.md, docs/110_state_reduction.md).
//!
//! **Handler Types** (per docs/002_theoretical_model.md §4):
//! - **CvHandler**: State-based CRDTs (join semilattice ⊔, idempotent merge)
//! - **CmHandler**: Operation-based CRDTs (causal ordering, effect commutativity)
//! - **DeltaHandler**: Delta-based CRDTs (incremental state transfer)
//! - **MvHandler**: Value-based CRDTs (meet semilattice ⊓ for policy refinement)
//! - **MultiConstraintHandler**: Manage multiple constraint scopes (per context)
//!
//! **Mathematical Invariants** (critical for convergence):
//! - **Associativity**: `(A ⊔ B) ⊔ C = A ⊔ (B ⊔ C)` → allows any merge order
//! - **Commutativity**: `A ⊔ B = B ⊔ A` → order-independent convergence
//! - **Idempotency**: `A ⊔ A = A` → duplicate messages safe
//!
//! **Integration**: Works with choreography layer (aura-mpst) to coordinate multi-party
//! synchronization protocols (anti-entropy, gossip, state reconciliation)
//!
//! ## Architecture
//!
//! The effect layer is organized into three main components:
//!
//! 1. **Handlers**: Enforce CRDT laws for different CRDT types
//!    - `CvHandler`: State-based CRDTs with join semilattice laws
//!    - `CmHandler`: Operation-based CRDTs with causal ordering
//!    - `DeltaHandler`: Delta-based CRDTs with incremental updates
//!
//! 2. **Delivery Effects**: Provide network guarantees required by CRDTs
//!    - Causal broadcast for CmRDTs
//!    - At-least-once delivery with deduplication
//!    - Gossip protocols for anti-entropy
//!
//! 3. **Handler Composition**: Utilities for combining handlers with delivery effects
//!
//! ## Usage Pattern with Builder API
//!
//! The recommended pattern uses the `CrdtCoordinator` builder for clean setup:
//!
//! ```rust,ignore
//! use crate::effects::semilattice::CrdtCoordinator;
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
//! use crate::effects::semilattice::CvHandler;
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

pub use cm_handler::CmHandler;
pub use crdt_coordinator::{CrdtCoordinator, CrdtCoordinatorError};
pub use cv_handler::CvHandler;
pub use delivery::{DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId};
pub use delta_handler::DeltaHandler;

// Re-export causal context types from aura-journal
pub use aura_journal::{CausalContext, VectorClock};
pub use mv_handler::{ConstraintEvent, ConstraintResult, MultiConstraintHandler, MvHandler};

pub mod cm_handler;
pub mod crdt_coordinator;
pub mod cv_handler;
pub mod delivery;
pub mod delta_handler;
pub mod mv_handler;

use aura_core::identifiers::{DeviceId, SessionId};
use aura_core::semilattice::{CausalOp, CmApply, CvState, Dedup, Delta};

/// DEPRECATED: Handler factory for creating CRDT effect handlers
///
/// This factory has been deprecated due to architectural violations.
/// HandlerFactory has been removed to comply with aura-composition-based registration.
///
/// Execution utilities for integrating handlers with choreographic protocols
pub mod execution {
    use super::*;
    // Types available for future use if needed
    // use aura_core::semilattice::{DeltaMsg, OpWithCtx, StateMsg};

    /// Execute state-based CRDT synchronization
    ///
    /// This is a placeholder for executing CvRDT protocols with proper
    /// session type integration. The actual implementation would bridge
    /// with the choreographic layer.
    pub async fn execute_cv_sync<S: CvState>(
        _handler: &mut CvHandler<S>,
        _peers: Vec<DeviceId>,
        _session_id: SessionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Integrate with choreographic execution
        // This would:
        // 1. Set up session-type communication
        // 2. Execute anti-entropy protocol
        // 3. Handle state message exchange
        // 4. Apply handler operations

        tracing::info!("Executing CvRDT synchronization (placeholder)");
        Ok(())
    }

    /// Execute delta-based CRDT gossip
    pub async fn execute_delta_gossip<S: CvState, D: Delta>(
        _handler: &mut DeltaHandler<S, D>,
        _peers: Vec<DeviceId>,
        _session_id: SessionId,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Integrate with choreographic execution
        tracing::info!("Executing delta CRDT gossip (placeholder)");
        Ok(())
    }

    /// Execute operation-based CRDT broadcast
    pub async fn execute_op_broadcast<S, Op, Id>(
        _handler: &mut CmHandler<S, Op, Id>,
        _peers: Vec<DeviceId>,
        _session_id: SessionId,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        S: CmApply<Op> + Dedup<Id>,
        Op: CausalOp<Id = Id, Ctx = aura_journal::CausalContext>,
        Id: Clone + PartialEq,
    {
        // TODO: Integrate with choreographic execution
        tracing::info!("Executing operation broadcast (placeholder)");
        Ok(())
    }
}

/// Composition utilities for combining multiple handlers
pub mod composition {
    use super::*;

    /// Composed handler for multiple CRDT types
    ///
    /// This allows a single session to manage multiple different CRDTs
    /// with their appropriate handlers and delivery guarantees.
    pub struct ComposedHandler {
        /// Registry of handlers by type name
        pub handlers: std::collections::HashMap<String, Box<dyn std::any::Any + Send + Sync>>,
        /// Delivery configuration per handler type
        pub delivery_configs: std::collections::HashMap<String, DeliveryConfig>,
    }

    impl ComposedHandler {
        /// Create a new composed handler
        pub fn new() -> Self {
            Self {
                handlers: std::collections::HashMap::new(),
                delivery_configs: std::collections::HashMap::new(),
            }
        }

        /// Register a CvRDT handler
        pub fn register_cv_handler<S: CvState + Send + Sync + 'static>(
            &mut self,
            type_name: String,
            handler: CvHandler<S>,
            config: DeliveryConfig,
        ) {
            self.handlers.insert(type_name.clone(), Box::new(handler));
            self.delivery_configs.insert(type_name, config);
        }

        /// Register a CmRDT handler
        pub fn register_cm_handler<S, Op, Id>(
            &mut self,
            type_name: String,
            handler: CmHandler<S, Op, Id>,
            config: DeliveryConfig,
        ) where
            S: CmApply<Op> + Dedup<Id> + Send + Sync + 'static,
            Op: CausalOp<Id = Id, Ctx = aura_journal::CausalContext> + Send + Sync + 'static,
            Id: Clone + PartialEq + Send + Sync + 'static,
        {
            self.handlers.insert(type_name.clone(), Box::new(handler));
            self.delivery_configs.insert(type_name, config);
        }

        /// Get a handler by type name
        pub fn get_cv_handler<S: CvState + 'static>(
            &self,
            type_name: &str,
        ) -> Option<&CvHandler<S>> {
            self.handlers.get(type_name)?.downcast_ref::<CvHandler<S>>()
        }

        /// Get a mutable handler by type name
        pub fn get_cv_handler_mut<S: CvState + 'static>(
            &mut self,
            type_name: &str,
        ) -> Option<&mut CvHandler<S>> {
            self.handlers
                .get_mut(type_name)?
                .downcast_mut::<CvHandler<S>>()
        }

        /// Get delivery configuration for a handler type
        pub fn get_delivery_config(&self, type_name: &str) -> Option<&DeliveryConfig> {
            self.delivery_configs.get(type_name)
        }

        /// List registered handler types
        pub fn list_handler_types(&self) -> Vec<&String> {
            self.handlers.keys().collect()
        }
    }

    impl Default for ComposedHandler {
        fn default() -> Self {
            Self::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_composition::{
        EffectRegistry, HandlerConfig, HandlerConfigBuilder, RegistrableHandler,
    };
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
                _effect_type: aura_core::EffectType,
                _operation: &str,
                _parameters: &[u8],
                _ctx: &aura_composition::HandlerContext,
            ) -> Result<Vec<u8>, aura_composition::HandlerError> {
                Err(aura_composition::HandlerError::UnsupportedEffect {
                    effect_type: aura_core::EffectType::Choreographic,
                })
            }
            fn supported_operations(&self, _effect_type: aura_core::EffectType) -> Vec<String> {
                vec![]
            }
            fn supports_effect(&self, _effect_type: aura_core::EffectType) -> bool {
                false
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
