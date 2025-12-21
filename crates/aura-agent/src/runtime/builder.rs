//! Effect System Builder
//!
//! Authority-first runtime system builder for constructing effect systems
//! with compile-time safety.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Authority-first runtime building
//! let runtime = EffectSystemBuilder::production()
//!     .with_authority(authority_id)
//!     .build(&ctx).await?;
//! ```

use std::sync::Arc;

use super::services::{ContextManager, FlowBudgetManager, ReceiptManager};
use super::system::RuntimeSystem;
use super::{ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager};
use crate::core::AgentConfig;
use aura_core::identifiers::AuthorityId;

// Re-export ExecutionMode from aura_core for convenience
pub use aura_core::effects::ExecutionMode;

/// Authority-first runtime system builder
pub struct EffectSystemBuilder {
    config: Option<AgentConfig>,
    authority_id: Option<AuthorityId>,
    execution_mode: ExecutionMode,
    sync_config: Option<super::services::SyncManagerConfig>,
    rendezvous_config: Option<super::services::RendezvousManagerConfig>,
    social_config: Option<super::services::SocialManagerConfig>,
    shared_transport_inbox:
        Option<std::sync::Arc<std::sync::RwLock<Vec<aura_core::effects::TransportEnvelope>>>>,
}

impl EffectSystemBuilder {
    /// Create a production builder
    pub fn production() -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Production,
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            shared_transport_inbox: None,
        }
    }

    /// Create a testing builder
    pub fn testing() -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Testing,
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            shared_transport_inbox: None,
        }
    }

    /// Create a simulation builder
    pub fn simulation(seed: u64) -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Simulation { seed },
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            shared_transport_inbox: None,
        }
    }

    /// Set shared transport inbox for multi-agent simulations
    pub fn with_shared_transport_inbox(
        mut self,
        inbox: std::sync::Arc<std::sync::RwLock<Vec<aura_core::effects::TransportEnvelope>>>,
    ) -> Self {
        self.shared_transport_inbox = Some(inbox);
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: AgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Set authority ID
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Enable sync service with default configuration
    pub fn with_sync(mut self) -> Self {
        self.sync_config = Some(super::services::SyncManagerConfig::default());
        self
    }

    /// Enable sync service with custom configuration
    pub fn with_sync_config(mut self, config: super::services::SyncManagerConfig) -> Self {
        self.sync_config = Some(config);
        self
    }

    /// Enable rendezvous service with default configuration
    pub fn with_rendezvous(mut self) -> Self {
        self.rendezvous_config = Some(super::services::RendezvousManagerConfig::default());
        self
    }

    /// Enable rendezvous service with custom configuration
    pub fn with_rendezvous_config(
        mut self,
        config: super::services::RendezvousManagerConfig,
    ) -> Self {
        self.rendezvous_config = Some(config);
        self
    }

    /// Enable social topology service with default configuration
    pub fn with_social(mut self) -> Self {
        self.social_config = Some(super::services::SocialManagerConfig::default());
        self
    }

    /// Enable social topology service with custom configuration
    pub fn with_social_config(mut self, config: super::services::SocialManagerConfig) -> Self {
        self.social_config = Some(config);
        self
    }

    /// Build the runtime system (async)
    pub async fn build(self, _ctx: &EffectContext) -> Result<RuntimeSystem, String> {
        let config = self.config.unwrap_or_default();
        let authority_id = self.authority_id.ok_or("Authority ID required")?;

        // Create lifecycle manager
        let lifecycle_manager = LifecycleManager::new();

        // Create a registry with appropriate execution mode
        let registry = Arc::new(super::registry::EffectRegistry::new(self.execution_mode));

        // Create effect system components based on execution mode
        let (effect_executor, effect_system) = match self.execution_mode {
            ExecutionMode::Production => {
                let executor = EffectExecutor::production(authority_id, registry.clone());
                let system =
                    super::AuraEffectSystem::production_for_authority(config.clone(), authority_id)
                        .map_err(|e| e.to_string())?;
                (executor, system)
            }
            ExecutionMode::Testing => {
                let executor = EffectExecutor::testing(authority_id, registry.clone());
                let system = super::AuraEffectSystem::testing_for_authority(&config, authority_id)
                    .map_err(|e| e.to_string())?;
                (executor, system)
            }
            ExecutionMode::Simulation { seed } => {
                let executor = EffectExecutor::simulation(authority_id, seed, registry.clone());
                // Use shared transport inbox if provided, otherwise standard simulation mode
                let system = if let Some(inbox) = self.shared_transport_inbox {
                    super::AuraEffectSystem::simulation_with_shared_transport_for_authority(
                        &config,
                        seed,
                        authority_id,
                        inbox,
                    )
                    .map_err(|e| e.to_string())?
                } else {
                    super::AuraEffectSystem::simulation_for_authority(&config, seed, authority_id)
                        .map_err(|e| e.to_string())?
                };
                (executor, system)
            }
        };

        // Create service managers
        let context_manager = ContextManager::new(&config);
        let flow_budget_manager = FlowBudgetManager::new(&config);
        let receipt_manager = ReceiptManager::new(&config);

        // Create choreography adapter
        let choreography_adapter = ChoreographyAdapter::new(authority_id);

        // Create optional sync service manager with indexed journal for Merkle verification
        let sync_manager = self.sync_config.map(|sync_config| {
            super::services::SyncServiceManager::with_indexed_journal(
                sync_config,
                effect_system.indexed_journal().clone(),
            )
        });

        // Create optional rendezvous manager
        let rendezvous_manager = self.rendezvous_config.map(|rendezvous_config| {
            super::services::RendezvousManager::new(authority_id, rendezvous_config)
        });

        // Create optional social manager
        let social_manager = self
            .social_config
            .map(|social_config| super::services::SocialManager::new(authority_id, social_config));

        // Build runtime system with configured services
        Ok(RuntimeSystem::new_with_services(
            effect_executor,
            Arc::new(effect_system),
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            social_manager,
            config,
            authority_id,
        ))
    }

    /// Build the runtime system (sync)
    pub fn build_sync(self) -> Result<RuntimeSystem, String> {
        // For testing/simulation, we can build synchronously
        match self.execution_mode {
            ExecutionMode::Production => Err("Production runtime requires async build".to_string()),
            _ => {
                // Create a build-time context for wiring handlers
                let authority_id = self.authority_id.ok_or("Authority ID required")?;
                let context_id = aura_core::identifiers::ContextId::default();
                let ctx = EffectContext::new(authority_id, context_id, self.execution_mode);

                // Use a minimal async runtime just for building
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| format!("Failed to create runtime: {}", e))?;
                rt.block_on(self.build(&ctx))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_modes() {
        assert_eq!(ExecutionMode::Production, ExecutionMode::Production);
        assert_eq!(ExecutionMode::Testing, ExecutionMode::Testing);
        assert_eq!(
            ExecutionMode::Simulation { seed: 42 },
            ExecutionMode::Simulation { seed: 42 }
        );
        assert_ne!(ExecutionMode::Production, ExecutionMode::Testing);
    }
}
