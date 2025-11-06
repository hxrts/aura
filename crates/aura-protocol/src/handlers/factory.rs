//! Factory system for creating Aura handlers
//!
//! This module provides a unified factory and builder system for creating
//! handler instances with consistent configuration patterns across all
//! execution modes and handler types.

use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

use super::{EffectType, ExecutionMode};
use aura_types::DeviceId;

/// Error type for factory operations
#[derive(Debug, Error)]
pub enum FactoryError {
    /// Configuration validation failed
    #[error("Configuration validation failed: {message}")]
    ConfigurationError {
        /// Description of the configuration error
        message: String,
    },

    /// Handler creation failed
    #[error("Failed to create handler for {effect_type:?}")]
    HandlerCreationFailed {
        /// The effect type that failed to create
        effect_type: EffectType,
        /// Underlying error from handler creation
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Middleware creation failed
    #[error("Failed to create middleware '{middleware_name}'")]
    MiddlewareCreationFailed {
        /// Name of the middleware that failed
        middleware_name: String,
        /// Underlying error from middleware creation
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Platform detection failed
    #[error("Failed to detect platform capabilities")]
    PlatformDetectionFailed {
        /// Underlying error from platform detection
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Required effect type not available
    #[error("Required effect type {effect_type:?} not available")]
    RequiredEffectUnavailable {
        /// The effect type that is required but unavailable
        effect_type: EffectType,
    },

    /// Invalid execution mode for platform
    #[error("Execution mode {mode:?} not supported on this platform")]
    UnsupportedExecutionMode {
        /// The execution mode that is not supported
        mode: ExecutionMode,
    },
}

impl FactoryError {
    /// Create a configuration error
    pub fn config_error(message: impl Into<String>) -> Self {
        Self::ConfigurationError {
            message: message.into(),
        }
    }

    /// Create a handler creation error
    pub fn handler_creation_failed(
        effect_type: EffectType,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerCreationFailed {
            effect_type,
            source: Box::new(source),
        }
    }

    /// Create a middleware creation error
    pub fn middleware_creation_failed(
        middleware_name: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::MiddlewareCreationFailed {
            middleware_name: middleware_name.into(),
            source: Box::new(source),
        }
    }
}

/// Configuration for handler factory
#[derive(Debug, Clone)]
pub struct AuraHandlerConfig {
    /// Device identifier
    pub device_id: DeviceId,
    /// Execution mode
    pub execution_mode: ExecutionMode,
    /// Required effect types
    pub required_effects: Vec<EffectType>,
    /// Optional effect types (will be included if available)
    pub optional_effects: Vec<EffectType>,
    /// Middleware configuration
    pub middleware: MiddlewareConfig,
    /// Platform-specific configuration
    pub platform: PlatformConfig,
    /// Simulation-specific configuration
    pub simulation: Option<SimulationConfig>,
}

/// Configuration for middleware stack
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Enable logging middleware
    pub enable_logging: bool,
    /// Enable metrics middleware
    pub enable_metrics: bool,
    /// Enable tracing middleware
    pub enable_tracing: bool,
    /// Custom middleware specifications
    pub custom_middleware: Vec<MiddlewareSpec>,
    /// Global timeout for operations
    pub global_timeout: Option<Duration>,
}

/// Specification for custom middleware
#[derive(Debug, Clone)]
pub struct MiddlewareSpec {
    /// Middleware name/type
    pub name: String,
    /// Priority for ordering
    pub priority: u32,
    /// Configuration parameters
    pub config: HashMap<String, String>,
}

/// Platform-specific configuration
#[derive(Debug, Clone)]
pub struct PlatformConfig {
    /// Force specific platform detection (for testing)
    pub force_platform: Option<String>,
    /// Enable hardware security features
    pub enable_hardware_security: bool,
    /// Storage backend preferences
    pub preferred_storage_backends: Vec<String>,
    /// Network interface preferences
    pub preferred_network_interfaces: Vec<String>,
}

/// Simulation-specific configuration
#[derive(Debug, Clone)]
pub struct SimulationConfig {
    /// Random seed for deterministic execution
    pub seed: u64,
    /// Enable fault injection
    pub enable_fault_injection: bool,
    /// Fault injection rate (0.0 to 1.0)
    pub fault_injection_rate: f64,
    /// Enable time control
    pub enable_time_control: bool,
    /// Enable property checking
    pub enable_property_checking: bool,
    /// Maximum simulation duration
    pub max_simulation_duration: Option<Duration>,
}

impl Default for AuraHandlerConfig {
    fn default() -> Self {
        #[allow(clippy::disallowed_methods)]
        let device_id = DeviceId::from(uuid::Uuid::new_v4());
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            required_effects: vec![EffectType::Console, EffectType::Random],
            optional_effects: vec![
                EffectType::Crypto,
                EffectType::Network,
                EffectType::Storage,
                EffectType::Time,
                EffectType::Choreographic,
            ],
            middleware: MiddlewareConfig::default(),
            platform: PlatformConfig::default(),
            simulation: None,
        }
    }
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enable_logging: true,
            enable_metrics: false,
            enable_tracing: false,
            custom_middleware: Vec::new(),
            global_timeout: Some(Duration::from_secs(30)),
        }
    }
}

