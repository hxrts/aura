use std::future::Future;
use std::time::Duration;

use aura_app::harness_mode_enabled;
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::{
    execute_with_timeout_budget, TimeoutBudget, TimeoutExecutionProfile, TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;

pub(crate) enum TerminalTimeoutError<E> {
    Setup {
        context: &'static str,
        detail: String,
    },
    Timeout,
    Operation(E),
}

fn terminal_timeout_profile() -> TimeoutExecutionProfile {
    if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

/// One original shell teardown window; no phase may mint a replacement.
pub(crate) struct TerminalShutdownWindow<T = PhysicalTimeHandler> {
    time: T,
    budget: TimeoutBudget,
}

impl TerminalShutdownWindow {
    pub(crate) async fn begin() -> Result<Self, std::sync::Arc<aura_core::AuraError>> {
        let time = PhysicalTimeHandler::new();
        let started_at = time.physical_time().await.map_err(|source| {
            std::sync::Arc::new(aura_core::AuraError::Internal {
                message: "read original terminal shutdown clock".into(),
                source: Some(std::sync::Arc::new(source)),
            })
        })?;
        let duration = terminal_timeout_profile()
            .scale_duration(Duration::from_secs(5))
            .map_err(|source| {
                std::sync::Arc::new(aura_core::AuraError::Internal {
                    message: "scale original terminal shutdown window".into(),
                    source: Some(std::sync::Arc::new(source)),
                })
            })?;
        let budget =
            TimeoutBudget::from_start_and_timeout(&started_at, duration).map_err(|source| {
                std::sync::Arc::new(aura_core::AuraError::Internal {
                    message: "allocate original terminal shutdown window".into(),
                    source: Some(std::sync::Arc::new(source)),
                })
            })?;
        Ok(Self { time, budget })
    }
}

impl<TClock: PhysicalTimeEffects> TerminalShutdownWindow<TClock> {
    pub(crate) async fn run<T, F, Fut>(&self, operation: F) -> std::io::Result<T>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, std::io::Error>>,
    {
        execute_with_timeout_budget(&self.time, &self.budget, operation)
            .await
            .map_err(|error| match error {
                TimeoutRunError::Timeout(source) => {
                    let kind = if matches!(
                        &source,
                        aura_core::TimeoutBudgetError::DeadlineExceeded { .. }
                    ) {
                        std::io::ErrorKind::TimedOut
                    } else {
                        std::io::ErrorKind::Other
                    };
                    std::io::Error::new(kind, source)
                }
                TimeoutRunError::Operation(source) => source,
            })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("harness sender clearing failed: {clear}; task drainage failed: {drain}")]
struct ShutdownCleanupFailure {
    #[source]
    clear: std::io::Error,
    drain: std::io::Error,
}

pub(crate) async fn run_terminal_shutdown_cleanup<TClock, Clear, ClearFuture, Drain, DrainFuture>(
    window: &TerminalShutdownWindow<TClock>,
    clear: Clear,
    drain: Drain,
) -> std::io::Result<()>
where
    TClock: PhysicalTimeEffects,
    Clear: FnOnce() -> ClearFuture,
    ClearFuture: Future<Output = std::io::Result<()>>,
    Drain: FnOnce() -> DrainFuture,
    DrainFuture: Future<Output = ()>,
{
    let mut clear_failure = None;
    let drainage = window
        .run(|| async {
            clear_failure = clear().await.err();
            drain().await;
            Ok(())
        })
        .await;
    match (clear_failure, drainage) {
        (None, result) => result,
        (Some(clear), Ok(())) => Err(clear),
        (Some(clear), Err(drain)) => Err(std::io::Error::other(ShutdownCleanupFailure {
            clear,
            drain,
        })),
    }
}

pub(crate) async fn execute_with_terminal_timeout<T, E, F, Fut>(
    context: &'static str,
    duration: Duration,
    operation: F,
) -> Result<T, TerminalTimeoutError<E>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let time = PhysicalTimeHandler::new();
    let started_at = time
        .physical_time()
        .await
        .map_err(|error| TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to read physical time: {error}"),
        })?;
    let scaled = terminal_timeout_profile()
        .scale_duration(duration)
        .map_err(|error| TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to scale timeout: {error}"),
        })?;
    let budget = TimeoutBudget::from_start_and_timeout(&started_at, scaled).map_err(|error| {
        TerminalTimeoutError::Setup {
            context,
            detail: format!("failed to create timeout budget: {error}"),
        }
    })?;

    execute_with_timeout_budget(&time, &budget, operation)
        .await
        .map_err(|error| match error {
            TimeoutRunError::Timeout(_timeout_error) => {
                let _ = context;
                TerminalTimeoutError::Timeout
            }
            TimeoutRunError::Operation(error) => TerminalTimeoutError::Operation(error),
        })
}

