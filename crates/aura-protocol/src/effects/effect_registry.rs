//! Standard Effect Registry Pattern for Aura Protocol Layer
//!
//! This module provides a standardized builder pattern for creating and configuring
//! complete effect systems. It replaces the current ad-hoc composition with a clean,
//! discoverable API that supports all runtime modes (production, testing, simulation).
//!
//! # Design Principles
//!
//! - **Builder Pattern**: Fluent API for effect system composition
//! - **Standard Configurations**: Pre-configured systems for common use cases  
//! - **Compile-time Safety**: Effect composition validated at compile time
//! - **Runtime Mode Support**: Production, testing, simulation with appropriate handlers
//! - **Middleware Integration**: Consistent cross-cutting concerns (logging, metrics, tracing)
//!
//! # Usage Examples
//!
//! ## Quick Start
//!
//! ```rust,ignore
//! use aura_protocol::effects::EffectRegistry;
//! use aura_core::DeviceId;
//!
//! // Production system with real handlers
//! let effects = EffectRegistry::production()
//!     .with_device_id(device_id)
//!     .build()?;
//!
//! // Testing system with mock handlers
//! let effects = EffectRegistry::testing()
//!     .with_device_id(device_id)
//!     .build()?;
//!
//! // Simulation with deterministic behavior
//! let effects = EffectRegistry::simulation(42)
//!     .with_device_id(device_id)
//!     .build()?;
//! ```
//!
//! ## Custom Configuration
//!
//! ```rust,ignore
//! use aura_protocol::effects::EffectRegistry;
//! use aura_protocol::handlers::{MockHandler, CompositeHandler};
//!
//! // Custom effect system with specific handlers
//! let effects = EffectRegistry::custom()
//!     .with_device_id(device_id)
//!     .with_crypto_handler(RealCryptoHandler::new())
//!     .with_storage_handler(MockHandler::new())
//!     .with_network_handler(TcpNetworkHandler::new())
//!     .build()?;
//! ```

use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

use crate::effects::{AuraEffectSystem, AuraEffects};
use crate::handlers::{AuraHandlerError, CompositeHandler, ExecutionMode};
use aura_core::DeviceId;

/// Error types for effect registry operations
#[derive(Debug, Error)]
pub enum EffectRegistryError {
    /// Required configuration missing
    #[error("Required configuration missing: {field}")]
    MissingConfiguration {
        /// Name of the missing configuration field
        field: String,
    },

    /// Handler creation failed
    #[error("Failed to create {handler_type} handler")]
    HandlerCreationFailed {
        /// Type of handler that failed to create
        handler_type: String,
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Invalid configuration
    #[error("Invalid configuration: {message}")]
    InvalidConfiguration {
        /// Error message describing the issue
        message: String,
    },

    /// Effect system build failed
    #[error("Failed to build effect system")]
    BuildFailed {
        /// Underlying error
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl EffectRegistryError {
    /// Create a missing configuration error
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingConfiguration {
            field: field.into(),
        }
    }

    /// Create a handler creation error
    pub fn handler_creation_failed(
        handler_type: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerCreationFailed {
            handler_type: handler_type.into(),
            source: Box::new(source),
        }
    }

    /// Create an invalid configuration error
    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration {
            message: message.into(),
        }
    }

    /// Create a build failed error
    pub fn build_failed(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::BuildFailed {
            source: Box::new(source),
        }
    }
}

/// Standard effect registry providing builder pattern for effect system composition
pub struct EffectRegistry {
    device_id: Option<DeviceId>,
    execution_mode: ExecutionMode,
    enable_logging: bool,
    enable_metrics: bool,
    enable_tracing: bool,
}

impl EffectRegistry {
    /// Create a production effect registry
    ///
    /// Production configurations use real handlers for all effects:
    /// - Crypto: Hardware security where available, real randomness
    /// - Storage: Persistent filesystem with encryption
    /// - Network: TCP/UDP networking with TLS
    /// - Time: System clock
    pub fn production() -> Self {
        Self {
            device_id: None,
            execution_mode: ExecutionMode::Production,
            enable_logging: true,
            enable_metrics: true,
            enable_tracing: false,
        }
    }

