//! Aura Agent: Device Runtime Composition with Effect System Architecture
//!
//! This crate provides device-side identity management by composing handlers and middleware
//! into unified device runtimes. It follows the runtime composition pattern from the
//! unified effect system architecture.
//!
//! # Architecture
//!
//! This crate follows **Layer 4 Runtime Composition** patterns:
//! - **Handler Composition**: Combines core effects into device-specific workflows
//! - **Runtime Creation**: Composes handlers + middleware into executable runtimes
//! - **Agent Effects**: Defines device-level capabilities and operations
//! - **Simulation Ready**: All behavior controllable through injected effects
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_agent::{AuraAgent, create_production_agent};
//!
//! // Production runtime with real effect handlers
//! let agent = create_production_agent(device_id).await?;
//!
//! // Testing runtime with mock handlers
//! let agent = AuraAgent::for_testing(device_id);
//!
//! // Custom runtime composition
//! let agent = AuraAgent::builder(device_id)
//!     .with_secure_storage()
//!     .with_biometric_auth()
//!     .with_metrics_middleware()
//!     .build().await?;
//! ```

// Core agent runtime
pub mod agent;
pub mod config;
pub mod errors;

// Authorization integration (Phase 5)
pub mod operations;

// Effect system integration
pub mod effects;
pub mod handlers;
pub mod maintenance;
pub mod middleware;

// Re-export public API
pub use agent::AuraAgent;
pub use config::AgentConfig;
pub use errors::Result as AgentResult;

// Re-export authorization operations (Phase 5)
pub use operations::{
    AgentOperation, AgentOperationContext, AgentOperationRequest, AgentOperationResult,
    AuthenticationOperation, AuthorizedAgentOperations, SessionOperation, StorageOperation,
};

// Re-export effect traits for documentation
pub use effects::*;

pub use middleware::{
    AgentMetrics, AgentMiddlewareStack, InputValidator, MetricsMiddleware, MiddlewareStackBuilder,
    OperationMetrics, TracingMiddleware, ValidationMiddleware, ValidationRule,
};

pub use maintenance::{MaintenanceController, SnapshotOutcome};

// Re-export core types from aura-core for convenience
pub use aura_core::{
    identifiers::{AccountId, DeviceId, SessionId},
    AuraError, AuraResult,
};

// Integration tests removed due to API changes - see tests/integration_tests.rs

/// Create an agent with production effects
///
/// This is a convenience function for creating an agent runtime with production
/// effect handlers. The runtime composes real system effects into device workflows.
pub async fn create_production_agent(device_id: DeviceId) -> AgentResult<AuraAgent> {
    use aura_protocol::effects::AuraEffectSystem;

    let core_effects = AuraEffectSystem::for_production(device_id);

    Ok(AuraAgent::new(core_effects, device_id))
}

/// Create an agent with testing effects
///
/// This creates an agent runtime with deterministic, mockable effects suitable
/// for unit testing. All handlers use controlled mock behaviors.
pub fn create_testing_agent(device_id: DeviceId) -> AuraAgent {
    AuraAgent::for_testing(device_id)
}

/// Create an agent with simulation effects
///
/// This creates an agent runtime with controlled effects for simulation scenarios.
/// The seed ensures deterministic behavior across simulation runs.
pub fn create_simulation_agent(device_id: DeviceId, seed: u64) -> AuraAgent {
    use aura_protocol::effects::AuraEffectSystem;

    let core_effects = AuraEffectSystem::for_simulation(device_id, seed);
    AuraAgent::new(core_effects, device_id)
}
