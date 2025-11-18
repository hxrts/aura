//! Builder pattern for constructing AuraEffectSystem instances
//!
//! This module provides a flexible builder pattern for creating effect systems
//! with custom configurations and handlers, supporting both async and sync
//! initialization paths.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use aura_core::{session_epochs::Epoch, AuraResult, DeviceId};

use crate::handlers::{AuraHandler, EffectType, ExecutionMode};
use crate::runtime::{
    container::EffectContainer,
    executor::{EffectExecutor, EffectExecutorBuilder},
    lifecycle::LifecycleManager,
    services::{ContextManager, FlowBudgetManager, ReceiptManager},
    AuraEffectSystem, EffectSystemConfig, StorageConfig,
};

/// Builder for constructing AuraEffectSystem instances
///
/// This builder provides a flexible way to construct effect systems with
/// custom configurations and handlers. It supports both synchronous and
/// asynchronous initialization paths.
///
/// # Example
/// ```no_run
/// # use aura_protocol::composition::{AuraEffectSystemBuilder};
/// # use aura_protocol::orchestration::ExecutionMode;
/// # use aura_core::DeviceId;
/// # async fn example() -> aura_core::AuraResult<()> {
/// let device_id = DeviceId::new();
/// let system = AuraEffectSystemBuilder::new()
///     .with_device_id(device_id)
///     .with_execution_mode(ExecutionMode::Testing)
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct AuraEffectSystemBuilder {
    config: Option<EffectSystemConfig>,
    device_id: Option<DeviceId>,
    execution_mode: Option<ExecutionMode>,
    storage_config: Option<StorageConfig>,
    default_flow_limit: Option<u64>,
    initial_epoch: Option<Epoch>,
    custom_handlers: HashMap<EffectType, Box<dyn AuraHandler>>,
    container: Option<Arc<EffectContainer>>,
}

impl AuraEffectSystemBuilder {
    /// Create a new builder instance
    pub fn new() -> Self {
        Self {
            config: None,
            device_id: None,
            execution_mode: None,
            storage_config: None,
            default_flow_limit: None,
            initial_epoch: None,
            custom_handlers: HashMap::new(),
            container: None,
        }
    }

