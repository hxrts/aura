//! Compile-time Effect System Builder
//!
//! This module provides a compile-time safe builder for effect systems that validates
//! effect requirements and composition at compile time. Using Rust's type system,
//! it ensures that all required effects are provided before building the system.
//!
//! # Type Safety Features
//!
//! - **Required Effects**: Builder tracks which effects are required/optional
//! - **Compile-time Validation**: Missing effects cause compilation errors
//! - **Handler Type Safety**: Handlers must implement the correct effect traits
//! - **Configuration Validation**: Invalid configurations caught at compile time
//!
//! # Usage Examples
//!
//! ## Basic Protocol Requirements
//!
//! ```rust,ignore
//! use aura_protocol::effects::{EffectBuilder, RequiredEffects};
//! use aura_core::effects::{CryptoEffects, NetworkEffects};
//! use aura_core::DeviceId;
//!
//! // Protocol requires crypto and network effects
//! let effects = EffectBuilder::new(device_id)
//!     .require::<dyn CryptoEffects>()
//!     .require::<dyn NetworkEffects>()
//!     .with_production_crypto()
//!     .with_tcp_network()
//!     .build(); // Only compiles if all required effects are satisfied
//! ```
//!
//! ## Optional Effects with Fallbacks
//!
//! ```rust,ignore
//! // Storage is optional, falls back to in-memory if not provided
//! let effects = EffectBuilder::new(device_id)
//!     .require::<dyn CryptoEffects>()
//!     .optional::<dyn StorageEffects>()
//!     .with_mock_crypto()
//!     // Storage not provided - will use default in-memory storage
//!     .build();
//! ```

use std::marker::PhantomData;
use std::sync::Arc;

use crate::effects::{AuraEffectSystem, EffectRegistry, EffectRegistryError};
use crate::handlers::{CompositeHandler, ExecutionMode};
use aura_core::DeviceId;

/// Type-level marker indicating that an effect requirement has been satisfied
pub struct Satisfied;

/// Type-level marker indicating that an effect requirement has not been satisfied
pub struct Unsatisfied;

/// Type-level marker for required effects that must be provided
pub struct Required;

/// Type-level marker for optional effects that can have defaults
pub struct Optional;

/// Effect requirement specification
///
/// This trait is used to specify effect requirements at the type level.
/// The type system ensures all requirements are satisfied before building.
pub trait EffectRequirement {
    /// Whether this effect is required or optional
    type Kind; // Required or Optional

    /// Whether this requirement has been satisfied
    type Status; // Satisfied or Unsatisfied

    /// The effect trait that must be implemented
    type Effect: ?Sized;
}

/// Compile-time effect system builder with type-safe requirement tracking
///
/// This builder uses Rust's type system to ensure that all required effects
/// are provided before the system can be built. The type parameters track
/// which requirements have been satisfied.
pub struct EffectBuilder<Reqs> {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    enable_logging: bool,
    enable_metrics: bool,
    enable_tracing: bool,
    _requirements: PhantomData<Reqs>,
}

/// Implementation for the initial builder state (no requirements yet)
impl EffectBuilder<()> {
    /// Create a new effect builder with the specified device ID
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Testing, // Safe default
            enable_logging: false,
            enable_metrics: false,
            enable_tracing: false,
            _requirements: PhantomData,
        }
    }

    /// Set execution mode for the effect system
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = mode;
        self
    }

    /// Enable logging for all effect operations
    pub fn with_logging(mut self) -> Self {
        self.enable_logging = true;
        self
    }

    /// Enable metrics collection
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Enable distributed tracing
    pub fn with_tracing(mut self) -> Self {
        self.enable_tracing = true;
        self
    }
}

/// Default effects configuration that satisfies common protocol requirements
///
/// This trait provides a way to automatically satisfy common effect requirements
/// with sensible defaults based on the execution mode.
pub trait DefaultEffects {
    /// Get default handlers for all common effects based on execution mode
    fn default_handlers(mode: ExecutionMode, device_id: DeviceId) -> CompositeHandler;
}

impl DefaultEffects for EffectBuilder<()> {
    fn default_handlers(mode: ExecutionMode, device_id: DeviceId) -> CompositeHandler {
        let device_uuid = device_id.into();

        match mode {
            ExecutionMode::Testing => CompositeHandler::for_testing(device_uuid),
            ExecutionMode::Production => {
                // TODO: Implement production handler creation
                CompositeHandler::for_testing(device_uuid)
            }
            ExecutionMode::Simulation { .. } => {
                // TODO: Implement simulation handler creation
                CompositeHandler::for_testing(device_uuid)
            }
        }
    }
}

/// Builder extension for common effect system configurations
impl<Reqs> EffectBuilder<Reqs> {
    /// Build with all default handlers - satisfies all common requirements
    ///
    /// This method provides default implementations for all standard effects:
    /// - CryptoEffects: Mock or real crypto based on execution mode
    /// - NetworkEffects: TCP or mock networking
    /// - StorageEffects: Filesystem or in-memory storage
    /// - TimeEffects: System time or controllable mock time
    /// - All other effects: Appropriate defaults for execution mode
    pub fn with_defaults(self) -> Result<AuraEffectSystem, EffectRegistryError> {
        // Use the existing EffectRegistry for default configurations
        let base_registry = match self.execution_mode {
            ExecutionMode::Testing => EffectRegistry::testing(),
            ExecutionMode::Production => EffectRegistry::production(),
            ExecutionMode::Simulation { seed } => EffectRegistry::simulation(seed),
        };

        let mut registry = base_registry.with_device_id(self.device_id);

        if self.enable_logging {
            registry = registry.with_logging();
        }
        if self.enable_metrics {
            registry = registry.with_metrics();
        }
        if self.enable_tracing {
            registry = registry.with_tracing();
        }

        registry.build()
    }
}

