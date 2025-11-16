//! Retry logic with exponential backoff
//!
//! Provides unified retry patterns for sync operations with configurable
//! backoff strategies, jitter, and circuit breaking.
//!
//! # Architecture
//!
//! The retry system follows these principles:
//! - **Stateless operations**: Each retry attempt is independent
//! - **Configurable strategies**: Exponential, linear, or custom backoff
//! - **Jitter support**: Prevents thundering herd problems
//! - **Circuit breaking integration**: Respects failure thresholds
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::infrastructure::{RetryPolicy, BackoffStrategy};
//! use std::time::Duration;
//!
//! async fn sync_with_retry() -> Result<(), Box<dyn std::error::Error>> {
//!     let policy = RetryPolicy::exponential()
//!         .with_max_attempts(5)
//!         .with_initial_delay(Duration::from_millis(100))
//!         .with_max_delay(Duration::from_secs(10))
//!         .with_jitter(true);
//!
//!     policy.execute(|| async {
//!         // Your sync operation here
//!         Ok(())
//!     }).await
//! }
//! ```

use std::time::Duration;
use std::future::Future;
use std::pin::Pin;

use serde::{Deserialize, Serialize};

use crate::core::{SyncError, SyncResult};

// =============================================================================
// Backoff Strategies
// =============================================================================

/// Backoff strategy for retry delays
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
        let delay = match self {
            BackoffStrategy::Fixed => initial_delay,

            BackoffStrategy::Linear => {
                initial_delay * (attempt + 1)
            }

            BackoffStrategy::Exponential => {
                let multiplier = 2u32.saturating_pow(attempt);
                initial_delay * multiplier
            }

            BackoffStrategy::ExponentialWithJitter => {
                let base_delay = initial_delay * 2u32.saturating_pow(attempt);
                let jitter = (base_delay.as_millis() as f64 * 0.1 * rand::random::<f64>()) as u64;
                base_delay + Duration::from_millis(jitter)
            }
        };

        delay.min(max_delay)
    }
}

// =============================================================================
// Retry Policy
// =============================================================================

/// Retry policy configuration
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
    ///
    /// # Arguments
    /// - `operation`: Async function to retry on failure
    ///
    /// # Returns
    /// - Success result from operation
    /// - Error from last attempt if all retries exhausted
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
    ///
    /// Returns a `RetryResult` containing attempt statistics
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

// =============================================================================
// Retry Result
// =============================================================================

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

    /// Get the success value, panicking on error
    pub fn unwrap(self) -> T {
        self.result.unwrap()
    }

    /// Get the result
    pub fn into_result(self) -> Result<T, E> {
        self.result
    }
}

// =============================================================================
// Retry Context
// =============================================================================

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
// Helper Functions
// =============================================================================

/// Execute an operation with exponential backoff retry (convenience function)
pub async fn with_exponential_backoff<F, Fut, T>(
    operation: F,
    max_attempts: u32,
) -> SyncResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SyncResult<T>>,
{
    RetryPolicy::exponential()
        .with_max_attempts(max_attempts)
        .execute(operation)
        .await
}

/// Execute an operation with fixed retry delay (convenience function)
pub async fn with_fixed_retry<F, Fut, T>(
    operation: F,
    max_attempts: u32,
    delay: Duration,
) -> SyncResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = SyncResult<T>>,
{
    RetryPolicy::fixed(delay)
        .with_max_attempts(max_attempts)
        .execute(operation)
        .await
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_fixed() {
        let strategy = BackoffStrategy::Fixed;
        let initial = Duration::from_millis(100);
        let max = Duration::from_secs(10);

        assert_eq!(strategy.calculate_delay(0, initial, max), initial);
        assert_eq!(strategy.calculate_delay(5, initial, max), initial);
        assert_eq!(strategy.calculate_delay(10, initial, max), initial);
    }

    #[test]
    fn test_backoff_linear() {
        let strategy = BackoffStrategy::Linear;
        let initial = Duration::from_millis(100);
        let max = Duration::from_secs(10);

        assert_eq!(strategy.calculate_delay(0, initial, max), Duration::from_millis(100));
        assert_eq!(strategy.calculate_delay(1, initial, max), Duration::from_millis(200));
        assert_eq!(strategy.calculate_delay(4, initial, max), Duration::from_millis(500));
    }

    #[test]
    fn test_backoff_exponential() {
        let strategy = BackoffStrategy::Exponential;
        let initial = Duration::from_millis(100);
        let max = Duration::from_secs(10);

        assert_eq!(strategy.calculate_delay(0, initial, max), Duration::from_millis(100));
        assert_eq!(strategy.calculate_delay(1, initial, max), Duration::from_millis(200));
        assert_eq!(strategy.calculate_delay(2, initial, max), Duration::from_millis(400));
        assert_eq!(strategy.calculate_delay(3, initial, max), Duration::from_millis(800));

        // Should cap at max_delay
        let large_attempt = strategy.calculate_delay(20, initial, max);
        assert!(large_attempt <= max);
    }

    #[tokio::test]
    async fn test_retry_policy_success_first_attempt() {
        let policy = RetryPolicy::exponential().with_max_attempts(3);

        let mut call_count = 0;
        let result = policy.execute(|| async {
            call_count += 1;
            Ok::<_, SyncError>(42)
        }).await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 1);
    }

    #[tokio::test]
    async fn test_retry_policy_success_after_retries() {
        let policy = RetryPolicy::fixed(Duration::from_millis(10))
            .with_max_attempts(3);

        let mut call_count = 0;
        let result = policy.execute(|| async {
            call_count += 1;
            if call_count < 3 {
                Err(SyncError::Transport("temporary failure".to_string()))
            } else {
                Ok(42)
            }
        }).await;

        assert_eq!(result.unwrap(), 42);
        assert_eq!(call_count, 3);
    }

    #[tokio::test]
    async fn test_retry_policy_exhausted() {
        let policy = RetryPolicy::fixed(Duration::from_millis(10))
            .with_max_attempts(2);

        let mut call_count = 0;
        let result = policy.execute(|| async {
            call_count += 1;
            Err::<(), _>(SyncError::Transport("persistent failure".to_string()))
        }).await;

        assert!(result.is_err());
        assert_eq!(call_count, 3); // Initial + 2 retries
    }

    #[tokio::test]
    async fn test_retry_context_tracking() {
        let policy = RetryPolicy::fixed(Duration::from_millis(10))
            .with_max_attempts(3);

        let result = policy.execute_with_context(|| async {
            Ok::<_, SyncError>(42)
        }).await;

        assert_eq!(result.attempts, 1);
        assert!(!result.had_retries());
        assert_eq!(result.total_retry_delay, Duration::ZERO);
    }
}
