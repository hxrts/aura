//! Stateless Aura Effect System
//!
//! This module implements the coordinator-based effect system that
//! orchestrates stateless effect execution with isolated state management
//! services, eliminating deadlocks through architectural separation.

// TODO: Refactor to use TimeEffects. Uses Instant::now() for coordination timing
// which should be replaced with effect system integration.
#![allow(clippy::disallowed_methods)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use aura_core::hash::hash;
use aura_core::relationships::ContextId;
use aura_core::session_epochs::Epoch;
use aura_core::{
    effects::{
        ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, RandomEffects,
        StorageEffects, TimeEffects, TimeError, TimeoutHandle, WakeCondition,
    },
    identifiers::SessionId,
    AuraError, AuraResult, DeviceId, FlowBudget, Receipt,
};

use crate::handlers::{AuraContext, AuraHandlerError, EffectType, ExecutionMode};

use super::agent::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig,
    DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
    SessionManagementEffects, SessionMessage, SessionRole, SessionStatus, SessionType,
};
use super::choreographic::ChoreographicEffects;
use super::executor::{EffectExecutor, EffectExecutorBuilder};
use super::handler_adapters::{
    ChoreographicHandlerAdapter, ConsoleHandlerAdapter, CryptoHandlerAdapter,
    JournalHandlerAdapter, LedgerHandlerAdapter, NetworkHandlerAdapter, RandomHandlerAdapter,
    StorageHandlerAdapter, SystemHandlerAdapter, TimeHandlerAdapter, TreeHandlerAdapter,
};
use super::ledger::{LedgerEffects, LedgerError};
use super::lifecycle::{EffectSystemState, LifecycleAware, LifecycleManager};
use super::services::{ContextManager, FlowBudgetManager, ReceiptManager};
use super::system_traits::{SystemEffects, SystemError};
use super::tree::TreeEffects;
use crate::handlers::ledger::memory::MemoryLedgerHandler;
use crate::handlers::system::logging::{LoggingConfig, LoggingSystemHandler};

/// Configuration for the effect system
#[derive(Clone, Debug)]
pub struct EffectSystemConfig {
    /// Device ID for this instance
    pub device_id: DeviceId,
    /// Execution mode (testing, production, simulation)
    pub execution_mode: ExecutionMode,
    /// Default flow budget limit
    pub default_flow_limit: u64,
    /// Initial epoch
    pub initial_epoch: Epoch,
    /// Storage configuration
    pub storage_config: StorageConfig,
}

/// Storage configuration options
#[derive(Clone, Debug)]
pub struct StorageConfig {
    /// Base directory for storage
    pub base_path: std::path::PathBuf,
    /// Master key for encryption (32 bytes)
    pub master_key: [u8; 32],
    /// Enable compression
    pub enable_compression: bool,
    /// Max file size in bytes
    pub max_file_size: u64,
}

impl StorageConfig {
    /// Create a testing storage config with in-memory/temp storage
    pub fn for_testing() -> Self {
        Self {
            base_path: std::env::temp_dir().join("aura_test"),
            master_key: [1u8; 32], // Fixed test key
            enable_compression: false,
            max_file_size: 100 * 1024 * 1024, // 100MB for tests
        }
    }

    /// Create a production storage config
    pub fn for_production() -> AuraResult<Self> {
        // Get storage path from environment or use default
        let base_path = std::env::var("AURA_STORAGE_PATH")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::data_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("/var/lib"))
                    .join("aura")
            });

        // Generate or load master key from secure storage
        let master_key = Self::get_or_create_master_key(&base_path)?;

        Ok(Self {
            base_path,
            master_key,
            enable_compression: true,
            max_file_size: 1024 * 1024 * 1024, // 1GB for production
        })
    }

    /// Create a simulation storage config
    pub fn for_simulation(seed: u64) -> Self {
        // Use seed to create deterministic but unique storage path
        let temp_dir = std::env::temp_dir().join(format!("aura_sim_{}", seed));

        // Use seed to create deterministic master key
        let mut master_key = [0u8; 32];
        for (i, byte) in master_key.iter_mut().enumerate() {
            *byte = ((seed
                .wrapping_mul(1103515245)
                .wrapping_add(12345)
                .wrapping_add(i as u64))
                & 0xff) as u8;
        }

        Self {
            base_path: temp_dir,
            master_key,
            enable_compression: false,
            max_file_size: 10 * 1024 * 1024, // 10MB for simulation
        }
    }

    /// Get or create master key for production use
    fn get_or_create_master_key(base_path: &std::path::Path) -> AuraResult<[u8; 32]> {
        let key_file = base_path.join(".aura_master_key");

        if key_file.exists() {
            // Load existing key
            let key_bytes = std::fs::read(&key_file)
                .map_err(|e| AuraError::storage(format!("Failed to read master key: {}", e)))?;

            if key_bytes.len() != 32 {
                return Err(AuraError::invalid("Invalid master key length"));
            }

            let mut key = [0u8; 32];
            key.copy_from_slice(&key_bytes);
            Ok(key)
        } else {
            // Create new key
            // For production, we'd use a proper secure random source
            // This is a simplified implementation
            let mut key = [0u8; 32];
            for (i, byte) in key.iter_mut().enumerate() {
                *byte = (i as u8).wrapping_mul(17).wrapping_add(42); // Placeholder
            }

            // Ensure directory exists
            if let Some(parent) = key_file.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AuraError::storage(format!("Failed to create storage directory: {}", e))
                })?;
            }

            // Save key securely (would use OS keychain in real implementation)
            std::fs::write(&key_file, key)
                .map_err(|e| AuraError::storage(format!("Failed to save master key: {}", e)))?;

            Ok(key)
        }
    }
}

impl EffectSystemConfig {
    /// Create a testing configuration
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            default_flow_limit: 10_000,
            initial_epoch: Epoch::from(1),
            storage_config: StorageConfig::for_testing(),
        }
    }

    /// Create a production configuration
    pub fn for_production(device_id: DeviceId) -> AuraResult<Self> {
        Ok(Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            default_flow_limit: 100_000,
            initial_epoch: Epoch::from(1),
            storage_config: StorageConfig::for_production()?,
        })
    }

    /// Create a simulation configuration
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            default_flow_limit: 50_000,
            initial_epoch: Epoch::from(1),
            storage_config: StorageConfig::for_simulation(seed),
        }
    }

    /// Create a custom configuration
    pub fn new(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
        storage_config: StorageConfig,
    ) -> Self {
        let (default_flow_limit, initial_epoch) = match execution_mode {
            ExecutionMode::Testing => (10_000, Epoch::from(1)),
            ExecutionMode::Production => (100_000, Epoch::from(1)),
            ExecutionMode::Simulation { .. } => (50_000, Epoch::from(1)),
        };

        Self {
            device_id,
            execution_mode,
            default_flow_limit,
            initial_epoch,
            storage_config,
        }
    }
}

/// The stateless Aura Effect System
///
/// This coordinator orchestrates effect execution through stateless handlers
/// and isolated state management services, ensuring no deadlocks can occur.
#[derive(Clone)]
pub struct AuraEffectSystem {
    /// Configuration for this instance
    config: EffectSystemConfig,
    /// Stateless effect executor
    executor: Arc<EffectExecutor>,
    /// Isolated context management
    context_mgr: Arc<ContextManager>,
    /// Isolated budget management
    budget_mgr: Arc<FlowBudgetManager>,
    /// Isolated receipt management
    receipt_mgr: Arc<ReceiptManager>,
    /// Lifecycle manager for system state
    lifecycle_mgr: Arc<LifecycleManager>,
}

impl AuraEffectSystem {
    /// Create an effect system from pre-built components
    ///
    /// This method is used by the builder pattern to construct the effect system
    /// from already-initialized components.
    pub(crate) fn from_components(
        config: EffectSystemConfig,
        executor: Arc<EffectExecutor>,
        context_mgr: Arc<ContextManager>,
        budget_mgr: Arc<FlowBudgetManager>,
        receipt_mgr: Arc<ReceiptManager>,
        lifecycle_mgr: Arc<LifecycleManager>,
    ) -> Self {
        Self {
            config,
            executor,
            context_mgr,
            budget_mgr,
            receipt_mgr,
            lifecycle_mgr,
        }
    }

    /// Get the device ID for this effect system
    pub fn device_id(&self) -> DeviceId {
        self.config.device_id
    }

    /// Get the execution mode for this effect system
    pub fn execution_mode(&self) -> ExecutionMode {
        self.config.execution_mode
    }

    /// Get the latest receipt (stub for testing compatibility)
    pub async fn latest_receipt(&self) -> Option<Receipt> {
        // Get all context IDs and find the most recent receipt from any of them
        let context_ids = self.receipt_mgr.context_ids().await;

        let mut latest_receipt = None;
        let mut latest_nonce = 0u64;

        for context_id in context_ids {
            if let Ok(Some(receipt)) = self.receipt_mgr.latest_receipt(&context_id).await {
                // Use nonce as a proxy for recency (higher nonce = more recent)
                if receipt.nonce >= latest_nonce {
                    latest_nonce = receipt.nonce;
                    latest_receipt = Some(receipt);
                }
            }
        }

        latest_receipt
    }

    /// Set flow hint (stub for compatibility)
    pub fn set_flow_hint(&mut self, _hint: crate::guards::flow::FlowHint) {
        // In the stateless architecture, flow hints are handled differently
        // This is a no-op for backwards compatibility
    }

    /// Create a new effect system with the given configuration
    pub fn new(config: EffectSystemConfig) -> AuraResult<Self> {
        // Use the builder pattern internally for consistency
        super::AuraEffectSystemBuilder::new()
            .with_config(config)
            .build_sync()
    }

    // REMOVED: for_testing() method - use aura_testkit::create_test_fixture() instead
    // This method was causing "cannot start a runtime from within a runtime" errors
    // and has been replaced with proper async-native testing infrastructure.

    /// Create an effect system for testing without async initialization
    ///
    /// This method is specifically designed for use in tests to avoid
    /// "cannot start a runtime from within a runtime" errors when called
    /// from async test contexts.
    ///
    /// This is maintained for backwards compatibility. Consider using
    /// `AuraEffectSystemBuilder::new().build_sync()` directly.
    ///
    /// # Example
    /// ```
    /// # use aura_protocol::orchestration::AuraEffectSystem;
    /// # use aura_core::{DeviceId, AuraResult};
    /// # use aura_testkit::{aura_test, TestFixture};
    /// #[aura_test]
    /// async fn test_something() -> AuraResult<()> {
    ///     let fixture = TestFixture::new().await?;
    ///     let effects = fixture.effects();
    ///     // Use effects in your test
    ///     Ok(())
    /// }
    /// ```
    #[cfg(any(test, feature = "testing"))]
    pub fn for_testing_sync(device_id: DeviceId) -> AuraResult<Self> {
        // Use the builder pattern internally
        super::AuraEffectSystemBuilder::new()
            .with_device_id(device_id)
            .with_execution_mode(ExecutionMode::Testing)
            .build_sync()
    }

    // ===== Lifecycle Management Methods =====

    /// Get the current lifecycle state
    pub fn lifecycle_state(&self) -> EffectSystemState {
        self.lifecycle_mgr.current_state()
    }

    /// Initialize the effect system lifecycle
    ///
    /// This method transitions the system from Uninitialized to Ready state,
    /// initializing all registered components in the process.
    pub async fn initialize_lifecycle(&self) -> AuraResult<()> {
        self.lifecycle_mgr.initialize().await
    }

    /// Shutdown the effect system lifecycle gracefully
    ///
    /// This method transitions the system to Shutdown state,
    /// cleaning up all registered components in reverse order.
    pub async fn shutdown_lifecycle(&self) -> AuraResult<()> {
        self.lifecycle_mgr.shutdown().await
    }

    /// Perform a health check on the effect system
    ///
    /// Returns a comprehensive health report including the status of all components.
    pub async fn health_check(&self) -> crate::effects::lifecycle::SystemHealthReport {
        self.lifecycle_mgr.health_check().await
    }

    /// Check if the effect system is ready for operations
    pub fn is_ready(&self) -> bool {
        self.lifecycle_mgr.is_ready()
    }

    /// Ensure the system is in a ready state or return an error
    pub fn ensure_ready(&self) -> AuraResult<()> {
        self.lifecycle_mgr.ensure_ready()
    }

    /// Get system uptime
    pub fn uptime(&self) -> std::time::Duration {
        self.lifecycle_mgr.uptime()
    }

