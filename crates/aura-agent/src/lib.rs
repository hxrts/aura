//! # Aura Agent - Layer 6: Runtime Composition
//!
//! This crate provides runtime composition and effect system assembly for authority-based
//! identity management in the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 6 runtime composition crate providing:
//! - Authority-first runtime assembly and lifecycle management
//! - Effect registry and builder infrastructure
//! - Context management for multi-threaded agent coordination
//! - Choreography adapter for protocol execution
//! - Production, testing, and simulation execution modes
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1-5**: All lower layers (core, domain crates, effects, protocols, features)
//! - **MUST NOT**: Create new effect implementations (use aura-effects)
//! - **MUST NOT**: Implement multi-party coordination (use aura-protocol)
//! - **MUST NOT**: Be imported by Layer 1-5 crates (no circular dependencies)
//!
//! ## What Belongs Here
//!
//! - Effect registry and builder infrastructure
//! - Runtime system composition and lifecycle
//! - Authority context management and tracking
//! - Execution mode implementation (production, testing, simulation)
//! - Choreography protocol adapter for protocol execution
//! - Receipt and flow budget management
//! - Public API for agent creation and operation
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Effect composition rules (belong in aura-composition)
//! - Multi-party protocol logic (belong in aura-protocol)
//! - Feature protocol implementations (belong in Layer 5 crates)
//! - Testing harnesses and fixtures (belong in aura-testkit)
//!
//! ## Design Principles
//!
//! - Authority-first design: all operations scoped to specific authorities
//! - Lazy composition: effects are assembled on-demand, not eagerly
//! - Stateless handlers: runtime delegates state to journals and contexts
//! - Mode-aware execution: production, testing, and simulation use same API
//! - Lifecycle management: resource cleanup and graceful shutdown
//! - Zero coupling to Layer 5: runtime is agnostic to specific protocols
//!
//! ## Key Components
//!
//! - **AgentBuilder**: Fluent API for composing agents with authority context
//! - **EffectRegistry**: Dynamic registry of available effect handlers
//! - **EffectSystemBuilder**: Assembly infrastructure for effect combinations
//! - **AuraAgent**: Public API for agent operations
//! - **RuntimeSystem**: Internal runtime coordination
//!
//! ## Usage
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
    AuraHandlerAdapter as ChoreographyAdapter, EffectContext, EffectExecutor, EffectRegistry, EffectRegistryError,
    EffectRegistryExt, EffectSystemBuilder, FlowBudgetManager, LifecycleManager,
    ReceiptManager, RuntimeBuilder, RuntimeSystem,
};

// Re-export core types for convenience
pub use aura_core::effects::ExecutionMode;

// Effect system types
pub use runtime::AuraEffectSystem;

// Re-export core types for convenience (authority-first)
pub use aura_core::identifiers::{AuthorityId, ContextId, SessionId};

/// Create a production agent (convenience function)
pub async fn create_production_agent(ctx: &EffectContext, authority_id: AuthorityId) -> AgentResult<AuraAgent> {
    AgentBuilder::new()
        .with_authority(authority_id)
        .build_production(ctx)
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