impl Default for PlatformConfig {
    fn default() -> Self {
        Self {
            force_platform: None,
            enable_hardware_security: false,
            preferred_storage_backends: vec!["filesystem".to_string()],
            preferred_network_interfaces: vec!["default".to_string()],
        }
    }
}

impl Default for SimulationConfig {
    fn default() -> Self {
        Self {
            seed: 42,
            enable_fault_injection: false,
            fault_injection_rate: 0.1,
            enable_time_control: false,
            enable_property_checking: true,
            max_simulation_duration: Some(Duration::from_secs(300)),
        }
    }
}

impl AuraHandlerConfig {
    /// Create configuration for testing mode
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing,
            middleware: MiddlewareConfig {
                enable_logging: false, // Reduced noise in tests
                enable_metrics: true,  // Useful for test validation
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create configuration for production mode
    pub fn for_production(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Production,
            required_effects: vec![
                EffectType::Console,
                EffectType::Random,
                EffectType::Crypto,
                EffectType::Network,
                EffectType::Storage,
                EffectType::Time,
            ],
            middleware: MiddlewareConfig {
                enable_logging: true,
                enable_metrics: true,
                enable_tracing: true,
                ..Default::default()
            },
            platform: PlatformConfig {
                enable_hardware_security: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create configuration for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            required_effects: vec![
                EffectType::Console,
                EffectType::Random,
                EffectType::Crypto,
                EffectType::Network,
                EffectType::Storage,
                EffectType::Time,
                EffectType::FaultInjection,
                EffectType::TimeControl,
            ],
            middleware: MiddlewareConfig {
                enable_logging: true,
                enable_metrics: true,
                ..Default::default()
            },
            simulation: Some(SimulationConfig {
                seed,
                enable_fault_injection: true,
                enable_time_control: true,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<(), FactoryError> {
        // Check that device ID is valid
        if self.device_id.0.is_nil() {
            return Err(FactoryError::config_error("Device ID cannot be nil"));
        }

        // Check simulation configuration consistency
        if let ExecutionMode::Simulation { seed } = self.execution_mode {
            match &self.simulation {
                Some(sim_config) => {
                    if sim_config.seed != seed {
                        return Err(FactoryError::config_error(
                            "Simulation seed mismatch between execution mode and config",
                        ));
                    }
                }
                None => {
                    return Err(FactoryError::config_error(
                        "Simulation execution mode requires simulation configuration",
                    ));
                }
            }
        }

        // Check middleware configuration
        if let Some(timeout) = self.middleware.global_timeout {
            if timeout.is_zero() {
                return Err(FactoryError::config_error("Global timeout cannot be zero"));
            }
        }

        // Check fault injection rate
        if let Some(sim_config) = &self.simulation {
            if sim_config.fault_injection_rate < 0.0 || sim_config.fault_injection_rate > 1.0 {
                return Err(FactoryError::config_error(
                    "Fault injection rate must be between 0.0 and 1.0",
                ));
            }
        }

        Ok(())
    }

    /// Add a required effect type
    pub fn with_required_effect(mut self, effect_type: EffectType) -> Self {
        if !self.required_effects.contains(&effect_type) {
            self.required_effects.push(effect_type);
        }
        self
    }

    /// Add an optional effect type
    pub fn with_optional_effect(mut self, effect_type: EffectType) -> Self {
        if !self.optional_effects.contains(&effect_type) {
            self.optional_effects.push(effect_type);
        }
        self
    }

    /// Enable middleware
    pub fn with_logging(mut self, enabled: bool) -> Self {
        self.middleware.enable_logging = enabled;
        self
    }

    /// Enable metrics
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.middleware.enable_metrics = enabled;
        self
    }

    /// Enable tracing
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.middleware.enable_tracing = enabled;
        self
    }

    /// Set global timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.middleware.global_timeout = Some(timeout);
        self
    }

    /// Add custom middleware
    pub fn with_custom_middleware(
        mut self,
        name: String,
        priority: u32,
        config: HashMap<String, String>,
    ) -> Self {
        self.middleware.custom_middleware.push(MiddlewareSpec {
            name,
            priority,
            config,
        });
        self
    }
}

/// Core factory trait for creating Aura handlers
///
/// Provides a consistent interface for creating handlers across different
/// execution modes and configurations.
pub trait AuraHandlerFactory {
    /// Create a handler with the given configuration
    fn create_handler(
        config: AuraHandlerConfig,
    ) -> Result<super::erased::BoxedHandler, FactoryError>;

    /// Create a testing handler with minimal configuration
    fn for_testing(device_id: DeviceId) -> Result<super::erased::BoxedHandler, FactoryError> {
        Self::create_handler(AuraHandlerConfig::for_testing(device_id))
    }

    /// Create a production handler with full capabilities
    fn for_production(device_id: DeviceId) -> Result<super::erased::BoxedHandler, FactoryError> {
        Self::create_handler(AuraHandlerConfig::for_production(device_id))
    }

    /// Create a simulation handler with deterministic behavior
    fn for_simulation(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<super::erased::BoxedHandler, FactoryError> {
        Self::create_handler(AuraHandlerConfig::for_simulation(device_id, seed))
    }

    /// Get the supported effect types for this factory
    fn supported_effect_types() -> Vec<EffectType>;

    /// Check if an effect type is supported
    fn supports_effect_type(effect_type: EffectType) -> bool {
        Self::supported_effect_types().contains(&effect_type)
    }
}

/// Builder for creating Aura handler configurations
pub struct AuraHandlerBuilder {
    config: AuraHandlerConfig,
}

impl AuraHandlerBuilder {
    /// Create a new builder with default configuration
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            config: AuraHandlerConfig {
                device_id,
                ..Default::default()
            },
        }
    }

    /// Set execution mode
    pub fn execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.config.execution_mode = mode;

        // Auto-configure simulation if needed
        if let ExecutionMode::Simulation { seed } = mode {
            self.config.simulation = Some(SimulationConfig {
                seed,
                ..Default::default()
            });
        }

        self
    }

