//! CRDT effect interpreter layer
//!
//! This module provides composable effect handlers that enforce CRDT semantic laws
//! independently of session type communication. The handlers bridge between
//! session-type messages and CRDT operations, ensuring convergence properties.
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
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::effects::semilattice::{CvHandler, execute_cv_sync};
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
pub use cv_handler::CvHandler;
pub use delivery::{
    CausalContext, DeliveryConfig, DeliveryEffect, DeliveryGuarantee, GossipStrategy, TopicId,
};
pub use delta_handler::DeltaHandler;
pub use mv_handler::{ConstraintEvent, ConstraintResult, MultiConstraintHandler, MvHandler};

pub mod cm_handler;
pub mod cv_handler;
pub mod delivery;
pub mod delta_handler;
pub mod mv_handler;

use aura_core::identifiers::{DeviceId, SessionId};
use aura_core::semilattice::{CausalOp, CmApply, CvState, Dedup, Delta, MvState, Top};

/// Handler factory for creating CRDT effect handlers
pub struct HandlerFactory;

impl HandlerFactory {
    /// Create a state-based CRDT handler
    pub fn cv_handler<S: CvState>() -> CvHandler<S> {
        CvHandler::new()
    }

    /// Create a state-based CRDT handler with initial state
    pub fn cv_handler_with_state<S: CvState>(state: S) -> CvHandler<S> {
        CvHandler::with_state(state)
    }

    /// Create an operation-based CRDT handler
    pub fn cm_handler<S, Op, Id, Ctx>(state: S) -> CmHandler<S, Op, Id, Ctx>
    where
        S: CmApply<Op> + Dedup<Id>,
        Op: CausalOp<Id = Id, Ctx = Ctx>,
        Id: Clone,
    {
        CmHandler::new(state)
    }

    /// Create a delta-based CRDT handler
    pub fn delta_handler<S: CvState, D: Delta>() -> DeltaHandler<S, D> {
        DeltaHandler::new()
    }

    /// Create a delta-based CRDT handler with custom fold threshold
    pub fn delta_handler_with_threshold<S: CvState, D: Delta>(
        threshold: usize,
    ) -> DeltaHandler<S, D> {
        DeltaHandler::with_threshold(threshold)
    }

    /// Create a meet-based CRDT handler
    pub fn mv_handler<S: MvState + Top>() -> MvHandler<S> {
        MvHandler::new()
    }

    /// Create a meet-based CRDT handler with initial state
    pub fn mv_handler_with_state<S: MvState + Top>(state: S) -> MvHandler<S> {
        MvHandler::with_state(state)
    }

    /// Create a multi-constraint handler for managing multiple constraint scopes
    pub fn multi_constraint_handler<S: MvState + Top>() -> MultiConstraintHandler<S> {
        MultiConstraintHandler::new()
    }
}

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
    pub async fn execute_op_broadcast<S, Op, Id, Ctx>(
        _handler: &mut CmHandler<S, Op, Id, Ctx>,
        _peers: Vec<DeviceId>,
        _session_id: SessionId,
    ) -> Result<(), Box<dyn std::error::Error>>
    where
        S: CmApply<Op> + Dedup<Id>,
        Op: CausalOp<Id = Id, Ctx = Ctx>,
        Id: Clone,
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
        pub fn register_cm_handler<S, Op, Id, Ctx>(
            &mut self,
            type_name: String,
            handler: CmHandler<S, Op, Id, Ctx>,
            config: DeliveryConfig,
        ) where
            S: CmApply<Op> + Dedup<Id> + Send + Sync + 'static,
            Op: CausalOp<Id = Id, Ctx = Ctx> + Send + Sync + 'static,
            Id: Clone + Send + Sync + 'static,
            Ctx: Send + Sync + 'static,
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
    fn test_handler_factory_cv_handler() {
        let handler = HandlerFactory::cv_handler::<TestCounter>();
        assert_eq!(handler.get_state(), &TestCounter(0));
    }

    #[test]
    fn test_handler_factory_cv_handler_with_state() {
        let handler = HandlerFactory::cv_handler_with_state(TestCounter(42));
        assert_eq!(handler.get_state(), &TestCounter(42));
    }

    #[test]
    fn test_composed_handler_creation() {
        let composed = composition::ComposedHandler::new();
        assert!(composed.list_handler_types().is_empty());
    }

    #[test]
    fn test_composed_handler_registration() {
        let mut composed = composition::ComposedHandler::new();
        let handler = HandlerFactory::cv_handler::<TestCounter>();
        let config = DeliveryConfig::default();

        composed.register_cv_handler("test-counter".to_string(), handler, config);

        let types = composed.list_handler_types();
        assert_eq!(types.len(), 1);
        assert_eq!(types[0], "test-counter");
    }
}
