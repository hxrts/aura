//! Runtime access helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

use crate::runtime_bridge::RuntimeBridge;
use crate::AppCore;
use aura_core::AuraError;

/// Get the runtime bridge or return a consistent error.
pub async fn require_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Arc<dyn RuntimeBridge>, AuraError> {
    let core = app_core.read().await;
    core.runtime()
        .cloned()
        .ok_or_else(|| AuraError::agent("Runtime bridge not available"))
}
