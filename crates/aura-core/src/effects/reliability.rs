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
use std::future::Future;
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
                let jitter =
                    (base_delay.as_millis() as f64 * 0.1 * rand::thread_rng().gen::<f64>()) as u64;
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
    pub async fn execute_with_context<F, Fut, T, E>(&self, mut operation: F) -> RetryResult<T, E>
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

// =============================================================================
// Unified Rate Limiting Implementation (consolidated from aura-sync)
// =============================================================================

/// Rate limiter configuration
///
/// **DRY Consolidation**: This struct replaces duplicate rate limiting configuration
/// in aura-sync, providing a unified configuration system for all crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Global rate limit (operations per second)
    pub global_ops_per_second: u32,

    /// Per-peer rate limit (operations per second)
    pub peer_ops_per_second: u32,

    /// Bucket capacity (maximum burst size)
    pub bucket_capacity: u32,

    /// Refill rate (tokens per second)
    pub refill_rate: u32,

    /// Window size for sliding window algorithm
    pub window_size: Duration,

    /// Enable adaptive rate limiting based on load
    pub adaptive: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            global_ops_per_second: 1000,
            peer_ops_per_second: 100,
            bucket_capacity: 200,
            refill_rate: 100,
            window_size: Duration::from_secs(60),
            adaptive: true,
        }
    }
}

/// Rate limit for a specific context
///
/// Implements token bucket algorithm for rate limiting with automatic refill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimit {
    /// Maximum operations per window
    pub max_operations: u32,

    /// Window duration
    pub window: Duration,

    /// Current token count (for token bucket)
    pub tokens: u32,

    /// Last refill time (skipped during serialization, reinitialized on deserialization)
    #[serde(skip, default = "instant_now")]
    pub last_refill: std::time::Instant,
}

/// Helper function for serde default
fn instant_now() -> std::time::Instant {
    std::time::Instant::now()
}

impl RateLimit {
    /// Create a new rate limit
    pub fn new(max_operations: u32, window: Duration) -> Self {
        Self {
            max_operations,
            window,
            tokens: max_operations,
            last_refill: std::time::Instant::now(),
        }
    }

    /// Check if operation is allowed and consume tokens
    pub fn check_and_consume(&mut self, cost: u32, refill_rate: u32) -> bool {
        // Refill tokens based on elapsed time
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let refill_tokens = (elapsed.as_secs_f64() * refill_rate as f64) as u32;

        if refill_tokens > 0 {
            self.tokens = (self.tokens + refill_tokens).min(self.max_operations);
            self.last_refill = now;
        }

        // Check if we have enough tokens
        if self.tokens >= cost {
            self.tokens -= cost;
            true
        } else {
            false
        }
    }

    /// Get current token count
    pub fn available_tokens(&self) -> u32 {
        self.tokens
    }

    /// Calculate time until tokens are available
    pub fn time_until_available(&self, cost: u32, refill_rate: u32) -> Option<Duration> {
        if self.tokens >= cost {
            return None;
        }

        let needed = cost - self.tokens;
        let seconds = needed as f64 / refill_rate as f64;

        Some(Duration::from_secs_f64(seconds))
    }
}

/// Result of a rate limit check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Operation allowed
    Allowed,

    /// Operation denied - rate limit exceeded
    Denied {
        /// Time to wait before retry
        retry_after: Duration,

        /// Reason for denial
        reason: String,
    },
}

impl RateLimitResult {
    /// Check if operation is allowed
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitResult::Allowed)
    }

    /// Get retry-after duration if denied
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            RateLimitResult::Denied { retry_after, .. } => Some(*retry_after),
            RateLimitResult::Allowed => None,
        }
    }

    /// Convert to Result type with AuraError
    pub fn into_result(self) -> Result<(), AuraError> {
        match self {
            RateLimitResult::Allowed => Ok(()),
            RateLimitResult::Denied { reason, .. } => Err(AuraError::invalid(reason)),
        }
    }
}

/// Rate limiter for operations
///
/// **DRY Consolidation**: Provides token bucket-based rate limiting with per-peer
/// and global limits. Moved from aura-sync to provide unified rate limiting for
/// all crates.
pub struct RateLimiter {
    /// Configuration
    config: RateLimitConfig,

