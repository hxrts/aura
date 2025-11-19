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
//! use aura_protocol::standard_patterns::EffectRegistry;
//!
//! // Production runtime with real effect handlers (NEW: uses EffectRegistry)
//! let agent = create_production_agent(device_id).await?;
//!
//! // Testing runtime with mock handlers
//! let agent = AuraAgent::for_testing(device_id);
//!
//! // Custom runtime composition using new effect registry pattern
//! let effects = EffectRegistry::production()
//!     .with_device_id(device_id)
//!     .with_logging()
//!     .with_metrics()
//!     .build()?;
//! let agent = AuraAgent::new(effects, device_id);
//! ```

// Allow expect() for testing and development code in this crate
#![allow(clippy::expect_used)]

use std::sync::Arc;

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
    identifiers::{AccountId, AuthorityId, DeviceId, SessionId},
    AuraError, AuraResult,
};

// Re-export maintenance types (moved from aura-core)
pub use maintenance::{AdminReplaced, MaintenanceEvent};

// Re-export runtime types for backward compatibility
// These were previously in aura_protocol::effects but belong in Layer 6
pub use runtime::{
    AuraEffectSystem, EffectExecutor, EffectSystemBuilder, EffectSystemConfig, EffectSystemState,
    LifecycleManager, StorageConfig,
};

/// Create an agent with production effects
///
/// This is a convenience function for creating an agent runtime with production
/// effect handlers. The runtime composes real system effects into authority workflows.
///
/// # Arguments
/// * `authority_id` - The authority identifier for this agent
///
/// # Device ID Derivation
/// For single-device authorities, device_id is derived from authority_id internally.
/// TODO: For multi-device authorities, support explicit device_id or lookup from authority state.
pub async fn create_production_agent(authority_id: AuthorityId) -> AgentResult<AuraAgent> {
    // Derive device_id from authority_id (1:1 mapping for single-device authorities)
    let device_id = DeviceId(authority_id.0);

    // Use new EffectRegistry pattern for standardized production setup
    let core_effects_arc = crate::runtime::EffectRegistry::production()
        .with_device_id(device_id)
        .with_logging()
        .with_metrics()
        .build()
        .map_err(|e| AuraError::internal(format!("Failed to create production effects: {}", e)))?;

    // Unwrap the Arc - we're the only owner at this point
    let core_effects = Arc::try_unwrap(core_effects_arc)
        .unwrap_or_else(|arc| (*arc).clone());

    Ok(AuraAgent::new(core_effects, authority_id))
}

/// Create an agent with testing effects
///
/// This creates an agent runtime with deterministic, mockable effects suitable
/// for unit testing. All handlers use controlled mock behaviors.
pub fn create_testing_agent(authority_id: AuthorityId) -> AuraAgent {
    AuraAgent::for_testing(authority_id)
}

/// Create an agent with simulation effects
///
/// This creates an agent runtime with controlled effects for simulation scenarios.
/// The seed ensures deterministic behavior across simulation runs.
pub fn create_simulation_agent(authority_id: AuthorityId, seed: u64) -> AgentResult<AuraAgent> {
    // Derive device_id from authority_id (1:1 mapping for single-device authorities)
    let device_id = DeviceId(authority_id.0);

    // Use new EffectRegistry pattern for standardized simulation setup
    let core_effects_arc = crate::runtime::EffectRegistry::simulation(seed)
        .with_device_id(device_id)
        .with_logging()
        .build()
        .map_err(|e| AuraError::internal(format!("Failed to create simulation effects: {}", e)))?;

    // Unwrap the Arc - we're the only owner at this point
    let core_effects = Arc::try_unwrap(core_effects_arc)
        .unwrap_or_else(|arc| (*arc).clone());

    Ok(AuraAgent::new(core_effects, authority_id))
}
