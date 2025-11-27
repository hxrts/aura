//! Factory system for creating Aura handlers
//!
//! This module provides a unified factory and builder system for creating
//! handler instances with consistent configuration patterns across all
//! execution modes and handler types.
//!
//! # Overview
//!
//! The Aura platform uses an algebraic effects architecture where business logic
//! is written against abstract effect interfaces, and concrete implementations
//! are provided by handlers. This factory system manages the complex task of
//! creating and configuring these handlers for different execution contexts.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐    ┌──────────────────┐    ┌─────────────────┐
//! │   Application   │────│ Effect Interface │────│ Handler Factory │
//! │     Logic       │    │   (abstract)     │    │   (concrete)    │
//! └─────────────────┘    └──────────────────┘    └─────────────────┘
//!                                                          │
//!                        ┌─────────────────────────────────┼─────────────────────────────────┐
//!                        │                                 │                                 │
//!                  ┌──────▼──────┐                ┌────────▼────────┐               ┌────────▼────────┐
//!                  │ Production  │                │    Testing      │               │  Simulation     │
//!                  │  Handlers   │                │   Handlers      │               │   Handlers      │
//!                  │             │                │                 │               │                 │
//!                  │ • Real I/O  │                │ • Mock/Memory   │               │ • Deterministic │
//!                  │ • Hardware  │                │ • Fast startup  │               │ • Fault inject. │
//!                  │ • Security  │                │ • No side       │               │ • Time control  │
//!                  │             │                │   effects       │               │                 │
//!                  └─────────────┘                └─────────────────┘               └─────────────────┘
//! ```
//!
//! # Usage Patterns
//!
//! ## Quick Start Examples
//!
//! ### Testing Setup
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerFactory, DefaultHandlerFactory};
//! use aura_core::DeviceId;
//!
//! // Simple testing handler - minimal configuration, fast startup
//! let device_id = DeviceId::generate();
//! let handler = DefaultHandlerFactory::for_testing(device_id)?;
//!
//! // Execute business logic against abstract interfaces
//! handler.console().print("Testing mode active").await?;
//! let random_bytes = handler.random().generate(32).await?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ### Production Setup
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerFactory, DefaultHandlerFactory};
//! use aura_core::DeviceId;
//!
//! // Production handler - full capabilities, hardware security
//! let device_id = DeviceId::from_persistent_storage().await?;
//! let handler = DefaultHandlerFactory::for_production(device_id)?;
//!
//! // Full cryptographic capabilities available
//! let keypair = handler.crypto().generate_frost_keypair().await?;
//! let encrypted_data = handler.crypto().encrypt(data, &key).await?;
//! handler.storage().store_encrypted(&content_id, encrypted_data).await?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Advanced Configuration
//!
//! ### Custom Handler Configuration
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerBuilder, AuraHandlerConfig, ExecutionMode, EffectType};
//! use aura_core::DeviceId;
//! use std::time::Duration;
//!
//! // Custom configuration with specific requirements
//! let device_id = DeviceId::generate();
//! let config = AuraHandlerBuilder::new(device_id)
//!     .execution_mode(ExecutionMode::Production)
//!     .require_effect(EffectType::Crypto)
//!     .require_effect(EffectType::Storage)
//!     .optional_effect(EffectType::Network)
//!     .with_logging(true)
//!     .with_metrics(true)
//!     .with_timeout(Duration::from_secs(60))
//!     .with_hardware_security(true)
//!     .build_config()?;
//!
//! config.validate()?;
//! let handler = DefaultHandlerFactory::create_handler(config)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ### Simulation Configuration
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerBuilder, SimulationConfig, ExecutionMode};
//! use aura_core::DeviceId;
//! use std::time::Duration;
//!
//! // Deterministic simulation with fault injection
//! let device_id = DeviceId::generate();
//! let simulation_config = SimulationConfig {
//!     seed: 12345,
//!     enable_fault_injection: true,
//!     fault_injection_rate: 0.05, // 5% failure rate
//!     enable_time_control: true,
//!     enable_property_checking: true,
//!     max_simulation_duration: Some(Duration::from_secs(3600)),
//! };
//!
//! let handler = AuraHandlerBuilder::new(device_id)
//!     .execution_mode(ExecutionMode::Simulation { seed: 12345 })
//!     .with_simulation_config(simulation_config)
//!     .build_with_factory::<DefaultHandlerFactory>()?;
//!
//! // All operations are now deterministic and repeatable
//! handler.time().set_virtual_time(Duration::from_secs(1000)).await?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Middleware Configuration
//!
//! ### Standard Middleware Stack
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerBuilder, MiddlewareConfig};
//! use aura_core::DeviceId;
//! use std::time::Duration;
//! use std::collections::HashMap;
//!
//! let device_id = DeviceId::generate();
//! let handler = AuraHandlerBuilder::new(device_id)
//!     .with_logging(true)           // Request/response logging
//!     .with_metrics(true)           // Performance metrics
//!     .with_tracing(true)           // Distributed tracing
//!     .with_timeout(Duration::from_secs(30))  // Global operation timeout
//!     .build_with_factory::<DefaultHandlerFactory>()?;
//!
//! // Middleware automatically wraps all handler operations
//! // Logging: All effect calls logged with context
//! // Metrics: Latency and throughput measured
//! // Tracing: Distributed traces across choreographic protocols
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ### Custom Middleware
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::AuraHandlerBuilder;
//! use aura_core::DeviceId;
//! use std::collections::HashMap;
//!
//! let device_id = DeviceId::generate();
//! let mut custom_config = HashMap::new();
//! custom_config.insert("rate_limit_per_second".to_string(), "100".to_string());
//! custom_config.insert("burst_capacity".to_string(), "10".to_string());
//!
//! let handler = AuraHandlerBuilder::new(device_id)
//!     .with_custom_middleware(
//!         "rate_limiter".to_string(),
//!         10, // Priority: lower numbers execute first
//!         custom_config,
//!     )
//!     .build_with_factory::<DefaultHandlerFactory>()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Platform-Specific Configuration
//!
//! ### Automatic Platform Detection
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{PlatformDetector, AuraHandlerBuilder};
//! use aura_core::DeviceId;
//!
//! // Detect platform capabilities
//! let platform = PlatformDetector::detect_platform()?;
//! println!("Detected platform: {} on {}", platform.os, platform.arch);
//! println!("Secure enclave available: {}", platform.has_secure_enclave);
//! println!("Storage backends: {:?}", platform.available_storage_backends);
//!
//! // Configure based on platform capabilities
//! let device_id = DeviceId::generate();
//! let mut builder = AuraHandlerBuilder::new(device_id);
//!
//! if platform.has_secure_enclave {
//!     builder = builder.with_hardware_security(true);
//! }
//!
//! let handler = builder.build_with_factory::<DefaultHandlerFactory>()?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ### Platform-Specific Storage
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerConfig, PlatformConfig};
//! use aura_core::DeviceId;
//!
//! let device_id = DeviceId::generate();
//! let mut config = AuraHandlerConfig::for_production(device_id);
//!
//! // Prefer platform-specific secure storage
//! #[cfg(target_os = "macos")]
//! {
//!     config.platform.preferred_storage_backends = vec![
//!         "keychain".to_string(),    // macOS Keychain
//!         "filesystem".to_string(),  // Fallback
//!     ];
//! }
//!
//! #[cfg(target_os = "windows")]
//! {
//!     config.platform.preferred_storage_backends = vec![
//!         "credential_store".to_string(), // Windows Credential Store
//!         "filesystem".to_string(),       // Fallback
//!     ];
//! }
//!
//! let handler = DefaultHandlerFactory::create_handler(config)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Error Handling
//!
//! ### Configuration Validation
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerConfig, FactoryError};
//! use aura_core::DeviceId;
//!
//! let device_id = DeviceId::generate();
//! let mut config = AuraHandlerConfig::for_production(device_id);
//!
//! // Validation catches configuration errors early
//! match config.validate() {
//!     Ok(()) => println!("Configuration valid"),
//!     Err(FactoryError::ConfigurationError { message }) => {
//!         eprintln!("Configuration error: {}", message);
//!         // Fix configuration and retry
//!     }
//!     Err(e) => eprintln!("Other error: {}", e),
//! }
//! ```
//!
//! ### Handler Creation Errors
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{DefaultHandlerFactory, FactoryError, EffectType};
//! use aura_core::DeviceId;
//!
//! let device_id = DeviceId::generate();
//! match DefaultHandlerFactory::for_production(device_id) {
//!     Ok(handler) => {
//!         // Handler created successfully
//!         println!("Handler ready with effects: {:?}",
//!                  DefaultHandlerFactory::supported_effect_types());
//!     }
//!     Err(FactoryError::RequiredEffectUnavailable { effect_type }) => {
//!         eprintln!("Required effect {:?} not available on this platform", effect_type);
//!         // Fallback to testing mode or fail gracefully
//!     }
//!     Err(FactoryError::HandlerCreationFailed { effect_type, source }) => {
//!         eprintln!("Failed to create handler for {:?}: {}", effect_type, source);
//!         // Platform-specific error handling
//!     }
//!     Err(e) => eprintln!("Handler creation failed: {}", e),
//! }
//! ```
//!
//! # Integration Patterns
//!
//! ## Application Integration
//!
//! ```rust,no_run
//! use aura_protocol::handlers::factory::{AuraHandlerFactory, DefaultHandlerFactory};
//! use aura_core::DeviceId;
//!
//! // Application-level handler management
//! pub struct AuraApplication {
//!     handler: Box<dyn aura_protocol::effects::AuraHandler>,
//!     device_id: DeviceId,
//! }
//!
//! impl AuraApplication {
//!     pub async fn new(device_id: DeviceId) -> Result<Self, Box<dyn std::error::Error>> {
//!         let handler = DefaultHandlerFactory::for_production(device_id)?;
//!         Ok(Self { handler, device_id })
//!     }
//!
//!     pub async fn initialize_account(&self) -> Result<(), Box<dyn std::error::Error>> {
//!         // Business logic using abstract effect interfaces
//!         let keypair = self.handler.crypto().generate_frost_keypair().await?;
//!         let account_data = self.handler.agent().create_account(keypair).await?;
//!         self.handler.storage().store_account(&account_data).await?;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! ## Testing Integration
//!
//! ```rust,no_run
//! #[cfg(test)]
//! mod tests {
//!     use super::*;
//!     use aura_protocol::handlers::factory::{AuraHandlerFactory, DefaultHandlerFactory};
//!     use aura_core::DeviceId;
//!
//!     #[test]
//!     async fn test_account_creation() -> Result<(), Box<dyn std::error::Error>> {
//!         // Fast, isolated testing
//!         let device_id = DeviceId::generate();
//!         let handler = DefaultHandlerFactory::for_testing(device_id)?;
//!
//!         let app = AuraApplication { handler, device_id };
//!         app.initialize_account().await?;
//!
//!         // Verify using test-specific introspection
//!         assert!(app.handler.storage().contains_account(&device_id).await?);
//!         Ok(())
//!     }
//!
//!     #[test]
//!     async fn test_with_simulation() -> Result<(), Box<dyn std::error::Error>> {
//!         // Deterministic simulation testing
//!         let device_id = DeviceId::generate();
//!         let handler = DefaultHandlerFactory::for_simulation(device_id, 42)?;
//!
//!         // Inject faults to test error handling
//!         handler.fault_injection().set_failure_rate(0.1).await?;
//!
//!         // Test with time control for timeout scenarios
//!         handler.time().advance_by(Duration::from_secs(3600)).await?;
//!
//!         Ok(())
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;

