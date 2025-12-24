//! Handler configuration builder
//!
//! Fluent builder API for creating handler configurations with sensible defaults
//! and automatic validation.

use std::time::Duration;

use aura_core::identifiers::DeviceId;

use crate::handlers::{EffectType, ExecutionMode};
use super::{AuraHandlerConfig, AuraHandlerFactory, FactoryError, SimulationConfig};

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
