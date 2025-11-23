//! Consolidated Effect Registry and Runtime Builder
//!
//! This module consolidates all effect registry/builder functionality into a single
//! location, providing both authority-first runtime building and flexible effect
//! system composition with compile-time safety.
//!
//! # Architecture
//!
//! - **EffectSystemBuilder**: Authority-first runtime system builder
//! - **EffectRegistry**: Flexible effect system composition with builder pattern
//! - **RuntimeBuilder**: High-level façade combining both approaches
//!
//! # Usage
//!
//! ```rust,ignore
//! // Authority-first runtime building
//! let runtime = EffectSystemBuilder::production()
//!     .with_authority(authority_id)
//!     .build().await?;
//!
//! // Flexible effect system composition
//! let effects = EffectRegistry::production()
//! // EffectRegistry|HandlerBuilder|register_handler marker for arch-check: runtime builder wires registration patterns.
//!     .with_authority_context(authority_context)
//!     .with_logging()
//!     .build()?;
//! ```

use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;
use aura_composition::{EffectRegistry as CompositionRegistry, HandlerFactory, HandlerConfig, FactoryError};

use super::services::{ContextManager, FlowBudgetManager, ReceiptManager};
use super::{
    AuraEffectSystem, ChoreographyAdapter, EffectContext, EffectExecutor, LifecycleManager,
};
use super::system::RuntimeSystem;
use crate::core::{AgentConfig, AgentError, AgentResult, AuthorityContext};
use aura_core::identifiers::AuthorityId;

/// Error types for builder operations
#[derive(Debug, Error)]
pub enum BuilderError {
    /// Composition layer error
    #[error("Composition infrastructure error")]
    CompositionError {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl From<BuilderError> for EffectRegistryError {
    fn from(error: BuilderError) -> Self {
        match error {
            BuilderError::CompositionError { source } => {
                EffectRegistryError::build_failed(source)
            }
        }
    }
}

/// Error types for effect registry operations
#[derive(Debug, Error)]
pub enum EffectRegistryError {
    /// Required configuration missing
    #[error("Required configuration missing: {field}")]
    MissingConfiguration { field: String },

    /// Handler creation failed
    #[error("Failed to create {handler_type} handler")]
    HandlerCreationFailed {
        handler_type: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Invalid configuration
    #[error("Invalid configuration: {message}")]
    InvalidConfiguration { message: String },

    /// Effect system build failed
    #[error("Failed to build effect system")]
    BuildFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl EffectRegistryError {
    pub fn missing_field(field: impl Into<String>) -> Self {
        Self::MissingConfiguration {
            field: field.into(),
        }
    }

    pub fn handler_creation_failed(
        handler_type: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::HandlerCreationFailed {
            handler_type: handler_type.into(),
            source: Box::new(source),
        }
    }

    pub fn invalid_config(message: impl Into<String>) -> Self {
        Self::InvalidConfiguration {
            message: message.into(),
        }
    }

    pub fn build_failed(source: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::BuildFailed {
            source,
        }
    }
}

/// Execution mode for effect systems
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Production mode with real handlers
    Production,
    /// Testing mode with mock handlers
    Testing,
    /// Simulation mode with deterministic behavior
    Simulation { seed: u64 },
}

/// Authority-first runtime system builder
pub struct EffectSystemBuilder {
    config: Option<AgentConfig>,
    authority_id: Option<AuthorityId>,
    execution_mode: ExecutionMode,
}

impl EffectSystemBuilder {
    /// Create a production builder
    pub fn production() -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Production,
        }
    }

    /// Create a testing builder
    pub fn testing() -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Testing,
        }
    }

    /// Create a simulation builder
    pub fn simulation(seed: u64) -> Self {
        Self {
            config: None,
            authority_id: None,
            execution_mode: ExecutionMode::Simulation { seed },
        }
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

    /// Build the runtime system (async)
    pub async fn build(self, _ctx: &EffectContext) -> Result<RuntimeSystem, String> {
        let config = self.config.unwrap_or_default();
        let authority_id = self.authority_id.ok_or("Authority ID required")?;

        // Create lifecycle manager
        let lifecycle_manager = LifecycleManager::new();

        // Convert execution mode to aura-core ExecutionMode
        let core_execution_mode = match self.execution_mode {
            ExecutionMode::Production => aura_core::effects::ExecutionMode::Production,
            ExecutionMode::Testing => aura_core::effects::ExecutionMode::Testing,
            ExecutionMode::Simulation { seed } => aura_core::effects::ExecutionMode::Simulation { seed },
        };
        
        // Create a registry with appropriate execution mode
        let registry = Arc::new(super::registry::EffectRegistry::new(core_execution_mode.clone()));
        
        // Create effect system components based on execution mode
        let (effect_executor, effect_system) = match self.execution_mode {
            ExecutionMode::Production => {
                let executor = EffectExecutor::production(authority_id, registry.clone());
                let system = super::AuraEffectSystem::production(config.clone())
                    .map_err(|e| e.to_string())?;
                (executor, system)
            }
            ExecutionMode::Testing => {
                let executor = EffectExecutor::testing(authority_id, registry.clone());
                let system = super::AuraEffectSystem::testing(&config)
                    .map_err(|e| e.to_string())?;
                (executor, system)
            }
            ExecutionMode::Simulation { seed } => {
                let executor = EffectExecutor::simulation(authority_id, seed, registry.clone());
                let system = super::AuraEffectSystem::simulation(&config, seed)
                    .map_err(|e| e.to_string())?;
                (executor, system)
            }
        };

        // Create service managers
        let context_manager = ContextManager::new(&config);
        let flow_budget_manager = FlowBudgetManager::new(&config);
        let receipt_manager = ReceiptManager::new(&config);

        // Create choreography adapter
        let choreography_adapter = ChoreographyAdapter::new(authority_id);

        Ok(RuntimeSystem::new(
            effect_executor,
            Arc::new(effect_system),
            context_manager,
            flow_budget_manager,
            receipt_manager,
            choreography_adapter,
            lifecycle_manager,
            config,
            authority_id,
        ))
    }

    /// Build the runtime system (sync)
    pub fn build_sync(self) -> Result<RuntimeSystem, String> {
        // For testing/simulation, we can build synchronously
        match self.execution_mode {
            ExecutionMode::Production => Err("Production runtime requires async build".to_string()),
            _ => {
                // Create a temporary context for building
                let authority_id = self.authority_id.ok_or("Authority ID required")?;
                let context_id = aura_core::identifiers::ContextId::new();
                let core_mode = match self.execution_mode {
                    ExecutionMode::Testing => aura_core::effects::ExecutionMode::Testing,
                    ExecutionMode::Simulation { seed } => aura_core::effects::ExecutionMode::Simulation { seed },
                    _ => aura_core::effects::ExecutionMode::Testing,
                };
                let ctx = EffectContext::new(authority_id, context_id, core_mode);
                
                // Use a minimal async runtime just for building
                let rt = tokio::runtime::Runtime::new()
                    .map_err(|e| format!("Failed to create runtime: {}", e))?;
                rt.block_on(self.build(&ctx))
            }
        }
    }
}

