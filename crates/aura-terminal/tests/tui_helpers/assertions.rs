//! Assertion Helpers
//!
//! Utility functions for asserting view state in tests.

use std::future::Future;
use std::time::Duration;

/// Error type for view assertions
#[derive(Debug)]
pub struct ViewAssertionError {
    pub message: String,
    pub timeout: Duration,
}

impl std::fmt::Display for ViewAssertionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "View assertion failed after {:?}: {}",
            self.timeout, self.message
        )
    }
}

impl std::error::Error for ViewAssertionError {}

/// Assert that a view eventually satisfies a predicate
///
/// Polls the predicate at regular intervals until it returns true or times out.
///
/// # Example
///
/// ```ignore
/// assert_view_eventually(
///     || async { view.get_count().await > 0 },
///     Duration::from_millis(100),
///     "Count should become positive",
/// ).await?;
/// ```
pub async fn assert_view_eventually<F, Fut>(
    predicate: F,
    timeout: Duration,
    message: &str,
) -> Result<(), ViewAssertionError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let poll_interval = Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        if predicate().await {
            return Ok(());
        }
        tokio::time::sleep(poll_interval).await;
    }

    Err(ViewAssertionError {
        message: message.to_string(),
        timeout,
    })
}

/// Assert that a view never satisfies a predicate within the timeout
///
/// This is useful for testing that certain conditions don't occur.
pub async fn assert_view_never<F, Fut>(
    predicate: F,
    timeout: Duration,
    message: &str,
) -> Result<(), ViewAssertionError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let poll_interval = Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        if predicate().await {
            return Err(ViewAssertionError {
                message: format!("Condition occurred unexpectedly: {}", message),
                timeout,
            });
        }
        tokio::time::sleep(poll_interval).await;
    }

    Ok(())
}

/// Assert that a value matches an expected value eventually
pub async fn assert_eventually_eq<T, F, Fut>(
    get_value: F,
    expected: T,
    timeout: Duration,
) -> Result<(), ViewAssertionError>
where
    T: PartialEq + std::fmt::Debug,
    F: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let poll_interval = Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + timeout;
    let mut last_value = None;

    while tokio::time::Instant::now() < deadline {
        let value = get_value().await;
        if value == expected {
            return Ok(());
        }
        last_value = Some(value);
        tokio::time::sleep(poll_interval).await;
    }

    let final_value = match last_value {
        Some(v) => v,
        None => get_value().await,
    };
    Err(ViewAssertionError {
        message: format!("Expected {:?}, got {:?}", expected, final_value),
        timeout,
    })
}

/// Assert that a value is within range eventually
#[allow(dead_code)]
pub async fn assert_eventually_in_range<T, F, Fut>(
    get_value: F,
    min: T,
    max: T,
    timeout: Duration,
) -> Result<(), ViewAssertionError>
where
    T: PartialOrd + std::fmt::Debug + Clone,
    F: Fn() -> Fut,
    Fut: Future<Output = T>,
{
    let poll_interval = Duration::from_millis(5);
    let deadline = tokio::time::Instant::now() + timeout;

    while tokio::time::Instant::now() < deadline {
        let value = get_value().await;
        if value >= min && value <= max {
            return Ok(());
        }
        tokio::time::sleep(poll_interval).await;
    }

    let final_value = get_value().await;
    Err(ViewAssertionError {
        message: format!(
            "Expected value in range [{:?}, {:?}], got {:?}",
            min, max, final_value
        ),
        timeout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_assert_view_eventually_success() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();

        // Start a task that increments the counter
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            counter_clone.store(1, Ordering::SeqCst);
        });

        let result = assert_view_eventually(
            || {
                let c = counter.clone();
                async move { c.load(Ordering::SeqCst) > 0 }
            },
            Duration::from_millis(100),
            "Counter should become positive",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assert_view_eventually_timeout() {
        let counter = Arc::new(AtomicUsize::new(0));

        let result = assert_view_eventually(
            || {
                let c = counter.clone();
                async move { c.load(Ordering::SeqCst) > 10 }
            },
            Duration::from_millis(20),
            "Counter should exceed 10",
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_assert_view_never_success() {
        let counter = Arc::new(AtomicUsize::new(0));

        let result = assert_view_never(
            || {
                let c = counter.clone();
                async move { c.load(Ordering::SeqCst) > 10 }
            },
            Duration::from_millis(20),
            "Counter should not exceed 10",
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_assert_eventually_eq_success() {
        let value = Arc::new(tokio::sync::RwLock::new(0));
        let value_clone = value.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            *value_clone.write().await = 42;
        });

        let result = assert_eventually_eq(
            || {
                let v = value.clone();
                async move { *v.read().await }
            },
            42,
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_ok());
    }
}
