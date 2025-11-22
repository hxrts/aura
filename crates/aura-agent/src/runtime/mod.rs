//! Agent Runtime - Effect Registry, Builder, Lifecycle, and Coordination
//!
//! This module contains the runtime composition logic for assembling effects
//! into a working agent system. It handles the Layer-6 runtime responsibilities.

pub mod builder;
pub mod choreography;
pub mod effects;
pub mod lifecycle;
pub mod reliability;
pub mod services;
pub mod system;
pub mod utilities;

pub use builder::{
    EffectRegistry, EffectRegistryError, EffectRegistryExt, EffectSystemBuilder, ExecutionMode,
    RuntimeBuilder,
};
pub use choreography::ChoreographyAdapter;
pub use effects::{AuraEffectSystem, EffectExecutor};
pub use lifecycle::LifecycleManager;
pub use reliability::ReliabilityManager;
pub use services::{ContextManager, FlowBudgetManager, ReceiptManager};
pub use system::RuntimeSystem;
pub use utilities::{EffectApiHelpers, EffectContext, PersistenceUtils, StorageKeyManager};