use async_trait::async_trait;
use aura_core::effects::{StorageEffects, StorageError, StorageStats};

use crate::handlers::{EffectType, ExecutionMode};
use aura_core::identifiers::DeviceId;

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
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([0u8; 16]));
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
/// execution modes and configurations. This trait abstracts the complex
/// process of handler creation, configuration, and dependency injection.
///
/// # Design Principles
///
/// - **Configuration-driven**: All behavior controlled through `AuraHandlerConfig`
/// - **Platform-aware**: Adapts to platform capabilities and constraints
/// - **Mode-specific**: Different implementations for production, testing, and simulation
/// - **Effect-typed**: Clear declaration of supported effect types
/// - **Fail-fast**: Configuration validation before expensive resource allocation
///
/// # Implementation Guidelines
///
/// When implementing this trait, factories should:
///
/// 1. **Validate Configuration**: Check all configuration parameters before allocation
/// 2. **Platform Detection**: Adapt to available platform capabilities
/// 3. **Resource Management**: Handle cleanup properly on creation failure
/// 4. **Effect Composition**: Build effect handler composition correctly
/// 5. **Middleware Integration**: Apply middleware in correct order
///
/// # Effect Dependencies
///
/// Factories must handle effect dependencies correctly:
///
/// ```text
/// Storage ──┐
///           ├──> Journal ──> Agent
/// Crypto  ──┘
///           ┌──> Network ──> Transport
/// Random  ──┘
/// ```
///
/// # Example Implementation
///
/// ```rust,no_run
/// use aura_protocol::handlers::factory::{AuraHandlerFactory, AuraHandlerConfig, FactoryError};
/// use aura_protocol::handlers::erased::BoxedHandler;
/// use aura_protocol::effects::EffectType;
///
/// pub struct MyHandlerFactory;
///
/// impl AuraHandlerFactory for MyHandlerFactory {
///     fn create_handler(config: AuraHandlerConfig) -> Result<BoxedHandler, FactoryError> {
///         // 1. Validate configuration
///         config.validate()?;
///
///         // 2. Check platform requirements
///         let platform = detect_platform()?;
///         for effect in &config.required_effects {
///             if !Self::supports_effect_type(*effect) {
///                 return Err(FactoryError::RequiredEffectUnavailable {
///                     effect_type: *effect
///                 });
///             }
///         }
///
///         // 3. Create effect handlers in dependency order
///         let console_handler = create_console_handler(&config)?;
///         let storage_handler = create_storage_handler(&config, &platform)?;
///         let crypto_handler = create_crypto_handler(&config, &storage_handler)?;
///
///         // 4. Apply middleware
///         let handler = apply_middleware(
///             compose_handlers(console_handler, storage_handler, crypto_handler),
///             &config.middleware
///         )?;
///
///         Ok(Box::new(handler))
///     }
///
///     fn supported_effect_types() -> Vec<EffectType> {
///         vec![
///             EffectType::Console,
///             EffectType::Random,
///             EffectType::Storage,
///             EffectType::Crypto,
///             // ... other supported effects
///         ]
///     }
/// }
/// ```
///
/// # Testing Support
///
/// Factories should provide special testing support:
///
/// - **Fast Creation**: Minimize startup time for test runs
/// - **Isolated State**: No shared state between test instances
/// - **Deterministic Behavior**: Reproducible results for property testing
/// - **Introspection**: Additional methods for verifying internal state
///
/// # Error Handling
///
/// Factory implementations should provide detailed error information:
///
/// - **Configuration Errors**: Clear messages about invalid configuration
/// - **Platform Errors**: Specific information about platform limitations
/// - **Dependency Errors**: Details about missing or failed dependencies
/// - **Resource Errors**: Information about resource allocation failures
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
        let storage: std::sync::Arc<dyn StorageEffects> =
            std::sync::Arc::new(PathStorageAdapter::default());
        Self::detect_platform_with_storage(&storage)
    }

    /// Detect the current platform using provided storage effects (for deterministic tests)
    pub fn detect_platform_with_storage(
        storage: &dyn StorageEffects,
    ) -> Result<PlatformInfo, FactoryError> {
        Ok(PlatformInfo {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            has_secure_enclave: Self::detect_secure_enclave(storage),
            available_storage_backends: Self::detect_storage_backends(),
            available_network_interfaces: Self::detect_network_interfaces(),
        })
    }

    /// Detect if secure enclave is available
    fn detect_secure_enclave(storage: &dyn StorageEffects) -> bool {
        // Platform-specific detection logic
        match std::env::consts::OS {
            "macos" => {
                // Check for Apple Secure Enclave on macOS
                std::env::consts::ARCH == "aarch64"
                    || std::process::Command::new("system_profiler")
                        .args(["SPHardwareDataType"])
                        .output()
                        .map(|output| String::from_utf8_lossy(&output.stdout).contains("Apple"))
                        .unwrap_or(false)
            }
            "linux" => {
                // Check for Intel SGX or AMD SEV on Linux
                std::path::Path::new("/dev/sgx_enclave").exists()
                    || std::path::Path::new("/dev/sgx/enclave").exists()
                    || futures::executor::block_on(async {
                        storage
                            .retrieve("/proc/cpuinfo")
                            .await
                            .map(|content| {
                                content
                                    .map(|bytes| String::from_utf8_lossy(&bytes).to_lowercase())
                                    .map(|cpuinfo| cpuinfo.contains("sgx") || cpuinfo.contains("sev"))
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false)
                    })
            }
            "windows" => {
                // Check for Intel SGX on Windows (conservative approach)
                std::env::var("PROCESSOR_IDENTIFIER")
                    .map(|proc| proc.to_lowercase().contains("intel"))
                    .unwrap_or(false)
            }
            _ => false, // Conservative default for other platforms
        }
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
        let mut interfaces = Vec::new();

        // Always include loopback
        interfaces.push("loopback".to_string());

        // Platform-specific interface detection
        match std::env::consts::OS {
            "linux" | "macos" => {
                // Check for common network interfaces on Unix-like systems
                if let Ok(output) = std::process::Command::new("ifconfig").output() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':') {
                            if let Some(iface_name) = line.split(':').next() {
                                let name = iface_name.trim();
                                if !name.is_empty() && name != "lo" && name != "lo0" {
                                    interfaces.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
            "windows" => {
                // Check network adapters on Windows
                if let Ok(output) = std::process::Command::new("ipconfig")
                    .args(["/all"])
                    .output()
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if line.contains("adapter") && line.contains(':') {
                            interfaces.push("ethernet".to_string());
                            break;
                        }
                    }
                }
            }
            _ => {
                // Conservative default for other platforms
                interfaces.push("default".to_string());
            }
        }

        // Ensure we always have at least one interface
        if interfaces.len() == 1 {
            interfaces.push("default".to_string());
        }

        interfaces
    }
}

// Use the proper FilesystemStorageHandler from aura-effects instead of reimplementing
// This maintains the architectural boundary and avoids direct runtime/filesystem usage outside effects layer
use aura_effects::storage::FilesystemStorageHandler;

/// Create a path-based storage handler for platform detection
fn create_path_storage_adapter() -> FilesystemStorageHandler {
    FilesystemStorageHandler::with_default_path()
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
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([0u8; 16]));

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
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));
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
        let device_id = DeviceId::from(uuid::Uuid::from_bytes([1u8; 16]));

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

        // Should have loopback network interface
        assert!(platform.has_network_interface("loopback"));

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