    /// Add required effect
    pub fn require_effect(mut self, effect_type: EffectType) -> Self {
        self.config = self.config.with_required_effect(effect_type);
        self
    }

    /// Add optional effect
    pub fn optional_effect(mut self, effect_type: EffectType) -> Self {
        self.config = self.config.with_optional_effect(effect_type);
        self
    }

    /// Configure middleware
    pub fn with_logging(mut self, enabled: bool) -> Self {
        self.config = self.config.with_logging(enabled);
        self
    }

    /// Configure metrics
    pub fn with_metrics(mut self, enabled: bool) -> Self {
        self.config = self.config.with_metrics(enabled);
        self
    }

    /// Configure tracing
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.config = self.config.with_tracing(enabled);
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.config = self.config.with_timeout(timeout);
        self
    }

    /// Enable hardware security
    pub fn with_hardware_security(mut self, enabled: bool) -> Self {
        self.config.platform.enable_hardware_security = enabled;
        self
    }

    /// Configure simulation
    pub fn with_simulation_config(mut self, config: SimulationConfig) -> Self {
        self.config.simulation = Some(config);
        self
    }

    /// Build the configuration
    pub fn build_config(self) -> Result<AuraHandlerConfig, FactoryError> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Build the handler using a specific factory
    pub fn build_with_factory<F: AuraHandlerFactory>(
        self,
    ) -> Result<super::erased::BoxedHandler, FactoryError> {
        let config = self.build_config()?;
        F::create_handler(config)
    }
}

