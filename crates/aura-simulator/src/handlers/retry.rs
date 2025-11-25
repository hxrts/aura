//! Retry helpers wired to simulation time.
//!
//! These wrappers ensure retry delays advance simulated time via `SimulationTimeHandler`,
//! keeping the simulator deterministic and avoiding real sleeps.

use crate::handlers::time_control::SimulationTimeHandler;
use aura_sync::core::SyncResult;
use aura_sync::infrastructure::retry::{with_exponential_backoff, with_fixed_retry};
use std::time::Duration;

/// Retry with exponential backoff using simulation time.
pub async fn simulated_exponential_backoff<F, Fut, T>(
    time: &SimulationTimeHandler,
    operation: F,
    max_attempts: u32,
) -> SyncResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SyncResult<T>>,
{
    with_exponential_backoff(time, operation, max_attempts).await
}

/// Retry with fixed delay using simulation time.
pub async fn simulated_fixed_retry<F, Fut, T>(
    time: &SimulationTimeHandler,
    operation: F,
    max_attempts: u32,
    delay: Duration,
) -> SyncResult<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = SyncResult<T>>,
{
    with_fixed_retry(time, operation, max_attempts, delay).await
}
