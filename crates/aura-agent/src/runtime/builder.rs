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

use super::services::{
    AuthorityManager, ContextManager, FlowBudgetManager, ReceiptManager, ReceiptManagerConfig,
};
use super::shared_transport::SharedTransport;
use super::system::RuntimeSystem;
use super::{ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager};
use crate::core::{AgentConfig, AuthorityContext};
use crate::handlers::RendezvousHandler;
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
    receipt_config: Option<ReceiptManagerConfig>,
    shared_transport: Option<SharedTransport>,
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
            receipt_config: None,
            shared_transport: None,
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
            receipt_config: Some(ReceiptManagerConfig::for_testing()),
            shared_transport: None,
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
            receipt_config: Some(ReceiptManagerConfig::for_testing()),
            shared_transport: None,
        }
    }

    /// Set shared transport wiring for multi-agent simulations.
    pub fn with_shared_transport(mut self, shared: SharedTransport) -> Self {
        self.shared_transport = Some(shared);
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

    /// Configure receipt manager with custom settings
    pub fn with_receipt_config(mut self, config: ReceiptManagerConfig) -> Self {
        self.receipt_config = Some(config);
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
                let system = if let Some(shared) = self.shared_transport {
                    super::AuraEffectSystem::simulation_with_shared_transport_for_authority(
                        &config,
                        seed,
                        authority_id,
                        shared,
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
        let authority_manager = AuthorityManager::new();
        let flow_budget_manager = FlowBudgetManager::new(&config);
        let receipt_manager = match self.receipt_config {
            Some(receipt_config) => ReceiptManager::with_config(&config, receipt_config),
            None => ReceiptManager::new(&config),
        };

        // Create choreography adapter
        let choreography_adapter = ChoreographyAdapter::new(authority_id);

        // Create optional sync service manager with indexed journal for Merkle verification
        let sync_manager = self.sync_config.map(|sync_config| {
            super::services::SyncServiceManager::with_indexed_journal(
                sync_config,
                effect_system.indexed_journal().clone(),
                Arc::new(effect_system.time_effects().clone()),
            )
        });

        // Create optional rendezvous manager
        let rendezvous_enabled = self.rendezvous_config.is_some();
        let rendezvous_manager = self.rendezvous_config.clone().map(|rendezvous_config| {
            super::services::RendezvousManager::new(
                authority_id,
                rendezvous_config,
                Arc::new(effect_system.time_effects().clone()),
                Arc::new(aura_effects::RealUdpEffectsHandler::new()),
            )
        });

        // Create optional social manager
        let social_manager = self
            .social_config
            .map(|social_config| super::services::SocialManager::new(authority_id, social_config));

        // Create optional LAN transport service (used for LAN advertising + future TCP ingress)
        let lan_transport = if rendezvous_enabled {
            match super::services::LanTransportService::bind(config.network.bind_address.as_str())
                .await
            {
                Ok(service) => Some(Arc::new(service)),
                Err(err) => {
                    tracing::warn!(error = %err, "Failed to start LAN transport listener");
                    None
                }
            }
        } else {
            None
        };

        let rendezvous_handler = if rendezvous_enabled {
            let authority_context =
                AuthorityContext::new_with_device(authority_id, config.device_id);
            let handler = RendezvousHandler::new(authority_context).map_err(|e| e.to_string())?;
            let handler = if let Some(manager) = rendezvous_manager.as_ref() {
                handler.with_rendezvous_manager(manager.clone())
            } else {
                handler
            };
            Some(handler)
        } else {
            None
        };

        // Wrap effect system in Arc for shared ownership
        let effect_system = Arc::new(effect_system);

        if let Some(rendezvous_manager) = rendezvous_manager.as_ref() {
            effect_system.attach_rendezvous_manager(rendezvous_manager.clone());
        }
        if let Some(lan_transport) = lan_transport.as_ref() {
            effect_system.attach_lan_transport(lan_transport.clone());
        }

        // Load persisted Biscuit tokens into the in-memory cache.
        // For returning users this restores guard chain authorization.
        // For new users the cache stays empty until bootstrap_authority() creates tokens.
        effect_system.initialize_biscuit_cache().await;

        // Build runtime system with configured services
        let mut system = RuntimeSystem::new_with_services(
            effect_executor,
            effect_system.clone(),
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            rendezvous_handler,
            lan_transport,
            social_manager,
            config,
            authority_id,
        );

        // Ensure the runtime's reactive signal graph is initialized before any scheduler emissions.
        // This prevents "SignalNotFound" races during startup.
        aura_app::signal_defs::register_app_signals(&system.effects().reactive_handler())
            .await
            .map_err(|e| format!("Failed to register app signals: {e}"))?;

        // Start reactive pipeline (facts → scheduler).
        system.start_reactive_pipeline().await?;

        // Start receipt cleanup background task
        system.receipts().start_cleanup_task(
            system.tasks(),
            Arc::new(effect_system.time_effects().clone()),
        );

        // Start runtime maintenance tasks (cleanup/pruning).
        system.start_maintenance_tasks();

        // Start runtime services (sync, rendezvous, social, etc).
        system.start_services().await.map_err(|e| e.to_string())?;

        Ok(system)
    }

    /// Build the runtime system (sync)
    pub fn build_sync(self) -> Result<RuntimeSystem, String> {
        // For testing/simulation, we can build synchronously
        match self.execution_mode {
            ExecutionMode::Production => Err("Production runtime requires async build".to_string()),
            _ => {
                // Create a build-time context for wiring handlers
                let authority_id = self.authority_id.ok_or("Authority ID required")?;
                let context_id = aura_core::identifiers::ContextId::new_from_entropy([2u8; 32]);
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

    #[test]
    fn build_starts_reactive_pipeline() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let runtime = EffectSystemBuilder::testing()
            .with_authority(authority_id)
            .build_sync()
            .expect("build_sync should succeed in testing mode");
        assert!(runtime.reactive_pipeline().is_some());
    }
}
