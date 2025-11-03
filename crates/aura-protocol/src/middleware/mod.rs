//! Middleware Module
//!
//! This module contains effect decorators that enhance handlers with cross-cutting concerns.
//! Following the algebraic effects pattern, middleware decorates existing effect handlers
//! without changing their core functionality.
//!
//! ## Architecture Principles
//!
//! 1. **Pure Decorators**: Middleware only adds behavior, never changes core effect semantics
//! 2. **Composability**: Multiple middleware can be stacked in any order
//! 3. **Transparency**: Decorated handlers still implement the same effect traits
//! 4. **Configurable**: Each middleware can be enabled/disabled and configured independently
//!
//! ## Middleware Categories
//!
//! - **Observability**: Tracing, metrics, logging decorators
//! - **Resilience**: Retry, timeout, circuit breaker decorators
//! - **Security**: Authorization, capability checking decorators
//! - **Caching**: Result caching and memoization decorators
//! - **Testing**: Fault injection, delay simulation decorators
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::{
//!     handlers::CompositeHandler,
//!     middleware::{
//!         observability::TracingMiddleware,
//!         resilience::RetryMiddleware,
//!         security::CapabilityMiddleware,
//!         stack::MiddlewareStack,
//!     },
//! };
//!
//! // Create base handler
//! let base_handler = CompositeHandler::for_production(device_id);
//!
//! // Wrap with middleware stack
//! let enhanced_handler = MiddlewareStack::new(base_handler)
//!     .with_tracing()
//!     .with_retry(RetryConfig::default())
//!     .with_capabilities()
//!     .build();
//! ```

pub mod caching;
pub mod observability;
pub mod resilience;
pub mod security;
pub mod stack;

// Re-export commonly used middleware
pub use observability::{MetricsMiddleware, TracingMiddleware};
pub use resilience::{CircuitBreakerMiddleware, RetryMiddleware, TimeoutMiddleware};
pub use security::{AuthorizationMiddleware, CapabilityMiddleware};
pub use stack::MiddlewareStack;
pub use resilience::retry::RetryConfig;

/// Base trait for all middleware decorators
pub trait Middleware<H> {
    /// The type of the decorated handler
    type Decorated;

    /// Apply this middleware to the given handler
    fn apply(self, handler: H) -> Self::Decorated;
}

/// Configuration for all middleware
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Device name for logging and identification
    pub device_name: String,
    /// Enable observability middleware (tracing, metrics)
    pub enable_observability: bool,
    /// Enable capability checking middleware
    pub enable_capabilities: bool,
    /// Enable error recovery middleware
    pub enable_error_recovery: bool,
    /// Configuration for specific middleware
    pub observability_config: Option<observability::ObservabilityConfig>,
    pub error_recovery_config: Option<resilience::ErrorRecoveryConfig>,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            device_name: "unknown".to_string(),
            enable_observability: true,
            enable_capabilities: true,
            enable_error_recovery: true,
            observability_config: None,
            error_recovery_config: None,
        }
    }
}

/// Create a standard middleware stack with common defaults
pub fn create_standard_stack<H>(handler: H, _config: MiddlewareConfig) -> H
where 
    H: Send + Sync + 'static,
{
    // TODO: Implement proper middleware composition
    // For now, just return the handler to get compilation working
    handler
}