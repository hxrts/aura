//! Runtime access helpers for workflows.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_lock::RwLock;
use futures::{
    future::{select, Either},
    pin_mut,
};

use crate::core::IntentError;
use crate::runtime_bridge::RuntimeBridge;
use crate::AppCore;
use aura_core::{
    time::PhysicalTime, AuraError, ExponentialBackoffPolicy, PostTerminalBestEffort,
    RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutBudgetError, TimeoutExecutionProfile,
    TimeoutRunError,
};

const DEFAULT_HARNESS_CONVERGENCE_ROUNDS: usize = 8;
const DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS: u64 = 150;
const DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS: u64 = 1_000;

/// Canonical best-effort collector for workflow follow-up that must not own
/// primary terminal lifecycle.
pub type WorkflowBestEffort = PostTerminalBestEffort<AuraError>;

#[cfg(test)]
static HARNESS_MODE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

fn harness_mode_enabled() -> bool {
    #[cfg(test)]
    match HARNESS_MODE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    std::env::var_os("AURA_HARNESS_MODE").is_some()
}

fn harness_convergence_rounds() -> usize {
    std::env::var("AURA_HARNESS_CONVERGENCE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(DEFAULT_HARNESS_CONVERGENCE_ROUNDS)
}

fn harness_convergence_backoff_ms() -> u64 {
    std::env::var("AURA_HARNESS_CONVERGENCE_BACKOFF_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS)
}

fn harness_convergence_step_timeout_ms() -> u64 {
    std::env::var("AURA_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|timeout_ms| *timeout_ms > 0)
        .unwrap_or(DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS)
}

/// Shared timeout scaling profile for workflow-owned local deadlines.
pub fn workflow_timeout_profile() -> TimeoutExecutionProfile {
    if harness_mode_enabled() {
        TimeoutExecutionProfile::harness()
    } else {
        TimeoutExecutionProfile::production()
    }
}

/// Scale a workflow-local timeout duration for the active execution lane.
pub fn scaled_workflow_duration(duration: Duration) -> Result<Duration, TimeoutBudgetError> {
    workflow_timeout_profile().scale_duration(duration)
}

/// Create a runtime-backed timeout budget for a workflow stage or operation.
pub async fn workflow_timeout_budget(
    runtime: &Arc<dyn RuntimeBridge>,
    duration: Duration,
) -> Result<TimeoutBudget, TimeoutBudgetError> {
    let started_at = runtime
        .current_time_ms()
        .await
        .map_err(|error| TimeoutBudgetError::time_source_unavailable(error.to_string()))
        .map(|ts_ms| PhysicalTime {
            ts_ms,
            uncertainty: None,
        })?;
    let scaled = scaled_workflow_duration(duration)?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled)
}

/// Execute a workflow operation under a runtime-backed timeout budget.
pub async fn execute_with_runtime_timeout_budget<T, E, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    budget: &TimeoutBudget,
    operation: F,
) -> Result<T, TimeoutRunError<E>>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let now = runtime_current_physical_time(runtime)
        .await
        .map_err(TimeoutRunError::Timeout)?;
    let remaining = budget
        .remaining_at(&now)
        .map_err(TimeoutRunError::Timeout)?;
    let sleep_ms = duration_to_ms(remaining).map_err(TimeoutRunError::Timeout)?;

    let operation_future = operation();
    let sleep_future = async {
        runtime.sleep_ms(sleep_ms).await;
    };
    pin_mut!(operation_future);
    pin_mut!(sleep_future);

    match select(operation_future, sleep_future).await {
        Either::Left((result, _sleep_future)) => result.map_err(TimeoutRunError::Operation),
        Either::Right(((), _operation_future)) => {
            let observed_at_ms = runtime
                .current_time_ms()
                .await
                .unwrap_or(budget.deadline_at_ms());
            Err(TimeoutRunError::Timeout(
                TimeoutBudgetError::deadline_exceeded(budget.deadline_at_ms(), observed_at_ms),
            ))
        }
    }
}

