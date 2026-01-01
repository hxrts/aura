//! Simulation Environment Factory Implementation
//!
//! This module provides the `SimulationEnvironmentFactory` implementation for creating
//! `AuraEffectSystem` instances suitable for simulation. This enables the simulator
//! to work through a trait-based abstraction rather than directly importing concrete types.
//!
//! # Architecture
//!
//! The factory pattern decouples the simulator (Layer 8) from the agent's effect system
//! internals (Layer 6), following the dependency inversion principle:
//!
//! ```text
//! aura-core (Layer 1)          aura-agent (Layer 6)          aura-simulator (Layer 8)
//! ┌────────────────────┐       ┌────────────────────┐        ┌────────────────────┐
//! │ SimulationEnv-     │       │ AuraEffectSystem   │        │ Uses factory via   │
//! │ vironmentFactory   │◄──────│ EffectSystemFactory│◄───────│ trait bounds       │
//! │ (trait)            │       │ (impl)             │        │                    │
//! └────────────────────┘       └────────────────────┘        └────────────────────┘
//! ```
//!
//! # Blocking Lock Usage
//!
//! Uses `parking_lot::RwLock` for shared simulation transport because this is
//! test/simulation infrastructure with brief sync-only operations. See
//! `SharedTransport` documentation for details.

#![allow(clippy::disallowed_types)]

#[cfg(feature = "simulation")]
use aura_core::effects::{
    SimulationEnvironmentConfig, SimulationEnvironmentError, SimulationEnvironmentFactory,
    TransportEnvelope,
};
#[cfg(feature = "simulation")]
use aura_core::hash::hash;
#[cfg(feature = "simulation")]
use aura_core::identifiers::AuthorityId;
#[cfg(feature = "simulation")]
use std::sync::Arc;

#[cfg(feature = "simulation")]
use super::effects::AuraEffectSystem;
#[cfg(feature = "simulation")]
use crate::core::AgentConfig;
#[cfg(feature = "simulation")]
use parking_lot::RwLock;

/// Factory for creating `AuraEffectSystem` instances for simulation
///
/// This factory implements the `SimulationEnvironmentFactory` trait from `aura-core`,
/// allowing the simulator to create effect systems without directly depending on
/// `AuraEffectSystem` internals.
///
/// # Example
///
/// ```rust,ignore
/// use aura_agent::runtime::EffectSystemFactory;
/// use aura_core::effects::{SimulationEnvironmentFactory, SimulationEnvironmentConfig};
///
/// async fn run_simulation<F: SimulationEnvironmentFactory>(factory: &F) {
///     let config = SimulationEnvironmentConfig::new(42, device_id);
///     let effects = factory.create_simulation_environment(config).await?;
///     // Use effects...
/// }
///
/// // Create factory and run simulation
/// let factory = EffectSystemFactory::default();
/// run_simulation(&factory).await;
/// ```
#[cfg(feature = "simulation")]
#[derive(Debug, Clone, Default)]
pub struct EffectSystemFactory {
    /// Base configuration for created effect systems
    base_config: AgentConfig,
}

#[cfg(feature = "simulation")]
impl EffectSystemFactory {
    /// Create a new factory with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new factory with custom base configuration
    pub fn with_config(config: AgentConfig) -> Self {
        Self {
            base_config: config,
        }
    }

    /// Convert simulation config to agent config
    fn to_agent_config(&self, config: &SimulationEnvironmentConfig) -> AgentConfig {
        let mut agent_config = self.base_config.clone();
        agent_config.device_id = config.device_id;
        agent_config
    }
}

#[cfg(feature = "simulation")]
#[async_trait::async_trait]
impl SimulationEnvironmentFactory for EffectSystemFactory {
    type EffectSystem = AuraEffectSystem;

    async fn create_simulation_environment(
        &self,
        config: SimulationEnvironmentConfig,
    ) -> Result<Arc<Self::EffectSystem>, SimulationEnvironmentError> {
        let agent_config = self.to_agent_config(&config);

        // Determine authority ID (use provided or derive from device ID)
        let authority_id = match config.authority_id {
            Some(authority_id) => authority_id,
            None => {
                let device_bytes = config.device_id.to_bytes().map_err(|e| {
                    SimulationEnvironmentError::CreationFailed(format!(
                        "DeviceId::to_bytes failed: {e}"
                    ))
                })?;
                AuthorityId::new_from_entropy(hash(&device_bytes))
            }
        };

        let effect_system =
            AuraEffectSystem::simulation_for_authority(&agent_config, config.seed, authority_id)
                .map_err(|e| SimulationEnvironmentError::CreationFailed(e.to_string()))?;

        Ok(Arc::new(effect_system))
    }

    async fn create_simulation_environment_with_shared_transport(
        &self,
        config: SimulationEnvironmentConfig,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Result<Arc<Self::EffectSystem>, SimulationEnvironmentError> {
        let agent_config = self.to_agent_config(&config);

        // Determine authority ID (use provided or derive from device ID)
        let authority_id = match config.authority_id {
            Some(authority_id) => authority_id,
            None => {
                let device_bytes = config.device_id.to_bytes().map_err(|e| {
                    SimulationEnvironmentError::CreationFailed(format!(
                        "DeviceId::to_bytes failed: {e}"
                    ))
                })?;
                AuthorityId::new_from_entropy(hash(&device_bytes))
            }
        };

        let effect_system = AuraEffectSystem::simulation_with_shared_inbox_for_authority(
            &agent_config,
            config.seed,
            authority_id,
            shared_inbox,
        )
        .map_err(|e| SimulationEnvironmentError::CreationFailed(e.to_string()))?;

        Ok(Arc::new(effect_system))
    }
}

#[cfg(all(test, feature = "simulation"))]
mod tests {
    use super::*;
    use aura_core::effects::RuntimeEffectsBundle;
    use aura_core::DeviceId;
    use parking_lot::RwLock;

    #[tokio::test]
    async fn test_factory_creates_effect_system() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([1u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id);

        let result = factory.create_simulation_environment(config).await;
        assert!(result.is_ok());

        let effects = match result {
            Ok(effects) => effects,
            Err(err) => panic!("failed to create simulation environment: {err:?}"),
        };
        assert!(effects.is_simulation_mode());
    }

    #[tokio::test]
    async fn test_factory_with_shared_transport() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([2u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id);
        let shared_inbox = Arc::new(RwLock::new(Vec::new()));

        let result = factory
            .create_simulation_environment_with_shared_transport(config, shared_inbox)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_with_explicit_authority() {
        let factory = EffectSystemFactory::new();
        let device_id = DeviceId::new_from_entropy([3u8; 32]);
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let config = SimulationEnvironmentConfig::new(42, device_id).with_authority(authority_id);

        let result = factory.create_simulation_environment(config).await;
        assert!(result.is_ok());
    }
}