    /// Set the device ID
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Set the execution mode
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = Some(mode);
        self
    }

    /// Set the storage configuration
    pub fn with_storage_config(mut self, config: StorageConfig) -> Self {
        self.storage_config = Some(config);
        self
    }

    /// Set the default flow budget limit
    pub fn with_default_flow_limit(mut self, limit: u64) -> Self {
        self.default_flow_limit = Some(limit);
        self
    }

    /// Set the initial epoch
    pub fn with_initial_epoch(mut self, epoch: Epoch) -> Self {
        self.initial_epoch = Some(epoch);
        self
    }

    /// Use a complete configuration instead of individual settings
    pub fn with_config(mut self, config: EffectSystemConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Add a custom handler for a specific effect type
    ///
    /// This allows replacing the default handler for any effect type
    /// with a custom implementation.
    pub fn with_handler(mut self, effect_type: EffectType, handler: Box<dyn AuraHandler>) -> Self {
        self.custom_handlers.insert(effect_type, handler);
        self
    }

    /// Use a custom effect container for dependency injection
    ///
    /// This allows using a pre-configured container with registered handlers.
    pub fn with_container(mut self, container: Arc<EffectContainer>) -> Self {
        self.container = Some(container);
        self
    }

    /// Build the effect system asynchronously
    ///
    /// This method performs async initialization and is suitable for
    /// production use where async operations are expected.
    pub async fn build(self) -> AuraResult<AuraEffectSystem> {
        let config = self.resolve_config()?;

        // For now, use the factory method from aura-protocol
        // TODO: Implement custom executor-based composition when handler adapters are available
        use aura_protocol::handlers::CompositeHandler;
        let handler: AuraEffectSystem = Box::new(CompositeHandler::for_testing(config.device_id.0));
        Ok(handler)
    }

    /// Build the effect system synchronously
    ///
    /// This method avoids async operations and is suitable for use in
    /// test contexts where async runtimes might already be active.
    pub fn build_sync(self) -> AuraResult<AuraEffectSystem> {
        let config = self.resolve_config()?;

        // For now, use the factory method from aura-protocol
        // TODO: Implement custom executor-based composition when handler adapters are available
        use aura_protocol::handlers::CompositeHandler;
        let handler: AuraEffectSystem = Box::new(CompositeHandler::for_testing(config.device_id.0));
        Ok(handler)
    }

    /// Resolve the final configuration from builder settings
    fn resolve_config(&self) -> AuraResult<EffectSystemConfig> {
        // If a complete config was provided, use it
        if let Some(config) = &self.config {
            return Ok(config.clone());
        }

        // Otherwise, build config from individual settings
        let device_id = self
            .device_id
            .ok_or_else(|| aura_core::AuraError::invalid("Device ID is required"))?;

        let execution_mode = self.execution_mode.unwrap_or(ExecutionMode::Testing);

        let storage_config = self.storage_config.clone().unwrap_or_else(|| {
            match execution_mode {
                ExecutionMode::Testing => StorageConfig::for_testing(),
                ExecutionMode::Production => {
                    // For production, we'd normally return an error if storage isn't configured
                    // For now, use a default that would need to be replaced
                    StorageConfig::for_testing()
                }
                ExecutionMode::Simulation { seed: _ } => StorageConfig::for_simulation(),
            }
        });

        // Use provided values or defaults based on execution mode
        let (default_flow_limit, initial_epoch) = match execution_mode {
            ExecutionMode::Testing => (
                self.default_flow_limit.unwrap_or(10_000),
                self.initial_epoch.unwrap_or_else(|| Epoch::from(1)),
            ),
            ExecutionMode::Production => (
                self.default_flow_limit.unwrap_or(100_000),
                self.initial_epoch.unwrap_or_else(|| Epoch::from(1)),
            ),
            ExecutionMode::Simulation { .. } => (
                self.default_flow_limit.unwrap_or(50_000),
                self.initial_epoch.unwrap_or_else(|| Epoch::from(1)),
            ),
        };

        Ok(EffectSystemConfig {
            device_id,
            execution_mode,
            default_flow_limit,
            initial_epoch: initial_epoch.into(), // Convert Epoch to u64
            storage_config: Some(storage_config), // Wrap in Option
        })
    }

    // DISABLED: These methods are disabled because they require handler adapter types
    // (CryptoHandlerAdapter, NetworkHandlerAdapter, etc.) that don't exist yet.
    // TODO: Re-enable once handler adapters are implemented
    /*
    /// Build the effect executor with appropriate handlers
    fn build_executor(self, config: &EffectSystemConfig) -> AuraResult<EffectExecutor> {
        let mut executor_builder = EffectExecutorBuilder::new();

        // First, add default handlers based on execution mode
        executor_builder = self.add_default_handlers(executor_builder, config)?;

        // Then, override with any custom handlers
        for (effect_type, handler) in self.custom_handlers {
            executor_builder = executor_builder.with_handler(effect_type, Arc::from(handler));
        }

        Ok(executor_builder.build())
    }

    /// Build the effect executor using container for dependency injection
    async fn build_executor_with_container(
        self,
        config: &EffectSystemConfig,
        container: Arc<EffectContainer>,
    ) -> AuraResult<EffectExecutor> {
        let mut executor_builder = EffectExecutorBuilder::new();
        let mode = config.execution_mode;

        // Try to resolve handlers from container first
        // If not found, fall back to default handlers

        // For now, we'll use a simplified approach
        // In a full implementation, we'd have a more sophisticated container integration
        // that can resolve handlers by their trait implementations

        // For now, we'll still use the default handler approach
        // In a full implementation, we'd resolve all handlers from the container
        executor_builder = self.add_default_handlers(executor_builder, config)?;

        // Override with any custom handlers
        for (effect_type, handler) in self.custom_handlers {
            executor_builder = executor_builder.with_handler(effect_type, Arc::from(handler));
        }

        Ok(executor_builder.build())
    }
    */

    /*
    /// Add default handlers based on execution mode
    fn add_default_handlers(
        &self,
        mut builder: EffectExecutorBuilder,
        config: &EffectSystemConfig,
    ) -> AuraResult<EffectExecutorBuilder> {
        use aura_effects::{
            console::MockConsoleHandler, crypto::MockCryptoHandler, journal::MockJournalHandler,
            random::MockRandomHandler, storage::MemoryStorageHandler, time::SimulatedTimeHandler,
            transport::InMemoryTransportHandler,
        };

        let mode = config.execution_mode;

        match mode {
            ExecutionMode::Testing => {
                // Add test handlers if not already customized
                if !self.custom_handlers.contains_key(&EffectType::Crypto) {
                    builder = builder.with_handler(
                        EffectType::Crypto,
                        Arc::new(CryptoHandlerAdapter::new(
                            MockCryptoHandler::with_seed(0),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Network) {
                    builder = builder.with_handler(
                        EffectType::Network,
                        Arc::new(NetworkHandlerAdapter::new(
                            InMemoryTransportHandler::new(
                                aura_effects::transport::TransportConfig::default(),
                            ),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Storage) {
                    builder = builder.with_handler(
                        EffectType::Storage,
                        Arc::new(StorageHandlerAdapter::new(
                            MemoryStorageHandler::new(),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Time) {
                    builder = builder.with_handler(
                        EffectType::Time,
                        Arc::new(TimeHandlerAdapter::new(
                            SimulatedTimeHandler::new_at_epoch(),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Console) {
                    builder = builder.with_handler(
                        EffectType::Console,
                        Arc::new(ConsoleHandlerAdapter::new(MockConsoleHandler::new(), mode)),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Random) {
                    builder = builder.with_handler(
                        EffectType::Random,
                        Arc::new(RandomHandlerAdapter::new(
                            MockRandomHandler::new_with_seed(0),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Journal) {
                    builder = builder.with_handler(
                        EffectType::Journal,
                        Arc::new(JournalHandlerAdapter::new(MockJournalHandler::new(), mode)),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::System) {
                    builder = builder.with_handler(
                        EffectType::System,
                        Arc::new(SystemHandlerAdapter::new(
                            crate::handlers::system::LoggingSystemHandler::default(),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Ledger) {
                    builder = builder.with_handler(
                        EffectType::Ledger,
                        Arc::new(LedgerHandlerAdapter::new(
                            crate::handlers::ledger::memory::MemoryLedgerHandler::new(),
                            mode,
                        )),
                    );
                }
                if !self.custom_handlers.contains_key(&EffectType::Tree) {
                    builder = builder.with_handler(
                        EffectType::Tree,
                        Arc::new(TreeHandlerAdapter::new(
                            crate::handlers::tree::dummy::DummyTreeHandler::new(),
                            mode,
                        )),
                    );
                }
                if !self
                    .custom_handlers
                    .contains_key(&EffectType::Choreographic)
                {
                    builder = builder.with_handler(
                        EffectType::Choreographic,
                        Arc::new(ChoreographicHandlerAdapter::new(
                            crate::handlers::choreographic::memory::MemoryChoreographicHandler::new(
                                config.device_id.0,
                            ),
                            mode,
                        )),
                    );
                }
            }
            ExecutionMode::Production => {
                // Production handlers would be added here
                // For now, reuse test handlers with a warning
                tracing::warn!(
                    "Using test handlers in production mode - replace with real implementations"
                );
                return self.add_default_handlers(
                    builder,
                    &EffectSystemConfig {
                        execution_mode: ExecutionMode::Testing,
                        ..config.clone()
                    },
                );
            }
            ExecutionMode::Simulation { seed } => {
                // Simulation handlers would be added here
                // For now, reuse test handlers with deterministic seed
                if !self.custom_handlers.contains_key(&EffectType::Random) {
                    builder = builder.with_handler(
                        EffectType::Random,
                        Arc::new(RandomHandlerAdapter::new(
                            MockRandomHandler::new_with_seed(0),
                            mode,
                        )),
                    );
                }
                // Add other simulation-specific handlers
                return self.add_default_handlers(
                    builder,
                    &EffectSystemConfig {
                        execution_mode: ExecutionMode::Testing,
                        ..config.clone()
                    },
                );
            }
        }

        Ok(builder)
    }
    */
}

