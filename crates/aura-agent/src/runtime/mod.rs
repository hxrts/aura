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
    EffectSystemBuilder, EffectRegistry, EffectRegistryError, EffectRegistryExt, 
    RuntimeBuilder, ExecutionMode
};
pub use choreography::ChoreographyAdapter;
pub use effects::{EffectExecutor, AuraEffectSystem};
pub use lifecycle::LifecycleManager;
pub use reliability::ReliabilityManager;
pub use services::{ContextManager, FlowBudgetManager, ReceiptManager};
pub use system::RuntimeSystem;
pub use utilities::{PersistenceUtils, StorageKeyManager, EffectApiHelpers, EffectContext};