/// Emit a diagnostic warning whenever a workflow-owned timeout fires.
pub fn warn_workflow_timeout(operation: &'static str, stage: &'static str, timeout_ms: u64) {
    #[cfg(feature = "instrumented")]
    tracing::warn!(
        operation,
        stage,
        timeout_ms,
        "workflow timeout triggered; treat this as a diagnostic for a deeper design or convergence flaw"
    );

    #[cfg(not(feature = "instrumented"))]
    let _ = (operation, stage, timeout_ms);
}

/// Execute a runtime call under an explicit workflow-owned timeout and surface a
/// typed workflow timeout on expiry.
pub async fn timeout_runtime_call<T, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    operation: &'static str,
    stage: &'static str,
    duration: Duration,
    call: F,
) -> Result<T, AuraError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let budget = workflow_timeout_budget(runtime, duration)
        .await
        .map_err(AuraError::from)?;
    match execute_with_runtime_timeout_budget(runtime, &budget, || async {
        Ok::<T, AuraError>(call().await)
    })
    .await
    {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. })) => {
            warn_workflow_timeout(operation, stage, budget.timeout_ms());
            Err(AuraError::from(
                crate::workflows::error::WorkflowError::TimedOut {
                    operation,
                    stage,
                    timeout_ms: budget.timeout_ms(),
                },
            ))
        }
        Err(TimeoutRunError::Timeout(error)) => Err(error.into()),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

/// Build a runtime-backed retry policy scaled for the active workflow lane.
/// Build a runtime-backed retry policy scaled for the active workflow lane.
pub fn workflow_retry_policy(
    max_attempts: u32,
    initial_delay: Duration,
    max_delay: Duration,
) -> Result<RetryBudgetPolicy, TimeoutBudgetError> {
    let base = RetryBudgetPolicy::new(
        max_attempts,
        ExponentialBackoffPolicy::new(
            initial_delay,
            max_delay,
            workflow_timeout_profile().jitter(),
        )?,
    );
    workflow_timeout_profile().apply_retry_policy(&base)
}

/// Create the canonical post-terminal best-effort collector for workflow code.
#[must_use]
pub fn workflow_best_effort() -> WorkflowBestEffort {
    WorkflowBestEffort::post_terminal_only()
}

/// Execute a workflow operation under a runtime-backed retry budget.
/// Execute a workflow operation under a runtime-backed retry budget.
pub async fn execute_with_runtime_retry_budget<T, E, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    policy: &RetryBudgetPolicy,
    operation: F,
) -> Result<T, RetryRunError<E>>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempts = policy.attempt_budget();
    let mut operation = operation;

    loop {
        let attempt = attempts.record_attempt().map_err(RetryRunError::Timeout)?;

        let result = if let Some(timeout) = policy.per_attempt_timeout() {
            let now = runtime_current_physical_time(runtime)
                .await
                .map_err(RetryRunError::Timeout)?;
            let budget = TimeoutBudget::from_start_and_timeout(&now, timeout)
                .map_err(RetryRunError::Timeout)?;
            execute_with_runtime_timeout_budget(runtime, &budget, || operation(attempt)).await
        } else {
            operation(attempt).await.map_err(TimeoutRunError::Operation)
        };

        match result {
            Ok(value) => return Ok(value),
            Err(TimeoutRunError::Timeout(error)) => return Err(RetryRunError::Timeout(error)),
            Err(TimeoutRunError::Operation(error)) => {
                if !attempts.can_attempt() {
                    return Err(RetryRunError::AttemptsExhausted {
                        attempts_used: attempts.attempts_used(),
                        last_error: error,
                    });
                }

                let delay_ms = duration_to_ms(policy.delay_for_attempt(attempt))
                    .map_err(RetryRunError::Timeout)?;
                runtime.sleep_ms(delay_ms).await;
            }
        }
    }
}

/// Get the runtime bridge or return a consistent error.
pub async fn require_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Arc<dyn RuntimeBridge>, AuraError> {
    let core = app_core.read().await;
    core.runtime()
        .cloned()
        .ok_or_else(|| AuraError::from(super::error::WorkflowError::RuntimeUnavailable))
}