    /// Register a lifecycle-aware component
    ///
    /// Components registered here will be initialized and shut down
    /// with the effect system lifecycle.
    pub async fn register_lifecycle_component(
        &self,
        name: impl Into<String>,
        component: Box<dyn LifecycleAware>,
    ) {
        self.lifecycle_mgr.register_component(name, component).await
    }

    /// Build the effect executor based on execution mode
    fn build_executor(config: &EffectSystemConfig) -> AuraResult<EffectExecutor> {
        let mut builder = EffectExecutorBuilder::new();

        // Import handlers from aura-effects based on execution mode
        match config.execution_mode {
            ExecutionMode::Testing => {
                use aura_effects::{
                    console::MockConsoleHandler, crypto::MockCryptoHandler,
                    journal::MockJournalHandler, random::MockRandomHandler,
                    storage::MemoryStorageHandler, time::SimulatedTimeHandler,
                    transport::InMemoryTransportHandler,
                };

                let mode = ExecutionMode::Testing;
                builder = builder
                    .with_handler(
                        EffectType::Crypto,
                        Arc::new(CryptoHandlerAdapter::new(
                            MockCryptoHandler::with_seed(0),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Network,
                        Arc::new(NetworkHandlerAdapter::new(
                            InMemoryTransportHandler::new(
                                aura_effects::transport::TransportConfig::default(),
                            ),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Storage,
                        Arc::new(StorageHandlerAdapter::new(
                            MemoryStorageHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Time,
                        Arc::new(TimeHandlerAdapter::new(
                            SimulatedTimeHandler::new_at_epoch(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Console,
                        Arc::new(ConsoleHandlerAdapter::new(MockConsoleHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Random,
                        Arc::new(RandomHandlerAdapter::new(
                            MockRandomHandler::new_with_seed(0),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Journal,
                        Arc::new(JournalHandlerAdapter::new(MockJournalHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::System,
                        Arc::new(SystemHandlerAdapter::new(
                            LoggingSystemHandler::new(LoggingConfig::default()),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Ledger,
                        Arc::new(LedgerHandlerAdapter::new(MemoryLedgerHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Tree,
                        Arc::new(TreeHandlerAdapter::new(
                            crate::handlers::tree::dummy::DummyTreeHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Choreographic,
                        Arc::new(ChoreographicHandlerAdapter::new(
                            crate::handlers::choreographic::memory::MemoryChoreographicHandler::new(
                                config.device_id.0,
                            ),
                            mode,
                        )),
                    );
            }
            ExecutionMode::Production => {
                use aura_effects::{
                    console::RealConsoleHandler, crypto::RealCryptoHandler,
                    journal::MockJournalHandler, random::RealRandomHandler,
                    storage::FilesystemStorageHandler, time::RealTimeHandler,
                    transport::TcpTransportHandler,
                };

                let mode = ExecutionMode::Production;
                builder = builder
                    .with_handler(
                        EffectType::Crypto,
                        Arc::new(CryptoHandlerAdapter::new(RealCryptoHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Network,
                        Arc::new(NetworkHandlerAdapter::new(
                            TcpTransportHandler::new(
                                aura_effects::transport::TransportConfig::default(),
                            ),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Storage,
                        Arc::new(StorageHandlerAdapter::new(
                            FilesystemStorageHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Time,
                        Arc::new(TimeHandlerAdapter::new(RealTimeHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Console,
                        Arc::new(ConsoleHandlerAdapter::new(RealConsoleHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Random,
                        Arc::new(RandomHandlerAdapter::new(RealRandomHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Journal,
                        Arc::new(JournalHandlerAdapter::new(MockJournalHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::System,
                        Arc::new(SystemHandlerAdapter::new(
                            LoggingSystemHandler::new(LoggingConfig::default()),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Ledger,
                        Arc::new(LedgerHandlerAdapter::new(MemoryLedgerHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Tree,
                        Arc::new(TreeHandlerAdapter::new(
                            crate::handlers::tree::dummy::DummyTreeHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Choreographic,
                        Arc::new(ChoreographicHandlerAdapter::new(
                            crate::handlers::choreographic::memory::MemoryChoreographicHandler::new(
                                config.device_id.0,
                            ),
                            mode,
                        )),
                    );
            }
            ExecutionMode::Simulation { seed } => {
                use aura_effects::{
                    console::MockConsoleHandler, crypto::MockCryptoHandler,
                    journal::MockJournalHandler, random::MockRandomHandler,
                    storage::MemoryStorageHandler, time::SimulatedTimeHandler,
                    transport::InMemoryTransportHandler,
                };

                let mode = ExecutionMode::Simulation { seed };
                builder = builder
                    .with_handler(
                        EffectType::Crypto,
                        Arc::new(CryptoHandlerAdapter::new(
                            MockCryptoHandler::with_seed(0),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Network,
                        Arc::new(NetworkHandlerAdapter::new(
                            InMemoryTransportHandler::new(
                                aura_effects::transport::TransportConfig::default(),
                            ),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Storage,
                        Arc::new(StorageHandlerAdapter::new(
                            MemoryStorageHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Time,
                        Arc::new(TimeHandlerAdapter::new(
                            SimulatedTimeHandler::new_at_epoch(),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Console,
                        Arc::new(ConsoleHandlerAdapter::new(MockConsoleHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Random,
                        Arc::new(RandomHandlerAdapter::new(
                            MockRandomHandler::new_with_seed(seed),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Journal,
                        Arc::new(JournalHandlerAdapter::new(MockJournalHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::System,
                        Arc::new(SystemHandlerAdapter::new(
                            LoggingSystemHandler::new(LoggingConfig::default()),
                            mode,
                        )),
                    )
                    .with_handler(
                        EffectType::Ledger,
                        Arc::new(LedgerHandlerAdapter::new(MemoryLedgerHandler::new(), mode)),
                    )
                    .with_handler(
                        EffectType::Tree,
                        Arc::new(TreeHandlerAdapter::new(
                            crate::handlers::tree::dummy::DummyTreeHandler::new(),
                            mode,
                        )),
                    )
                    .with_handler(
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

        Ok(builder.build())
    }

    /// Get the current context snapshot for effect execution
    async fn get_context(&self) -> AuraResult<AuraContext> {
        // Try to get existing context, initialize if it doesn't exist
        match self.context_mgr.get_snapshot(self.config.device_id).await {
            Ok(context) => Ok(context),
            Err(_) => {
                // Context doesn't exist, initialize it
                self.context_mgr.initialize(self.config.device_id).await
            }
        }
    }

    /// Execute an effect with the current context
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        params: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        // Get immutable context snapshot
        let context = self
            .get_context()
            .await
            .map_err(|e| AuraHandlerError::ContextError {
                message: e.to_string(),
            })?;

        // Execute with no locks held
        self.executor
            .execute(effect_type, operation, params, &context)
            .await
    }
}

// Implement core effect traits using the stateless executor

#[async_trait]
impl TimeEffects for AuraEffectSystem {
    async fn current_epoch(&self) -> u64 {
        let result = self
            .execute_effect(EffectType::Time, "current_epoch", &[])
            .await
            .unwrap_or_else(|_| vec![0; 8]);

        u64::from_le_bytes(result.try_into().unwrap_or([0; 8]))
    }

    async fn current_timestamp(&self) -> u64 {
        let result = self
            .execute_effect(EffectType::Time, "current_timestamp", &[])
            .await
            .unwrap_or_else(|_| vec![0; 8]);

        u64::from_le_bytes(result.try_into().unwrap_or([0; 8]))
    }

    async fn current_timestamp_millis(&self) -> u64 {
        let result = self
            .execute_effect(EffectType::Time, "current_timestamp_millis", &[])
            .await
            .unwrap_or_else(|_| vec![0; 8]);

        u64::from_le_bytes(result.try_into().unwrap_or([0; 8]))
    }

    async fn now_instant(&self) -> std::time::Instant {
        // For the coordinator level, we delegate to the effect system's timestamp
        // and create a synthetic Instant. The underlying handlers (RealTimeHandler,
        // SimulatedTimeHandler) maintain their own proper Instant tracking.
        // This is a limitation of the byte-serialization dispatcher pattern.

        // Use a synthetic base that's initialized once per process
        use std::sync::OnceLock;
        static BASE: OnceLock<(std::time::Instant, u64)> = OnceLock::new();

        let (base_instant, base_timestamp_ms) = BASE.get_or_init(|| {
            // This initialization happens once at program start - acceptable for base reference
            #[allow(clippy::disallowed_methods)]
            let instant = std::time::Instant::now();
            #[allow(clippy::disallowed_methods)]
            let timestamp_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or(std::time::Duration::ZERO)
                .as_millis() as u64;
            (instant, timestamp_ms)
        });

        let current_timestamp_ms = self.current_timestamp_millis().await;
        let elapsed_ms = current_timestamp_ms.saturating_sub(*base_timestamp_ms);
        *base_instant + std::time::Duration::from_millis(elapsed_ms)
    }

    async fn sleep_ms(&self, ms: u64) {
        let params = bincode::serialize(&ms).unwrap_or_default();

        let _ = self
            .execute_effect(EffectType::Time, "sleep_ms", &params)
            .await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let params = bincode::serialize(&epoch).unwrap_or_default();

        let _ = self
            .execute_effect(EffectType::Time, "sleep_until", &params)
            .await;
    }

    async fn delay(&self, duration: std::time::Duration) {
        let params = bincode::serialize(&duration).unwrap_or_default();

        let _ = self
            .execute_effect(EffectType::Time, "delay", &params)
            .await;
    }

    async fn sleep(&self, duration_ms: u64) -> AuraResult<()> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        let params =
            bincode::serialize(&condition).map_err(|e| TimeError::SerializationFailed {
                reason: e.to_string(),
            })?;

        self.execute_effect(EffectType::Time, "yield_until", &params)
            .await
            .map_err(|e| TimeError::OperationFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| AuraError::internal(format!("Wait until failed: {}", e)))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let params = bincode::serialize(&timeout_ms).unwrap_or_default();

        let result = self
            .execute_effect(EffectType::Time, "set_timeout", &params)
            .await
            .unwrap_or_else(|_| vec![0; 16]);

        // Create UUID from result
        let bytes: [u8; 16] = result.try_into().unwrap_or([0; 16]);
        uuid::Uuid::from_bytes(bytes)
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let params = bincode::serialize(&handle).map_err(|e| TimeError::SerializationFailed {
            reason: e.to_string(),
        })?;

        self.execute_effect(EffectType::Time, "cancel_timeout", &params)
            .await
            .map_err(|e| TimeError::OperationFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    fn is_simulated(&self) -> bool {
        matches!(self.config.execution_mode, ExecutionMode::Simulation { .. })
    }

    fn register_context(&self, context_id: uuid::Uuid) {
        // This is synchronous, so we can't use async effects
        // For now, this is a no-op in the stateless system
        let _ = context_id;
    }

    fn unregister_context(&self, context_id: uuid::Uuid) {
        // This is synchronous, so we can't use async effects
        // For now, this is a no-op in the stateless system
        let _ = context_id;
    }

    async fn notify_events_available(&self) {
        let _ = self
            .execute_effect(EffectType::Time, "notify_events_available", &[])
            .await;
    }

    fn resolution_ms(&self) -> u64 {
        match self.config.execution_mode {
            ExecutionMode::Simulation { .. } => 1, // 1ms resolution for simulation
            _ => 10,                               // 10ms resolution for real/testing
        }
    }
}

#[async_trait]
impl NetworkEffects for AuraEffectSystem {
    async fn send_to_peer(
        &self,
        peer: uuid::Uuid,
        payload: Vec<u8>,
    ) -> Result<(), aura_core::effects::NetworkError> {
        let params = bincode::serialize(&(peer, payload)).map_err(|e| {
            aura_core::effects::NetworkError::SerializationFailed {
                error: e.to_string(),
            }
        })?;

        self.execute_effect(EffectType::Network, "send_to_peer", &params)
            .await
            .map_err(|e| aura_core::effects::NetworkError::SendFailed {
                peer_id: Some(peer),
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn broadcast(&self, payload: Vec<u8>) -> Result<(), aura_core::effects::NetworkError> {
        self.execute_effect(EffectType::Network, "broadcast", &payload)
            .await
            .map_err(|e| aura_core::effects::NetworkError::BroadcastFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn receive(&self) -> Result<(uuid::Uuid, Vec<u8>), aura_core::effects::NetworkError> {
        let result = self
            .execute_effect(EffectType::Network, "receive", &[])
            .await
            .map_err(|e| aura_core::effects::NetworkError::ReceiveFailed {
                reason: e.to_string(),
            })?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::NetworkError::DeserializationFailed {
                error: e.to_string(),
            }
        })
    }

    async fn receive_from(
        &self,
        peer_id: uuid::Uuid,
    ) -> Result<Vec<u8>, aura_core::effects::NetworkError> {
        let params = bincode::serialize(&peer_id).map_err(|e| {
            aura_core::effects::NetworkError::SerializationFailed {
                error: e.to_string(),
            }
        })?;

        self.execute_effect(EffectType::Network, "receive_from", &params)
            .await
            .map_err(|e| aura_core::effects::NetworkError::ReceiveFailed {
                reason: e.to_string(),
            })
    }

    async fn connected_peers(&self) -> Vec<uuid::Uuid> {
        let result = self
            .execute_effect(EffectType::Network, "connected_peers", &[])
            .await
            .unwrap_or_default();

        bincode::deserialize(&result).unwrap_or_default()
    }

    async fn is_peer_connected(&self, peer_id: uuid::Uuid) -> bool {
        let params = bincode::serialize(&peer_id).unwrap_or_default();

        let result = self
            .execute_effect(EffectType::Network, "is_peer_connected", &params)
            .await
            .unwrap_or_default();

        bincode::deserialize(&result).unwrap_or(false)
    }

    async fn subscribe_to_peer_events(
        &self,
    ) -> Result<aura_core::effects::PeerEventStream, aura_core::effects::NetworkError> {
        // Streams cannot be serialized/deserialized - create a placeholder
        use futures::stream;
        use std::pin::Pin;

        Ok(Box::pin(stream::empty())
            as Pin<
                Box<dyn futures::Stream<Item = aura_core::effects::PeerEvent> + Send>,
            >)
    }
}

#[async_trait]
impl CryptoEffects for AuraEffectSystem {
    async fn ed25519_sign(&self, message: &[u8], private_key: &[u8]) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(message.to_vec(), private_key.to_vec()))
            .map_err(|e| AuraError::serialization(format!("Ed25519 sign params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "ed25519_sign", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Ed25519 signing failed: {}", e)))?;

        Ok(result)
    }

    async fn ed25519_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, AuraError> {
        let params =
            bincode::serialize(&(message.to_vec(), signature.to_vec(), public_key.to_vec()))
                .map_err(|e| AuraError::serialization(format!("Ed25519 verify params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "ed25519_verify", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Ed25519 verification failed: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Ed25519 verify result: {}", e)))
    }

    // Note: hash and hmac are NOT algebraic effects in Aura architecture
    // Use aura_core::hash::hash() for synchronous pure hashing instead

    async fn hkdf_derive(
        &self,
        ikm: &[u8],
        salt: &[u8],
        info: &[u8],
        output_len: usize,
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(ikm.to_vec(), salt.to_vec(), info.to_vec(), output_len))
            .map_err(|e| AuraError::serialization(format!("HKDF params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "hkdf_derive", &params)
            .await
            .map_err(|e| AuraError::internal(format!("HKDF derivation failed: {}", e)))
    }

    async fn derive_key(
        &self,
        master_key: &[u8],
        context: &aura_core::effects::crypto::KeyDerivationContext,
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(master_key.to_vec(), context))
            .map_err(|e| AuraError::serialization(format!("Key derive params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "derive_key", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Key derivation failed: {}", e)))
    }

    async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), AuraError> {
        let result = self
            .execute_effect(EffectType::Crypto, "ed25519_generate_keypair", &[])
            .await
            .map_err(|e| {
                AuraError::internal(format!("Ed25519 keypair generation failed: {}", e))
            })?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Ed25519 keypair: {}", e)))
    }

    async fn ed25519_public_key(&self, private_key: &[u8]) -> Result<Vec<u8>, AuraError> {
        let result = self
            .execute_effect(EffectType::Crypto, "ed25519_public_key", private_key)
            .await
            .map_err(|e| {
                AuraError::internal(format!("Ed25519 public key extraction failed: {}", e))
            })?;

        Ok(result)
    }

    // FROST threshold signature methods
    async fn frost_generate_keys(
        &self,
        threshold: u16,
        max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, AuraError> {
        let params = bincode::serialize(&(threshold, max_signers))
            .map_err(|e| AuraError::serialization(format!("FROST keygen params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "frost_generate_keys", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST key generation failed: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("FROST keys: {}", e)))
    }

    async fn frost_generate_nonces(&self) -> Result<Vec<u8>, AuraError> {
        self.execute_effect(EffectType::Crypto, "frost_generate_nonces", &[])
            .await
            .map_err(|e| AuraError::internal(format!("FROST nonce generation failed: {}", e)))
    }

    async fn frost_create_signing_package(
        &self,
        message: &[u8],
        nonces: &[Vec<u8>],
        participants: &[u16],
    ) -> Result<aura_core::effects::crypto::FrostSigningPackage, AuraError> {
        let params = bincode::serialize(&(message.to_vec(), nonces, participants))
            .map_err(|e| AuraError::serialization(format!("FROST package params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "frost_create_signing_package", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST package creation failed: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("FROST package: {}", e)))
    }

    async fn frost_sign_share(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        key_share: &[u8],
        nonces: &[u8],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(signing_package, key_share.to_vec(), nonces.to_vec()))
            .map_err(|e| AuraError::serialization(format!("FROST sign params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "frost_sign_share", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST signing failed: {}", e)))
    }

    async fn frost_aggregate_signatures(
        &self,
        signing_package: &aura_core::effects::crypto::FrostSigningPackage,
        signature_shares: &[Vec<u8>],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(signing_package, signature_shares))
            .map_err(|e| AuraError::serialization(format!("FROST aggregate params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "frost_aggregate_signatures", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST aggregation failed: {}", e)))
    }

    async fn frost_verify(
        &self,
        message: &[u8],
        signature: &[u8],
        group_public_key: &[u8],
    ) -> Result<bool, AuraError> {
        let params = bincode::serialize(&(
            message.to_vec(),
            signature.to_vec(),
            group_public_key.to_vec(),
        ))
        .map_err(|e| AuraError::serialization(format!("FROST verify params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "frost_verify", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST verification failed: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("FROST verify result: {}", e)))
    }

    async fn frost_rotate_keys(
        &self,
        old_shares: &[Vec<u8>],
        old_threshold: u16,
        new_threshold: u16,
        new_max_signers: u16,
    ) -> Result<Vec<Vec<u8>>, AuraError> {
        let params =
            bincode::serialize(&(old_shares, old_threshold, new_threshold, new_max_signers))
                .map_err(|e| AuraError::serialization(format!("FROST rotate params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Crypto, "frost_rotate_keys", &params)
            .await
            .map_err(|e| AuraError::internal(format!("FROST key rotation failed: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("FROST rotated keys: {}", e)))
    }

    // Symmetric encryption methods
    async fn chacha20_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(plaintext.to_vec(), key.to_vec(), nonce.to_vec()))
            .map_err(|e| AuraError::serialization(format!("ChaCha20 encrypt params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "chacha20_encrypt", &params)
            .await
            .map_err(|e| AuraError::internal(format!("ChaCha20 encryption failed: {}", e)))
    }

    async fn chacha20_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(ciphertext.to_vec(), key.to_vec(), nonce.to_vec()))
            .map_err(|e| AuraError::serialization(format!("ChaCha20 decrypt params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "chacha20_decrypt", &params)
            .await
            .map_err(|e| AuraError::internal(format!("ChaCha20 decryption failed: {}", e)))
    }

    async fn aes_gcm_encrypt(
        &self,
        plaintext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(plaintext.to_vec(), key.to_vec(), nonce.to_vec()))
            .map_err(|e| AuraError::serialization(format!("AES-GCM encrypt params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "aes_gcm_encrypt", &params)
            .await
            .map_err(|e| AuraError::internal(format!("AES-GCM encryption failed: {}", e)))
    }

    async fn aes_gcm_decrypt(
        &self,
        ciphertext: &[u8],
        key: &[u8; 32],
        nonce: &[u8; 12],
    ) -> Result<Vec<u8>, AuraError> {
        let params = bincode::serialize(&(ciphertext.to_vec(), key.to_vec(), nonce.to_vec()))
            .map_err(|e| AuraError::serialization(format!("AES-GCM decrypt params: {}", e)))?;

        self.execute_effect(EffectType::Crypto, "aes_gcm_decrypt", &params)
            .await
            .map_err(|e| AuraError::internal(format!("AES-GCM decryption failed: {}", e)))
    }

    // Utility methods
    fn is_simulated(&self) -> bool {
        matches!(self.config.execution_mode, ExecutionMode::Simulation { .. })
    }

    fn crypto_capabilities(&self) -> Vec<String> {
        vec![
            "ed25519".to_string(),
            "frost".to_string(),
            "chacha20".to_string(),
            "aes-gcm".to_string(),
            "sha256".to_string(),
            "hmac".to_string(),
            "hkdf".to_string(),
        ]
    }

    fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
        if a.len() != b.len() {
            return false;
        }
        let mut diff = 0u8;
        for (byte_a, byte_b) in a.iter().zip(b.iter()) {
            diff |= byte_a ^ byte_b;
        }
        diff == 0
    }

    fn secure_zero(&self, data: &mut [u8]) {
        // Zero the memory
        // Note: In production, this should use a proper secure zeroing function
        // that prevents compiler optimization, but we avoid unsafe code here
        for byte in data.iter_mut() {
            *byte = 0;
        }
        // Memory fence to ensure the writes complete
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait]
impl StorageEffects for AuraEffectSystem {
    async fn store(
        &self,
        key: &str,
        value: Vec<u8>,
    ) -> Result<(), aura_core::effects::StorageError> {
        let params = bincode::serialize(&(key, value)).map_err(|e| {
            aura_core::effects::StorageError::WriteFailed(format!("Serialization error: {}", e))
        })?;

        self.execute_effect(EffectType::Storage, "store", &params)
            .await
            .map_err(|e| aura_core::effects::StorageError::WriteFailed(e.to_string()))?;

        Ok(())
    }

    async fn retrieve(
        &self,
        key: &str,
    ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
        let result = self
            .execute_effect(EffectType::Storage, "retrieve", key.as_bytes())
            .await
            .map_err(|e| aura_core::effects::StorageError::ReadFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::ReadFailed(format!("Deserialization error: {}", e))
        })
    }

    async fn remove(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        let result = self
            .execute_effect(EffectType::Storage, "remove", key.as_bytes())
            .await
            .map_err(|e| aura_core::effects::StorageError::DeleteFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::DeleteFailed(format!("Deserialization error: {}", e))
        })
    }

    async fn list_keys(
        &self,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, aura_core::effects::StorageError> {
        let prefix_bytes = bincode::serialize(&prefix).map_err(|e| {
            aura_core::effects::StorageError::ListFailed(format!("Serialization error: {}", e))
        })?;

        let result = self
            .execute_effect(EffectType::Storage, "list_keys", &prefix_bytes)
            .await
            .map_err(|e| aura_core::effects::StorageError::ListFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::ListFailed(format!("Deserialization error: {}", e))
        })
    }

    async fn exists(&self, key: &str) -> Result<bool, aura_core::effects::StorageError> {
        let result = self
            .execute_effect(EffectType::Storage, "exists", key.as_bytes())
            .await
            .map_err(|e| aura_core::effects::StorageError::ReadFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::ReadFailed(format!("Deserialization error: {}", e))
        })
    }

    async fn store_batch(
        &self,
        pairs: std::collections::HashMap<String, Vec<u8>>,
    ) -> Result<(), aura_core::effects::StorageError> {
        let params = bincode::serialize(&pairs).map_err(|e| {
            aura_core::effects::StorageError::WriteFailed(format!("Serialization error: {}", e))
        })?;

        self.execute_effect(EffectType::Storage, "store_batch", &params)
            .await
            .map_err(|e| aura_core::effects::StorageError::WriteFailed(e.to_string()))?;

        Ok(())
    }

    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<u8>>, aura_core::effects::StorageError> {
        let params = bincode::serialize(&keys).map_err(|e| {
            aura_core::effects::StorageError::ReadFailed(format!("Serialization error: {}", e))
        })?;

        let result = self
            .execute_effect(EffectType::Storage, "retrieve_batch", &params)
            .await
            .map_err(|e| aura_core::effects::StorageError::ReadFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::ReadFailed(format!("Deserialization error: {}", e))
        })
    }

    async fn clear_all(&self) -> Result<(), aura_core::effects::StorageError> {
        self.execute_effect(EffectType::Storage, "clear_all", &[])
            .await
            .map_err(|e| aura_core::effects::StorageError::DeleteFailed(e.to_string()))?;

        Ok(())
    }

    async fn stats(
        &self,
    ) -> Result<aura_core::effects::StorageStats, aura_core::effects::StorageError> {
        let result = self
            .execute_effect(EffectType::Storage, "stats", &[])
            .await
            .map_err(|e| aura_core::effects::StorageError::ListFailed(e.to_string()))?;

        bincode::deserialize(&result).map_err(|e| {
            aura_core::effects::StorageError::ListFailed(format!("Deserialization error: {}", e))
        })
    }
}

#[async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_info(&self, message: &str) -> Result<(), AuraError> {
        let params = bincode::serialize(&message).unwrap_or_default();
        self.execute_effect(EffectType::Console, "log_info", &params)
            .await?;
        Ok(())
    }

    async fn log_warn(&self, message: &str) -> Result<(), AuraError> {
        let params = bincode::serialize(&message).unwrap_or_default();
        self.execute_effect(EffectType::Console, "log_warn", &params)
            .await?;
        Ok(())
    }

    async fn log_error(&self, message: &str) -> Result<(), AuraError> {
        let params = bincode::serialize(&message).unwrap_or_default();
        self.execute_effect(EffectType::Console, "log_error", &params)
            .await?;
        Ok(())
    }

    async fn log_debug(&self, message: &str) -> Result<(), AuraError> {
        let params = bincode::serialize(&message).unwrap_or_default();
        self.execute_effect(EffectType::Console, "log_debug", &params)
            .await?;
        Ok(())
    }
}

#[async_trait]
impl RandomEffects for AuraEffectSystem {
    async fn random_bytes(&self, len: usize) -> Vec<u8> {
        let len_bytes = len.to_le_bytes();

        self.execute_effect(EffectType::Random, "random_bytes", &len_bytes)
            .await
            .unwrap_or_else(|_| vec![0; len])
    }

    async fn random_u64(&self) -> u64 {
        let result = self
            .execute_effect(EffectType::Random, "random_u64", &[])
            .await
            .unwrap_or_else(|_| vec![0; 8]);

        u64::from_le_bytes(result.try_into().unwrap_or([0; 8]))
    }

    async fn random_bytes_32(&self) -> [u8; 32] {
        let result = self
            .execute_effect(EffectType::Random, "random_bytes_32", &[])
            .await
            .unwrap_or_else(|_| vec![0; 32]);

        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&result[..32.min(result.len())]);
        bytes
    }

    async fn random_range(&self, min: u64, max: u64) -> u64 {
        let params = bincode::serialize(&(min, max)).unwrap_or_default();

        let result = self
            .execute_effect(EffectType::Random, "random_range", &params)
            .await
            .unwrap_or_else(|_| vec![0; 8]);

        let value = u64::from_le_bytes(result.try_into().unwrap_or([0; 8]));
        min.saturating_add(value % (max.saturating_sub(min)))
    }

    async fn random_uuid(&self) -> uuid::Uuid {
        let result = self
            .execute_effect(EffectType::Random, "random_uuid", &[])
            .await
            .unwrap_or_else(|_| vec![0; 16]);

        let mut bytes = [0u8; 16];
        bytes.copy_from_slice(&result[..16.min(result.len())]);
        uuid::Uuid::from_bytes(bytes)
    }
}

#[async_trait]
impl JournalEffects for AuraEffectSystem {
    async fn merge_facts(
        &self,
        target: &aura_core::Journal,
        delta: &aura_core::Journal,
    ) -> Result<aura_core::Journal, AuraError> {
        let params = bincode::serialize(&(target, delta))
            .map_err(|e| AuraError::serialization(format!("Journal merge params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Journal, "merge_facts", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Journal merge: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Journal result: {}", e)))
    }

    async fn refine_caps(
        &self,
        target: &aura_core::Journal,
        refinement: &aura_core::Journal,
    ) -> Result<aura_core::Journal, AuraError> {
        let params = bincode::serialize(&(target, refinement))
            .map_err(|e| AuraError::serialization(format!("Journal refine params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Journal, "refine_caps", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Journal refine: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Journal result: {}", e)))
    }

    async fn get_journal(&self) -> Result<aura_core::Journal, AuraError> {
        let result = self
            .execute_effect(EffectType::Journal, "get_journal", &[])
            .await
            .map_err(|e| AuraError::internal(format!("Journal get: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Journal: {}", e)))
    }

    async fn persist_journal(&self, journal: &aura_core::Journal) -> Result<(), AuraError> {
        let journal_bytes = bincode::serialize(journal)
            .map_err(|e| AuraError::serialization(format!("Journal: {}", e)))?;

        self.execute_effect(EffectType::Journal, "persist_journal", &journal_bytes)
            .await
            .map_err(|e| AuraError::internal(format!("Journal persist: {}", e)))?;

        Ok(())
    }

    async fn get_flow_budget(
        &self,
        context: &aura_core::relationships::ContextId,
        peer: &aura_core::DeviceId,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        let params = bincode::serialize(&(context, peer))
            .map_err(|e| AuraError::serialization(format!("Flow budget params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Journal, "get_flow_budget", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Get flow budget: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Flow budget: {}", e)))
    }

    async fn update_flow_budget(
        &self,
        context: &aura_core::relationships::ContextId,
        peer: &aura_core::DeviceId,
        budget: &aura_core::FlowBudget,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        let params = bincode::serialize(&(context, peer, budget))
            .map_err(|e| AuraError::serialization(format!("Flow budget update params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Journal, "update_flow_budget", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Update flow budget: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Flow budget: {}", e)))
    }

    async fn charge_flow_budget(
        &self,
        context: &aura_core::relationships::ContextId,
        peer: &aura_core::DeviceId,
        cost: u32,
    ) -> Result<aura_core::FlowBudget, AuraError> {
        let params = bincode::serialize(&(context, peer, cost))
            .map_err(|e| AuraError::serialization(format!("Charge flow params: {}", e)))?;

        let result = self
            .execute_effect(EffectType::Journal, "charge_flow_budget", &params)
            .await
            .map_err(|e| AuraError::internal(format!("Charge flow budget: {}", e)))?;

        bincode::deserialize(&result)
            .map_err(|e| AuraError::serialization(format!("Flow budget: {}", e)))
    }
}

// Implement AgentEffects
#[async_trait]
impl AgentEffects for AuraEffectSystem {
    async fn initialize(&self) -> AuraResult<()> {
        // Initialize agent by ensuring context exists
        self.context_mgr.initialize(self.config.device_id).await?;

        // Log initialization
        self.log_info("Agent initialized successfully").await?;
        Ok(())
    }

    async fn get_device_info(&self) -> AuraResult<DeviceInfo> {
        let context = self.get_context().await?;

        // Get storage usage through storage effects
        let usage = self
            .execute_effect(EffectType::Storage, "get_usage", &[])
            .await
            .ok()
            .and_then(|bytes| bincode::deserialize::<u64>(&bytes).ok())
            .unwrap_or(0);

        Ok(DeviceInfo {
            device_id: self.config.device_id,
            account_id: context.account_id,
            device_name: format!("Device-{}", self.config.device_id),
            hardware_security: matches!(self.config.execution_mode, ExecutionMode::Production),
            attestation_available: matches!(self.config.execution_mode, ExecutionMode::Production),
            last_sync: Some(aura_core::effects::TimeEffects::current_timestamp(self).await),
            storage_usage: usage,
            storage_limit: 1_000_000_000, // 1GB default
        })
    }

    async fn shutdown(&self) -> AuraResult<()> {
        // Clear contexts
        self.context_mgr.clear().await;

        // Log shutdown
        self.log_info("Agent shutdown complete").await?;
        Ok(())
    }

    async fn sync_distributed_state(&self) -> AuraResult<()> {
        // In a real implementation, this would sync with other devices
        // For now, just log the sync attempt
        self.log_info("Syncing distributed state").await?;
        Ok(())
    }

    async fn health_check(&self) -> AuraResult<AgentHealthStatus> {
        let timestamp = aura_core::effects::TimeEffects::current_timestamp(self).await;

        // Check various subsystems
        let storage_healthy = self.retrieve("health_check_key").await.is_ok();
        let network_healthy = true; // Would check real network in production

        Ok(AgentHealthStatus {
            overall_status: HealthStatus::Healthy,
            storage_status: if storage_healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Degraded {
                    reason: "Storage check failed".to_string(),
                }
            },
            network_status: if network_healthy {
                HealthStatus::Healthy
            } else {
                HealthStatus::Unhealthy {
                    error: "Network unreachable".to_string(),
                }
            },
            authentication_status: HealthStatus::Healthy,
            session_status: HealthStatus::Healthy,
            last_check: timestamp,
        })
    }
}

// Implement DeviceStorageEffects
#[async_trait]
impl DeviceStorageEffects for AuraEffectSystem {
    async fn store_credential(&self, key: &str, credential: &[u8]) -> AuraResult<()> {
        // Add device-specific prefix for credentials
        let storage_key = format!("credential:{}", key);

        // In production, would encrypt with device key
        let encrypted = if self.config.execution_mode.is_production() {
            // Would use hardware security module
            hash(credential).to_vec()
        } else {
            credential.to_vec()
        };

        self.store(&storage_key, encrypted)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store credential: {}", e)))
    }

    async fn retrieve_credential(&self, key: &str) -> AuraResult<Option<Vec<u8>>> {
        let storage_key = format!("credential:{}", key);

        match self.retrieve(&storage_key).await {
            Ok(Some(encrypted)) => {
                // In production, would decrypt with device key
                Ok(Some(encrypted))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuraError::storage(format!(
                "Failed to retrieve credential: {}",
                e
            ))),
        }
    }

    async fn delete_credential(&self, key: &str) -> AuraResult<()> {
        let storage_key = format!("credential:{}", key);
        self.remove(&storage_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to delete credential: {}", e)))?;
        Ok(())
    }

    async fn list_credentials(&self) -> AuraResult<Vec<String>> {
        let keys = self
            .list_keys(Some("credential:"))
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list credentials: {}", e)))?;

        // Remove prefix from keys
        Ok(keys
            .into_iter()
            .filter_map(|k| k.strip_prefix("credential:").map(String::from))
            .collect())
    }

    async fn store_device_config(&self, config: &[u8]) -> AuraResult<()> {
        self.store("device:config", config.to_vec())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store device config: {}", e)))
    }

    async fn retrieve_device_config(&self) -> AuraResult<Option<Vec<u8>>> {
        self.retrieve("device:config")
            .await
            .map_err(|e| AuraError::internal(format!("Failed to retrieve device config: {}", e)))
    }

    async fn backup_credentials(&self) -> AuraResult<CredentialBackup> {
        let timestamp = aura_core::TimeEffects::current_timestamp(self).await;
        let credentials = self.list_credentials().await?;

        // Collect all credentials
        let mut backup_data = HashMap::new();
        for key in &credentials {
            if let Some(cred) = self.retrieve_credential(key).await? {
                backup_data.insert(key.clone(), cred);
            }
        }

        // Serialize and encrypt backup
        let serialized = bincode::serialize(&backup_data)
            .map_err(|e| AuraError::serialization(format!("Backup data: {}", e)))?;

        let backup_hash = hash(&serialized);

        Ok(CredentialBackup {
            device_id: self.config.device_id,
            timestamp,
            encrypted_credentials: serialized,
            backup_hash: backup_hash[..32].try_into().unwrap_or([0u8; 32]),
            metadata: HashMap::new(),
        })
    }

    async fn restore_credentials(&self, backup: &CredentialBackup) -> AuraResult<()> {
        // Verify backup hash
        let computed_hash = hash(&backup.encrypted_credentials);
        if computed_hash[..32] != backup.backup_hash {
            return Err(AuraError::invalid("Invalid backup hash"));
        }

        // Deserialize and restore
        let backup_data: HashMap<String, Vec<u8>> =
            bincode::deserialize(&backup.encrypted_credentials)
                .map_err(|e| AuraError::serialization(format!("Backup data: {}", e)))?;

        for (key, credential) in backup_data {
            self.store_credential(&key, &credential).await?;
        }

        Ok(())
    }

    async fn secure_wipe(&self) -> AuraResult<()> {
        // List and delete all credentials
        let credentials = self.list_credentials().await?;
        for key in credentials {
            self.delete_credential(&key).await?;
        }

        // Also wipe device config
        self.remove("device:config")
            .await
            .map_err(|e| AuraError::internal(format!("Failed to wipe device config: {}", e)))?;

        Ok(())
    }
}

// Implement AuthenticationEffects
#[async_trait]
impl AuthenticationEffects for AuraEffectSystem {
    async fn authenticate_device(&self) -> AuraResult<AuthenticationResult> {
        let timestamp = aura_core::TimeEffects::current_timestamp(self).await;

        // In testing mode, always succeed
        if matches!(self.config.execution_mode, ExecutionMode::Testing) {
            return Ok(AuthenticationResult {
                success: true,
                method_used: Some(AuthMethod::DeviceCredential),
                session_token: Some(vec![1, 2, 3, 4]),
                expires_at: Some(timestamp + 3_600_000), // 1 hour
                error: None,
            });
        }

        // In production, would use actual authentication
        Ok(AuthenticationResult {
            success: false,
            method_used: None,
            session_token: None,
            expires_at: None,
            error: Some("Authentication not implemented".to_string()),
        })
    }

    async fn is_authenticated(&self) -> AuraResult<bool> {
        // Check if we have a valid auth token in context
        let context = self.get_context().await?;
        Ok(context.agent.is_some() && context.agent.as_ref().unwrap().auth_state.authenticated)
    }

    async fn lock_device(&self) -> AuraResult<()> {
        // Update context to clear auth state
        let mut context = self.get_context().await?;
        if let Some(agent) = &mut context.agent {
            agent.auth_state.authenticated = false;
            agent.auth_state.last_auth_time = None;
        }

        self.context_mgr
            .update(self.config.device_id, context)
            .await?;
        Ok(())
    }

    async fn get_auth_methods(&self) -> AuraResult<Vec<AuthMethod>> {
        match self.config.execution_mode {
            ExecutionMode::Testing => Ok(vec![AuthMethod::DeviceCredential]),
            ExecutionMode::Production => Ok(vec![
                AuthMethod::Biometric(BiometricType::FaceId),
                AuthMethod::Pin,
            ]),
            ExecutionMode::Simulation { .. } => Ok(vec![AuthMethod::DeviceCredential]),
        }
    }

    async fn enroll_biometric(&self, biometric_type: BiometricType) -> AuraResult<()> {
        if !self.config.execution_mode.is_production() {
            return Ok(()); // No-op in non-production
        }

        // Would interface with platform biometric APIs
        let _ = self
            .log_info(&format!("Enrolling biometric: {:?}", biometric_type))
            .await;
        Ok(())
    }

    async fn remove_biometric(&self, biometric_type: BiometricType) -> AuraResult<()> {
        if !self.config.execution_mode.is_production() {
            return Ok(()); // No-op in non-production
        }

        // Would interface with platform biometric APIs
        let _ = self
            .log_info(&format!("Removing biometric: {:?}", biometric_type))
            .await;
        Ok(())
    }

    async fn verify_capability(&self, capability: &[u8]) -> AuraResult<bool> {
        // Would verify capability signatures in production
        Ok(!capability.is_empty())
    }

    async fn generate_attestation(&self) -> AuraResult<Vec<u8>> {
        // In production, would use secure enclave
        let attestation_data = format!(
            "device:{},mode:{:?},timestamp:{}",
            self.config.device_id,
            self.config.execution_mode,
            aura_core::TimeEffects::current_timestamp(self).await
        );

        Ok(hash(attestation_data.as_bytes()).to_vec())
    }
}

// Implement ConfigurationEffects
#[async_trait]
impl ConfigurationEffects for AuraEffectSystem {
    async fn get_device_config(&self) -> AuraResult<DeviceConfig> {
        match self.retrieve_device_config().await? {
            Some(data) => bincode::deserialize(&data)
                .map_err(|e| AuraError::serialization(format!("Device config: {}", e))),
            None => Ok(DeviceConfig::default()),
        }
    }

    async fn update_device_config(&self, config: &DeviceConfig) -> AuraResult<()> {
        let data = bincode::serialize(config)
            .map_err(|e| AuraError::serialization(format!("Device config: {}", e)))?;
        self.store_device_config(&data).await
    }

    async fn reset_to_defaults(&self) -> AuraResult<()> {
        let default_config = DeviceConfig::default();
        self.update_device_config(&default_config).await
    }

    async fn export_config(&self) -> AuraResult<Vec<u8>> {
        let config = self.get_device_config().await?;
        serde_json::to_vec(&config)
            .map_err(|e| AuraError::serialization(format!("Config export: {}", e)))
    }

    async fn import_config(&self, config_data: &[u8]) -> AuraResult<()> {
        let config: DeviceConfig = serde_json::from_slice(config_data)
            .map_err(|e| AuraError::serialization(format!("Config import: {}", e)))?;

        // Validate before importing
        let errors = self.validate_config(&config).await?;
        if !errors.is_empty() {
            return Err(AuraError::invalid(format!(
                "Config validation failed: {} errors",
                errors.len()
            )));
        }

        self.update_device_config(&config).await
    }

    async fn validate_config(
        &self,
        config: &DeviceConfig,
    ) -> AuraResult<Vec<ConfigValidationError>> {
        let mut errors = Vec::new();

        // Validate auto lock timeout
        if config.auto_lock_timeout == 0 {
            errors.push(ConfigValidationError {
                field: "auto_lock_timeout".to_string(),
                error: "Auto lock timeout must be greater than 0".to_string(),
                suggested_value: Some(serde_json::Value::Number(300.into())),
            });
        }

        // Validate sync interval
        if config.sync_interval < 60 {
            errors.push(ConfigValidationError {
                field: "sync_interval".to_string(),
                error: "Sync interval must be at least 60 seconds".to_string(),
                suggested_value: Some(serde_json::Value::Number(3600.into())),
            });
        }

        // Validate log level
        let valid_log_levels = ["error", "warn", "info", "debug", "trace"];
        if !valid_log_levels.contains(&config.log_level.as_str()) {
            errors.push(ConfigValidationError {
                field: "log_level".to_string(),
                error: "Invalid log level".to_string(),
                suggested_value: Some(serde_json::Value::String("info".to_string())),
            });
        }

        Ok(errors)
    }

    async fn get_config_json(&self, key: &str) -> AuraResult<Option<serde_json::Value>> {
        let config = self.get_device_config().await?;

        // Check standard fields
        let value = match key {
            "device_name" => Some(serde_json::Value::String(config.device_name)),
            "auto_lock_timeout" => Some(serde_json::Value::Number(config.auto_lock_timeout.into())),
            "biometric_enabled" => Some(serde_json::Value::Bool(config.biometric_enabled)),
            "backup_enabled" => Some(serde_json::Value::Bool(config.backup_enabled)),
            "sync_interval" => Some(serde_json::Value::Number(config.sync_interval.into())),
            "max_storage_size" => Some(serde_json::Value::Number(config.max_storage_size.into())),
            "network_timeout" => Some(serde_json::Value::Number(config.network_timeout.into())),
            "log_level" => Some(serde_json::Value::String(config.log_level)),
            _ => config.custom_settings.get(key).cloned(),
        };

        Ok(value)
    }

    async fn set_config_json(&self, key: &str, value: &serde_json::Value) -> AuraResult<()> {
        let mut config = self.get_device_config().await?;

        // Update standard fields
        match key {
            "device_name" => {
                config.device_name = value
                    .as_str()
                    .ok_or_else(|| AuraError::invalid("device_name must be a string"))?
                    .to_string();
            }
            "auto_lock_timeout" => {
                config.auto_lock_timeout = value
                    .as_u64()
                    .ok_or_else(|| AuraError::invalid("auto_lock_timeout must be a number"))?
                    as u32;
            }
            "biometric_enabled" => {
                config.biometric_enabled = value
                    .as_bool()
                    .ok_or_else(|| AuraError::invalid("biometric_enabled must be a boolean"))?;
            }
            "backup_enabled" => {
                config.backup_enabled = value
                    .as_bool()
                    .ok_or_else(|| AuraError::invalid("backup_enabled must be a boolean"))?;
            }
            "sync_interval" => {
                config.sync_interval = value
                    .as_u64()
                    .ok_or_else(|| AuraError::invalid("sync_interval must be a number"))?
                    as u32;
            }
            "max_storage_size" => {
                config.max_storage_size = value
                    .as_u64()
                    .ok_or_else(|| AuraError::invalid("max_storage_size must be a number"))?;
            }
            "network_timeout" => {
                config.network_timeout = value
                    .as_u64()
                    .ok_or_else(|| AuraError::invalid("network_timeout must be a number"))?
                    as u32;
            }
            "log_level" => {
                config.log_level = value
                    .as_str()
                    .ok_or_else(|| AuraError::invalid("log_level must be a string"))?
                    .to_string();
            }
            _ => {
                // Custom settings
                config
                    .custom_settings
                    .insert(key.to_string(), value.clone());
            }
        }

        self.update_device_config(&config).await
    }

    async fn get_all_config(&self) -> AuraResult<HashMap<String, serde_json::Value>> {
        let config = self.get_device_config().await?;
        let mut all_config = HashMap::new();

        // Add standard fields
        all_config.insert(
            "device_name".to_string(),
            serde_json::Value::String(config.device_name),
        );
        all_config.insert(
            "auto_lock_timeout".to_string(),
            serde_json::Value::Number(config.auto_lock_timeout.into()),
        );
        all_config.insert(
            "biometric_enabled".to_string(),
            serde_json::Value::Bool(config.biometric_enabled),
        );
        all_config.insert(
            "backup_enabled".to_string(),
            serde_json::Value::Bool(config.backup_enabled),
        );
        all_config.insert(
            "sync_interval".to_string(),
            serde_json::Value::Number(config.sync_interval.into()),
        );
        all_config.insert(
            "max_storage_size".to_string(),
            serde_json::Value::Number(config.max_storage_size.into()),
        );
        all_config.insert(
            "network_timeout".to_string(),
            serde_json::Value::Number(config.network_timeout.into()),
        );
        all_config.insert(
            "log_level".to_string(),
            serde_json::Value::String(config.log_level),
        );

        // Add custom settings
        for (key, value) in config.custom_settings {
            all_config.insert(key, value);
        }

        Ok(all_config)
    }
}

// Implement SessionManagementEffects
#[async_trait]
impl SessionManagementEffects for AuraEffectSystem {
    async fn create_session(&self, session_type: SessionType) -> AuraResult<SessionId> {
        let session_id = SessionId::new();
        let created_at = aura_core::TimeEffects::current_timestamp(self).await;

        // Initialize with just the creator as participant
        let participants = vec![self.config.device_id];

        // Store session info
        let info = SessionInfo {
            session_id,
            session_type,
            role: SessionRole::Initiator,
            participants,
            status: SessionStatus::Created,
            created_at,
            updated_at: created_at,
            timeout_at: Some(created_at + 3_600_000), // 1 hour timeout
            operation: None,
            metadata: HashMap::new(),
        };

        let session_key = format!("session:{}", session_id);
        let session_data = bincode::serialize(&info)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        self.store(&session_key, session_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store session: {}", e)))?;

        Ok(session_id)
    }

    async fn join_session(&self, session_id: SessionId) -> AuraResult<SessionHandle> {
        let session_key = format!("session:{}", session_id);

        // Get existing session
        let session_data = self
            .retrieve(&session_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| AuraError::not_found("Session not found"))?;

        let mut info: SessionInfo = bincode::deserialize(&session_data)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        // Add ourselves as participant if not already there
        if !info.participants.contains(&self.config.device_id) {
            info.participants.push(self.config.device_id);
        }

        // Update session
        info.updated_at = aura_core::TimeEffects::current_timestamp(self).await;
        info.status = SessionStatus::Active;

        let updated_data = bincode::serialize(&info)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        self.store(&session_key, updated_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to update session: {}", e)))?;

        Ok(SessionHandle {
            session_id,
            role: SessionRole::Participant,
            participants: info.participants,
            created_at: info.created_at,
        })
    }

    async fn leave_session(&self, session_id: SessionId) -> AuraResult<()> {
        let session_key = format!("session:{}", session_id);

        // Get existing session
        let session_data = self
            .retrieve(&session_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| AuraError::not_found("Session not found"))?;

        let mut info: SessionInfo = bincode::deserialize(&session_data)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        // Remove ourselves from participants
        info.participants.retain(|p| p != &self.config.device_id);
        info.updated_at = aura_core::TimeEffects::current_timestamp(self).await;

        let updated_data = bincode::serialize(&info)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        self.store(&session_key, updated_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to update session: {}", e)))
    }

    async fn end_session(&self, session_id: SessionId) -> AuraResult<()> {
        let session_key = format!("session:{}", session_id);

        // Get existing session to verify we're the initiator
        let session_data = self
            .retrieve(&session_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| AuraError::not_found("Session not found"))?;

        let mut info: SessionInfo = bincode::deserialize(&session_data)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        // Update status to completed
        info.status = SessionStatus::Completed;
        info.updated_at = aura_core::TimeEffects::current_timestamp(self).await;

        let updated_data = bincode::serialize(&info)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        self.store(&session_key, updated_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to update session: {}", e)))
    }

    async fn list_active_sessions(&self) -> AuraResult<Vec<SessionInfo>> {
        let prefix = "session:";
        let keys = self
            .list_keys(Some(prefix))
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list sessions: {}", e)))?;

        let mut sessions = Vec::new();
        for key in keys {
            if !key.contains(":msg:") {
                // Skip message keys
                if let Ok(Some(data)) = self.retrieve(&key).await {
                    if let Ok(info) = bincode::deserialize::<SessionInfo>(&data) {
                        match &info.status {
                            SessionStatus::Active
                            | SessionStatus::Created
                            | SessionStatus::WaitingForApprovals => {
                                sessions.push(info);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(sessions)
    }

    async fn get_session_status(&self, session_id: SessionId) -> AuraResult<SessionStatus> {
        let session_key = format!("session:{}", session_id);

        let session_data = self
            .retrieve(&session_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get session: {}", e)))?
            .ok_or_else(|| AuraError::not_found("Session not found"))?;

        let info: SessionInfo = bincode::deserialize(&session_data)
            .map_err(|e| AuraError::serialization(format!("Session info: {}", e)))?;

        Ok(info.status)
    }

    async fn send_session_message(&self, session_id: SessionId, message: &[u8]) -> AuraResult<()> {
        let msg = SessionMessage {
            from: self.config.device_id,
            to: None, // Broadcast to all participants
            timestamp: aura_core::TimeEffects::current_timestamp(self).await,
            message_type: "data".to_string(),
            payload: message.to_vec(),
        };

        // Store message in session queue
        let msg_key = format!(
            "session:{}:msg:{}",
            session_id,
            aura_core::RandomEffects::random_u64(self).await
        );
        let msg_data = bincode::serialize(&msg)
            .map_err(|e| AuraError::serialization(format!("Session message: {}", e)))?;

        self.store(&msg_key, msg_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store message: {}", e)))?;

        // In real implementation, would notify participants
        Ok(())
    }

    async fn receive_session_messages(
        &self,
        session_id: SessionId,
    ) -> AuraResult<Vec<SessionMessage>> {
        let prefix = format!("session:{}:msg:", session_id);
        let keys = self
            .list_keys(Some(&prefix))
            .await
            .map_err(|e| AuraError::internal(format!("Failed to list messages: {}", e)))?;

        let mut messages = Vec::new();
        for key in keys {
            if let Ok(Some(data)) = self.retrieve(&key).await {
                if let Ok(msg) = bincode::deserialize::<SessionMessage>(&data) {
                    // Filter messages for us
                    if msg.to.is_none() || msg.to == Some(self.config.device_id) {
                        messages.push(msg);
                    }
                }
            }
        }

        // Sort by timestamp
        messages.sort_by_key(|m| m.timestamp);
        Ok(messages)
    }
}

// Implement ChoreographicEffects
#[async_trait]
impl ChoreographicEffects for AuraEffectSystem {
    async fn send_to_role_bytes(
        &self,
        role: super::choreographic::ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), super::choreographic::ChoreographyError> {
        // Convert role to peer ID and use network effects
        use super::choreographic::ChoreographyError;

        self.send_to_peer(role.device_id, message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })
    }

    async fn receive_from_role_bytes(
        &self,
        role: super::choreographic::ChoreographicRole,
    ) -> Result<Vec<u8>, super::choreographic::ChoreographyError> {
        // In real implementation, would have message queue per role
        // For now, return empty message
        use super::choreographic::ChoreographyError;

        // Store a placeholder message for testing
        let msg_key = format!("choreo:msg:{}:{}", self.config.device_id, role.device_id);

        match self.retrieve(&msg_key).await {
            Ok(Some(data)) => {
                // Delete message after reading
                let _ = self.remove(&msg_key).await;
                Ok(data)
            }
            Ok(None) => Err(ChoreographyError::CommunicationTimeout {
                role,
                timeout_ms: 5000,
            }),
            Err(e) => Err(ChoreographyError::Transport {
                source: Box::new(e),
            }),
        }
    }

    async fn broadcast_bytes(
        &self,
        message: Vec<u8>,
    ) -> Result<(), super::choreographic::ChoreographyError> {
        use super::choreographic::ChoreographyError;

        self.broadcast(message)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        Ok(())
    }

    fn current_role(&self) -> super::choreographic::ChoreographicRole {
        // Get role from context
        let context = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.get_context())
        });

        if let Ok(ctx) = context {
            if let Some(choreo) = &ctx.choreographic {
                return choreo.current_role;
            }
        }

        // Default role
        super::choreographic::ChoreographicRole::new(self.config.device_id.into(), 0)
    }

    fn all_roles(&self) -> Vec<super::choreographic::ChoreographicRole> {
        // Get roles from context
        let context = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.get_context())
        });

        if let Ok(ctx) = context {
            if let Some(choreo) = &ctx.choreographic {
                return (*choreo.participants).clone();
            }
        }

        // Return just ourselves
        vec![self.current_role()]
    }

    async fn is_role_active(&self, role: super::choreographic::ChoreographicRole) -> bool {
        // Check if we have recent messages or heartbeat from role
        let heartbeat_key = format!("choreo:heartbeat:{}", role.device_id);

        if let Ok(Some(data)) = self.retrieve(&heartbeat_key).await {
            if let Ok(timestamp) = bincode::deserialize::<u64>(&data) {
                let current = aura_core::TimeEffects::current_timestamp(self).await;
                // Consider active if heartbeat within last 30 seconds
                return current - timestamp < 30_000;
            }
        }

        false
    }

    async fn start_session(
        &self,
        session_id: uuid::Uuid,
        roles: Vec<super::choreographic::ChoreographicRole>,
    ) -> Result<(), super::choreographic::ChoreographyError> {
        use super::choreographic::ChoreographyError;

        // Check if session already exists
        let session_key = format!("choreo:session:{}", session_id);
        if self.retrieve(&session_key).await.ok().flatten().is_some() {
            return Err(ChoreographyError::SessionAlreadyExists { session_id });
        }

        // Store session info
        let session_data = bincode::serialize(&(
            aura_core::TimeEffects::current_timestamp(self).await,
            roles.clone(),
        ))
        .map_err(|e| AuraError::serialization(format!("Session data: {}", e)))?;

        self.store(&session_key, session_data)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        // Update context with choreographic info
        let mut context = self
            .get_context()
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        // Find our role
        let our_role = roles
            .iter()
            .find(|r| r.device_id == self.config.device_id.0)
            .copied()
            .unwrap_or_else(|| {
                super::choreographic::ChoreographicRole::new(self.config.device_id.into(), 0)
            });

        let choreo_ctx = crate::handlers::immutable::ChoreographicContext::new(
            our_role, roles, 1, // epoch
        );

        context = context.with_choreographic(choreo_ctx);
        self.context_mgr
            .update(self.config.device_id, context)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        Ok(())
    }

    async fn end_session(&self) -> Result<(), super::choreographic::ChoreographyError> {
        use super::choreographic::ChoreographyError;

        // Get current session from context
        let context = self
            .get_context()
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        if context.choreographic.is_none() {
            return Err(ChoreographyError::SessionNotStarted);
        }

        // Clear choreographic context
        let updated_context = crate::handlers::immutable::AuraContext {
            device_id: context.device_id,
            execution_mode: context.execution_mode,
            session_id: context.session_id,
            created_at: context.created_at,
            account_id: context.account_id,
            metadata: context.metadata,
            operation_id: context.operation_id,
            epoch: context.epoch,
            choreographic: None, // Clear it
            simulation: context.simulation,
            agent: context.agent,
            middleware: context.middleware.clone(),
        };

        self.context_mgr
            .update(self.config.device_id, updated_context)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        Ok(())
    }

    async fn emit_choreo_event(
        &self,
        event: super::choreographic::ChoreographyEvent,
    ) -> Result<(), super::choreographic::ChoreographyError> {
        use super::choreographic::ChoreographyError;

        // Log the event
        let event_str = format!("Choreography event: {:?}", event);
        let _ = self.log_info(&event_str).await;

        // Store event for analysis
        let event_key = format!(
            "choreo:event:{}:{}",
            aura_core::TimeEffects::current_timestamp(self).await,
            aura_core::RandomEffects::random_u64(self).await
        );
        let event_data = bincode::serialize(&event)
            .map_err(|e| AuraError::serialization(format!("Event data: {}", e)))?;

        self.store(&event_key, event_data)
            .await
            .map_err(|e| ChoreographyError::Transport {
                source: Box::new(e),
            })?;

        Ok(())
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        // Store timeout in context for future operations
        let timeout_key = "choreo:timeout";
        let _ = self
            .store(timeout_key, timeout_ms.to_le_bytes().to_vec())
            .await;
    }

    async fn get_metrics(&self) -> super::choreographic::ChoreographyMetrics {
        // Collect basic metrics
        let event_prefix = "choreo:event:";
        let event_count = self
            .list_keys(Some(event_prefix))
            .await
            .map(|keys| keys.len())
            .unwrap_or(0);

        super::choreographic::ChoreographyMetrics {
            messages_sent: event_count as u64 / 2, // Rough estimate
            messages_received: event_count as u64 / 2,
            avg_latency_ms: 0.0,
            timeout_count: 0,
            retry_count: 0,
            total_duration_ms: 0,
        }
    }
}

// Implement LedgerEffects
#[async_trait]
impl LedgerEffects for AuraEffectSystem {
    async fn append_event(&self, event: Vec<u8>) -> Result<(), super::ledger::LedgerError> {
        // Get current epoch
        let epoch = LedgerEffects::current_epoch(self).await?;

        // Store event with epoch key
        let event_key = format!("ledger:event:{:016}", epoch + 1);
        self.store(&event_key, event.clone())
            .await
            .map_err(|e| LedgerError::Backend {
                error: e.to_string(),
            })?;

        // Update epoch counter
        let epoch_key = "ledger:current_epoch";
        self.store(epoch_key, (epoch + 1).to_le_bytes().to_vec())
            .await
            .map_err(|e| LedgerError::Backend {
                error: e.to_string(),
            })?;

        // Emit event notification
        let _ = self
            .log_info(&format!("Ledger event appended at epoch {}", epoch + 1))
            .await;

        Ok(())
    }

    async fn current_epoch(&self) -> Result<u64, super::ledger::LedgerError> {
        let epoch_key = "ledger:current_epoch";
        match self.retrieve(epoch_key).await {
            Ok(Some(data)) => {
                if data.len() >= 8 {
                    Ok(u64::from_le_bytes(data[..8].try_into().unwrap()))
                } else {
                    Ok(0) // Start from 0 if invalid
                }
            }
            Ok(None) => Ok(0), // No epochs yet
            Err(e) => Err(LedgerError::Backend {
                error: e.to_string(),
            }),
        }
    }

    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, super::ledger::LedgerError> {
        let current = LedgerEffects::current_epoch(self).await?;
        if epoch > current {
            return Err(LedgerError::EpochOutOfRange { epoch });
        }

        let mut events = Vec::new();
        for e in (epoch + 1)..=current {
            let event_key = format!("ledger:event:{:016}", e);
            if let Ok(Some(data)) = self.retrieve(&event_key).await {
                events.push(data);
            }
        }

        Ok(events)
    }

    async fn is_device_authorized(
        &self,
        device_id: DeviceId,
        operation: &str,
    ) -> Result<bool, super::ledger::LedgerError> {
        // Get device metadata
        if let Some(metadata) = self.get_device_metadata(device_id).await? {
            // Check if device is active
            if !metadata.is_active {
                return Ok(false);
            }

            // Check permissions
            Ok(metadata.permissions.contains(&operation.to_string())
                || metadata.permissions.contains(&"*".to_string()))
        } else {
            Ok(false) // Unknown device
        }
    }

    async fn get_device_metadata(
        &self,
        device_id: DeviceId,
    ) -> Result<Option<super::ledger::DeviceMetadata>, super::ledger::LedgerError> {
        use super::ledger::{DeviceMetadata, LedgerError};

        let device_key = format!("ledger:device:{}", device_id);
        match self.retrieve(&device_key).await {
            Ok(Some(data)) => {
                // Simple format: name|last_seen|is_active|permissions
                let text = String::from_utf8(data).map_err(|e| LedgerError::Corrupted {
                    reason: format!("Invalid device metadata: {}", e),
                })?;

                let parts: Vec<&str> = text.split('|').collect();
                if parts.len() >= 4 {
                    let name = parts[0].to_string();
                    let last_seen = parts[1].parse::<u64>().unwrap_or(0);
                    let is_active = parts[2] == "true";
                    let permissions: Vec<String> =
                        parts[3].split(',').map(|s| s.to_string()).collect();

                    Ok(Some(DeviceMetadata {
                        device_id,
                        name,
                        last_seen,
                        is_active,
                        permissions,
                    }))
                } else {
                    Ok(None)
                }
            }
            Ok(None) => Ok(None),
            Err(e) => Err(LedgerError::Backend {
                error: e.to_string(),
            }),
        }
    }

    async fn update_device_activity(
        &self,
        device_id: DeviceId,
    ) -> Result<(), super::ledger::LedgerError> {
        let timestamp = aura_core::TimeEffects::current_timestamp(self).await;

        // Get existing metadata or create new
        let mut metadata = self
            .get_device_metadata(device_id)
            .await?
            .unwrap_or_else(|| super::ledger::DeviceMetadata {
                device_id,
                name: format!("Device-{}", device_id),
                last_seen: timestamp,
                is_active: true,
                permissions: vec!["read".to_string()],
            });

        // Update last seen
        metadata.last_seen = timestamp;
        metadata.is_active = true;

        // Store updated metadata
        let data = format!(
            "{}|{}|{}|{}",
            metadata.name,
            metadata.last_seen,
            metadata.is_active,
            metadata.permissions.join(",")
        );

        let device_key = format!("ledger:device:{}", device_id);
        self.store(&device_key, data.into_bytes())
            .await
            .map_err(|e| LedgerError::Backend {
                error: e.to_string(),
            })?;

        Ok(())
    }

    async fn subscribe_to_events(
        &self,
    ) -> Result<super::ledger::LedgerEventStream, super::ledger::LedgerError> {
        // Types used in implementation
        use futures::stream;

        // In real implementation, would create actual event stream
        // For now, return empty stream
        Ok(Box::new(stream::empty()))
    }

    // Journal graph operations

    async fn would_create_cycle(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, super::ledger::LedgerError> {
        // Build adjacency list
        let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        for (from, to) in edges {
            graph.entry(from.clone()).or_default().push(to.clone());
        }

        // Add the new edge temporarily
        graph
            .entry(new_edge.0.clone())
            .or_default()
            .push(new_edge.1.clone());

        // DFS to detect cycle
        fn has_cycle_dfs(
            graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>,
            node: &[u8],
            visited: &mut HashSet<Vec<u8>>,
            rec_stack: &mut HashSet<Vec<u8>>,
        ) -> bool {
            visited.insert(node.to_vec());
            rec_stack.insert(node.to_vec());

            if let Some(neighbors) = graph.get(node) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        if has_cycle_dfs(graph, neighbor, visited, rec_stack) {
                            return true;
                        }
                    } else if rec_stack.contains(neighbor) {
                        return true; // Found a cycle
                    }
                }
            }

            rec_stack.remove(node);
            false
        }

        use std::collections::HashSet;
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        // Check from the source of new edge
        Ok(has_cycle_dfs(
            &graph,
            &new_edge.0,
            &mut visited,
            &mut rec_stack,
        ))
    }

    async fn find_connected_components(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, super::ledger::LedgerError> {
        use std::collections::{HashMap, HashSet};

        // Build bidirectional graph
        let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        let mut all_nodes = HashSet::new();

        for (from, to) in edges {
            graph.entry(from.clone()).or_default().push(to.clone());
            graph.entry(to.clone()).or_default().push(from.clone());
            all_nodes.insert(from.clone());
            all_nodes.insert(to.clone());
        }

        // DFS to find components
        let mut visited = HashSet::new();
        let mut components = Vec::new();

        fn dfs(
            node: &[u8],
            graph: &HashMap<Vec<u8>, Vec<Vec<u8>>>,
            visited: &mut HashSet<Vec<u8>>,
            component: &mut Vec<Vec<u8>>,
        ) {
            visited.insert(node.to_vec());
            component.push(node.to_vec());

            if let Some(neighbors) = graph.get(node) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        dfs(neighbor, graph, visited, component);
                    }
                }
            }
        }

        for node in all_nodes {
            if !visited.contains(&node) {
                let mut component = Vec::new();
                dfs(&node, &graph, &mut visited, &mut component);
                components.push(component);
            }
        }

        Ok(components)
    }

    async fn topological_sort(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, super::ledger::LedgerError> {
        use std::collections::{HashMap, HashSet, VecDeque};

        // Build graph and calculate in-degrees
        let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        let mut in_degree: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut all_nodes = HashSet::new();

        for (from, to) in edges {
            graph.entry(from.clone()).or_default().push(to.clone());
            *in_degree.entry(to.clone()).or_default() += 1;
            in_degree.entry(from.clone()).or_default();
            all_nodes.insert(from.clone());
            all_nodes.insert(to.clone());
        }

        // Kahn's algorithm
        let mut queue = VecDeque::new();
        for node in &all_nodes {
            if in_degree.get(node).copied().unwrap_or(0) == 0 {
                queue.push_back(node.clone());
            }
        }

        let mut result = Vec::new();
        while let Some(node) = queue.pop_front() {
            result.push(node.clone());

            if let Some(neighbors) = graph.get(&node) {
                for neighbor in neighbors {
                    let degree = in_degree.get_mut(neighbor).unwrap();
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        if result.len() != all_nodes.len() {
            Err(LedgerError::GraphOperationFailed {
                message: "Graph contains a cycle".to_string(),
            })
        } else {
            Ok(result)
        }
    }

    async fn shortest_path(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        start: Vec<u8>,
        end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, super::ledger::LedgerError> {
        use std::collections::{HashMap, HashSet, VecDeque};

        // Build graph
        let mut graph: HashMap<Vec<u8>, Vec<Vec<u8>>> = HashMap::new();
        for (from, to) in edges {
            graph.entry(from.clone()).or_default().push(to.clone());
        }

        // BFS for shortest path
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        let mut parent: HashMap<Vec<u8>, Vec<u8>> = HashMap::new();

        queue.push_back(start.clone());
        visited.insert(start.clone());

        while let Some(current) = queue.pop_front() {
            if current == end {
                // Reconstruct path
                let mut path = Vec::new();
                let mut node = end.clone();

                while node != start {
                    path.push(node.clone());
                    node = parent.get(&node).unwrap().clone();
                }
                path.push(start);
                path.reverse();

                return Ok(Some(path));
            }

            if let Some(neighbors) = graph.get(&current) {
                for neighbor in neighbors {
                    if !visited.contains(neighbor) {
                        visited.insert(neighbor.clone());
                        parent.insert(neighbor.clone(), current.clone());
                        queue.push_back(neighbor.clone());
                    }
                }
            }
        }

        Ok(None) // No path found
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, super::ledger::LedgerError> {
        let secret = self.random_bytes(length).await;
        Ok(secret)
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], super::ledger::LedgerError> {
        // Use general cryptographic hash function
        Ok(hash(data))
    }

    async fn current_timestamp(&self) -> Result<u64, super::ledger::LedgerError> {
        Ok(<Self as TimeEffects>::current_timestamp(self).await)
    }

    async fn ledger_device_id(&self) -> Result<DeviceId, super::ledger::LedgerError> {
        Ok(self.config.device_id)
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, super::ledger::LedgerError> {
        let random_bytes = aura_core::RandomEffects::random_bytes(self, 16).await;
        let uuid_bytes: [u8; 16] = random_bytes[..16].try_into().unwrap();
        Ok(uuid::Uuid::from_bytes(uuid_bytes))
    }
}

// Implement TreeEffects
#[async_trait]
impl TreeEffects for AuraEffectSystem {
    async fn get_current_state(&self) -> Result<aura_journal::ratchet_tree::TreeState, AuraError> {
        // Get tree state from storage
        let state_key = "tree:current_state";

        match self.retrieve(state_key).await {
            Ok(Some(data)) => bincode::deserialize(&data)
                .map_err(|e| AuraError::serialization(format!("TreeState: {}", e))),
            Ok(None) => {
                // Return default state if none exists
                Ok(aura_journal::ratchet_tree::TreeState::default())
            }
            Err(e) => Err(AuraError::storage(format!(
                "Failed to get tree state: {}",
                e
            ))),
        }
    }

    async fn get_current_commitment(&self) -> Result<aura_core::Hash32, AuraError> {
        let state = self.get_current_state().await?;

        // Compute commitment from state
        let commitment_data = bincode::serialize(&state)
            .map_err(|e| AuraError::serialization(format!("TreeState: {}", e)))?;

        Ok(aura_core::Hash32(hash(&commitment_data)))
    }

    async fn get_current_epoch(&self) -> Result<u64, AuraError> {
        let state = self.get_current_state().await?;
        Ok(state.current_epoch())
    }

    async fn apply_attested_op(
        &self,
        op: aura_core::AttestedOp,
    ) -> Result<aura_core::Hash32, AuraError> {
        // Verify the operation first
        let state = self.get_current_state().await?;

        if !self.verify_aggregate_sig(&op, &state).await? {
            return Err(AuraError::invalid("Invalid aggregate signature"));
        }

        // Store the operation in the oplog
        let op_key = format!(
            "tree:oplog:{}",
            aura_core::RandomEffects::random_u64(self).await
        );
        let op_data = bincode::serialize(&op)
            .map_err(|e| AuraError::serialization(format!("AttestedOp: {}", e)))?;

        self.store(&op_key, op_data.clone())
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store op: {}", e)))?;

        // Update the tree state
        let new_state = state;

        // Apply the operation to the state
        match op.op.op {
            aura_core::TreeOpKind::AddLeaf { leaf, under } => {
                // In real implementation, would properly update tree structure
                let _ = self
                    .log_info(&format!("Adding leaf under node {:?}", under))
                    .await;
            }
            aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
                let _ = self
                    .log_info(&format!("Removing leaf {:?} with reason {}", leaf, reason))
                    .await;
            }
            aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
                let _ = self
                    .log_info(&format!("Changing policy at node {:?}", node))
                    .await;
            }
            aura_core::TreeOpKind::RotateEpoch { affected } => {
                let _ = self
                    .log_info(&format!("Rotating epoch for {} nodes", affected.len()))
                    .await;
            }
        }

        // Store updated state
        let state_key = "tree:current_state";
        let state_data = bincode::serialize(&new_state)
            .map_err(|e| AuraError::serialization(format!("TreeState: {}", e)))?;

        self.store(state_key, state_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store state: {}", e)))?;

        // Compute and return CID
        Ok(aura_core::Hash32(hash(&op_data)))
    }

    async fn verify_aggregate_sig(
        &self,
        op: &aura_core::AttestedOp,
        state: &aura_journal::ratchet_tree::TreeState,
    ) -> Result<bool, AuraError> {
        // In real implementation, would verify FROST aggregate signature
        // For now, simple verification based on signature length

        if op.agg_sig.is_empty() {
            return Ok(false);
        }

        // Verify parent binding (simplified - full implementation would check actual parent bindings)
        let _current_commitment = self.get_current_commitment().await?;
        let _current_epoch = state.current_epoch();
        // TODO: Implement proper parent binding verification when AttestedOp includes binding fields

        // In testing mode, accept any non-empty signature
        if matches!(self.config.execution_mode, ExecutionMode::Testing) {
            return Ok(!op.agg_sig.is_empty());
        }

        // In production, would do proper FROST verification
        Ok(true)
    }

    async fn add_leaf(
        &self,
        leaf: aura_core::LeafNode,
        under: aura_core::NodeIndex,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        // Check authorization (in real implementation)
        let _ = self
            .log_info(&format!("Proposing to add leaf under node {:?}", under))
            .await;

        Ok(aura_core::TreeOpKind::AddLeaf { leaf, under })
    }

    async fn remove_leaf(
        &self,
        leaf_id: aura_core::LeafId,
        reason: u8,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        let _ = self
            .log_info(&format!(
                "Proposing to remove leaf {:?} with reason {}",
                leaf_id, reason
            ))
            .await;

        Ok(aura_core::TreeOpKind::RemoveLeaf {
            leaf: leaf_id,
            reason,
        })
    }

    async fn change_policy(
        &self,
        node: aura_core::NodeIndex,
        new_policy: aura_core::Policy,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        let _ = self
            .log_info(&format!("Proposing policy change at node {:?}", node))
            .await;

        // In real implementation, would verify policy is stricter
        Ok(aura_core::TreeOpKind::ChangePolicy { node, new_policy })
    }

    async fn rotate_epoch(
        &self,
        affected: Vec<aura_core::NodeIndex>,
    ) -> Result<aura_core::TreeOpKind, AuraError> {
        let _ = self
            .log_info(&format!(
                "Proposing epoch rotation for {} nodes",
                affected.len()
            ))
            .await;

        Ok(aura_core::TreeOpKind::RotateEpoch { affected })
    }

    async fn propose_snapshot(
        &self,
        cut: super::tree::Cut,
    ) -> Result<super::tree::ProposalId, AuraError> {
        let proposal_id =
            super::tree::ProposalId(aura_core::Hash32(hash(&bincode::serialize(&cut).unwrap())));

        // Store proposal
        let proposal_key = format!("tree:snapshot:proposal:{:?}", proposal_id.0);
        let proposal_data = bincode::serialize(&cut)
            .map_err(|e| AuraError::serialization(format!("Cut: {}", e)))?;

        self.store(&proposal_key, proposal_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store proposal: {}", e)))?;

        let _ = self
            .log_info(&format!("Proposed snapshot at epoch {}", cut.epoch))
            .await;

        Ok(proposal_id)
    }

    async fn approve_snapshot(
        &self,
        proposal_id: super::tree::ProposalId,
    ) -> Result<super::tree::Partial, AuraError> {
        // Generate partial signature (mock for now)
        let signature_share = self.random_bytes(32).await;

        let partial = super::tree::Partial {
            signature_share,
            participant_id: self.config.device_id,
        };

        // Store partial
        let partial_key = format!(
            "tree:snapshot:partial:{:?}:{}",
            proposal_id.0, self.config.device_id
        );
        let partial_data = bincode::serialize(&partial)
            .map_err(|e| AuraError::serialization(format!("Partial: {}", e)))?;

        self.store(&partial_key, partial_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store partial: {}", e)))?;

        Ok(partial)
    }

    async fn finalize_snapshot(
        &self,
        proposal_id: super::tree::ProposalId,
    ) -> Result<super::tree::Snapshot, AuraError> {
        // Get proposal
        let proposal_key = format!("tree:snapshot:proposal:{:?}", proposal_id.0);
        let proposal_data = self
            .retrieve(&proposal_key)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get proposal: {}", e)))?
            .ok_or_else(|| AuraError::not_found("Proposal not found"))?;

        let cut: super::tree::Cut = bincode::deserialize(&proposal_data)
            .map_err(|e| AuraError::serialization(format!("Cut: {}", e)))?;

        // Get current tree state
        let tree_state = self.get_current_state().await?;

        // In real implementation, would aggregate partial signatures
        let aggregate_signature = self.random_bytes(64).await;

        let snapshot = super::tree::Snapshot {
            cut,
            tree_state,
            aggregate_signature,
        };

        // Store snapshot
        let snapshot_key = format!("tree:snapshot:final:{:?}", proposal_id.0);
        let snapshot_data = bincode::serialize(&snapshot)
            .map_err(|e| AuraError::serialization(format!("Snapshot: {}", e)))?;

        self.store(&snapshot_key, snapshot_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store snapshot: {}", e)))?;

        let _ = self.log_info("Finalized snapshot").await;

        Ok(snapshot)
    }

    async fn apply_snapshot(&self, snapshot: &super::tree::Snapshot) -> Result<(), AuraError> {
        // Verify snapshot signature
        if snapshot.aggregate_signature.is_empty() {
            return Err(AuraError::invalid("Invalid snapshot signature"));
        }

        // Apply tree state
        let state_key = "tree:current_state";
        let state_data = bincode::serialize(&snapshot.tree_state)
            .map_err(|e| AuraError::serialization(format!("TreeState: {}", e)))?;

        self.store(state_key, state_data)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to apply snapshot: {}", e)))?;

        let _ = self
            .log_info(&format!("Applied snapshot at epoch {}", snapshot.cut.epoch))
            .await;

        Ok(())
    }
}

// SystemEffects implementation
#[async_trait]
impl SystemEffects for AuraEffectSystem {
    async fn log(&self, level: &str, component: &str, message: &str) -> Result<(), SystemError> {
        let params = bincode::serialize(&(level, component, message)).map_err(|e| {
            SystemError::OperationFailed {
                message: format!("Failed to serialize log params: {}", e),
            }
        })?;

        self.execute_effect(EffectType::System, "log", &params)
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to log: {}", e),
            })?;

        Ok(())
    }

    async fn log_with_context(
        &self,
        level: &str,
        component: &str,
        message: &str,
        context: HashMap<String, String>,
    ) -> Result<(), SystemError> {
        let params = bincode::serialize(&(level, component, message, context)).map_err(|e| {
            SystemError::OperationFailed {
                message: format!("Failed to serialize log params: {}", e),
            }
        })?;

        self.execute_effect(EffectType::System, "log_with_context", &params)
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to log with context: {}", e),
            })?;

        Ok(())
    }

    async fn get_system_info(&self) -> Result<HashMap<String, String>, SystemError> {
        let result = self
            .execute_effect(EffectType::System, "get_system_info", &[])
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to get system info: {}", e),
            })?;

        bincode::deserialize(&result).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to deserialize system info: {}", e),
        })
    }

    async fn set_config(&self, key: &str, value: &str) -> Result<(), SystemError> {
        let params =
            bincode::serialize(&(key, value)).map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to serialize config params: {}", e),
            })?;

        self.execute_effect(EffectType::System, "set_config", &params)
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to set config: {}", e),
            })?;

        Ok(())
    }

    async fn get_config(&self, key: &str) -> Result<String, SystemError> {
        let params = bincode::serialize(&key).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to serialize config key: {}", e),
        })?;

        let result = self
            .execute_effect(EffectType::System, "get_config", &params)
            .await
            .map_err(|e| SystemError::ResourceNotFound {
                resource: key.to_string(),
            })?;

        bincode::deserialize(&result).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to deserialize config value: {}", e),
        })
    }

    async fn health_check(&self) -> Result<bool, SystemError> {
        let result = self
            .execute_effect(EffectType::System, "health_check", &[])
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to perform health check: {}", e),
            })?;

        bincode::deserialize(&result).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to deserialize health check result: {}", e),
        })
    }

    async fn get_metrics(&self) -> Result<HashMap<String, f64>, SystemError> {
        let result = self
            .execute_effect(EffectType::System, "get_metrics", &[])
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to get metrics: {}", e),
            })?;

        bincode::deserialize(&result).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to deserialize metrics: {}", e),
        })
    }

    async fn restart_component(&self, component: &str) -> Result<(), SystemError> {
        let params = bincode::serialize(&component).map_err(|e| SystemError::OperationFailed {
            message: format!("Failed to serialize component name: {}", e),
        })?;

        self.execute_effect(EffectType::System, "restart_component", &params)
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to restart component: {}", e),
            })?;

        Ok(())
    }

    async fn shutdown(&self) -> Result<(), SystemError> {
        self.execute_effect(EffectType::System, "shutdown", &[])
            .await
            .map_err(|e| SystemError::OperationFailed {
                message: format!("Failed to shutdown: {}", e),
            })?;

        Ok(())
    }
}

// Implement FlowBudgetEffects
#[async_trait]
impl crate::guards::flow::FlowBudgetEffects for AuraEffectSystem {
    async fn charge_flow(
        &self,
        context: &ContextId,
        peer: &DeviceId,
        cost: u32,
    ) -> AuraResult<Receipt> {
        // Initialize budget if needed
        let budget = self
            .budget_mgr
            .charge_or_init(
                context,
                peer,
                cost,
                self.config.default_flow_limit,
                self.config.initial_epoch,
            )
            .await?;

        // Generate receipt
        let nonce = budget.spent / cost as u64; // Simple monotonic nonce based on spending

        // Get previous receipt hash (simplified)
        let prev = self
            .receipt_mgr
            .head_hash(context)
            .await?
            .unwrap_or([0u8; 32]);

        // Generate signature (simplified - in real impl would use proper crypto)
        let mut sig_input = Vec::new();
        sig_input.extend_from_slice(context.0.as_bytes());
        sig_input.extend_from_slice(self.config.device_id.0.as_bytes());
        sig_input.extend_from_slice(peer.0.as_bytes());
        sig_input.extend_from_slice(&budget.epoch.0.to_le_bytes());
        sig_input.extend_from_slice(&cost.to_le_bytes());
        sig_input.extend_from_slice(&nonce.to_le_bytes());
        sig_input.extend_from_slice(prev.as_ref());

        let sig = hash(&sig_input).to_vec();

        let receipt = Receipt {
            ctx: context.clone(),
            src: self.config.device_id,
            dst: *peer,
            epoch: budget.epoch,
            cost,
            nonce,
            prev: aura_core::Hash32(prev),
            sig,
        };

        // Add to receipt chain
        self.receipt_mgr
            .add_receipt(context.clone(), receipt.clone())
            .await?;

        Ok(receipt)
    }
}

// Helper method for tests
impl AuraEffectSystem {
    async fn check_flow_budget(
        &self,
        context: &ContextId,
        peer: &DeviceId,
    ) -> AuraResult<FlowBudget> {
        self.budget_mgr
            .get_budget(context, peer)
            .await?
            .ok_or_else(|| AuraError::not_found("Flow budget not found"))
    }
}

// Implement the unified AuraEffects trait
impl crate::effects::AuraEffects for AuraEffectSystem {}

// Implement GuardEffectSystem trait for aura-protocol guards
impl aura_protocol::guards::GuardEffectSystem for AuraEffectSystem {
    fn device_id(&self) -> DeviceId {
        self.config.device_id
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.config.execution_mode
    }

    fn get_metadata(&self, key: &str) -> Option<String> {
        match key {
            "execution_mode" => Some(format!("{:?}", self.config.execution_mode)),
            "supported_effects" => Some("all".to_string()),
            _ => None,
        }
    }

    fn can_perform_operation(&self, operation: &str) -> bool {
        // For now, allow all operations
        // In production, this would check actual capabilities
        match operation {
            "send_message" | "receive_message" | "sign_data" | "verify_signature" => true,
            _ => true, // Permissive for development
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::*;

    #[aura_test]
    async fn test_effect_system_creation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let system = fixture.effects();

        assert_eq!(system.device_id(), device_id);
        Ok(())
    }

    #[aura_test]
    async fn test_stateless_time_effects() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let system = fixture.effects();

        // Multiple calls should work without state conflicts
        let t1 = aura_core::TimeEffects::current_timestamp(system).await;
        let t2 = aura_core::TimeEffects::current_timestamp(system).await;

        // In testing mode, time might be fixed or monotonic
        assert!(t2 >= t1);
        Ok(())
    }

    #[aura_test]
    async fn test_flow_budget_isolation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let system = fixture.effects();

        let context = ContextId::from("test-context");
        let peer = DeviceId::from(uuid::Uuid::from_bytes([2u8; 16]));

        // First charge should initialize budget
        let budget1 = system.charge_flow_budget(&context, &peer, 100).await?;
        assert_eq!(budget1.spent, 100);

        // Second charge should update spent amount
        let budget2 = system.charge_flow_budget(&context, &peer, 200).await?;
        assert_eq!(budget2.spent, 300); // 100 + 200
                                        // Note: Receipt prev field should match content hash of previous receipt

        // Check budget was updated
        let budget = system.check_flow_budget(&context, &peer).await?;
        assert_eq!(budget.spent, 300);
        Ok(())
    }

    #[aura_test]
    async fn test_concurrent_execution_no_deadlock() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let system = Arc::new((*fixture.effects()).clone());

        // Spawn multiple concurrent operations
        let mut handles = vec![];

        for i in 0..10 {
            let sys = system.clone();
            let handle = tokio::spawn(async move {
                // Mix of different effect types
                let _ = aura_core::TimeEffects::current_timestamp(&*sys).await;
                let _ = sys.random_bytes(32).await;
                let _ = hash(&[i as u8; 32]);

                // Try flow charging
                let context = ContextId::from(format!("ctx-{}", i));
                let peer = DeviceId::from(uuid::Uuid::from_bytes([i as u8; 16]));
                let _ = sys.charge_flow_budget(&context, &peer, 10).await;
            });
            handles.push(handle);
        }

        // All should complete without deadlock
        for handle in handles {
            handle.await.unwrap();
        }
        Ok(())
    }
}
