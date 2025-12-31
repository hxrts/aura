//! Time helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

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
        .map_err(|e| AuraError::agent(format!("Failed to get time: {e}")))
}
