//! Handler Factory system for creating and configuring effect handlers
//!
//! This module provides a unified factory system for creating handler instances
//! with consistent configuration patterns across different execution modes.

use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

use aura_core::{DeviceId, EffectType, ExecutionMode};
use aura_effects::{
    console::RealConsoleHandler,
    crypto::RealCryptoHandler,
    random::RealRandomHandler,
    storage::FilesystemStorageHandler,
    system::logging::{LoggingConfig, LoggingSystemHandler},
    time::RealTimeHandler,
    TcpTransportHandler as RealTransportHandler,
    StandardAuthorizationHandler,
    StandardJournalHandler,
};
use crate::registry::{EffectRegistry, HandlerContext, RegistrableHandler};
use crate::adapters::{
    AuthorizationHandlerAdapter, ConsoleHandlerAdapter, CryptoHandlerAdapter, JournalHandlerAdapter,
    LoggingSystemHandlerAdapter, RandomHandlerAdapter, StorageHandlerAdapter, TimeHandlerAdapter,
    TransportHandlerAdapter,
};

/// Error type for factory operations
#[derive(Debug, Error)]
pub enum FactoryError {
    /// Configuration validation failed
    #[error("Configuration validation failed: {message}")]
    ConfigurationError { message: String },

    /// Handler creation failed
    #[error("Failed to create handler for {effect_type:?}")]
    HandlerCreationFailed {
        effect_type: EffectType,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Required effect type not available
    #[error("Required effect type {effect_type:?} not available")]
    RequiredEffectUnavailable { effect_type: EffectType },

    /// Invalid execution mode for platform
    #[error("Execution mode {mode:?} not supported on this platform")]
    InvalidExecutionMode { mode: ExecutionMode },

    /// Registry operation failed
    #[error("Registry operation failed")]
    RegistryError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl FactoryError {
    /// Create a configuration error
    pub fn configuration_error(message: impl Into<String>) -> Self {
        Self::ConfigurationError { message: message.into() }
    }

    /// Create a handler creation failed error
    pub fn handler_creation_failed(
        effect_type: EffectType,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerCreationFailed {
            effect_type,
            source: Box::new(source),
        }
    }
}

/// Configuration for handler creation
#[derive(Debug, Clone)]
pub struct HandlerConfig {
    /// Device ID for the handler
    pub device_id: DeviceId,
    /// Execution mode
    pub execution_mode: ExecutionMode,
    /// Required effect types
    pub required_effects: Vec<EffectType>,
    /// Optional effect types
    pub optional_effects: Vec<EffectType>,
    /// Timeout for operations
    pub timeout: Option<Duration>,
    /// Whether to enable logging
    pub enable_logging: bool,
    /// Whether to enable metrics
    pub enable_metrics: bool,
    /// Custom configuration parameters
    pub custom_params: HashMap<String, String>,
}

impl HandlerConfig { // Registry helper
    /// Create a new handler configuration
    pub fn new(device_id: DeviceId, execution_mode: ExecutionMode) -> Self {
        Self {
            device_id,
            execution_mode,
            required_effects: Vec::new(),
            optional_effects: Vec::new(),
            timeout: None,
            enable_logging: false,
            enable_metrics: false,
            custom_params: HashMap::new(),
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), FactoryError> {
        if self.required_effects.is_empty() {
            return Err(FactoryError::configuration_error(
                "At least one required effect type must be specified",
            ));
        }

        // Check for duplicate effect types
        let mut all_effects = self.required_effects.clone();
        all_effects.extend(&self.optional_effects);
        all_effects.sort();
        all_effects.dedup();
        
        if all_effects.len() != self.required_effects.len() + self.optional_effects.len() {
            return Err(FactoryError::configuration_error(
                "Duplicate effect types in required and optional lists",
            ));
        }

        Ok(())
    }

    /// Add a required effect type
    pub fn with_required_effect(mut self, effect_type: EffectType) -> Self {
        self.required_effects.push(effect_type);
        self
    }

    /// Add an optional effect type
    pub fn with_optional_effect(mut self, effect_type: EffectType) -> Self {
        self.optional_effects.push(effect_type);
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Enable logging
    pub fn with_logging(mut self, enable: bool) -> Self {
        self.enable_logging = enable;
        self
    }

    /// Enable metrics
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.enable_metrics = enable;
        self
    }

    /// Add custom parameter
    pub fn with_custom_param(mut self, key: String, value: String) -> Self {
        self.custom_params.insert(key, value);
        self
    }
}

/// Builder for creating handler configurations
#[derive(Debug)]
pub struct HandlerConfigBuilder {
    config: HandlerConfig,
}

impl HandlerConfigBuilder { // Registry helper
    /// Create a new builder
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            config: HandlerConfig::new(device_id, ExecutionMode::Testing),
        }
    }

    /// Set execution mode
    pub fn execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.config.execution_mode = mode;
        self
    }

