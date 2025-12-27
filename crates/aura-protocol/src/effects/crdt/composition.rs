//! Composition utilities for combining multiple handlers

use super::{CmHandler, CvHandler, DeliveryConfig};
use aura_core::semilattice::{CausalOp, CmApply, CvState, Dedup};
use aura_journal::CausalContext;
use std::collections::HashMap;

/// Composed handler for multiple CRDT types
///
/// This allows a single session to manage multiple different CRDTs
/// with their appropriate handlers and delivery guarantees.
pub struct ComposedHandler {
    /// Registry of handlers by type name
    pub handlers: HashMap<String, Box<dyn std::any::Any + Send + Sync>>,
    /// Delivery configuration per handler type
    pub delivery_configs: HashMap<String, DeliveryConfig>,
}

impl ComposedHandler {
    /// Create a new composed handler
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            delivery_configs: HashMap::new(),
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
        Op: CausalOp<Id = Id, Ctx = CausalContext> + Send + Sync + 'static,
        Id: Clone + PartialEq + Send + Sync + 'static,
    {
        self.handlers.insert(type_name.clone(), Box::new(handler));
        self.delivery_configs.insert(type_name, config);
    }

    /// Get a handler by type name
    pub fn get_cv_handler<S: CvState + 'static>(&self, type_name: &str) -> Option<&CvHandler<S>> {
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
