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
    RuntimeUnavailable {
        /// Original runtime lookup failure.
        #[source]
        source: AuraError,
    },
    /// The runtime bridge reported a time-query failure.
    #[error("runtime time query failed: {detail}")]
    RuntimeQuery {
        /// Details from the runtime bridge failure.
        detail: String,
        /// Original bounded runtime query failure.
        #[source]
        source: AuraError,
    },
    /// Harness parity mode requested a deterministic timestamp but no harness
    /// parity context was available.
    #[error("harness parity time unavailable")]
    HarnessParityUnavailable {
        /// Original deterministic parity context failure.
        #[source]
        source: AuraError,
    },
}

impl From<TimeUnavailable> for AuraError {
    fn from(value: TimeUnavailable) -> Self {
        AuraError::Internal {
            message: value.to_string(),
            source: Some(Arc::new(value)),
        }
    }
}

/// Resolve current wall-clock time in milliseconds via the runtime bridge.
pub async fn current_time_ms(app_core: &Arc<RwLock<AppCore>>) -> Result<u64, TimeUnavailable> {
    let runtime = require_runtime(app_core)
        .await
        .map_err(|source| TimeUnavailable::RuntimeUnavailable { source })?;
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
        source: error,
    })?;
    result.map_err(|error| {
        let context = runtime_call("get current time", error);
        TimeUnavailable::RuntimeQuery {
            detail: context.to_string(),
            source: context.into(),
        }
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
            .map_err(|source| TimeUnavailable::HarnessParityUnavailable { source });
    }

    current_time_ms(app_core).await
}

/// Sleep through the runtime bridge so callers stay runtime-neutral.
pub async fn sleep_ms(app_core: &Arc<RwLock<AppCore>>, ms: u64) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    runtime
        .sleep_ms(ms)
        .await
        .map_err(|error| super::error::runtime_call("required sleep", error).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_conversion_retains_typed_query_and_runtime_cause() {
        use std::error::Error;
        let original = AuraError::from(super::super::error::WorkflowError::TimedOut {
            operation: "time",
            stage: "query",
            timeout_ms: 2000,
        });
        let unavailable = TimeUnavailable::RuntimeQuery {
            detail: original.to_string(),
            source: original,
        };
        let display = unavailable.to_string();
        let outer = AuraError::from(unavailable);
        assert_eq!(outer.to_string(), format!("Internal error: {display}"));
        let query = outer
            .source()
            .unwrap()
            .downcast_ref::<TimeUnavailable>()
            .unwrap();
        let bounded = query.source().unwrap().downcast_ref::<AuraError>().unwrap();
        assert!(matches!(
            bounded
                .source()
                .unwrap()
                .downcast_ref::<super::super::error::WorkflowError>(),
            Some(super::super::error::WorkflowError::TimedOut { .. })
        ));
    }

    #[test]
    fn parity_time_preserves_original_source_without_fallback() {
        use std::error::Error;
        let original = AuraError::Invalid {
            message: "missing parity context".into(),
            source: Some(Arc::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "context",
            ))),
        };
        let outer = AuraError::from(TimeUnavailable::HarnessParityUnavailable { source: original });
        let parity = outer
            .source()
            .unwrap()
            .downcast_ref::<TimeUnavailable>()
            .unwrap();
        assert!(matches!(
            parity,
            TimeUnavailable::HarnessParityUnavailable { .. }
        ));
        let original = parity
            .source()
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert_eq!(
            original
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[tokio::test]
    async fn current_time_ms_without_runtime_returns_typed_error() {
        let app_core = crate::testing::default_test_app_core();

        let error = current_time_ms(&app_core)
            .await
            .expect_err("missing runtime should be surfaced explicitly");
        assert!(matches!(error, TimeUnavailable::RuntimeUnavailable { .. }));
        let source = std::error::Error::source(&error)
            .unwrap()
            .downcast_ref::<AuraError>()
            .unwrap();
        assert!(matches!(
            std::error::Error::source(source)
                .unwrap()
                .downcast_ref::<super::super::error::WorkflowError>(),
            Some(super::super::error::WorkflowError::RuntimeUnavailable)
        ));
    }

    #[tokio::test]
    async fn local_first_timestamp_without_runtime_returns_typed_error() {
        let app_core = crate::testing::default_test_app_core();

        let error = local_first_timestamp_ms(&app_core, "time-test", &[])
            .await
            .expect_err("missing runtime should not fall back to a fake timestamp");
        assert!(matches!(error, TimeUnavailable::RuntimeUnavailable { .. }));
    }
}
