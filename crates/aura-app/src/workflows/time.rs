//! Time helpers for workflows.

use std::sync::Arc;

use async_lock::RwLock;

use super::error::runtime_call;
use super::harness_determinism;
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::AppCore;
use aura_core::AuraError;
use thiserror::Error;

/// Typed failure modes for workflow time resolution.
#[derive(Debug, Clone, Error)]
pub enum TimeUnavailable {
    /// No runtime bridge is installed, so no authoritative time source exists.
    #[error("runtime is unavailable for workflow time")]
    RuntimeUnavailable,
    /// The runtime bridge reported a time-query failure.
    #[error("runtime time query failed: {detail}")]
    RuntimeQuery {
        /// Details from the runtime bridge failure.
        detail: String,
    },
    /// Harness parity mode requested a deterministic timestamp but no harness
    /// parity context was available.
    #[error("harness parity time unavailable")]
    HarnessParityUnavailable,
}

impl From<TimeUnavailable> for AuraError {
    fn from(value: TimeUnavailable) -> Self {
        AuraError::internal(value.to_string())
    }
}

/// Resolve current wall-clock time in milliseconds via the runtime bridge.
pub async fn current_time_ms(app_core: &Arc<RwLock<AppCore>>) -> Result<u64, TimeUnavailable> {
    let runtime = require_runtime(app_core)
        .await
        .map_err(|_| TimeUnavailable::RuntimeUnavailable)?;
    let result = timeout_runtime_call(
        &runtime,
        "workflow_time",
        "current_time_ms",
        std::time::Duration::from_secs(2),
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|error| TimeUnavailable::RuntimeQuery {
        detail: error.to_string(),
    })?;
    result.map_err(|error| TimeUnavailable::RuntimeQuery {
        detail: runtime_call("get current time", error).to_string(),
    })
}

/// Resolve a workflow timestamp using harness parity time when enabled and a
/// local fallback when no runtime clock is available.
pub async fn local_first_timestamp_ms(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &str,
    components: &[&str],
) -> Result<u64, TimeUnavailable> {
    if harness_determinism::harness_mode_enabled() {
        return harness_determinism::parity_timestamp_ms(app_core, scope, components)
            .await
            .map_err(|_| TimeUnavailable::HarnessParityUnavailable);
    }

    current_time_ms(app_core).await
}

/// Sleep through the runtime bridge so callers stay runtime-neutral.
pub async fn sleep_ms(app_core: &Arc<RwLock<AppCore>>, ms: u64) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    runtime.sleep_ms(ms).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn current_time_ms_without_runtime_returns_typed_error() {
        let app_core = crate::testing::default_test_app_core();

        let error = current_time_ms(&app_core)
            .await
            .expect_err("missing runtime should be surfaced explicitly");
        assert!(matches!(error, TimeUnavailable::RuntimeUnavailable));
    }

    #[tokio::test]
    async fn local_first_timestamp_without_runtime_returns_typed_error() {
        let app_core = crate::testing::default_test_app_core();

        let error = local_first_timestamp_ms(&app_core, "time-test", &[])
            .await
            .expect_err("missing runtime should not fall back to a fake timestamp");
        assert!(matches!(error, TimeUnavailable::RuntimeUnavailable));
    }
}
