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
use super::{EffectContext, EffectExecutor, LifecycleManager};
use crate::core::{AgentConfig, AuthorityContext};
use crate::handlers::RendezvousHandler;
use aura_core::types::identifiers::AuthorityId;

// Re-export ExecutionMode from aura_core for convenience
pub use aura_core::effects::ExecutionMode;

pub(crate) struct TestingOwnedProfileCapability {
    owner: Arc<aura_effects::profile_storage::OwnedProfileLease>,
}
impl TestingOwnedProfileCapability {
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "TestingOwnedProfileCapability",
        family = "proof_issuer"
    )]
    #[aura_macros::authoritative_source(kind = "proof_issuer")]
    pub(crate) fn acquire(
        config: &AgentConfig,
    ) -> Result<TestingOwnedProfileCapability, crate::core::AgentError> {
        #[cfg(unix)]
        {
            let owner = aura_effects::profile_storage::FilesystemProfileStorageHandler::new(
                config.storage.base_path.clone(),
            )
            .acquire_owned_native()
            .map_err(|source| {
                crate::core::AgentError::from(aura_core::AuraError::Storage {
                    message: "acquire actual isolated testing profile".into(),
                    source: Some(Arc::new(source)),
                })
            })?;
            Ok(Self {
                owner: Arc::new(owner),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = config;
            Err(aura_core::effects::secret_lifetime::SecretLifetimeProviderUnavailable::UnsupportedSelectedProvider.into_aura_error().into())
        }
    }
    pub(super) fn into_owner(self) -> Arc<aura_effects::profile_storage::OwnedProfileLease> {
        self.owner
    }
}

/// Authority-first runtime system builder
pub struct EffectSystemBuilder {
    config: Option<AgentConfig>,
    authority_id: Option<AuthorityId>,
    physical_time_provider: Option<Arc<dyn aura_core::effects::PhysicalTimeEffects>>,
    custom_providers: Option<super::effects::SelectedCustomProviders>,
    execution_mode: ExecutionMode,
    sync_config: Option<super::services::SyncManagerConfig>,
    rendezvous_config: Option<super::services::RendezvousManagerConfig>,
    social_config: Option<super::services::SocialManagerConfig>,
    receipt_config: Option<ReceiptManagerConfig>,
    shared_transport: Option<SharedTransport>,
    selected_profile_owner: Option<Arc<aura_effects::profile_storage::OwnedProfileLease>>,
    testing_profile_owner: Option<TestingOwnedProfileCapability>,
}

impl EffectSystemBuilder {
    /// Create a production builder
    pub fn production() -> Self {
        Self {
            config: None,
            authority_id: None,
            physical_time_provider: None,
            custom_providers: None,
            execution_mode: ExecutionMode::Production,
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            receipt_config: None,
            shared_transport: None,
            selected_profile_owner: None,
            testing_profile_owner: None,
        }
    }

    /// Transfer the actual provider lease acquired before reading bootstrap state.
    /// Every adapter retains this same resource until the last owner is dropped.
    ///
    /// ```compile_fail
    /// use aura_agent::EffectSystemBuilder;
    /// EffectSystemBuilder::production().with_profile_owner(std::sync::Arc::new(()));
    /// ```
    pub fn with_profile_owner(
        mut self,
        owner: Arc<aura_effects::profile_storage::OwnedProfileLease>,
    ) -> Self {
        self.selected_profile_owner = Some(owner);
        self
    }

    /// Actual selected profile custody for integration tests, kept separate from
    /// production-lease ingress and the ordinary unowned Testing constructor.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "TestingOwnedProfileCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn testing_with_owned_profile(profile: TestingOwnedProfileCapability) -> Self {
        let mut builder = Self::testing();
        builder.testing_profile_owner = Some(profile);
        builder
    }

    /// Simulation assembly retaining the same kind of isolated profile lease.
    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "TestingOwnedProfileCapability",
        family = "runtime_helper"
    )]
    pub(crate) fn simulation_with_owned_profile(
        seed: u64,
        profile: TestingOwnedProfileCapability,
    ) -> Self {
        let mut builder = Self::simulation(seed);
        builder.testing_profile_owner = Some(profile);
        builder
    }

    /// Create a testing builder
    pub fn testing() -> Self {
        Self {
            config: None,
            authority_id: None,
            physical_time_provider: None,
            custom_providers: None,
            execution_mode: ExecutionMode::Testing,
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            receipt_config: Some(ReceiptManagerConfig::for_testing()),
            shared_transport: None,
            selected_profile_owner: None,
            testing_profile_owner: None,
        }
    }

    /// Create a simulation builder
    pub fn simulation(seed: u64) -> Self {
        Self {
            config: None,
            authority_id: None,
            physical_time_provider: None,
            custom_providers: None,
            execution_mode: ExecutionMode::Simulation { seed },
            sync_config: None,
            rendezvous_config: None,
            social_config: None,
            receipt_config: Some(ReceiptManagerConfig::for_testing()),
            shared_transport: None,
            selected_profile_owner: None,
            testing_profile_owner: None,
        }
    }

    pub(crate) fn with_custom_providers(
        mut self,
        providers: super::effects::SelectedCustomProviders,
    ) -> Self {
        self.custom_providers = Some(providers);
        self
    }

    /// Set shared transport wiring for multi-agent simulations.
    pub fn with_shared_transport(mut self, shared: SharedTransport) -> Self {
        self.shared_transport = Some(shared);
        self
    }

    /// Bind every runtime physical-time owner to the same injected provider.
    pub fn with_physical_time_provider(
        mut self,
        provider: Arc<dyn aura_core::effects::PhysicalTimeEffects>,
    ) -> Self {
        self.physical_time_provider = Some(provider);
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

    /// Build the runtime under the caller's lexical owner.
    ///
    /// The delegated future is allocated before it enters an async caller's
    /// frame. This does not spawn a task or transfer the selected profile lease
    /// to another supervisor. Cancellation drops the same owned builder future.
    pub fn build(
        self,
        ctx: &EffectContext,
    ) -> impl std::future::Future<Output = Result<RuntimeSystem, crate::builder::error::BuildError>> + '_
    {
        Box::pin(self.build_owned(ctx))
    }

    async fn build_owned(
        self,
        _ctx: &EffectContext,
    ) -> Result<RuntimeSystem, crate::builder::error::BuildError> {
        if !self.execution_mode.is_production() && self.selected_profile_owner.is_some() {
            return Err(
                crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(
                    aura_core::effects::profile_storage::ProfileStorageError::Invalid(
                        "production profile lease supplied to a nonproduction runtime".into(),
                    ),
                )),
            );
        }
        let config = self.config.unwrap_or_default();
        let authority_id =
            self.authority_id
                .ok_or(crate::builder::error::BuildError::MissingRequired(
                    "authority_id",
                ))?;

        // Create lifecycle manager
        let lifecycle_manager = LifecycleManager::new();

        // Create a registry with appropriate execution mode
        let registry = Arc::new(super::registry::EffectRegistry::new(self.execution_mode));

        // Create effect system components based on execution mode
        let (effect_executor, effect_system) = match self.execution_mode {
            ExecutionMode::Production => {
                let owner = match self.selected_profile_owner {
                    Some(owner) => owner,
                    None => {
                        let profile =
                            aura_effects::profile_storage::FilesystemProfileStorageHandler::new(
                                config.storage.base_path.clone(),
                            );
                        #[cfg(target_arch = "wasm32")]
                        let owner = profile.acquire_owned_browser().await;
                        #[cfg(not(target_arch = "wasm32"))]
                        let owner = profile.acquire_owned_native();
                        owner.map(Arc::new).map_err(|e| {
                            crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(
                                e,
                            ))
                        })?
                    }
                };
                let executor = EffectExecutor::production(authority_id, registry.clone());
                let system = match self.custom_providers {
                    Some(providers) => super::AuraEffectSystem::custom_for_authority(
                        config.clone(),
                        authority_id,
                        self.execution_mode,
                        providers,
                        Some(owner),
                        self.shared_transport,
                    ),
                    None => super::AuraEffectSystem::production_for_authority_shared_profile(
                        config.clone(),
                        authority_id,
                        owner,
                    ),
                }
                .map_err(|e| {
                    crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
                })?;
                (executor, system)
            }
            ExecutionMode::Testing => {
                let executor = EffectExecutor::testing(authority_id, registry.clone());
                // Runtime builder intentionally uses explicit execution-mode constructors.
                // Test-only callsites must use simulation_for_test* helpers instead.
                #[allow(clippy::disallowed_methods)]
                let unowned = |providers, transport| {
                    if let Some(providers) = providers {
                        super::AuraEffectSystem::custom_for_authority(
                            config.clone(),
                            authority_id,
                            self.execution_mode,
                            providers,
                            None,
                            transport,
                        )
                    } else if let Some(shared) = transport {
                        super::AuraEffectSystem::testing_with_shared_transport(
                            &config,
                            authority_id,
                            shared,
                        )
                    } else {
                        super::AuraEffectSystem::testing_for_authority(&config, authority_id)
                    }
                };
                let system = match self.testing_profile_owner {
                    Some(profile) => super::AuraEffectSystem::testing_with_owned_profile(
                        &config,
                        authority_id,
                        self.shared_transport,
                        profile,
                        self.custom_providers,
                    ),
                    None => unowned(self.custom_providers, self.shared_transport),
                };
                let system = system.map_err(|source| {
                    crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(source))
                })?;
                (executor, system)
            }
            ExecutionMode::Simulation { seed } => {
                let executor = EffectExecutor::simulation(authority_id, seed, registry.clone());
                // Use shared transport inbox if provided, otherwise standard simulation mode
                #[allow(clippy::disallowed_methods)]
                let system = if let Some(profile) = self.testing_profile_owner {
                    super::AuraEffectSystem::simulation_with_owned_profile(
                        &config,
                        seed,
                        authority_id,
                        self.shared_transport,
                        profile,
                    )
                    .map_err(|e| {
                        crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
                    })?
                } else if let Some(providers) = self.custom_providers {
                    super::AuraEffectSystem::custom_for_authority(
                        config.clone(),
                        authority_id,
                        self.execution_mode,
                        providers,
                        None,
                        self.shared_transport,
                    )
                    .map_err(|error| {
                        crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(
                            error,
                        ))
                    })?
                } else if let Some(shared) = self.shared_transport {
                    super::AuraEffectSystem::simulation_with_shared_transport_for_authority(
                        &config,
                        seed,
                        authority_id,
                        shared,
                    )
                    .map_err(|e| {
                        crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
                    })?
                } else {
                    super::AuraEffectSystem::simulation_for_authority(&config, seed, authority_id)
                        .map_err(|e| {
                        crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
                    })?
                };
                (executor, system)
            }
        };

        // Configure the actual effect owner before any service retains its clock.
        let effect_system = match self.physical_time_provider {
            Some(provider) => effect_system.with_physical_time_provider(provider),
            None => effect_system,
        };

        effect_system.initialize_selected_receipt_key().await;

        // Create service managers

        let context_manager = ContextManager::new(&config);
        let authority_manager = AuthorityManager::new();
        let flow_budget_manager = FlowBudgetManager::new(&config);
        let receipt_manager = match self.receipt_config {
            Some(receipt_config) => ReceiptManager::with_config(&config, receipt_config),
            None => ReceiptManager::new(&config),
        };

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
            super::services::RendezvousManager::new_with_default_udp(
                authority_id,
                rendezvous_config,
                Arc::new(effect_system.time_effects().clone()),
            )
        });

        // Create optional social manager
        let social_manager = self
            .social_config
            .map(|social_config| super::services::SocialManager::new(authority_id, social_config));

        let service_registry = rendezvous_manager
            .as_ref()
            .map(|manager| manager.registry())
            .unwrap_or_else(|| Arc::new(super::services::ServiceRegistry::new()));

        let move_manager = Some(super::services::MoveManager::new(
            super::services::MoveManagerConfig::default(),
            service_registry.clone(),
        ));
        let local_health_observer_instance = super::services::LocalHealthObserver::new(
            super::services::LocalHealthObserverConfig::default(),
        );
        let local_health_observer = Some(local_health_observer_instance.clone());
        let selection_manager = Some(super::services::SelectionManager::new(
            super::services::SelectionManagerConfig::default(),
            service_registry.clone(),
            local_health_observer_instance,
        ));
        let anonymous_path_manager = Some(super::services::AnonymousPathManager::new(
            super::services::AnonymousPathManagerConfig::default(),
            service_registry.clone(),
        ));
        let hold_manager = Some(super::services::HoldManager::new(
            authority_id,
            super::services::HoldManagerConfig::default(),
            service_registry,
        ));
        let cover_traffic_generator = Some(super::services::CoverTrafficGenerator::new(
            super::services::CoverTrafficGeneratorConfig::default(),
        ));

        // Create optional LAN transport service (used for LAN advertising + future TCP ingress)
        let lan_transport: Option<Arc<super::services::LanTransportService>> = {
            #[cfg(target_arch = "wasm32")]
            {
                if rendezvous_enabled {
                    match super::services::LanTransportService::bind(
                        config.network.bind_address.as_str(),
                    )
                    .await
                    {
                        Ok(service) => Some(Arc::new(service)),
                        Err(err) => {
                            tracing::warn!(
                                error = %err,
                                "Failed to initialize browser transport advertisement"
                            );
                            None
                        }
                    }
                } else {
                    None
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            {
                if rendezvous_enabled {
                    match super::services::LanTransportService::bind(
                        config.network.bind_address.as_str(),
                    )
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
                }
            }
        };

        let rendezvous_handler = if rendezvous_enabled {
            let authority_context =
                AuthorityContext::new_with_device(authority_id, config.device_id);
            let handler = RendezvousHandler::new(authority_context).map_err(|e| {
                crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
            })?;
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
        if let Some(move_manager) = move_manager.as_ref() {
            effect_system.attach_move_manager(move_manager.clone());
        }
        if let Some(lan_transport) = lan_transport.as_ref() {
            effect_system.attach_lan_transport(lan_transport.clone());
        }

        // Load persisted Biscuit tokens into the in-memory cache.
        // For returning users this restores guard chain authorization.
        // For new users the cache stays empty until bootstrap_authority() creates tokens.
        effect_system
            .initialize_biscuit_cache()
            .await
            .map_err(|source| crate::builder::BuildError::EffectInitSource {
                effect: "persisted Biscuit authorization",
                source: Box::new(source),
            })?;

        // Build runtime system with configured services
        let system = RuntimeSystem::new_with_services(
            effect_executor,
            effect_system.clone(),
            context_manager,
            authority_manager,
            flow_budget_manager,
            receipt_manager,
            lifecycle_manager,
            sync_manager,
            rendezvous_manager,
            move_manager,
            local_health_observer,
            selection_manager,
            anonymous_path_manager,
            hold_manager,
            cover_traffic_generator,
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
            .map_err(|e| crate::builder::error::BuildError::EffectInitSource {
                effect: "app_signals",
                source: Box::new(e),
            })?;

        // Start runtime services (sync, rendezvous, social, etc).
        system.start_services().await.map_err(|e| {
            crate::builder::error::BuildError::RuntimeConstructionSource(Box::new(e))
        })?;

        Ok(system)
    }

    /// Build the runtime system (sync)
    pub fn build_sync(self) -> Result<RuntimeSystem, crate::builder::error::BuildError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = self;
            Err(crate::builder::error::BuildError::RuntimeConstruction(
                "build_sync is unavailable on wasm32; use build(...).await".to_string(),
            ))
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::builder::error::BuildError;
            // For testing/simulation, we can build synchronously
            match self.execution_mode {
                ExecutionMode::Production => Err(BuildError::RuntimeConstruction(
                    "Production runtime requires async build".to_string(),
                )),
                _ => {
                    // Create a build-time context for wiring handlers
                    let authority_id = self
                        .authority_id
                        .ok_or(BuildError::MissingRequired("authority_id"))?;
                    let context_id =
                        aura_core::types::identifiers::ContextId::new_from_entropy([2u8; 32]);
                    let ctx = EffectContext::new(authority_id, context_id, self.execution_mode);

                    // Use a minimal async runtime just for building
                    let rt = tokio::runtime::Runtime::new()
                        .map_err(|e| BuildError::RuntimeConstructionSource(Box::new(e)))?;
                    rt.block_on(self.build(&ctx))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::EffectContext;

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
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        assert!(rt.block_on(runtime.reactive_pipeline_running()));
    }

    #[test]
    fn build_shutdown_typed_succeeds() {
        let authority_id = AuthorityId::new_from_entropy([7u8; 32]);
        let ctx = EffectContext::new(
            authority_id,
            aura_core::types::identifiers::ContextId::new_from_entropy([9u8; 32]),
            ExecutionMode::Testing,
        );

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let runtime = EffectSystemBuilder::testing()
                .with_authority(authority_id)
                .build(&ctx)
                .await
                .expect("build should succeed in testing mode");
            runtime
                .shutdown_typed(&ctx)
                .await
                .expect("shutdown_typed should succeed");
        });
    }

    #[test]
    fn build_shutdown_typed_cancels_runtime_load() {
        let authority_id = AuthorityId::new_from_entropy([8u8; 32]);
        let ctx = EffectContext::new(
            authority_id,
            aura_core::types::identifiers::ContextId::new_from_entropy([10u8; 32]),
            ExecutionMode::Testing,
        );

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async move {
            let runtime = EffectSystemBuilder::testing()
                .with_authority(authority_id)
                .build(&ctx)
                .await
                .expect("build should succeed in testing mode");
            let tasks = runtime.tasks();
            let activity_gate = runtime.activity_gate();

            let _task_handle = tasks.spawn_named("test.shutdown.load", async move {
                std::future::pending::<()>().await;
            });

            runtime
                .shutdown_typed(&ctx)
                .await
                .expect("shutdown_typed should cancel runtime-owned load");

            assert!(tasks.active_tasks().is_empty());
            assert_eq!(
                activity_gate.state(),
                crate::runtime::RuntimeActivityState::Stopped
            );
        });
    }
}

