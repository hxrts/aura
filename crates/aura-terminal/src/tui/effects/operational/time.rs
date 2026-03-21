//! Time helpers for the TUI operational layer.
//!
//! The TUI should not read OS time directly. Prefer runtime-provided time via
//! `RuntimeBridge`, which is backed by `PhysicalTimeEffects` in production and
//! deterministic implementations in simulation.

use std::sync::Arc;
use std::time::Duration;

use async_lock::RwLock;
use aura_app::ui::prelude::*;
use aura_app::ui::workflows::runtime as runtime_workflows;

/// Get current time in milliseconds since Unix epoch.
///
/// Returns `0` when no runtime is configured.
pub async fn current_time_ms(app_core: &Arc<RwLock<AppCore>>) -> u64 {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };

    let Some(runtime) = runtime else {
        return 0;
    };

    match runtime_workflows::timeout_runtime_call(
        &runtime,
        "terminal_operational_time",
        "current_time_ms",
        Duration::from_secs(2),
        || runtime.current_time_ms(),
    )
    .await
    {
        Ok(Ok(ts)) => ts,
        Ok(Err(error)) => {
            tracing::warn!("terminal time query failed: {error}");
            0
        }
        Err(error) => {
            tracing::warn!("terminal bounded time query failed: {error}");
            0
        }
    }
}