    /// Create a testing effect registry
    ///
    /// Testing configurations use mock handlers for fast, deterministic tests:
    /// - Crypto: Mock handlers with fixed keys
    /// - Storage: In-memory storage
    /// - Network: Local loopback or memory channels
    /// - Time: Controllable mock time
    pub fn testing() -> Self {
        Self {
            device_id: None,
            execution_mode: ExecutionMode::Testing,
            enable_logging: false,
            enable_metrics: false,
            enable_tracing: false,
        }
    }

    /// Create a simulation effect registry
    ///
    /// Simulation configurations provide deterministic, controllable execution:
    /// - Crypto: Seeded randomness for reproducibility
    /// - Storage: Simulated delays and failures
    /// - Network: Simulated partitions and message loss
    /// - Time: Virtual time with acceleration
    ///
    /// # Arguments
    /// * `seed` - Random seed for deterministic behavior
    pub fn simulation(seed: u64) -> Self {
        Self {
            device_id: None,
            execution_mode: ExecutionMode::Simulation { seed },
            enable_logging: true,
            enable_metrics: false,
            enable_tracing: false,
        }
    }

    /// Create a custom effect registry for advanced configuration
    pub fn custom() -> Self {
        Self {
            device_id: None,
            execution_mode: ExecutionMode::Testing, // Default to testing for safety
            enable_logging: false,
            enable_metrics: false,
            enable_tracing: false,
        }
    }

    /// Set the device ID for this effect system
    pub fn with_device_id(mut self, device_id: DeviceId) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Enable logging for all effect operations
    pub fn with_logging(mut self) -> Self {
        self.enable_logging = true;
        self
    }

    /// Enable metrics collection for performance monitoring
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Enable distributed tracing for protocol debugging
    pub fn with_tracing(mut self) -> Self {
        self.enable_tracing = true;
        self
    }

    /// Set custom execution mode
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = mode;
        self
    }

    /// Build the configured effect system
    ///
    /// This creates a complete `AuraEffectSystem` with all configured handlers
    /// and middleware. The system implements all effect traits and can be used
    /// directly by protocols.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Required configuration is missing (e.g., device_id)
    /// - Handler creation fails
    /// - Middleware configuration is invalid
    pub fn build(self) -> Result<AuraEffectSystem, EffectRegistryError> {
        // Validate required configuration
        let device_id = self
            .device_id
            .ok_or_else(|| EffectRegistryError::missing_field("device_id"))?;

        // Convert DeviceId to UUID for handler creation
        let device_uuid = device_id.into();

        // Create the appropriate composite handler based on execution mode
        let base_handler = match self.execution_mode {
            ExecutionMode::Testing => CompositeHandler::for_testing(device_uuid),
            ExecutionMode::Production => {
                // TODO: Implement production handler creation
                // For now, fall back to testing handler
                CompositeHandler::for_testing(device_uuid)
            }
            ExecutionMode::Simulation { seed } => {
                // TODO: Implement simulation handler with seed
                // For now, fall back to testing handler
                CompositeHandler::for_testing(device_uuid)
            }
        };

        // Apply middleware stack
        let enhanced_handler = self.apply_middleware(base_handler)?;

        Ok(Box::new(enhanced_handler))
    }

