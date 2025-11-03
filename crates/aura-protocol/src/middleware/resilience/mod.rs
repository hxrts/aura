//! Resilience middleware
//!
//! Provides retry, timeout, and circuit breaker decorators for effect handlers.

pub mod circuit_breaker;
pub mod retry;
pub mod timeout;

pub use circuit_breaker::CircuitBreakerMiddleware;
pub use retry::RetryMiddleware;
pub use timeout::TimeoutMiddleware;

/// Configuration for error recovery middleware
#[derive(Debug, Clone)]
pub struct ErrorRecoveryConfig {
    /// Device name for logging
    pub device_name: String,
    /// Maximum number of retry attempts
    pub max_retries: u32,
    /// Base retry delay in milliseconds
    pub base_delay_ms: u64,
    /// Maximum retry delay in milliseconds
    pub max_delay_ms: u64,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker timeout in milliseconds
    pub circuit_breaker_timeout_ms: u64,
}

impl Default for ErrorRecoveryConfig {
    fn default() -> Self {
        Self {
            device_name: "unknown".to_string(),
            max_retries: 3,
            base_delay_ms: 100,
            max_delay_ms: 5000,
            circuit_breaker_threshold: 10,
            circuit_breaker_timeout_ms: 30000,
        }
    }
}