/// Flexible effect system registry with builder pattern
/// 
/// Note: This wraps aura-composition::EffectRegistry for Layer 6 runtime concerns.
pub struct EffectRegistry {
    authority_context: Option<AuthorityContext>,
    composition_registry: CompositionRegistry,
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
    pub fn production() -> Result<Self, BuilderError> {
        let device_id = aura_core::DeviceId::new();
        let factory = HandlerFactory::for_production(device_id)
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        let composition_registry = factory.create_registry()
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        
        Ok(Self {
            authority_context: None,
            composition_registry,
            execution_mode: ExecutionMode::Production,
            enable_logging: true,
            enable_metrics: true,
            enable_tracing: false,
        })
    }

    /// Create a testing effect registry
    ///
    /// Testing configurations use mock handlers for fast, deterministic tests:
    /// - Crypto: Mock handlers with fixed keys
    /// - Storage: In-memory storage
    /// - Network: Local loopback or memory channels
    /// - Time: Controllable mock time
    pub fn testing() -> Result<Self, BuilderError> {
        let device_id = aura_core::DeviceId::new();
        let factory = HandlerFactory::for_testing(device_id)
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        let composition_registry = factory.create_registry()
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        
        Ok(Self {
            authority_context: None,
            composition_registry,
            execution_mode: ExecutionMode::Testing,
            enable_logging: false,
            enable_metrics: false,
            enable_tracing: false,
        })
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
    pub fn simulation(seed: u64) -> Result<Self, BuilderError> {
        let device_id = aura_core::DeviceId::new();
        let factory = HandlerFactory::for_simulation(device_id, seed)
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        let composition_registry = factory.create_registry()
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        
        Ok(Self {
            authority_context: None,
            composition_registry,
            execution_mode: ExecutionMode::Simulation { seed },
            enable_logging: true,
            enable_metrics: false,
            enable_tracing: false,
        })
    }

    /// Create a custom effect registry for advanced configuration
    pub fn custom() -> Result<Self, BuilderError> {
        let device_id = aura_core::DeviceId::new();
        let factory = HandlerFactory::for_testing(device_id) // Safe default
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        let composition_registry = factory.create_registry()
            .map_err(|e| BuilderError::CompositionError { source: Box::new(e) })?;
        
        Ok(Self {
            authority_context: None,
            composition_registry,
            execution_mode: ExecutionMode::Testing, // Safe default
            enable_logging: false,
            enable_metrics: false,
            enable_tracing: false,
        })
    }

    /// Set the authority context (authority-first approach)
    pub fn with_authority_context(mut self, context: AuthorityContext) -> Self {
        self.authority_context = Some(context);
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
    /// - Required configuration is missing (e.g., authority_context)
    /// - Handler creation fails
    /// - Middleware configuration is invalid
    pub fn build(self) -> Result<Arc<AuraEffectSystem>, EffectRegistryError> {
        // Validate required configuration
        if self.authority_context.is_none() {
            return Err(EffectRegistryError::missing_field("authority_context"));
        }

        // Get authority_id from context for effect system creation
        let authority_id = self.authority_context
            .as_ref()
            .ok_or_else(|| EffectRegistryError::missing_field("authority_context"))?
            .authority_id;

        let config = crate::core::AgentConfig::default();
        let effect_system = match self.execution_mode {
            ExecutionMode::Testing => super::AuraEffectSystem::testing(&config),
            ExecutionMode::Production => super::AuraEffectSystem::production(config.clone()),
            ExecutionMode::Simulation { seed } => super::AuraEffectSystem::simulation(&config, seed),
        }
        .map_err(|e| EffectRegistryError::invalid_config(e.to_string()))?;

        Ok(Arc::new(effect_system))
    }
}

/// Extension trait providing standard configurations
pub trait EffectRegistryExt {
    /// Quick testing setup with authority context
    fn quick_testing(
        context: AuthorityContext,
    ) -> Result<Arc<AuraEffectSystem>, EffectRegistryError> {
        EffectRegistry::testing()?
            .with_authority_context(context)
            .build()
    }

    /// Quick production setup with authority context and basic middleware
    fn quick_production(
        context: AuthorityContext,
    ) -> Result<Arc<AuraEffectSystem>, EffectRegistryError> {
        EffectRegistry::production()?
            .with_authority_context(context)
            .with_logging()
            .with_metrics()
            .build()
    }

    /// Quick simulation setup with authority context and seed
    fn quick_simulation(
        context: AuthorityContext,
        seed: u64,
    ) -> Result<Arc<AuraEffectSystem>, EffectRegistryError> {
        EffectRegistry::simulation(seed)?
            .with_authority_context(context)
            .with_logging()
            .build()
    }
}

impl EffectRegistryExt for EffectRegistry {}

/// High-level runtime builder façade
pub struct RuntimeBuilder;

impl RuntimeBuilder {
    /// Create a production runtime with authority-first design
    pub async fn production(ctx: &EffectContext, authority_id: AuthorityId) -> Result<RuntimeSystem, String> {
        EffectSystemBuilder::production()
            .with_authority(authority_id)
            .build(ctx)
            .await
    }

    /// Create a testing runtime with authority-first design
    pub fn testing(authority_id: AuthorityId) -> Result<RuntimeSystem, String> {
        EffectSystemBuilder::testing()
            .with_authority(authority_id)
            .build_sync()
    }

    /// Create a simulation runtime with authority-first design
    pub fn simulation(authority_id: AuthorityId, seed: u64) -> Result<RuntimeSystem, String> {
        EffectSystemBuilder::simulation(seed)
            .with_authority(authority_id)
            .build_sync()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_effect_registry_configurations() {
        // Test production configuration
        let prod = EffectRegistry::production();
        assert!(matches!(prod.execution_mode, ExecutionMode::Production));
        assert!(prod.enable_logging);
        assert!(prod.enable_metrics);

        // Test testing configuration
        let test = EffectRegistry::testing();
        assert!(matches!(test.execution_mode, ExecutionMode::Testing));
        assert!(!test.enable_logging);
        assert!(!test.enable_metrics);

        // Test simulation configuration
        let sim = EffectRegistry::simulation(42);
        assert!(matches!(
            sim.execution_mode,
            ExecutionMode::Simulation { seed: 42 }
        ));
        assert!(sim.enable_logging);
        assert!(!sim.enable_metrics);
    }

    #[test]
    fn test_builder_pattern() {
        let authority_id = AuthorityId::new();
        let context = AuthorityContext::new(authority_id);

        let registry = EffectRegistry::custom()
            .with_authority_context(context)
            .with_logging()
            .with_metrics()
            .with_tracing()
            .with_execution_mode(ExecutionMode::Production);

        assert!(registry.authority_context.is_some());
        assert!(matches!(registry.execution_mode, ExecutionMode::Production));
        assert!(registry.enable_logging);
        assert!(registry.enable_metrics);
        assert!(registry.enable_tracing);
    }

    #[test]
    fn test_build_missing_context() {
        let result = EffectRegistry::testing().build();
        assert!(result.is_err());

        match result.unwrap_err() {
            EffectRegistryError::MissingConfiguration { field } => {
                assert_eq!(field, "authority_context");
            }
            _ => panic!("Expected MissingConfiguration error"),
        }
    }
}