    /// Require an effect type
    pub fn require_effect(mut self, effect_type: EffectType) -> Self {
        self.config.required_effects.push(effect_type);
        self
    }

    /// Add optional effect type
    pub fn optional_effect(mut self, effect_type: EffectType) -> Self {
        self.config.optional_effects.push(effect_type);
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = Some(timeout);
        self
    }

    /// Enable logging
    pub fn with_logging(mut self, enable: bool) -> Self {
        self.config.enable_logging = enable;
        self
    }

    /// Enable metrics
    pub fn with_metrics(mut self, enable: bool) -> Self {
        self.config.enable_metrics = enable;
        self
    }

    /// Add custom parameter
    pub fn with_custom_param(mut self, key: String, value: String) -> Self {
        self.config.custom_params.insert(key, value);
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<HandlerConfig, FactoryError> {
        self.config.validate()?;
        Ok(self.config)
    }
}

/// Factory for creating effect handlers
pub struct HandlerFactory {
    /// Default configuration
    default_config: HandlerConfig,
}

impl HandlerFactory { // Registry helper
    /// Create a new handler factory
    pub fn new(default_config: HandlerConfig) -> Result<Self, FactoryError> {
        default_config.validate()?;
        Ok(Self { default_config })
    }

    /// Create a factory for testing
    pub fn for_testing(device_id: DeviceId) -> Result<Self, FactoryError> {
        let config = HandlerConfig::new(device_id, ExecutionMode::Testing)
            .with_required_effect(EffectType::Console)
            .with_required_effect(EffectType::Random)
            .with_logging(true);
        Self::new(config)
    }

    /// Create a factory for production
    pub fn for_production(device_id: DeviceId) -> Result<Self, FactoryError> {
        let config = HandlerConfig::new(device_id, ExecutionMode::Production)
            .with_required_effect(EffectType::Crypto)
            .with_required_effect(EffectType::Network)
            .with_required_effect(EffectType::Storage)
            .with_required_effect(EffectType::Console)
            .with_required_effect(EffectType::Random)
            .with_logging(true)
            .with_metrics(true);
        Self::new(config)
    }

    /// Create a factory for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Result<Self, FactoryError> {
        let config = HandlerConfig::new(device_id, ExecutionMode::Simulation { seed })
            .with_required_effect(EffectType::Crypto)
            .with_required_effect(EffectType::Network)
            .with_required_effect(EffectType::Storage)
            .with_required_effect(EffectType::Console)
            .with_required_effect(EffectType::Random)
            .with_required_effect(EffectType::Time)
            .with_logging(true)
            .with_metrics(true);
        Self::new(config)
    }

    /// Create an effect registry with handlers
    pub fn create_registry(&self) -> Result<EffectRegistry, FactoryError> {
        self.create_registry_with_config(&self.default_config)
    }

    /// Create an effect registry with custom configuration
    pub fn create_registry_with_config(&self, config: &HandlerConfig) -> Result<EffectRegistry, FactoryError> {
        config.validate()?;

        let mut registry = EffectRegistry::new(config.execution_mode);

        // Create and register handlers based on execution mode and required effects
        match config.execution_mode {
            ExecutionMode::Production => {
                self.register_production_handlers(&mut registry, config)?;
            },
            ExecutionMode::Testing => {
                self.register_testing_handlers(&mut registry, config)?;
            },
            ExecutionMode::Simulation { seed } => {
                self.register_simulation_handlers(&mut registry, config, seed)?;
            },
        }

        Ok(registry)
    }

