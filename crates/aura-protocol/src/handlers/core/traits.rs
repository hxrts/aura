//! Factory trait definition
//!
//! Core trait for creating Aura handlers with consistent configuration patterns
//! across different execution modes.

use aura_core::identifiers::DeviceId;

use super::{AuraHandlerConfig, FactoryError};
use crate::handlers::EffectType;

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
/// ```rust,ignore
/// use aura_protocol::handlers::core::{AuraHandlerFactory, AuraHandlerConfig, FactoryError, BoxedHandler};
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
        // Production handler assembly is owned by aura-agent.
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