#[cfg(all(test, unix))]
mod owned_testing_profile_tests {
    use super::*;
    fn config(path: &std::path::Path) -> AgentConfig {
        AgentConfig {
            storage: crate::core::config::StorageConfig {
                base_path: path.to_path_buf(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn context(authority: AuthorityId) -> EffectContext {
        EffectContext::new(
            authority,
            aura_core::ContextId::new_from_entropy([83; 32]),
            ExecutionMode::Testing,
        )
    }
    fn actual_invalid_profile_source(error: &impl std::error::Error) -> bool {
        let mut source = error.source();
        while let Some(error) = source {
            if matches!(
                error.downcast_ref::<aura_core::effects::profile_storage::ProfileStorageError>(),
                Some(aura_core::effects::profile_storage::ProfileStorageError::Invalid(_))
            ) {
                return true;
            }
            source = error.source();
        }
        false
    }
    #[tokio::test]
    async fn testing_owned_profile_retains_actual_lease_until_runtime_shutdown() {
        let directory = tempfile::tempdir().expect("isolated owned runtime profile");
        let config = config(directory.path());
        let capability =
            TestingOwnedProfileCapability::acquire(&config).expect("actual exclusive descriptor");
        let physical = Arc::downgrade(&capability.owner);
        let authority = AuthorityId::new_from_entropy([81; 32]);
        let context = context(authority);
        let runtime = EffectSystemBuilder::testing_with_owned_profile(capability)
            .with_authority(authority)
            .with_config(config)
            .build(&context)
            .await
            .expect("actual owned Testing assembly");
        assert!(
            physical.upgrade().is_some(),
            "selected provider and runtime retain original physical lease"
        );
        runtime
            .tasks()
            .shutdown_with_timeout(std::time::Duration::from_secs(2))
            .await
            .expect("acknowledged actual service teardown");
        drop(runtime);
        assert!(
            physical.upgrade().is_none(),
            "drained runtime releases physical ownership before restart"
        );
    }
    #[tokio::test]
    async fn ordinary_testing_cannot_accept_production_profile_lease() {
        let directory = tempfile::tempdir().expect("isolated production lease guard");
        let config = config(directory.path());
        let capability =
            TestingOwnedProfileCapability::acquire(&config).expect("actual exclusive descriptor");
        let authority = AuthorityId::new_from_entropy([82; 32]);
        let context = context(authority);
        let error = match EffectSystemBuilder::testing()
            .with_profile_owner(capability.into_owner())
            .with_authority(authority)
            .with_config(config)
            .build(&context)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("ordinary Testing must reject production lease ingress"),
        };
        assert!(
            actual_invalid_profile_source(&error),
            "original typed guard retained"
        );
    }
    #[tokio::test]
    async fn testing_owned_profile_cannot_retarget_a_foreign_configuration() {
        let original = tempfile::tempdir().expect("original physical profile");
        let foreign = tempfile::tempdir().expect("foreign physical profile");
        let capability = TestingOwnedProfileCapability::acquire(&config(original.path()))
            .expect("actual original lease");
        let authority = AuthorityId::new_from_entropy([84; 32]);
        let context = context(authority);
        let error = match EffectSystemBuilder::testing_with_owned_profile(capability)
            .with_authority(authority)
            .with_config(config(foreign.path()))
            .build(&context)
            .await
        {
            Err(error) => error,
            Ok(_) => panic!("strong capability cannot select another physical profile"),
        };
        assert!(
            actual_invalid_profile_source(&error),
            "actual profile mismatch source preserved"
        );
        assert!(
            !foreign.path().join("secure_storage").exists(),
            "mismatch rejected before foreign provider construction"
        );
    }
}