    /// Global rate limit
    global_limit: RateLimit,

    /// Per-peer rate limits (using DeviceId as key)
    peer_limits: std::collections::HashMap<crate::DeviceId, RateLimit>,

    /// Statistics
    stats: RateLimiterStatistics,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(config: RateLimitConfig) -> Self {
        let global_limit = RateLimit::new(config.global_ops_per_second, Duration::from_secs(1));

        Self {
            config,
            global_limit,
            peer_limits: std::collections::HashMap::new(),
            stats: RateLimiterStatistics::default(),
        }
    }

    /// Check rate limit for a peer operation
    ///
    /// # Arguments
    /// - `peer_id`: Peer device ID
    /// - `cost`: Operation cost in tokens
    ///
    /// # Returns
    /// - `RateLimitResult::Allowed` if operation can proceed
    /// - `RateLimitResult::Denied` if rate limit exceeded
    pub fn check_rate_limit(&mut self, peer_id: crate::DeviceId, cost: u32) -> RateLimitResult {
        // Check global limit first
        if !self
            .global_limit
            .check_and_consume(cost, self.config.refill_rate)
        {
            self.stats.global_limit_hits += 1;

            let retry_after = self
                .global_limit
                .time_until_available(cost, self.config.refill_rate)
                .unwrap_or(Duration::from_secs(1));

            return RateLimitResult::Denied {
                retry_after,
                reason: "Global rate limit exceeded".to_string(),
            };
        }

        // Check per-peer limit
        let peer_limit = self.peer_limits.entry(peer_id).or_insert_with(|| {
            RateLimit::new(self.config.peer_ops_per_second, Duration::from_secs(1))
        });

        if !peer_limit.check_and_consume(cost, self.config.refill_rate) {
            self.stats.peer_limit_hits += 1;

            // Return tokens to global limit since peer limit blocked
            self.global_limit.tokens =
                (self.global_limit.tokens + cost).min(self.config.global_ops_per_second);

            let retry_after = peer_limit
                .time_until_available(cost, self.config.refill_rate)
                .unwrap_or(Duration::from_secs(1));

            return RateLimitResult::Denied {
                retry_after,
                reason: format!("Peer rate limit exceeded for {:?}", peer_id),
            };
        }

        self.stats.operations_allowed += 1;
        RateLimitResult::Allowed
    }

    /// Check if operation would exceed rate limit without consuming tokens
    pub fn would_exceed_limit(&self, peer_id: &crate::DeviceId, cost: u32) -> bool {
        // Check global limit
        if self.global_limit.available_tokens() < cost {
            return true;
        }

        // Check peer limit
        if let Some(peer_limit) = self.peer_limits.get(peer_id) {
            if peer_limit.available_tokens() < cost {
                return true;
            }
        }

        false
    }

    /// Get available tokens for a peer
    pub fn available_tokens(&self, peer_id: &crate::DeviceId) -> u32 {
        let global_tokens = self.global_limit.available_tokens();

        let peer_tokens = self
            .peer_limits
            .get(peer_id)
            .map(|l| l.available_tokens())
            .unwrap_or(self.config.peer_ops_per_second);

        global_tokens.min(peer_tokens)
    }

    /// Get statistics
    pub fn statistics(&self) -> &RateLimiterStatistics {
        &self.stats
    }

    /// Reset rate limiter state
    pub fn reset(&mut self) {
        self.global_limit =
            RateLimit::new(self.config.global_ops_per_second, Duration::from_secs(1));
        self.peer_limits.clear();
        self.stats = RateLimiterStatistics::default();
    }

    /// Remove rate limit for a peer
    pub fn remove_peer(&mut self, peer_id: &crate::DeviceId) {
        self.peer_limits.remove(peer_id);
    }

    /// Get number of tracked peers
    pub fn tracked_peers(&self) -> usize {
        self.peer_limits.len()
    }
}

/// Rate limiter statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimiterStatistics {
    /// Total operations allowed
    pub operations_allowed: u64,

    /// Number of times global limit was hit
    pub global_limit_hits: u64,

    /// Number of times per-peer limit was hit
    pub peer_limit_hits: u64,
}
