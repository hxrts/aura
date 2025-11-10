//! Effect executor for coordinating effect operations
//!
//! Provides the execution engine that coordinates effect handlers, middleware, and protocol operations.

use crate::{
    effects::ProtocolEffects,
    handlers::CompositeHandler,
    middleware::{create_standard_stack, MiddlewareConfig},
};
use aura_core::DeviceId;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Effect executor that coordinates effect operations
pub struct EffectExecutor {
    /// Device ID for this executor
    device_id: DeviceId,
    /// Executor configuration
    config: ExecutorConfig,
    /// Currently active effect handlers
    handlers: Arc<RwLock<Vec<Box<dyn ProtocolEffects>>>>,
}

/// Configuration for the effect executor
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum number of concurrent effect operations
    pub max_concurrent_operations: usize,
    /// Default timeout for effect operations
    pub default_timeout: Duration,
    /// Whether to enable operation tracing
    pub enable_tracing: bool,
    /// Whether to enable operation metrics
    pub enable_metrics: bool,
    /// Middleware configuration
    pub middleware_config: MiddlewareConfig,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrent_operations: 100,
            default_timeout: Duration::from_secs(30),
            enable_tracing: true,
            enable_metrics: true,
            middleware_config: MiddlewareConfig::default(),
        }
    }
}

impl EffectExecutor {
    /// Create a new effect executor
    pub fn new(device_id: DeviceId, config: ExecutorConfig) -> Self {
        Self {
            device_id,
            config,
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create an effect handler for the given execution mode
    pub fn create_handler(&self, mode: ExecutionMode) -> Box<dyn ProtocolEffects> {
        let base_handler = match mode {
            ExecutionMode::Testing => CompositeHandler::for_testing(self.device_id.into()),
            ExecutionMode::Production => CompositeHandler::for_production(self.device_id.into()),
            ExecutionMode::Simulation => CompositeHandler::for_simulation(self.device_id.into()),
        };

        // Apply middleware stack
        let handler = create_standard_stack(base_handler, self.config.middleware_config.clone());
        Box::new(handler)
    }

    /// Register a new effect handler
    pub async fn register_handler(&self, handler: Box<dyn ProtocolEffects>) {
        let mut handlers = self.handlers.write().await;
        handlers.push(handler);
    }

    /// Get the number of registered handlers
    pub async fn handler_count(&self) -> usize {
        let handlers = self.handlers.read().await;
        handlers.len()
    }

    /// Execute an effect operation with timeout
    pub async fn execute_with_timeout<F, T, E>(
        &self,
        operation: F,
        timeout: Option<Duration>,
    ) -> Result<T, ExecutorError>
    where
        F: std::future::Future<Output = Result<T, E>> + Send,
        E: std::error::Error + Send + Sync + 'static,
    {
        let timeout = timeout.unwrap_or(self.config.default_timeout);

        match tokio::time::timeout(timeout, operation).await {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(e)) => Err(ExecutorError::OperationFailed {
                source: Box::new(e),
            }),
            Err(_) => Err(ExecutorError::Timeout { timeout }),
        }
    }

    /// Get executor statistics
    pub async fn stats(&self) -> ExecutorStats {
        let handler_count = self.handler_count().await;

        ExecutorStats {
            device_id: self.device_id,
            handler_count,
            max_concurrent_operations: self.config.max_concurrent_operations,
            // In a full implementation, these would track actual operation counts
            operations_executed: 0,
            operations_failed: 0,
            average_operation_duration_ms: 0.0,
        }
    }
}

/// Execution mode for effect handlers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    /// Testing mode with mock handlers
    Testing,
    /// Production mode with real handlers
    Production,
    /// Simulation mode with controllable handlers
    Simulation,
}

/// Executor errors
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("Operation timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("Operation failed: {source}")]
    OperationFailed {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Too many concurrent operations")]
    TooManyConcurrentOperations,

    #[error("Handler not available")]
    HandlerNotAvailable,

    #[error("Invalid execution mode")]
    InvalidExecutionMode,
}

/// Executor statistics
#[derive(Debug, Clone)]
pub struct ExecutorStats {
    pub device_id: DeviceId,
    pub handler_count: usize,
    pub max_concurrent_operations: usize,
    pub operations_executed: u64,
    pub operations_failed: u64,
    pub average_operation_duration_ms: f64,
}
