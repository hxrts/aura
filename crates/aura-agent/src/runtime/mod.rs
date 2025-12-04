//! Layer 6: Runtime Effect System Composition - Assembly & Lifecycle
//!
//! Effect system assembly and lifecycle management for authority-based runtime.
//! Coordinates effect registry, builder infrastructure, context management, and
//! handler composition for production/testing/simulation modes (per docs/106_effect_system_and_runtime.md).
//!
//! **Key Components**:
//! - **EffectSystemBuilder**: Compose handlers via builder pattern
//! - **EffectRegistry**: Map (EffectType, Operation) → Handler implementation
//! - **EffectContext**: Per-execution context (authority, epoch, budgets)
//! - **LifecycleManager**: Service startup/shutdown coordination
//! - **AuthorityManager**: Multi-authority state management
//!
//! **Execution Modes** (per docs/106_effect_system_and_runtime.md):
//! - **Production**: Real handlers (crypto, storage, network)
//! - **Testing**: Mock handlers with deterministic behavior
//! - **Simulation**: Deterministic handlers with scenario injection, time control

// Runtime builder and container
pub mod builder;
pub mod container;

// Effect system registry
pub mod registry;

// Execution infrastructure
pub mod executor;
pub mod lifecycle;

// Context management
pub mod context;
pub mod contextual;
pub mod propagation;

// Runtime services
pub mod services;

// Effect system implementation
pub mod effects;

pub mod system;

// Cross-cutting concerns
pub mod migration;
pub mod reliability;

// Choreography integration
pub mod choreography_adapter;

// Re-export main types for convenience
use aura_core::effects::ExecutionMode;
pub use builder::EffectSystemBuilder;
pub use choreography_adapter::{AuraHandlerAdapter, ChoreographyAdapter};
pub use context::EffectContext;
pub use effects::AuraEffectSystem;

// Runtime system type aliases for backwards compatibility
pub type RuntimeSystem = AuraEffectSystem;
pub type RuntimeBuilder = EffectSystemBuilder;
pub use registry::{EffectRegistry, EffectRegistryError, EffectRegistryExt};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EffectSystemConfig {
    pub device_id: aura_core::identifiers::DeviceId,
    pub execution_mode: ExecutionMode,
    pub storage_config: Option<StorageConfig>,
    pub initial_epoch: u64,
    pub default_flow_limit: u64,
}

#[allow(dead_code)]
impl EffectSystemConfig {
    /// Create config for production
    pub fn for_production(
        device_id: aura_core::identifiers::DeviceId,
    ) -> Result<Self, aura_core::AuraError> {
        Ok(Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            storage_config: None,
            initial_epoch: 1,
            default_flow_limit: 10000,
        })
    }

    /// Create config for simulation
    pub fn for_simulation(device_id: aura_core::identifiers::DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            storage_config: None,
            initial_epoch: 1,
            default_flow_limit: 10000,
        }
    }

    /// Create config for testing
    pub fn for_testing(device_id: aura_core::identifiers::DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            storage_config: None,
            initial_epoch: 1,
            default_flow_limit: 10000,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct StorageConfig {
    pub storage_path: String,
}

#[allow(dead_code)]
impl StorageConfig {
    /// Create config for testing
    pub fn for_testing() -> Self {
        Self {
            storage_path: "/tmp/aura-test".to_string(),
        }
    }

    /// Create config for simulation
    pub fn for_simulation() -> Self {
        Self {
            storage_path: "/tmp/aura-sim".to_string(),
        }
    }
}
pub use executor::EffectExecutor;
pub use lifecycle::LifecycleManager;
#[allow(unused_imports)] // Re-exported for public API
pub use services::{
    AuthorityError, AuthorityManager, AuthorityState, AuthorityStatus, FlowBudgetManager,
    ReceiptManager, SharedAuthorityManager, SyncManagerConfig, SyncManagerState,
    SyncServiceManager,
};