#[cfg(test)]
mod shutdown_window_tests {
    use super::*;
    use aura_testkit::time::ManualPhysicalClock;
    use std::sync::Arc;

    #[tokio::test]
    async fn bootstrap_exit_and_task_drain_share_original_shutdown_endpoint() {
        let time = Arc::new(ManualPhysicalClock::new(1000));
        // Explicit positive test-only fixture owns the actual effect provider
        // and immutable deadline; both phases execute the production run method.
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time: time.clone(),
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let exit = window
            .run(|| async {
                time.set_time(5000);
                Ok(())
            })
            .await;
        assert!(
            matches!(exit, Ok(())),
            "fullscreen completion still lies in original window"
        );
        let drainage = window.run(|| futures::future::pending::<Result<(), std::io::Error>>());
        futures::pin_mut!(drainage);
        assert!(futures::poll!(drainage.as_mut()).is_pending());
        assert_eq!(time.now_ms(), 5000);
        time.advance(1000);
        let source = drainage.await.expect_err("original endpoint expires");
        assert_eq!(source.kind(), std::io::ErrorKind::TimedOut);
        assert!(matches!(
            source
                .get_ref()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 6000,
                observed_at_ms: 6000,
            })
        ));
        assert_eq!(time.now_ms(), 6000);
    }

    #[tokio::test]
    async fn shutdown_clock_rollback_retains_native_budget_cause_without_timeout_classification() {
        let time = Arc::new(ManualPhysicalClock::new(1000));
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time: time.clone(),
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let first = window.run(|| async { Ok(()) }).await;
        assert!(first.is_ok());
        time.set_time(999);
        let failure = window.run(|| async { Ok(()) }).await.unwrap_err();
        assert_eq!(failure.kind(), std::io::ErrorKind::Other);
        assert!(matches!(
            failure
                .get_ref()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
        ));
        assert_eq!(time.now_ms(), 999);
    }

    #[tokio::test]
    async fn shutdown_clear_failure_survives_original_endpoint_during_task_drain() {
        #[derive(Debug, thiserror::Error)]
        #[error("actual harness sender clearing fault")]
        struct NativeClearFailure;
        let time = Arc::new(ManualPhysicalClock::new(1000));
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time: time.clone(),
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let cleanup = run_terminal_shutdown_cleanup(
            &window,
            || async { Err(std::io::Error::other(NativeClearFailure)) },
            || futures::future::pending::<()>(),
        );
        futures::pin_mut!(cleanup);
        assert!(futures::poll!(cleanup.as_mut()).is_pending());
        time.advance(5000);
        let failure = cleanup.await.unwrap_err();
        let retained = failure
            .get_ref()
            .and_then(|source| source.downcast_ref::<ShutdownCleanupFailure>())
            .expect("both original local causes retained");
        assert!(retained
            .clear
            .get_ref()
            .is_some_and(|source| source.is::<NativeClearFailure>()));
        assert_eq!(retained.drain.kind(), std::io::ErrorKind::TimedOut);
        assert!(matches!(
            retained
                .drain
                .get_ref()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 6000,
                observed_at_ms: 6000,
            })
        ));
        assert_eq!(time.now_ms(), 6000);
    }
}
