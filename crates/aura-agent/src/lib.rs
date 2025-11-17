//! Aura Agent: Device Runtime Composition with Effect System Architecture
//!
//! This crate provides device-side identity management by composing handlers
//! into unified device runtimes. It follows the runtime composition pattern from the
//! unified effect system architecture.
//!
//! # Architecture
//!
//! This crate follows **Layer 6 Runtime Composition** patterns:
//! - **Handler Composition**: Combines core effects into device-specific workflows
//! - **Runtime Creation**: Composes handlers into executable device runtimes
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
//! // Custom runtime composition using effect system
//! let config = aura_protocol::effects::EffectSystemConfig::for_production(device_id)?
//!     .with_logging(true)
//!     .with_metrics(true);
//! let effects = aura_protocol::effects::AuraEffectSystem::new(config)?;
//! let agent = AuraAgent::new(effects, device_id);
//! ```

// Allow expect() for testing and development code in this crate
#![allow(clippy::expect_used)]

// Core agent runtime
pub mod agent;
pub mod config;
pub mod errors;

// Effect system integration
pub mod effects;
pub mod handlers;

// Storage utilities
pub mod storage_keys;

// OTA and maintenance
pub mod maintenance;
pub mod ota_orchestrator;

// Runtime composition (Layer 6) - moved from aura-protocol
pub mod optimizations;
pub mod runtime;

// Unified agent effect system (DISABLED - superseded by AuraEffectSystem in aura-protocol)
// pub mod system;

// Agent operations with authorization
pub mod operations;

// Re-export public API
pub use agent::AuraAgent;
pub use config::AgentConfig;
pub use errors::Result as AgentResult;

// Re-export effect traits for documentation
pub use effects::*;

// Re-export core types from aura-core for convenience
pub use aura_core::{
    identifiers::{AccountId, DeviceId, SessionId},
    AuraError, AuraResult,
};

// Re-export runtime types for backward compatibility
// These were previously in aura_protocol::effects but belong in Layer 6
pub use runtime::{
    AuraEffectSystem, EffectExecutor, EffectSystemBuilder, EffectSystemConfig, EffectSystemState,
    LifecycleManager, StorageConfig,
};

/// Create an agent with production effects
///
/// This is a convenience function for creating an agent runtime with production
/// effect handlers. The runtime composes real system effects into device workflows.
pub async fn create_production_agent(device_id: DeviceId) -> AgentResult<AuraAgent> {
    let config = runtime::EffectSystemConfig::for_production(device_id)?;
    let core_effects = aura_protocol::effects::AuraEffectSystemFactory::new(
        aura_protocol::effects::EffectSystemConfig { device_id },
    )?;

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
    let config = runtime::EffectSystemConfig::for_simulation(device_id, seed);
    let core_effects = aura_protocol::effects::AuraEffectSystemFactory::new(
        aura_protocol::effects::EffectSystemConfig { device_id },
    )
    .expect("Failed to create simulation effect system");
    AuraAgent::new(core_effects, device_id)
}
