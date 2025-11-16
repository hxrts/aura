//! Reliability Effects
//!
//! Provides reliability patterns for fault-tolerant operation in distributed systems.
//! These effects enable retry logic, circuit breaking, and graceful degradation
//! while maintaining the stateless, composable nature of the effect system.
//!
//! **DRY Consolidation**: This module consolidates retry logic from aura-sync, aura-agent,
//! and provides a unified implementation for all crates. Includes BackoffStrategy, RetryPolicy,
//! and helper types for comprehensive retry patterns.

use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::future::Future;

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

// =============================================================================
// Unified Retry Implementation (consolidated from aura-sync)
// =============================================================================

/// Backoff strategy for retry delays
///
/// **DRY Consolidation**: This enum replaces duplicate backoff strategies in
/// aura-sync, aura-agent, and provides a single source of truth for retry delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed,
    /// Linear increase: delay * attempt
    Linear,
    /// Exponential increase: delay * 2^attempt
    Exponential,
    /// Exponential with jitter to prevent thundering herd
    ExponentialWithJitter,
}

impl BackoffStrategy {
    /// Calculate delay for a given attempt number
    ///
    /// # Arguments
    /// - `attempt`: Zero-based attempt number (0 = first retry)
    /// - `initial_delay`: Base delay duration
    /// - `max_delay`: Maximum delay duration
    pub fn calculate_delay(
        &self,
        attempt: u32,
        initial_delay: Duration,
        max_delay: Duration,
    ) -> Duration {
        use rand::Rng;

        let delay = match self {
            BackoffStrategy::Fixed => initial_delay,
            BackoffStrategy::Linear => initial_delay * (attempt + 1),
            BackoffStrategy::Exponential => {
                let multiplier = 2u32.saturating_pow(attempt);
                initial_delay * multiplier
            }
            BackoffStrategy::ExponentialWithJitter => {
                let base_delay = initial_delay * 2u32.saturating_pow(attempt);
                let jitter = (base_delay.as_millis() as f64 * 0.1 * rand::thread_rng().gen::<f64>()) as u64;
                base_delay + Duration::from_millis(jitter)
            }
        };

        delay.min(max_delay)
    }
}

/// Retry policy configuration
///
/// **DRY Consolidation**: This struct replaces duplicate retry policies across crates,
/// providing a unified builder pattern for configuring retry behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts (0 = no retries)
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Backoff strategy to use
    pub strategy: BackoffStrategy,
    /// Whether to add jitter to delays
    pub jitter: bool,
    /// Timeout for individual retry attempts
    pub timeout: Option<Duration>,
}

impl RetryPolicy {
    /// Create a new retry policy with exponential backoff
    pub fn exponential() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            strategy: BackoffStrategy::Exponential,
            jitter: false,
            timeout: None,
        }
    }

    /// Create a retry policy with fixed delay
    pub fn fixed(delay: Duration) -> Self {
        Self {
            max_attempts: 3,
            initial_delay: delay,
            max_delay: delay,
            strategy: BackoffStrategy::Fixed,
            jitter: false,
            timeout: None,
        }
    }

    /// Create a retry policy with linear backoff
    pub fn linear() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            strategy: BackoffStrategy::Linear,
            jitter: false,
            timeout: None,
        }
    }

    /// Set maximum retry attempts
    pub fn with_max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts;
        self
    }

    /// Set initial delay
    pub fn with_initial_delay(mut self, delay: Duration) -> Self {
        self.initial_delay = delay;
        self
    }

    /// Set maximum delay
    pub fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Enable or disable jitter
    pub fn with_jitter(mut self, enable: bool) -> Self {
        self.jitter = enable;
        if enable {
            self.strategy = BackoffStrategy::ExponentialWithJitter;
        }
        self
    }

    /// Set timeout for individual attempts
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Calculate delay for a specific attempt
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let strategy = if self.jitter {
            BackoffStrategy::ExponentialWithJitter
        } else {
            self.strategy
        };

        strategy.calculate_delay(attempt, self.initial_delay, self.max_delay)
    }

    /// Execute an async operation with retry logic
    pub async fn execute<F, Fut, T, E>(&self, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut attempt = 0;

        loop {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(err) => {
                    if attempt >= self.max_attempts {
                        return Err(err);
                    }

                    let delay = self.calculate_delay(attempt);
                    tokio::time::sleep(delay).await;

                    attempt += 1;
                }
            }
        }
    }

    /// Execute an async operation with retry logic and detailed context
    pub async fn execute_with_context<F, Fut, T, E>(
        &self,
        mut operation: F,
    ) -> RetryResult<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let start = std::time::Instant::now();
        let mut attempt = 0;
        let mut total_delay = Duration::ZERO;

        loop {
            match operation().await {
                Ok(result) => {
                    return RetryResult {
                        result: Ok(result),
                        attempts: attempt + 1,
                        total_duration: start.elapsed(),
                        total_retry_delay: total_delay,
                    };
                }
                Err(err) => {
                    if attempt >= self.max_attempts {
                        return RetryResult {
                            result: Err(err),
                            attempts: attempt + 1,
                            total_duration: start.elapsed(),
                            total_retry_delay: total_delay,
                        };
                    }

                    let delay = self.calculate_delay(attempt);
                    total_delay += delay;
                    tokio::time::sleep(delay).await;

                    attempt += 1;
                }
            }
        }
    }
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::exponential()
    }
}

/// Result of a retry operation with statistics
#[derive(Debug, Clone)]
pub struct RetryResult<T, E> {
    /// Final result (success or failure)
    pub result: Result<T, E>,
    /// Number of attempts made
    pub attempts: u32,
    /// Total duration including retries
    pub total_duration: Duration,
    /// Total time spent waiting between retries
    pub total_retry_delay: Duration,
}

impl<T, E> RetryResult<T, E> {
    /// Check if operation succeeded
    pub fn is_success(&self) -> bool {
        self.result.is_ok()
    }

    /// Check if any retries were performed
    pub fn had_retries(&self) -> bool {
        self.attempts > 1
    }

    /// Get the result
    pub fn into_result(self) -> Result<T, E> {
        self.result
    }
}

impl<T, E: std::fmt::Debug> RetryResult<T, E> {
    /// Get the success value, panicking on error
    pub fn unwrap(self) -> T {
        self.result.unwrap()
    }
}

/// Context for tracking retry state
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Current attempt number (0-based)
    pub attempt: u32,
    /// Time of first attempt
    pub started_at: std::time::Instant,
    /// Total delay accumulated
    pub accumulated_delay: Duration,
    /// Whether this is the last attempt
    pub is_last_attempt: bool,
}

impl RetryContext {
    /// Create a new retry context
    pub fn new(max_attempts: u32) -> Self {
        Self {
            attempt: 0,
            started_at: std::time::Instant::now(),
            accumulated_delay: Duration::ZERO,
            is_last_attempt: max_attempts == 0,
        }
    }

    /// Advance to next attempt
    pub fn next_attempt(&mut self, delay: Duration, max_attempts: u32) {
        self.attempt += 1;
        self.accumulated_delay += delay;
        self.is_last_attempt = self.attempt >= max_attempts;
    }

    /// Get elapsed time since first attempt
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Get total time including delays
    pub fn total_time(&self) -> Duration {
        self.elapsed()
    }
}