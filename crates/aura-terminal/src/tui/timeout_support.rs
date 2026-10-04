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
    use aura_core::{effects::time::TimeError, PhysicalTime};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };

    struct ControlledShutdownClock {
        now: Arc<AtomicU64>,
        last_sleep: AtomicU64,
    }

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for ControlledShutdownClock {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            Ok(PhysicalTime {
                ts_ms: self.now.load(Ordering::SeqCst),
                uncertainty: None,
            })
        }
        async fn sleep_ms(&self, duration: u64) -> Result<(), TimeError> {
            self.last_sleep.store(duration, Ordering::SeqCst);
            self.now.fetch_add(duration, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn bootstrap_exit_and_task_drain_share_original_shutdown_endpoint() {
        let now = Arc::new(AtomicU64::new(1000));
        let time = ControlledShutdownClock {
            now: now.clone(),
            last_sleep: AtomicU64::new(0),
        };
        // Explicit positive test-only fixture owns the actual effect provider
        // and immutable deadline; both phases execute the production run method.
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time,
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let exit = window
            .run(|| async {
                now.store(5000, Ordering::SeqCst);
                Ok(())
            })
            .await;
        assert!(
            matches!(exit, Ok(())),
            "fullscreen completion still lies in original window"
        );
        let result = window
            .run(|| futures::future::pending::<Result<(), std::io::Error>>())
            .await;
        let source = result.expect_err("original endpoint expires");
        assert_eq!(source.kind(), std::io::ErrorKind::TimedOut);
        assert!(matches!(
            source
                .get_ref()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded { .. })
        ));
        assert_eq!(
            window.time.last_sleep.load(Ordering::SeqCst),
            1000,
            "task drainage receives only the original remaining second"
        );
        assert_eq!(now.load(Ordering::SeqCst), 6000);
    }

    #[tokio::test]
    async fn shutdown_clock_rollback_retains_native_budget_cause_without_timeout_classification() {
        let now = Arc::new(AtomicU64::new(1000));
        let time = ControlledShutdownClock {
            now: now.clone(),
            last_sleep: AtomicU64::new(0),
        };
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time,
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let first = window.run(|| async { Ok(()) }).await;
        assert!(first.is_ok());
        now.store(999, Ordering::SeqCst);
        let failure = window.run(|| async { Ok(()) }).await.unwrap_err();
        assert_eq!(failure.kind(), std::io::ErrorKind::Other);
        assert!(matches!(
            failure
                .get_ref()
                .and_then(|source| source.downcast_ref::<aura_core::TimeoutBudgetError>()),
            Some(aura_core::TimeoutBudgetError::ClockRollback { .. })
        ));
        assert_eq!(window.time.last_sleep.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn shutdown_clear_failure_survives_original_endpoint_during_task_drain() {
        #[derive(Debug, thiserror::Error)]
        #[error("actual harness sender clearing fault")]
        struct NativeClearFailure;
        let now = Arc::new(AtomicU64::new(1000));
        let time = ControlledShutdownClock {
            now: now.clone(),
            last_sleep: AtomicU64::new(0),
        };
        let start = time.physical_time().await.unwrap();
        let window = TerminalShutdownWindow {
            time,
            budget: TimeoutBudget::from_start_and_timeout(&start, Duration::from_secs(5)).unwrap(),
        };
        let failure = run_terminal_shutdown_cleanup(
            &window,
            || async { Err(std::io::Error::other(NativeClearFailure)) },
            || futures::future::pending::<()>(),
        )
        .await
        .unwrap_err();
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
            Some(aura_core::TimeoutBudgetError::DeadlineExceeded { .. })
        ));
        assert_eq!(now.load(Ordering::SeqCst), 6000);
    }
}
