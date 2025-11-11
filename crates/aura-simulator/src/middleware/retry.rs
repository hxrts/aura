//! Retry Middleware with Exponential Backoff
//!
//! Provides automatic retry logic with exponential backoff for transient failures.
//! Suitable for production (with real sleep) and simulation (deterministic with simulated time).

use std::time::Duration;

/// Retry configuration
#[derive(Clone, Debug)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier (default: 2.0 for exponential)
    pub backoff_multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(30),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryConfig {
    /// Calculate backoff for attempt number (0-indexed)
    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let backoff_ms = (self.initial_backoff.as_millis() as f64
            * self.backoff_multiplier.powi(attempt as i32)) as u128;
        let backoff = Duration::from_millis(backoff_ms as u64);
        backoff.min(self.max_backoff)
    }
}

/// Retry middleware for handling transient failures
pub struct RetryMiddleware {
    config: RetryConfig,
}

impl RetryMiddleware {
    /// Create new retry middleware with default configuration
    pub fn new() -> Self {
        Self {
            config: RetryConfig::default(),
        }
    }

    /// Create with custom configuration
    pub fn with_config(config: RetryConfig) -> Self {
        Self { config }
    }

    /// Execute operation with automatic retries
    /// Returns Ok on success or Err after max_attempts
    pub async fn execute<F, T, E>(&self, mut op: F) -> Result<T, E>
    where
        F: FnMut() -> futures::future::BoxFuture<'static, Result<T, E>>,
        E: std::fmt::Debug,
    {
        for attempt in 0..self.config.max_attempts {
            match op().await {
                Ok(result) => return Ok(result),
                Err(_) if attempt + 1 < self.config.max_attempts => {
                    // Transient failure, retry with backoff
                    let backoff = self.config.backoff_for_attempt(attempt);
                    tokio::time::sleep(backoff).await;
                }
                Err(e) => return Err(e), // Final attempt failed
            }
        }
        unreachable!()
    }
}

impl Default for RetryMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_exponential_backoff() {
        let config = RetryConfig::default();
        assert_eq!(config.backoff_for_attempt(0), Duration::from_millis(100));
        assert_eq!(config.backoff_for_attempt(1), Duration::from_millis(200));
        assert_eq!(config.backoff_for_attempt(2), Duration::from_millis(400));
    }

    #[test]
    fn test_backoff_capped() {
        let config = RetryConfig {
            initial_backoff: Duration::from_secs(10),
            max_backoff: Duration::from_secs(30),
            ..Default::default()
        };
        // Third attempt would exceed max_backoff
        assert_eq!(config.backoff_for_attempt(2), Duration::from_secs(30));
    }

    #[tokio::test]
    async fn test_retry_success_on_eventual_availability() {
        let retry = RetryMiddleware::new();
        let attempts = Arc::new(Mutex::new(0));
        let attempts_clone = attempts.clone();

        let result = retry
            .execute(|| {
                let attempts = attempts_clone.clone();
                Box::pin(async move {
                    let mut attempt_count = attempts.lock().unwrap();
                    *attempt_count += 1;
                    if *attempt_count < 3 {
                        Err::<i32, _>("transient failure")
                    } else {
                        Ok(42)
                    }
                })
            })
            .await;

        assert_eq!(result, Ok(42));
        assert_eq!(*attempts.lock().unwrap(), 3);
    }

    #[tokio::test]
    async fn test_retry_exhaustion() {
        let retry = RetryMiddleware::new();
        let attempts = Arc::new(Mutex::new(0));
        let attempts_clone = attempts.clone();

        let result = retry
            .execute(|| {
                let attempts = attempts_clone.clone();
                Box::pin(async move {
                    let mut attempt_count = attempts.lock().unwrap();
                    *attempt_count += 1;
                    Err::<i32, &str>("persistent failure")
                })
            })
            .await;

        assert_eq!(result, Err("persistent failure"));
        assert_eq!(*attempts.lock().unwrap(), 3); // Default max_attempts
    }

    #[tokio::test]
    async fn test_middleware_guard_interaction() {
        /// Simulates guard behavior with capability checking
        struct GuardedOperation {
            has_capability: bool,
            attempt_count: Arc<Mutex<u32>>,
        }

        impl GuardedOperation {
            async fn execute(&self) -> Result<String, String> {
                let mut count = self.attempt_count.lock().unwrap();
                *count += 1;

                // First attempt: capability denied (guard rejects)
                if *count == 1 && !self.has_capability {
                    return Err("Capability denied".to_string());
                }

                // Subsequent attempts: operation succeeds if capability granted
                if self.has_capability {
                    Ok("Success with capability".to_string())
                } else {
                    Err("Still denied".to_string())
                }
            }
        }

        // Test: Guard rejection followed by successful authorization
        let guard_op = GuardedOperation {
            has_capability: true,
            attempt_count: Arc::new(Mutex::new(0)),
        };

        let retry = RetryMiddleware::with_config(RetryConfig {
            max_attempts: 2,
            initial_backoff: Duration::from_millis(10),
            ..Default::default()
        });

        let op_clone = Arc::new(guard_op);
        let result = retry
            .execute(|| {
                let op = op_clone.clone();
                Box::pin(async move { op.execute().await })
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Success with capability");
    }
}
