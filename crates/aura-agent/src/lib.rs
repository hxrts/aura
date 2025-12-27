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
#![allow(unexpected_cfgs)]
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

// Builder system for ergonomic runtime construction
pub mod builder;

// Runtime modules (internal)
mod runtime;

// Handler modules (public for service access)
pub mod handlers;

// Runtime-owned indexed journal utilities (stateful)
pub mod database;

// Reactive programming infrastructure (public)
pub mod reactive;

// RuntimeBridge implementation (for aura-app dependency inversion)
mod runtime_bridge;

// Journal fact registry helpers (public helper functions)
pub mod fact_registry;
pub mod fact_types;

// Public API - authority-first design
pub use core::{AgentBuilder, AgentConfig, AgentError, AgentResult, AuraAgent, AuthorityContext};

// Builder system exports
pub use builder::{
    AndroidPresetBuilder, BuildError, CliPresetBuilder, CustomPresetBuilder, DataProtectionClass,
    IosPresetBuilder, WebPresetBuilder,
};

// Session management types
pub use handlers::{SessionHandle, SessionService, SessionStats};

// Authentication types
pub use handlers::{AuthChallenge, AuthMethod, AuthResponse, AuthResult, AuthService};

// Invitation types
pub use handlers::{
    Invitation, InvitationResult, InvitationService, InvitationStatus, InvitationType,
};

// Recovery types
pub use handlers::{
    GuardianApproval, RecoveryOperation, RecoveryRequest, RecoveryResult, RecoveryService,
    RecoveryState,
};

// OTA types
pub use handlers::{OtaHandler, UpdateInfo, UpdateResult, UpdateStatus};

// Rendezvous types
pub use handlers::{ChannelResult, RendezvousHandler, RendezvousResult, RendezvousServiceApi};

// Runtime types for advanced usage
pub use runtime::{
    AuraHandlerAdapter as ChoreographyAdapter, EffectContext, EffectExecutor, EffectRegistry,
    EffectRegistryError, EffectRegistryExt, EffectSystemBuilder, FlowBudgetManager,
    LifecycleManager, ReceiptManager, RuntimeBuilder, RuntimeSystem, SharedTransport,
};

// Sync service types
pub use runtime::services::{SyncManagerConfig, SyncManagerState, SyncServiceManager};

// Rendezvous service types
pub use runtime::services::{RendezvousManager, RendezvousManagerConfig};

// Social service types
pub use runtime::services::{SocialManager, SocialManagerConfig, SocialManagerState};

// Threshold signing service types
pub use runtime::services::ThresholdSigningService;

// Re-export core types for convenience
pub use aura_core::effects::ExecutionMode;

// Effect system types
pub use runtime::AuraEffectSystem;

// Simulation factory (feature-gated)
#[cfg(feature = "simulation")]
pub use runtime::EffectSystemFactory;

// Re-export core types for convenience (authority-first)
pub use aura_core::identifiers::{AuthorityId, ContextId, SessionId};

pub use fact_registry::build_fact_registry;

/// Create a production agent (convenience function)
pub async fn create_production_agent(
    ctx: &EffectContext,
    authority_id: AuthorityId,
) -> AgentResult<AuraAgent> {
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
