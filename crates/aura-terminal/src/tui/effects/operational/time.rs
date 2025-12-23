//! Time helpers for the TUI operational layer.
//!
//! The TUI should not read OS time directly. Prefer runtime-provided time via
//! `RuntimeBridge`, which is backed by `PhysicalTimeEffects` in production and
//! deterministic implementations in simulation.

use std::sync::Arc;

use async_lock::RwLock;
use aura_app::AppCore;

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

    runtime.current_time_ms().await.unwrap_or(0)
}
