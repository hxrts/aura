//! Reliability Utilities
//!
//! Consolidated reliability, propagation, and backoff utilities.

use aura_core::effects::time::TimeError;
use aura_core::effects::PhysicalTimeEffects;
use std::time::Duration;

/// Reliability configuration and utilities
#[allow(dead_code)] // Part of future reliability API
pub struct ReliabilityManager {
    max_retries: usize,
    base_backoff: Duration,
    max_backoff: Duration,
}

impl ReliabilityManager {
    /// Create a new reliability manager
    #[allow(dead_code)] // Part of future reliability API
    pub fn new(max_retries: usize, base_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_retries,
            base_backoff,
            max_backoff,
        }
    }

    /// Calculate backoff delay for attempt number
    #[allow(dead_code)] // Part of future reliability API
    pub fn backoff_delay(&self, attempt: usize) -> Duration {
        let exponential = self.base_backoff * (2_u32.pow(attempt.min(20) as u32));
        exponential.min(self.max_backoff)
    }

    /// Execute operation with retry and backoff
    #[allow(dead_code)] // Part of future reliability API
    pub async fn with_retry<T, E, F, P>(&self, time: &P, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Result<T, E>,
        P: PhysicalTimeEffects,
        E: From<TimeError>,
    {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match operation() {
                Ok(result) => return Ok(result),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < self.max_retries {
                        let delay = self.backoff_delay(attempt);
                        time.sleep_ms(delay.as_millis() as u64).await?;
                    }
                }
            }
        }

        Err(last_error.unwrap())
    }
}

impl Default for ReliabilityManager {
    fn default() -> Self {
        Self::new(3, Duration::from_millis(100), Duration::from_secs(5))
    }
}