impl Default for AuraEffectSystemBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::{ TestFixture};

    #[test]
    fn test_builder_basic() {
        let device_id = DeviceId::new();
        let builder = AuraEffectSystemBuilder::new()
            .with_device_id(device_id)
            .with_execution_mode(ExecutionMode::Testing);

        let system = builder.build_sync().unwrap();
        assert_eq!(system.device_id(), device_id);
        assert_eq!(system.execution_mode(), ExecutionMode::Testing);
    }

    #[aura_test]
    async fn test_builder_async() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let device_id = fixture.device_id();
        let system = AuraEffectSystemBuilder::new()
            .with_device_id(device_id)
            .with_execution_mode(ExecutionMode::Testing)
            .with_default_flow_limit(5000)
            .build()
            .await?;

        assert_eq!(system.device_id(), device_id);
        Ok(())
    }

    #[test]
    fn test_builder_with_config() {
        let device_id = DeviceId::new();
        let config = EffectSystemConfig::for_testing(device_id);

        let system = AuraEffectSystemBuilder::new()
            .with_config(config.clone())
            .build_sync()
            .unwrap();

        assert_eq!(system.device_id(), device_id);
        assert_eq!(system.execution_mode(), ExecutionMode::Testing);
    }

    #[test]
    fn test_builder_custom_handler() {
        use aura_effects::crypto::MockCryptoHandler;

        let device_id = DeviceId::new();
        let custom_crypto = MockCryptoHandler::with_seed(12345);

        let system = AuraEffectSystemBuilder::new()
            .with_device_id(device_id)
            .with_handler(
                EffectType::Crypto,
                Box::new(CryptoHandlerAdapter::new(
                    custom_crypto,
                    ExecutionMode::Testing,
                )),
            )
            .build_sync()
            .unwrap();

        assert_eq!(system.device_id(), device_id);
    }

    #[test]
    fn test_builder_error_missing_device_id() {
        let result = AuraEffectSystemBuilder::new().build_sync();
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(err.to_string().contains("Device ID is required"));
        }
    }
}