    /// Register a handler in a registry
    pub fn register_handler(
        &self,
        registry: &mut EffectRegistry,
        effect_type: EffectType,
        handler: Box<dyn RegistrableHandler>,
    ) -> Result<(), FactoryError> {
        registry
            .register_handler(effect_type, handler)
            .map_err(|e| FactoryError::RegistryError {
                source: Box::new(e),
            })
    }

    /// Get default configuration
    pub fn default_config(&self) -> &HandlerConfig {
        &self.default_config
    }

    /// Create a handler context
    pub fn create_context(&self) -> HandlerContext {
        HandlerContext::new(self.default_config.device_id, self.default_config.execution_mode)
    }

    /// Create a handler context with custom configuration
    pub fn create_context_with_config(&self, config: &HandlerConfig) -> HandlerContext {
        HandlerContext::new(config.device_id, config.execution_mode)
    }

    /// Register production handlers
    fn register_production_handlers(&self, registry: &mut EffectRegistry, config: &HandlerConfig) -> Result<(), FactoryError> {
        use aura_effects::*;

        // Register required effect handlers for production
        for effect_type in &config.required_effects {
            match effect_type {
                EffectType::Console => {
                    let handler = ConsoleHandlerAdapter::new(RealConsoleHandler::new());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Random => {
                    let handler = RandomHandlerAdapter::new(RealRandomHandler::new());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Crypto => {
                    let handler = CryptoHandlerAdapter::new(RealCryptoHandler::new());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Storage => {
                    // Use filesystem storage for production
                    let storage_dir = config
                        .custom_params
                        .get("storage_dir")
                        .map(|s| s.as_str())
                        .unwrap_or("./aura_storage");
                    let handler = StorageHandlerAdapter::new(FilesystemStorageHandler::new(
                        std::path::PathBuf::from(storage_dir),
                    ));
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Time => {
                    let handler = TimeHandlerAdapter::new(RealTimeHandler::new());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Network => {
                    let handler = TransportHandlerAdapter::new(TcpTransportHandler::default());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::Journal => {
                    let handler = JournalHandlerAdapter::new(StandardJournalHandler::new());
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                EffectType::System => {
                    let handler =
                        LoggingSystemHandlerAdapter::new(LoggingSystemHandler::new(LoggingConfig::default()));
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                }
                _ => {
                    // For other effect types, skip or provide basic implementation
                    tracing::warn!("No production handler available for effect type: {:?}", effect_type);
                }
            }
        }

        // Register optional effects if available
        for effect_type in &config.optional_effects {
            if !registry.is_registered(*effect_type) {
                match effect_type {
                    EffectType::Authentication => {
                        let handler =
                            AuthorizationHandlerAdapter::new(StandardAuthorizationHandler::with_standard_rules());
                        self.register_handler(registry, *effect_type, Box::new(handler))?;
                    },
                    _ => {
                        // Skip optional effects that aren't implemented
                        tracing::debug!("Optional effect handler not implemented: {:?}", effect_type);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register testing handlers
    /// 
    /// Note: Mock handlers are in aura-testkit (Layer 8), not the effects crate (Layer 3).
    /// For now, we use production handlers with safer configurations for testing.
    /// Real testing should use aura-testkit's CompositeTestHandler.
    fn register_testing_handlers(&self, registry: &mut EffectRegistry, config: &HandlerConfig) -> Result<(), FactoryError> {
        tracing::info!("Registering testing handlers using production implementations with safe defaults");
        
        // Use production handlers with safer configurations for testing
        for effect_type in &config.required_effects {
            match effect_type {
                EffectType::Storage => {
                    // Use filesystem storage with temporary directory
                    let temp_dir = std::env::temp_dir().join("aura_test_storage");
                    let handler = StorageHandlerAdapter::new(FilesystemStorageHandler::new(temp_dir));
                    self.register_handler(registry, *effect_type, Box::new(handler))?;
                },
                _ => {
                    // For all other effects, use production handlers with default configs
                    if let Err(_) = self.register_production_handler_safe(registry, *effect_type, config) {
                        tracing::warn!("No test handler available for effect type: {:?}", effect_type);
                    }
                }
            }
        }

        Ok(())
    }

    /// Register simulation handlers
    /// 
    /// Note: Simulation handlers are in aura-testkit (Layer 8), not the effects crate (Layer 3).
    /// For now, we use production handlers. Real simulation should use aura-testkit.
    fn register_simulation_handlers(&self, registry: &mut EffectRegistry, config: &HandlerConfig, _seed: u64) -> Result<(), FactoryError> {
        tracing::info!("Registering simulation handlers using production implementations");
        
        // For simulation, use production handlers (deterministic simulation requires aura-testkit)
        for effect_type in &config.required_effects {
            if let Err(_) = self.register_production_handler_safe(registry, *effect_type, config) {
                tracing::warn!("No simulation handler available for effect type: {:?}", effect_type);
            }
        }

        Ok(())
    }

    /// Safely register a production handler (helper method)
    fn register_production_handler_safe(&self, registry: &mut EffectRegistry, effect_type: EffectType, config: &HandlerConfig) -> Result<(), FactoryError> {
        match effect_type {
            EffectType::Console => {
                let handler = ConsoleHandlerAdapter::new(RealConsoleHandler::new());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Random => {
                let handler = RandomHandlerAdapter::new(RealRandomHandler::new());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Crypto => {
                let handler = CryptoHandlerAdapter::new(RealCryptoHandler::new());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Storage => {
                let storage_dir = config.custom_params
                    .get("storage_dir")
                    .map(|s| std::path::PathBuf::from(s))
                    .unwrap_or_else(|| std::path::PathBuf::from("./aura_storage"));
                let handler = StorageHandlerAdapter::new(FilesystemStorageHandler::new(storage_dir));
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Time => {
                let handler = TimeHandlerAdapter::new(RealTimeHandler::new());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Network => {
                let handler = TransportHandlerAdapter::new(RealTransportHandler::default());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Journal => {
                let handler = JournalHandlerAdapter::new(StandardJournalHandler::new());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::System => {
                let handler = LoggingSystemHandlerAdapter::new(LoggingSystemHandler::new(LoggingConfig::default()));
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            EffectType::Authentication => {
                let handler = AuthorizationHandlerAdapter::new(StandardAuthorizationHandler::with_standard_rules());
                self.register_handler(registry, effect_type, Box::new(handler))
            },
            _ => {
                tracing::warn!("No production handler available for effect type: {:?}", effect_type);
                Err(FactoryError::RequiredEffectUnavailable { effect_type })
            }
        }
    }
}

/// Platform detection utilities
pub struct PlatformDetector;

impl PlatformDetector {
    /// Detect available effect types on the current platform
    pub fn detect_available_effects() -> Vec<EffectType> {
        // Basic detection - in a real implementation this would probe hardware/OS capabilities
        vec![
            EffectType::Console,
            EffectType::Random,
            EffectType::Storage,
            EffectType::Time,
            EffectType::Crypto,
            EffectType::Network,
        ]
    }

    /// Check if an effect type is available on the current platform
    pub fn is_effect_available(effect_type: EffectType) -> bool {
        Self::detect_available_effects().contains(&effect_type)
    }

    /// Get recommended execution mode for the current platform
    pub fn recommended_execution_mode() -> ExecutionMode {
        if cfg!(test) {
            ExecutionMode::Testing
        } else {
            ExecutionMode::Production
        }
    }

    /// Check if the platform supports hardware security features
    pub fn has_hardware_security() -> bool {
        // Simplified detection - real implementation would check TPM, secure enclaves, etc.
        cfg!(target_os = "linux") || cfg!(target_os = "macos") || cfg!(target_os = "windows")
    }

    /// Check if the platform supports networking
    pub fn has_network_support() -> bool {
        // Most platforms support networking
        true
    }

    /// Check if the platform has persistent storage
    pub fn has_persistent_storage() -> bool {
        // Most platforms support file I/O
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_handler_config_validation() {
        let device_id = DeviceId::new();
        
        // Valid configuration
        let config = HandlerConfig::new(device_id, ExecutionMode::Testing)
            .with_required_effect(EffectType::Console);
        assert!(config.validate().is_ok());

        // Invalid: no required effects
        let config = HandlerConfig::new(device_id, ExecutionMode::Testing);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_handler_config_builder() {
        let device_id = DeviceId::new();
        
        let config = HandlerConfigBuilder::new(device_id)
            .execution_mode(ExecutionMode::Production)
            .require_effect(EffectType::Crypto)
            .require_effect(EffectType::Network)
            .optional_effect(EffectType::Storage)
            .with_timeout(Duration::from_secs(30))
            .with_logging(true)
            .build()
            .unwrap();

        assert_eq!(config.execution_mode, ExecutionMode::Production);
        assert_eq!(config.required_effects.len(), 2);
        assert_eq!(config.optional_effects.len(), 1);
        assert!(config.enable_logging);
        assert_eq!(config.timeout, Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_factory_creation() {
        let device_id = DeviceId::new();
        
        // Test factory creation
        let factory = HandlerFactory::for_testing(device_id).unwrap();
        assert_eq!(factory.default_config().execution_mode, ExecutionMode::Testing);

        let factory = HandlerFactory::for_production(device_id).unwrap();
        assert_eq!(factory.default_config().execution_mode, ExecutionMode::Production);

        let factory = HandlerFactory::for_simulation(device_id, 42).unwrap();
        assert_eq!(factory.default_config().execution_mode, ExecutionMode::Simulation { seed: 42 });
    }

    #[test]
    fn test_platform_detection() {
        let available = PlatformDetector::detect_available_effects();
        assert!(!available.is_empty());
        
        assert!(PlatformDetector::is_effect_available(EffectType::Console));
        
        let mode = PlatformDetector::recommended_execution_mode();
        assert!(matches!(mode, ExecutionMode::Testing | ExecutionMode::Production));
    }

    #[test]
    fn test_registry_creation() {
        let device_id = DeviceId::new();
        let factory = HandlerFactory::for_testing(device_id).unwrap();
        
        let registry = factory.create_registry().unwrap();
        assert_eq!(registry.execution_mode(), ExecutionMode::Testing);
    }

    #[test]
    fn test_production_handler_creation() {
        let device_id = DeviceId::new();
        let factory = HandlerFactory::for_production(device_id).unwrap();
        
        // Create registry and verify handlers are registered
        let registry = factory.create_registry().unwrap();
        assert_eq!(registry.execution_mode(), ExecutionMode::Production);
        
        // Verify that production effects are registered
        let registered_effects = registry.registered_effect_types();
        let expected_effects: HashSet<EffectType> = [
            EffectType::Crypto,
            EffectType::Network, 
            EffectType::Storage,
            EffectType::Console,
            EffectType::Random,
        ].into();
        
        let actual_effects: HashSet<EffectType> = registered_effects.into_iter().collect();
        
        // Check that all expected effects are present
        for effect in &expected_effects {
            assert!(actual_effects.contains(effect), 
                "Expected effect {:?} not found in registry", effect);
        }
        
        println!("Production registry created with {} effect handlers", actual_effects.len());
    }

    #[test]  
    fn test_custom_storage_configuration() {
        let device_id = DeviceId::new();
        let custom_storage_path = "/tmp/custom_aura_storage";
        
        let config = HandlerConfigBuilder::new(device_id)
            .execution_mode(ExecutionMode::Production)
            .require_effect(EffectType::Storage)
            .with_custom_param("storage_dir".to_string(), custom_storage_path.to_string())
            .build()
            .unwrap();
            
        let factory = HandlerFactory::new(config).unwrap();
        let registry = factory.create_registry().unwrap();
        
        // Verify storage handler is registered
        assert!(registry.is_registered(EffectType::Storage));
        
        println!("Custom storage configuration test passed");
    }

    #[test]
    fn test_simulation_handler_creation() {
        let device_id = DeviceId::new();
        let factory = HandlerFactory::for_simulation(device_id, 12345).unwrap();
        
        let registry = factory.create_registry().unwrap();
        assert!(matches!(registry.execution_mode(), ExecutionMode::Simulation { seed: 12345 }));
        
        // For now, simulation uses production handlers (mock handlers are in aura-testkit)
        let registered_effects = registry.registered_effect_types();
        assert!(!registered_effects.is_empty());
        
        println!("Simulation registry created with {} effect handlers", registered_effects.len());
    }
}
