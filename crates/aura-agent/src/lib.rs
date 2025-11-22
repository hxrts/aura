//! Aura Agent: Authority-First Runtime Composition
//!
//! This crate provides Layer-6 runtime composition for authority-based identity
//! management. It follows clean architectural layering with authority-first design.
//!
//! # Architecture
//!
//! - **agent-core**: Public API, config, authority-centric context types
//! - **agent-runtime**: Effect registry, builder, lifecycle, and coordination
//! - **agent-handlers**: Domain-specific handlers with shared utilities
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_agent::{AgentBuilder, AuthorityId};
//!
//! // Production agent with authority-first design
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_production()
//!     .await?;
//!
//! // Testing agent  
//! let agent = AgentBuilder::new()
//!     .with_authority(authority_id)
//!     .build_testing()?;
//! ```

#![allow(clippy::expect_used)]

// Core modules (public API)
pub mod core;

// Runtime modules (internal)
mod runtime;

// Handler modules (internal)
mod handlers;

// Public API - authority-first design
pub use core::{AgentBuilder, AgentConfig, AgentError, AgentResult, AuraAgent, AuthorityContext};

// Runtime types for advanced usage
pub use runtime::{
    ChoreographyAdapter, ContextManager, EffectExecutor, EffectRegistry, EffectRegistryError,
    EffectRegistryExt, EffectSystemBuilder, ExecutionMode, FlowBudgetManager, LifecycleManager,
    ReceiptManager, RuntimeBuilder, RuntimeSystem,
};

// Effect system types
pub use runtime::effects::AuraEffectSystem;

// Re-export core types for convenience (authority-first)
pub use aura_core::identifiers::{AuthorityId, ContextId, SessionId};

/// Create a production agent (convenience function)
pub async fn create_production_agent(authority_id: AuthorityId) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_production()
        .await
}

/// Create a testing agent (convenience function)  
pub fn create_testing_agent(authority_id: AuthorityId) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing()
}

/// Create a simulation agent (convenience function)
pub fn create_simulation_agent(authority_id: AuthorityId, seed: u64) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_simulation(seed)
}
