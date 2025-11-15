//! Reliability Effects
//!
//! Provides reliability patterns for fault-tolerant operation in distributed systems.
//! These effects enable retry logic, circuit breaking, and graceful degradation
//! while maintaining the stateless, composable nature of the effect system.

use crate::AuraError;
use async_trait::async_trait;
use std::time::Duration;

/// Reliability operations for fault tolerance and graceful degradation
///
/// This trait provides pure reliability primitives that can be composed
/// to create resilient distributed systems. All operations are coordination
/// patterns that work with other effects through explicit composition.
#[async_trait]
pub trait ReliabilityEffects {
    /// Execute an operation with retry logic and exponential backoff
    ///
    /// # Arguments
    /// * `operation` - The async operation to execute
    /// * `max_attempts` - Maximum number of retry attempts
    /// * `base_delay` - Base delay before first retry
    /// * `max_delay` - Maximum delay between retries
    ///
    /// # Returns
    /// The result of the first successful attempt, or the final error
    async fn with_retry<T, F, Fut>(
        &self,
        operation: F,
        max_attempts: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send;

    /// Execute an operation with circuit breaker protection
    ///
    /// Circuit breaker prevents cascading failures by failing fast when
    /// error rate exceeds threshold. Automatically recovers when operation
    /// starts succeeding again.
    ///
    /// # Arguments
    /// * `operation` - The async operation to execute
    /// * `circuit_id` - Unique identifier for this circuit
    /// * `failure_threshold` - Number of failures before opening circuit
    /// * `timeout` - How long to keep circuit open before trying again
    ///
    /// # Returns
    /// The operation result, or circuit breaker error if circuit is open
    async fn with_circuit_breaker<T, F, Fut>(
        &self,
        operation: F,
        circuit_id: &str,
        failure_threshold: u32,
        timeout: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send;

    /// Execute an operation with timeout protection
    ///
    /// Ensures that operations don't hang indefinitely by cancelling
    /// them after a specified timeout period.
    ///
    /// # Arguments
    /// * `operation` - The async operation to execute
    /// * `timeout` - Maximum time to wait for completion
    ///
    /// # Returns
    /// The operation result, or timeout error if operation takes too long
    async fn with_timeout<T, F, Fut>(
        &self,
        operation: F,
        timeout: Duration,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send;

    /// Execute an operation with rate limiting
    ///
    /// Prevents resource exhaustion by limiting the rate at which
    /// operations can be executed.
    ///
    /// # Arguments
    /// * `operation` - The async operation to execute
    /// * `rate_limit_id` - Unique identifier for this rate limit
    /// * `max_operations_per_second` - Maximum operations per second
    ///
    /// # Returns
    /// The operation result, or rate limit error if limit is exceeded
    async fn with_rate_limit<T, F, Fut>(
        &self,
        operation: F,
        rate_limit_id: &str,
        max_operations_per_second: f64,
    ) -> Result<T, ReliabilityError>
    where
        F: Fn() -> Fut + Send,
        Fut: std::future::Future<Output = Result<T, AuraError>> + Send,
        T: Send;
}

/// Errors that can occur during reliability operations
#[derive(Debug, thiserror::Error)]
pub enum ReliabilityError {
    /// Maximum retry attempts exceeded
    #[error("Operation failed after {attempts} attempts: {last_error}")]
    RetryExhausted {
        attempts: u32,
        last_error: AuraError,
    },

    /// Circuit breaker is open
    #[error("Circuit breaker '{circuit_id}' is open, failing fast")]
    CircuitBreakerOpen { circuit_id: String },

    /// Operation timed out
    #[error("Operation timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    /// Rate limit exceeded
    #[error("Rate limit '{rate_limit_id}' exceeded: {max_rate} ops/sec")]
    RateLimitExceeded {
        rate_limit_id: String,
        max_rate: f64,
    },

    /// Underlying operation error
    #[error("Operation failed: {0}")]
    OperationError(#[from] AuraError),
}