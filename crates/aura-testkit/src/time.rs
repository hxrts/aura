//! Time control utilities for deterministic testing
//!
//! This module provides utilities for controlling time in tests,
//! including freezing time, advancing time, and time-based assertions.

use once_cell::sync::Lazy;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Global time controller for tests
static TIME_CONTROLLER: Lazy<Arc<Mutex<TimeController>>> =
    Lazy::new(|| Arc::new(Mutex::new(TimeController::new())));

/// Time controller for managing test time
#[derive(Debug)]
struct TimeController {
    /// Current frozen time (if Some)
    frozen_time: Option<SystemTime>,
    /// Time offset from real time
    offset: Duration,
}

impl TimeController {
    fn new() -> Self {
        Self {
            frozen_time: None,
            offset: Duration::ZERO,
        }
    }

    fn current_time(&self) -> SystemTime {
        if let Some(frozen) = self.frozen_time {
            frozen
        } else {
            SystemTime::now() + self.offset
        }
    }

    fn freeze_at(&mut self, time: SystemTime) {
        self.frozen_time = Some(time);
    }

    fn advance_by(&mut self, duration: Duration) {
        if let Some(ref mut frozen) = self.frozen_time {
            *frozen += duration;
        } else {
            self.offset += duration;
        }
    }

    fn reset(&mut self) {
        self.frozen_time = None;
        self.offset = Duration::ZERO;
    }
}

/// Freeze time at the Unix epoch (0)
pub fn freeze_time_at_epoch() {
    freeze_time_at(UNIX_EPOCH);
}

/// Freeze time at a specific instant
pub fn freeze_time_at(time: SystemTime) {
    let mut controller = TIME_CONTROLLER.lock().unwrap();
    controller.freeze_at(time);
}

/// Freeze time at the current instant
pub fn freeze_time() {
    let now = SystemTime::now();
    freeze_time_at(now);
}

/// Advance time by a duration
pub fn advance_time_by(duration: Duration) {
    let mut controller = TIME_CONTROLLER.lock().unwrap();
    controller.advance_by(duration);
}

/// Reset time to normal behavior
pub fn reset_time() {
    let mut controller = TIME_CONTROLLER.lock().unwrap();
    controller.reset();
}

/// Get the current test time
pub fn current_test_time() -> SystemTime {
    let controller = TIME_CONTROLLER.lock().unwrap();
    controller.current_time()
}

/// Guard that resets time when dropped
pub struct TimeGuard {
    _private: (),
}

impl TimeGuard {
    /// Create a new time guard that freezes time at epoch
    pub fn freeze_at_epoch() -> Self {
        freeze_time_at_epoch();
        Self { _private: () }
    }

    /// Create a new time guard that freezes time at current instant
    pub fn freeze() -> Self {
        freeze_time();
        Self { _private: () }
    }
}

impl Drop for TimeGuard {
    fn drop(&mut self) {
        reset_time();
    }
}

/// Time-based test assertions
pub mod assertions {
    use super::*;
    use aura_core::{AuraError, AuraResult};

    /// Assert that an operation completes within a duration
    pub async fn assert_completes_within<F, T>(duration: Duration, future: F) -> AuraResult<T>
    where
        F: std::future::Future<Output = AuraResult<T>>,
    {
        match tokio::time::timeout(duration, future).await {
            Ok(result) => result,
            Err(_) => Err(AuraError::invalid(format!(
                "Operation did not complete within {:?}",
                duration
            ))),
        }
    }

    /// Assert that an operation takes at least a certain duration
    pub async fn assert_takes_at_least<F, T>(duration: Duration, future: F) -> AuraResult<T>
    where
        F: std::future::Future<Output = AuraResult<T>>,
    {
        let start = tokio::time::Instant::now();
        let result = future.await?;
        let elapsed = start.elapsed();

        if elapsed < duration {
            return Err(AuraError::invalid(format!(
                "Operation completed too quickly: {:?} < {:?}",
                elapsed, duration
            )));
        }

        Ok(result)
    }

    /// Run a test with a specific time progression
    pub async fn with_time_progression<F, T>(
        steps: Vec<(Duration, Duration)>, // (advance_by, wait_for)
        test_fn: F,
    ) -> AuraResult<T>
    where
        F: FnOnce() -> T,
    {
        freeze_time_at_epoch();

        // Start the test
        let result = tokio::spawn(async move { test_fn() });

        // Progress through time steps
        for (advance_by, wait_for) in steps {
            tokio::time::sleep(wait_for).await;
            advance_time_by(advance_by);
        }

        // Get the result
        let output = result
            .await
            .map_err(|e| AuraError::invalid(format!("Test panicked: {:?}", e)))?;

        reset_time();

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_freeze() {
        let _guard = TimeGuard::freeze_at_epoch();

        let time1 = current_test_time();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let time2 = current_test_time();

        assert_eq!(time1, time2);
        assert_eq!(time1, UNIX_EPOCH);
    }

    #[test]
    fn test_time_advance() {
        let _guard = TimeGuard::freeze_at_epoch();

        let time1 = current_test_time();
        advance_time_by(Duration::from_secs(60));
        let time2 = current_test_time();

        assert_eq!(
            time2.duration_since(time1).unwrap(),
            Duration::from_secs(60)
        );
    }
}
