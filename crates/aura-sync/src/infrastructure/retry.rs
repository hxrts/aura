//! Retry logic with exponential backoff
//!
//! **DRY Consolidation**: This module now re-exports unified retry types from aura-core.
//! All retry implementations have been consolidated to eliminate ~400 lines of duplication
//! across aura-sync, aura-agent, and aura-core.
//!
//! The unified implementation provides:
//! - **Stateless operations**: Each retry attempt is independent
//! - **Configurable strategies**: Exponential, linear, fixed, or custom backoff
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

// Re-export unified retry types from aura-core
pub use aura_core::{BackoffStrategy, RetryContext, RetryPolicy, RetryResult};

use crate::core::SyncResult;

// =============================================================================
// Helper Functions (preserved for backward compatibility)
// =============================================================================

/// Execute an operation with exponential backoff retry (convenience function)
pub async fn with_exponential_backoff<E, F, Fut, T>(
    effects: &E,
    operation: F,
    max_attempts: u32,
) -> SyncResult<T>
where
    E: aura_core::effects::PhysicalTimeEffects + Send + Sync,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SyncResult<T>>,
{
    let policy = RetryPolicy::exponential().with_max_attempts(max_attempts);

    policy
        .execute_with_sleep(operation, |delay| async move {
            let _ = effects.sleep_ms(delay.as_millis() as u64).await;
        })
        .await
}

/// Execute an operation with fixed retry delay (convenience function)
pub async fn with_fixed_retry<E, F, Fut, T>(
    effects: &E,
    operation: F,
    max_attempts: u32,
    delay: Duration,
) -> SyncResult<T>
where
    E: aura_core::effects::PhysicalTimeEffects + Send + Sync,
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SyncResult<T>>,
{
    let policy = RetryPolicy::fixed(delay).with_max_attempts(max_attempts);

    policy
        .execute_with_sleep(operation, |d| async move {
            let _ = effects.sleep_ms(d.as_millis() as u64).await;
        })
        .await
}
