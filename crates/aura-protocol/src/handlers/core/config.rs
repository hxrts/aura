//! Handler configuration types
//!
//! Configuration structures for the handler factory system including
//! main handler config, middleware config, platform config, and simulation config.

use std::collections::HashMap;
use std::time::Duration;

use aura_core::identifiers::DeviceId;

use crate::handlers::{EffectType, ExecutionMode};
use super::error::FactoryError;

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
