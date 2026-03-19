//! Time helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

use super::error::runtime_call;
use super::harness_determinism;
use crate::workflows::runtime::require_runtime;
use crate::AppCore;
use aura_core::AuraError;

/// Resolve current wall-clock time in milliseconds via the runtime bridge.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn current_time_ms(app_core: &Arc<RwLock<AppCore>>) -> Result<u64, AuraError> {
    let runtime = require_runtime(app_core).await?;
    runtime
        .current_time_ms()
        .await
        .map_err(|e| runtime_call("get current time", e).into())
}

/// Resolve a workflow timestamp using harness parity time when enabled and a
/// local fallback when no runtime clock is available.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn local_first_timestamp_ms(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &str,
    components: &[&str],
) -> u64 {
    if harness_determinism::harness_mode_enabled() {
        return harness_determinism::parity_timestamp_ms(app_core, scope, components)
            .await
            .unwrap_or(1);
    }

    match current_time_ms(app_core).await {
        Ok(ts) => ts,
        Err(_e) => {
            #[cfg(feature = "instrumented")]
            tracing::warn!(
                error = %_e,
                "time source unavailable — using fallback timestamp 1"
            );
            // Use 1 instead of 0 so the timestamp is distinguishable from
            // "not set" (which uses 0 by convention) while still being
            // obviously wrong if it shows up in debugging.
            1
        }
    }
}

/// Sleep through the runtime bridge so callers stay runtime-neutral.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn sleep_ms(app_core: &Arc<RwLock<AppCore>>, ms: u64) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    runtime.sleep_ms(ms).await;
    Ok(())
}
