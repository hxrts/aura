//! Middleware Layer for Agent Operations
//!
//! This module provides cross-cutting concerns and middleware that can be layered
//! on top of agent operations. Following the unified effect system architecture,
//! middleware can intercept, transform, and enhance operations while maintaining
//! the pure effect consumption pattern.
//!
//! # Architecture
//!
//! ```text
//! Agent Operations
//!       ↓
//! ┌─────────────────┐
//! │   Middleware    │
//! │   - Validation  │
//! │   - Metrics     │
//! │   - Tracing     │
//! │   - Retry       │
//! └─────────────────┘
//!       ↓
//! Effect System
//! ```
//!
//! # Middleware Types
//!
//! - **Validation**: Input validation and constraint checking
//! - **Metrics**: Operation metrics collection and telemetry  
//! - **Tracing**: Operation tracing and debug logging
//! - **Retry**: Retry logic for transient failures
//! - **Rate Limiting**: Request throttling and backpressure
//! - **Circuit Breaker**: Fault tolerance patterns

pub mod metrics;
pub mod tracing;
pub mod validation;

pub use metrics::{AgentMetrics, MetricsMiddleware, OperationMetrics};
pub use tracing::{OperationTracer, TracingMiddleware};
pub use validation::{InputValidator, ValidationMiddleware, ValidationRule};

use aura_core::{identifiers::DeviceId, AuraResult as Result};
use aura_protocol::effects::AuraEffectSystem;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Middleware stack for agent operations
///
/// This provides a composable way to add cross-cutting concerns to agent operations
/// while maintaining the effect-based architecture. Middleware can be layered in
/// any order and each middleware has access to the effect system.
pub struct AgentMiddlewareStack {
    /// The underlying effect system
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this stack
    device_id: DeviceId,
    /// Validation middleware
    validator: Option<ValidationMiddleware>,
    /// Metrics middleware
    metrics: Option<MetricsMiddleware>,
    /// Tracing middleware
    tracer: Option<TracingMiddleware>,
}

impl AgentMiddlewareStack {
    /// Create a new middleware stack
    pub fn new(effects: AuraEffectSystem, device_id: DeviceId) -> Self {
        Self {
            effects: Arc::new(RwLock::new(effects)),
            device_id,
            validator: None,
            metrics: None,
            tracer: None,
        }
    }

    /// Add validation middleware
    pub fn with_validation(mut self, validator: ValidationMiddleware) -> Self {
        self.validator = Some(validator);
        self
    }

    /// Add metrics middleware
    pub fn with_metrics(mut self, metrics: MetricsMiddleware) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Add tracing middleware
    pub fn with_tracing(mut self, tracer: TracingMiddleware) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// Execute an operation through the middleware stack
    pub async fn execute_operation<T, F, Fut>(
        &self,
        operation_name: &str,
        operation: F,
    ) -> Result<T>
    where
        F: FnOnce(Arc<RwLock<AuraEffectSystem>>) -> Fut + Send,
        Fut: std::future::Future<Output = Result<T>> + Send,
        T: Send,
    {
        // Start tracing if enabled
        let trace_id = if let Some(tracer) = &self.tracer {
            Some(tracer.start_operation(operation_name).await?)
        } else {
            None
        };

        // Validate inputs if enabled
        if let Some(validator) = &self.validator {
            validator.validate_operation(operation_name).await?;
        }

        // Record metrics start if enabled
        let start_time = if self.metrics.is_some() {
            Some(std::time::Instant::now())
        } else {
            None
        };

        // Execute the actual operation
        let result = operation(self.effects.clone()).await;

        // Record metrics end if enabled
        if let (Some(metrics), Some(start_time)) = (&self.metrics, start_time) {
            let duration = start_time.elapsed();
            let success = result.is_ok();
            metrics
                .record_operation(operation_name, duration, success)
                .await?;
        }

        // End tracing if enabled
        if let (Some(tracer), Some(trace_id)) = (&self.tracer, trace_id) {
            let success = result.is_ok();
            tracer.end_operation(trace_id, success).await?;
        }

        result
    }

    /// Get the underlying effect system
    pub fn effects(&self) -> Arc<RwLock<AuraEffectSystem>> {
        self.effects.clone()
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if validation middleware is enabled
    pub fn has_validation(&self) -> bool {
        self.validator.is_some()
    }

    /// Check if metrics middleware is enabled
    pub fn has_metrics(&self) -> bool {
        self.metrics.is_some()
    }

    /// Check if tracing middleware is enabled
    pub fn has_tracing(&self) -> bool {
        self.tracer.is_some()
    }
}

/// Builder for creating middleware stacks with different configurations
pub struct MiddlewareStackBuilder {
    effects: AuraEffectSystem,
    device_id: DeviceId,
    enable_validation: bool,
    enable_metrics: bool,
    enable_tracing: bool,
    validation_rules: Vec<ValidationRule>,
}

impl MiddlewareStackBuilder {
    /// Create a new builder
    pub fn new(effects: AuraEffectSystem, device_id: DeviceId) -> Self {
        Self {
            effects,
            device_id,
            enable_validation: false,
            enable_metrics: false,
            enable_tracing: false,
            validation_rules: Vec::new(),
        }
    }

    /// Enable validation middleware
    pub fn with_validation(mut self) -> Self {
        self.enable_validation = true;
        self
    }

    /// Enable metrics middleware
    pub fn with_metrics(mut self) -> Self {
        self.enable_metrics = true;
        self
    }

    /// Enable tracing middleware
    pub fn with_tracing(mut self) -> Self {
        self.enable_tracing = true;
        self
    }

    /// Add validation rules
    pub fn add_validation_rule(mut self, rule: ValidationRule) -> Self {
        self.validation_rules.push(rule);
        self
    }

    /// Build the middleware stack
    pub async fn build(self) -> Result<AgentMiddlewareStack> {
        let mut stack = AgentMiddlewareStack::new(self.effects, self.device_id);

        // Add validation if enabled
        if self.enable_validation {
            let validator = ValidationMiddleware::new(self.validation_rules);
            stack = stack.with_validation(validator);
        }

        // Add metrics if enabled
        if self.enable_metrics {
            let metrics = MetricsMiddleware::new(self.device_id).await?;
            stack = stack.with_metrics(metrics);
        }

        // Add tracing if enabled
        if self.enable_tracing {
            let tracer = TracingMiddleware::new(self.device_id).await?;
            stack = stack.with_tracing(tracer);
        }

        Ok(stack)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::effects::{AuraEffectSystem, EffectSystemConfig};

    #[tokio::test]
    async fn test_middleware_stack_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).unwrap();
        let stack = AgentMiddlewareStack::new(effects, device_id);

        assert_eq!(stack.device_id(), device_id);
        assert!(!stack.has_validation());
        assert!(!stack.has_metrics());
        assert!(!stack.has_tracing());
    }

    #[tokio::test]
    async fn test_middleware_builder() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).unwrap();

        let stack = MiddlewareStackBuilder::new(effects, device_id)
            .with_validation()
            .with_tracing()
            .build()
            .await
            .unwrap();

        assert!(stack.has_validation());
        assert!(!stack.has_metrics());
        assert!(stack.has_tracing());
    }

    #[tokio::test]
    async fn test_operation_execution() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).unwrap();
        let stack = AgentMiddlewareStack::new(effects, device_id);

        // Test simple operation execution
        let result = stack
            .execute_operation("test_op", |_effects| async { Ok(42u32) })
            .await;

        assert_eq!(result.unwrap(), 42);
    }
}
