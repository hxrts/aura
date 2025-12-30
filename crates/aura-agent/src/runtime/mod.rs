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
pub mod consensus;
pub mod services;

// Effect system implementation
pub mod effects;

pub mod system;

// Shared in-memory transport wiring for simulations/demos
pub mod shared_transport;

// Simulation factory (feature-gated)
#[cfg(feature = "simulation")]
pub mod simulation_factory;

// Cross-cutting concerns
pub mod migration;
pub mod reliability;

// Choreography integration
pub mod choreography_adapter;

// Runtime utilities
pub mod storage_coordinator;
pub mod time_handler;

// Re-export main types for convenience
pub use builder::EffectSystemBuilder;
pub use choreography_adapter::{AuraHandlerAdapter, ChoreographyAdapter};
pub use context::EffectContext;
pub use effects::AuraEffectSystem;
pub use shared_transport::SharedTransport;

// Runtime system type aliases for backwards compatibility
pub type RuntimeSystem = AuraEffectSystem;
pub type RuntimeBuilder = EffectSystemBuilder;
pub use registry::{EffectRegistry, EffectRegistryError, EffectRegistryExt};

pub use executor::EffectExecutor;
pub use lifecycle::LifecycleManager;
#[allow(unused_imports)] // Re-exported for public API
pub use services::{
    AuthorityError, AuthorityManager, AuthorityState, AuthorityStatus, FlowBudgetManager,
    ReceiptManager, SharedAuthorityManager, SyncManagerConfig, SyncManagerState,
    SyncServiceManager,
};

// Simulation factory re-export
#[cfg(feature = "simulation")]
pub use simulation_factory::EffectSystemFactory;
