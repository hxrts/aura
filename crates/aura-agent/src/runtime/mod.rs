//! Runtime Coordination Module
//!
//! This module contains the runtime effect system coordinator and related
//! infrastructure for composing effect handlers into a unified runtime.
//!
//! This is Layer 6 (Runtime Composition) code - it orchestrates the effect
//! system and manages the lifecycle of effect handlers.

// Core runtime coordinator (formerly system.rs in aura-protocol)
// DISABLED: pub mod coordinator; // Complex import issues, using aura-protocol directly

// Runtime builder and container
pub mod builder;
pub mod container;

// Execution infrastructure
pub mod executor;
pub mod lifecycle;

// Context management
pub mod context;
pub mod contextual;
pub mod propagation;

// Runtime services
pub mod services;

// Initialization
pub mod initialization;

// Cross-cutting concerns
pub mod migration;
pub mod reliability;

// Choreography integration (moved from aura-protocol)
pub mod choreography_adapter;

// OTA orchestration (moved from aura-protocol)
pub mod ota_orchestration;

// Re-export main types for convenience
pub use builder::AuraEffectSystemBuilder as EffectSystemBuilder;
pub use choreography_adapter::AuraHandlerAdapter;
pub use container::HandlerContainer;
pub use context::{EffectContext, RequestMetadata};
// Import AuraEffectSystem from aura-protocol instead of local coordinator
pub use aura_protocol::effects::AuraEffectSystem;

// TODO: Define these locally or import from appropriate location
#[derive(Debug, Clone)]
pub struct EffectSystemConfig {
    pub device_id: crate::DeviceId,
}

impl EffectSystemConfig {
    /// Create config for production
    pub fn for_production(device_id: crate::DeviceId) -> crate::errors::Result<Self> {
        Ok(Self { device_id })
    }

    /// Create config for simulation
    pub fn for_simulation(device_id: crate::DeviceId, _seed: u64) -> Self {
        Self { device_id }
    }
}

#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub storage_path: String,
}
pub use executor::{EffectExecutor, EffectExecutorBuilder};
pub use lifecycle::{EffectSystemState, LifecycleAware, LifecycleManager};
pub use services::{ContextManager, FlowBudgetManager, ReceiptManager};

#[cfg(any(test, feature = "testing"))]
pub use services::SyncContextManager;