/// Platform detection utilities
pub struct PlatformDetector;

impl PlatformDetector {
    /// Detect the current platform
    pub fn detect_platform() -> Result<PlatformInfo, FactoryError> {
        Ok(PlatformInfo {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            has_secure_enclave: Self::detect_secure_enclave(),
            available_storage_backends: Self::detect_storage_backends(),
            available_network_interfaces: Self::detect_network_interfaces(),
        })
    }

    /// Detect if secure enclave is available
    fn detect_secure_enclave() -> bool {
        // Platform-specific detection logic would go here
        // For now, conservative default
        false
    }

    /// Detect available storage backends
    fn detect_storage_backends() -> Vec<String> {
        let mut backends = vec!["memory".to_string()];

        // Always available
        backends.push("filesystem".to_string());

        // Platform-specific backends
        #[cfg(target_os = "macos")]
        backends.push("keychain".to_string());

        #[cfg(target_os = "windows")]
        backends.push("credential_store".to_string());

        #[cfg(target_os = "linux")]
        backends.push("secret_service".to_string());

        backends
    }

    /// Detect available network interfaces
    fn detect_network_interfaces() -> Vec<String> {
        // Simplified detection - real implementation would enumerate actual interfaces
        vec!["default".to_string(), "loopback".to_string()]
    }
}

/// Platform information
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    /// Operating system
    pub os: String,
    /// Architecture
    pub arch: String,
    /// Whether secure enclave is available
    pub has_secure_enclave: bool,
    /// Available storage backends
    pub available_storage_backends: Vec<String>,
    /// Available network interfaces
    pub available_network_interfaces: Vec<String>,
}

impl PlatformInfo {
    /// Check if a storage backend is available
    pub fn has_storage_backend(&self, backend: &str) -> bool {
        self.available_storage_backends
            .contains(&backend.to_string())
    }

    /// Check if a network interface is available
    pub fn has_network_interface(&self, interface: &str) -> bool {
        self.available_network_interfaces
            .contains(&interface.to_string())
    }

    /// Get the best storage backend from preferences
    pub fn best_storage_backend(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            if self.has_storage_backend(pref) {
                return Some(pref.clone());
            }
        }

        // Fallback to first available
        self.available_storage_backends.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let device_id = DeviceId::from(uuid::Uuid::new_v4());

        let testing_config = AuraHandlerConfig::for_testing(device_id);
        assert_eq!(testing_config.execution_mode, ExecutionMode::Testing);
        assert!(!testing_config.middleware.enable_logging);
        assert!(testing_config.middleware.enable_metrics);

        let production_config = AuraHandlerConfig::for_production(device_id);
        assert_eq!(production_config.execution_mode, ExecutionMode::Production);
        assert!(production_config.middleware.enable_logging);
        assert!(production_config.platform.enable_hardware_security);