/// Yield to the scheduler once without binding workflows to a runtime crate.
pub async fn cooperative_yield() {
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    YieldOnce(false).await;
}

async fn runtime_current_physical_time(
    runtime: &Arc<dyn RuntimeBridge>,
) -> Result<PhysicalTime, TimeoutBudgetError> {
    runtime
        .current_time_ms()
        .await
        .map(|ts_ms| PhysicalTime {
            ts_ms,
            uncertainty: None,
        })
        .map_err(|error| TimeoutBudgetError::time_source_unavailable(error.to_string()))
}

fn duration_to_ms(duration: Duration) -> Result<u64, TimeoutBudgetError> {
    u64::try_from(duration.as_millis()).map_err(|_| {
        TimeoutBudgetError::invalid_policy("duration does not fit in u64 milliseconds")
    })
}

/// Ask the runtime to perform a bounded convergence pass suitable for harness-mode real-runtime
/// execution. The runtime bridge owns the actual harness profile policy.
pub async fn converge_runtime(runtime: &Arc<dyn RuntimeBridge>) {
    let rounds = if harness_mode_enabled() {
        harness_convergence_rounds()
    } else {
        1
    };
    let backoff_ms = harness_convergence_backoff_ms();
    let step_timeout_ms = harness_convergence_step_timeout_ms();

    async fn run_step<T, F>(runtime: &Arc<dyn RuntimeBridge>, step_timeout_ms: u64, future: F)
    where
        F: Future<Output = Result<T, IntentError>>,
    {
        let requested = Duration::from_millis(step_timeout_ms);
        match workflow_timeout_budget(runtime, requested).await {
            Ok(budget) => {
                let _ = execute_with_runtime_timeout_budget(runtime, &budget, || future).await;
            }
            Err(_) => {
                // Budget creation failed (time source unavailable, etc.).  Rather than
                // awaiting without any deadline — which can stall the convergence loop
                // indefinitely — race the operation against a hard ceiling sleep.
                let ceiling_ms = step_timeout_ms.max(DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS);
                let operation = future;
                let sleep = runtime.sleep_ms(ceiling_ms);
                pin_mut!(operation);
                pin_mut!(sleep);
                match select(operation, sleep).await {
                    Either::Left((result, _)) => {
                        let _ = result;
                    }
                    Either::Right(((), _)) => {
                        // Hard ceiling reached — drop the operation and continue.
                    }
                }
            }
        }
    }

    for round in 0..rounds {
        if harness_mode_enabled() {
            run_step(runtime, step_timeout_ms, runtime.trigger_discovery()).await;
            run_step(
                runtime,
                step_timeout_ms,
                runtime.process_ceremony_messages(),
            )
            .await;
        }
        run_step(runtime, step_timeout_ms, runtime.trigger_sync()).await;
        // Sync can pull fresh acceptance/envelope traffic into the local inbox, so
        // process ceremony messages again after sync before the caller observes
        // any readiness derived from that traffic.
        run_step(
            runtime,
            step_timeout_ms,
            runtime.process_ceremony_messages(),
        )
        .await;
        cooperative_yield().await;

        if round + 1 < rounds && harness_mode_enabled() && backoff_ms > 0 {
            runtime.sleep_ms(backoff_ms).await;
        }
    }
}

/// Run one bounded harness/runtime upkeep pass and then republish observed
/// account state from the authoritative workflow boundary.
///
/// This is the shared frontend-facing maintenance shape for harness-mode real
/// runtime execution. Frontend shells may schedule when to run the pass, but
/// they should not fork their own step ordering.
pub async fn run_harness_runtime_maintenance_pass(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
) -> Result<(), AuraError> {
    converge_runtime(runtime).await;
    super::system::refresh_account(app_core).await
}