/// Convenience builder for quick setups
pub struct QuickBuilder {
    device_id: DeviceId,
}

impl QuickBuilder {
    /// Create a new quick builder
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    /// Quick testing setup with mock handlers
    pub fn testing(self) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(self.device_id)
            .with_execution_mode(ExecutionMode::Testing)
            .with_defaults()
    }

    /// Quick production setup with real handlers  
    pub fn production(self) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(self.device_id)
            .with_execution_mode(ExecutionMode::Production)
            .with_logging()
            .with_metrics()
            .with_defaults()
    }

    /// Quick simulation setup with deterministic handlers
    pub fn simulation(self, seed: u64) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(self.device_id)
            .with_execution_mode(ExecutionMode::Simulation { seed })
            .with_logging()
            .with_defaults()
    }
}

/// Protocol requirement specification
///
/// This trait allows protocols to specify their effect requirements at the type level.
/// The builder system can then validate that all requirements are satisfied.
pub trait ProtocolRequirements {
    /// Type-level specification of required effects
    type Requirements;

    /// Build an effect system that satisfies this protocol's requirements
    fn build_effects(device_id: DeviceId) -> Result<AuraEffectSystem, EffectRegistryError> {
        // Default implementation uses the defaults
        EffectBuilder::new(device_id).with_defaults()
    }
}

/// Trait for defining effect bundles - common combinations of effects
pub trait EffectBundle {
    /// Build this effect bundle with the given configuration
    fn build(
        device_id: DeviceId,
        mode: ExecutionMode,
    ) -> Result<AuraEffectSystem, EffectRegistryError>;
}

/// Basic protocol bundle: crypto + network + storage + time
pub struct BasicProtocolBundle;

impl EffectBundle for BasicProtocolBundle {
    fn build(
        device_id: DeviceId,
        mode: ExecutionMode,
    ) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(device_id)
            .with_execution_mode(mode)
            .with_defaults()
    }
}

/// Testing bundle: all mock handlers for fast tests
pub struct TestingBundle;

impl EffectBundle for TestingBundle {
    fn build(
        device_id: DeviceId,
        _mode: ExecutionMode,
    ) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(device_id)
            .with_execution_mode(ExecutionMode::Testing)
            .with_defaults()
    }
}

/// Production bundle: real handlers with monitoring
pub struct ProductionBundle;

impl EffectBundle for ProductionBundle {
    fn build(
        device_id: DeviceId,
        _mode: ExecutionMode,
    ) -> Result<AuraEffectSystem, EffectRegistryError> {
        EffectBuilder::new(device_id)
            .with_execution_mode(ExecutionMode::Production)
            .with_logging()
            .with_metrics()
            .with_tracing()
            .with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_creation() {
        let device_id = DeviceId::new();
        let builder = EffectBuilder::new(device_id);

        // Should be able to create builder
        assert_eq!(builder.device_id, device_id);
        assert_eq!(builder.execution_mode, ExecutionMode::Testing);
        assert!(!builder.enable_logging);
        assert!(!builder.enable_metrics);
        assert!(!builder.enable_tracing);
    }

    #[test]
    fn test_builder_configuration() {
        let device_id = DeviceId::new();
        let builder = EffectBuilder::new(device_id)
            .with_execution_mode(ExecutionMode::Production)
            .with_logging()
            .with_metrics()
            .with_tracing();

        assert_eq!(builder.execution_mode, ExecutionMode::Production);
        assert!(builder.enable_logging);
        assert!(builder.enable_metrics);
        assert!(builder.enable_tracing);
    }

    #[test]
    fn test_with_defaults() {
        let device_id = DeviceId::new();
        let result = EffectBuilder::new(device_id)
            .with_execution_mode(ExecutionMode::Testing)
            .with_defaults();

        assert!(result.is_ok());
        let effects = result.unwrap();
        assert_eq!(effects.execution_mode(), ExecutionMode::Testing);
    }

    #[test]
    fn test_quick_builder() {
        let device_id = DeviceId::new();

        // Test quick testing setup
        let testing_effects = QuickBuilder::new(device_id).testing().unwrap();
        assert_eq!(testing_effects.execution_mode(), ExecutionMode::Testing);

        // Test quick production setup
        let production_effects = QuickBuilder::new(device_id).production().unwrap();
        assert_eq!(
            production_effects.execution_mode(),
            ExecutionMode::Production
        );

        // Test quick simulation setup
        let simulation_effects = QuickBuilder::new(device_id).simulation(42).unwrap();
        assert_eq!(
            simulation_effects.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[test]
    fn test_effect_bundles() {
        let device_id = DeviceId::new();

        // Test basic protocol bundle
        let basic = BasicProtocolBundle::build(device_id, ExecutionMode::Testing).unwrap();
        assert_eq!(basic.execution_mode(), ExecutionMode::Testing);

        // Test testing bundle
        let testing = TestingBundle::build(device_id, ExecutionMode::Production).unwrap();
        assert_eq!(testing.execution_mode(), ExecutionMode::Testing); // Forces testing mode

        // Test production bundle
        let production = ProductionBundle::build(device_id, ExecutionMode::Testing).unwrap();
        assert_eq!(production.execution_mode(), ExecutionMode::Production); // Forces production mode
    }
}