        let simulation_config = AuraHandlerConfig::for_simulation(device_id, 42);
        assert_eq!(
            simulation_config.execution_mode,
            ExecutionMode::Simulation { seed: 42 }
        );
        assert!(simulation_config.simulation.is_some());
        assert_eq!(simulation_config.simulation.unwrap().seed, 42);
    }

    #[test]
    fn test_config_validation() {
        let device_id = DeviceId::from(uuid::Uuid::new_v4());
        let mut config = AuraHandlerConfig::for_testing(device_id);

        // Valid config should pass
        assert!(config.validate().is_ok());

        // Invalid device ID should fail
        config.device_id = DeviceId(uuid::Uuid::nil());
        assert!(config.validate().is_err());

        // Reset device ID
        config.device_id = device_id;

        // Invalid simulation config should fail
        config.execution_mode = ExecutionMode::Simulation { seed: 42 };
        config.simulation = Some(SimulationConfig {
            seed: 99, // Mismatch with execution mode
            ..Default::default()
        });
        assert!(config.validate().is_err());

        // Fix simulation config
        config.simulation.as_mut().unwrap().seed = 42;
        assert!(config.validate().is_ok());

        // Invalid timeout should fail
        config.middleware.global_timeout = Some(Duration::ZERO);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_builder_pattern() {
        let device_id = DeviceId::from(uuid::Uuid::new_v4());

        let config = AuraHandlerBuilder::new(device_id)
            .execution_mode(ExecutionMode::Testing)
            .require_effect(EffectType::Crypto)
            .optional_effect(EffectType::Network)
            .with_logging(true)
            .with_metrics(false)
            .with_timeout(Duration::from_secs(10))
            .build_config()
            .unwrap();

        assert_eq!(config.execution_mode, ExecutionMode::Testing);
        assert!(config.required_effects.contains(&EffectType::Crypto));
        assert!(config.optional_effects.contains(&EffectType::Network));
        assert!(config.middleware.enable_logging);
        assert!(!config.middleware.enable_metrics);
        assert_eq!(
            config.middleware.global_timeout,
            Some(Duration::from_secs(10))
        );
    }

    #[test]
    fn test_platform_detection() {
        let platform = PlatformDetector::detect_platform().unwrap();

        // Should detect current OS
        assert!(!platform.os.is_empty());
        assert!(!platform.arch.is_empty());

        // Should have at least memory and filesystem storage
        assert!(platform.has_storage_backend("memory"));
        assert!(platform.has_storage_backend("filesystem"));

        // Should have default network interface
        assert!(platform.has_network_interface("default"));

        // Test storage backend selection
        let prefs = vec!["nonexistent".to_string(), "filesystem".to_string()];
        let best = platform.best_storage_backend(&prefs);
        assert_eq!(best, Some("filesystem".to_string()));
    }

    #[test]
    fn test_middleware_config() {
        let mut config = MiddlewareConfig::default();

        // Test defaults
        assert!(config.enable_logging);
        assert!(!config.enable_metrics);
        assert!(!config.enable_tracing);
        assert!(config.custom_middleware.is_empty());
        assert!(config.global_timeout.is_some());

        // Test custom middleware
        config.custom_middleware.push(MiddlewareSpec {
            name: "test".to_string(),
            priority: 50,
            config: HashMap::new(),
        });
        assert_eq!(config.custom_middleware.len(), 1);
    }

    #[test]
    fn test_simulation_config() {
        let config = SimulationConfig::default();

        assert_eq!(config.seed, 42);
        assert!(!config.enable_fault_injection);
        assert_eq!(config.fault_injection_rate, 0.1);
        assert!(!config.enable_time_control);
        assert!(config.enable_property_checking);
        assert!(config.max_simulation_duration.is_some());
    }
}
