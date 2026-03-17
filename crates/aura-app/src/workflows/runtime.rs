//! Runtime access helpers for workflows.

use async_trait::async_trait;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use async_lock::RwLock;

use crate::core::IntentError;
use crate::runtime_bridge::RuntimeBridge;
use crate::AppCore;
use aura_core::{
    effects::{PhysicalTimeEffects, TimeError},
    execute_with_retry_budget, execute_with_timeout_budget, ExponentialBackoffPolicy,
    time::PhysicalTime,
    AuraError, RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutBudgetError,
    TimeoutExecutionProfile, TimeoutRunError,
};

const DEFAULT_HARNESS_CONVERGENCE_ROUNDS: usize = 8;
const DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS: u64 = 150;
const DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS: u64 = 1_000;

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

struct RuntimeTimeEffects<'a> {
    runtime: &'a Arc<dyn RuntimeBridge>,
}

#[async_trait]
impl PhysicalTimeEffects for RuntimeTimeEffects<'_> {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        self.runtime
            .current_time_ms()
            .await
            .map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            })
            .map_err(|error| TimeError::OperationFailed {
                reason: error.to_string(),
            })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        self.runtime.sleep_ms(ms).await;
        Ok(())
    }
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
    let time = RuntimeTimeEffects { runtime };
    execute_with_timeout_budget(&time, budget, operation).await
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
    let time = RuntimeTimeEffects { runtime };
    execute_with_retry_budget(&time, policy, operation).await
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

    let run_step =
        |future: Pin<Box<dyn Future<Output = Result<(), IntentError>> + Send>>| async move {
            let requested = Duration::from_millis(step_timeout_ms);
            match workflow_timeout_budget(runtime, requested).await {
                Ok(budget) => {
                    let _ =
                        execute_with_runtime_timeout_budget(runtime, &budget, || future).await;
                }
                Err(_) => {
                    let _ = future.await;
                }
            }
    };

    for round in 0..rounds {
        if harness_mode_enabled() {
            run_step(Box::pin(runtime.trigger_discovery())).await;
            run_step(Box::pin(runtime.process_ceremony_messages())).await;
        }
        run_step(Box::pin(runtime.trigger_sync())).await;
        cooperative_yield().await;

        if round + 1 < rounds && harness_mode_enabled() && backoff_ms > 0 {
            runtime.sleep_ms(backoff_ms).await;
        }
    }
}

/// Validate that the runtime has at least one viable connectivity path before a
/// shared-flow operation relies on remote convergence.
pub async fn ensure_runtime_peer_connectivity(
    runtime: &Arc<dyn RuntimeBridge>,
    flow: &str,
) -> Result<(), AuraError> {
    let sync_status = runtime.get_sync_status().await;
    let connected_peers = sync_status.connected_peers;
    let sync_peers = runtime.get_sync_peers().await;
    let discovered_peers = runtime.get_discovered_peers().await;
    let lan_peers = runtime.get_lan_peers().await;

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
    use super::ensure_runtime_peer_connectivity;
    use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
    use aura_core::types::identifiers::AuthorityId;
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
        assert!(message.contains("Missing connectivity prerequisite"));
        assert!(message.contains("test_flow"));
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
}