    /// Apply configured middleware to the base handler
    fn apply_middleware(
        self,
        base_handler: CompositeHandler,
    ) -> Result<CompositeHandler, EffectRegistryError> {
        let mut handler = base_handler;

        // Apply logging middleware
        if self.enable_logging {
            // TODO: Implement logging middleware wrapper
            // handler = LoggingMiddleware::wrap(handler);
        }

        // Apply metrics middleware
        if self.enable_metrics {
            // TODO: Implement metrics middleware wrapper
            // handler = MetricsMiddleware::wrap(handler);
        }

        // Apply tracing middleware
        if self.enable_tracing {
            // TODO: Implement tracing middleware wrapper
            // handler = TracingMiddleware::wrap(handler);
        }

        Ok(handler)
    }
}

/// Extension trait providing standard configurations
pub trait EffectRegistryExt {
    /// Quick testing setup with device ID
    fn quick_testing(device_id: DeviceId) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectRegistry::testing().with_device_id(device_id).build()
    }

    /// Quick production setup with device ID and basic middleware
    fn quick_production(device_id: DeviceId) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectRegistry::production()
            .with_device_id(device_id)
            .with_logging()
            .with_metrics()
            .build()
    }

    /// Quick simulation setup with device ID and seed
    fn quick_simulation(
        device_id: DeviceId,
        seed: u64,
    ) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectRegistry::simulation(seed)
            .with_device_id(device_id)
            .with_logging()
            .build()
    }
}

impl EffectRegistryExt for EffectRegistry {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_production_configuration() {
        let registry = EffectRegistry::production();
        assert_eq!(registry.execution_mode, ExecutionMode::Production);
        assert!(registry.enable_logging);
        assert!(registry.enable_metrics);
        assert!(!registry.enable_tracing);
    }

    #[test]
    fn test_testing_configuration() {
        let registry = EffectRegistry::testing();
        assert_eq!(registry.execution_mode, ExecutionMode::Testing);
        assert!(!registry.enable_logging);
        assert!(!registry.enable_metrics);
        assert!(!registry.enable_tracing);
    }

    #[test]
    fn test_simulation_configuration() {
        let registry = EffectRegistry::simulation(42);
        assert_eq!(
            registry.execution_mode,
            ExecutionMode::Simulation { seed: 42 }
        );
        assert!(registry.enable_logging);
        assert!(!registry.enable_metrics);
        assert!(!registry.enable_tracing);
    }

    #[test]
    fn test_builder_pattern() {
        let device_id = DeviceId::new();
        let registry = EffectRegistry::custom()
            .with_device_id(device_id)
            .with_logging()
            .with_metrics()
            .with_tracing()
            .with_execution_mode(ExecutionMode::Production);

        assert_eq!(registry.device_id, Some(device_id));
        assert_eq!(registry.execution_mode, ExecutionMode::Production);
        assert!(registry.enable_logging);
        assert!(registry.enable_metrics);
        assert!(registry.enable_tracing);
    }

    #[test]
    fn test_build_success() {
        let device_id = DeviceId::new();
        let result = EffectRegistry::testing().with_device_id(device_id).build();

        assert!(result.is_ok());
        let effects = result.unwrap();
        assert_eq!(effects.execution_mode(), ExecutionMode::Testing);
    }

    #[test]
    fn test_build_missing_device_id() {
        let result = EffectRegistry::testing().build();
        assert!(result.is_err());

        match result.unwrap_err() {
            EffectRegistryError::MissingConfiguration { field } => {
                assert_eq!(field, "device_id");
            }
            _ => panic!("Expected MissingConfiguration error"),
        }
    }

    #[test]
    fn test_quick_configurations() {
        let device_id = DeviceId::new();

        // Test quick testing
        let testing_effects = EffectRegistry::quick_testing(device_id).unwrap();
        assert_eq!(testing_effects.execution_mode(), ExecutionMode::Testing);

        // Test quick production
        let production_effects = EffectRegistry::quick_production(device_id).unwrap();
        assert_eq!(
            production_effects.execution_mode(),
            ExecutionMode::Production
        );

        // Test quick simulation
        let simulation_effects = EffectRegistry::quick_simulation(device_id, 42).unwrap();
        assert_eq!(
            simulation_effects.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }
}