/// Validate that the runtime has at least one viable connectivity path before a
/// shared-flow operation relies on remote convergence.
pub async fn ensure_runtime_peer_connectivity(
    runtime: &Arc<dyn RuntimeBridge>,
    flow: &str,
) -> Result<(), AuraError> {
    let sync_status = runtime
        .try_get_sync_status()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get sync status", e)))?;
    let connected_peers = sync_status.connected_peers;
    let sync_peers = runtime
        .try_get_sync_peers()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get sync peers", e)))?;
    let discovered_peers = runtime
        .try_get_discovered_peers()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get discovered peers", e)))?;
    let lan_peers = runtime
        .try_get_lan_peers()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get lan peers", e)))?;

    if connected_peers == 0 {
        return Err(super::error::WorkflowError::ConnectivityRequired {
            flow: flow.to_string(),
            connected_peers,
            sync_peers: sync_peers.len(),
            discovered_peers: discovered_peers.len(),
            lan_peers: lan_peers.len(),
        }
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ensure_runtime_peer_connectivity, workflow_best_effort};
    use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
    use aura_core::{types::identifiers::AuthorityId, AuraError};
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    fn block_on<F: Future>(future: F) -> F::Output {
        struct NoopWake;

        impl Wake for NoopWake {
            fn wake(self: Arc<Self>) {}
        }

        let waker = Waker::from(Arc::new(NoopWake));
        let mut future = std::pin::pin!(future);
        let mut context = Context::from_waker(&waker);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    struct HarnessEnvGuard;

    impl Drop for HarnessEnvGuard {
        fn drop(&mut self) {
            HARNESS_ENV_LOCK.store(false, Ordering::Release);
        }
    }

    static HARNESS_ENV_LOCK: AtomicBool = AtomicBool::new(false);

    fn harness_env_lock() -> HarnessEnvGuard {
        while HARNESS_ENV_LOCK
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            std::thread::yield_now();
        }
        HarnessEnvGuard
    }

    fn with_harness_mode_env<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
        let _guard = harness_env_lock();
        let previous =
            super::HARNESS_MODE_OVERRIDE.swap(if enabled { 2 } else { 1 }, Ordering::Relaxed);
        let result = f();
        super::HARNESS_MODE_OVERRIDE.store(previous, Ordering::Relaxed);
        result
    }

    #[tokio::test]
    async fn connectivity_check_fails_when_no_peers_exist() {
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(OfflineRuntimeBridge::new(
            AuthorityId::new_from_entropy([7_u8; 32]),
        ));

        let error = ensure_runtime_peer_connectivity(&runtime, "test_flow")
            .await
            .expect_err("offline runtime should not satisfy peer connectivity");

        let message = error.to_string();
        assert!(message.contains("get sync status"));
        assert!(message.contains("No agent configured"));
    }

    #[test]
    fn connectivity_check_is_harness_mode_neutral() {
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(OfflineRuntimeBridge::new(
            AuthorityId::new_from_entropy([9_u8; 32]),
        ));

        let normal = with_harness_mode_env(false, || {
            block_on(async {
                ensure_runtime_peer_connectivity(&runtime, "neutral_flow")
                    .await
                    .expect_err("offline runtime should fail without harness mode")
                    .to_string()
            })
        });
        let harness = with_harness_mode_env(true, || {
            block_on(async {
                ensure_runtime_peer_connectivity(&runtime, "neutral_flow")
                    .await
                    .expect_err("offline runtime should fail with harness mode")
                    .to_string()
            })
        });

        assert_eq!(normal, harness);
    }

    #[tokio::test]
    async fn workflow_best_effort_preserves_first_error_across_multiple_captures() {
        let mut best_effort = workflow_best_effort();

        let _ = best_effort
            .capture(async { Err::<(), _>(AuraError::agent("first best-effort failure")) })
            .await;
        let _ = best_effort
            .capture(async { Err::<(), _>(AuraError::agent("second best-effort failure")) })
            .await;

        let first_error = best_effort
            .first_error()
            .expect("first error should be retained")
            .to_string();
        assert!(first_error.contains("first best-effort failure"));

        let final_error = best_effort
            .finish()
            .expect_err("best-effort collector should surface the first error");
        let message = final_error.to_string();
        assert!(message.contains("first best-effort failure"));
        assert!(!message.contains("second best-effort failure"));
    }
}
