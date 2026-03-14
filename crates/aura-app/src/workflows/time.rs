//! Time helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

use super::error::runtime_call;
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

/// Sleep through the runtime bridge so callers stay runtime-neutral.
#[cfg_attr(not(feature = "signals"), allow(dead_code))]
pub async fn sleep_ms(app_core: &Arc<RwLock<AppCore>>, ms: u64) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    runtime.sleep_ms(ms).await;
    Ok(())
}
