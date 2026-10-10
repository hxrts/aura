//! Runtime access helpers for workflows.

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
    time::PhysicalTime, AuraError, ExponentialBackoffPolicy, PostTerminalBestEffort,
    RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutBudgetError, TimeoutExecutionProfile,
    TimeoutRunError,
};

// Harness-only convergence tuning for observed workflow stabilization. These
// are intentionally env-tunable because they affect test/debug pacing rather
// than production protocol semantics.
const DEFAULT_HARNESS_CONVERGENCE_ROUNDS: usize = 8;
const DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS: u64 = 150;
const DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS: u64 = 1_000;

/// Canonical best-effort collector for workflow follow-up that must not own
/// primary terminal lifecycle.
pub type WorkflowBestEffort = PostTerminalBestEffort<AuraError>;

/// Prepare the native issuer's retained allocation. Native readiness transfers
/// only after its original sealed clock and task owner have been admitted;
/// early failure returns the source and performs the owner's teardown.
pub(crate) async fn prepare_original_enrollment_issuer(
    runtime: &Arc<dyn RuntimeBridge>,
    nickname_suggestion: String,
    setup: crate::workflows::ceremonies::UserTransferredEnrollmentSetup,
) -> Result<
    crate::runtime_bridge::PreparedDeviceEnrollmentSigning,
    aura_invitation::enrollment_setup::EnrollmentIssuanceError,
> {
    runtime
        .prepare_device_enrollment_ceremony(nickname_suggestion, setup)
        .await
}

/// Admit only the original device's explicit consent. Native readiness keeps
/// the actual participant task in its bounded registry under the local sealed
/// approval window; this adapter cannot turn packet receipt into consent.
pub(crate) async fn approve_original_enrollment_participant(
    runtime: &Arc<dyn RuntimeBridge>,
    approval: crate::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
) -> Result<(), AuraError> {
    approval.require_runtime_owner(runtime.as_ref())?;
    runtime.approve_device_enrollment_signing(approval).await
}

/// Resume the retained native issuer and observe its original bounded owner.
/// The runtime bridge consumes this original approval, checks its runtime
/// identity, and waits/drains through the prepared issuer's restricted original
/// completion observer. An app timeout would replace that clock or drop the
/// retained completion future, so this adapter delegates that entire boundary.
pub(crate) async fn resume_original_enrollment_issuer(
    runtime: &Arc<dyn RuntimeBridge>,
    approval: crate::workflows::ceremonies::UserApprovedEnrollmentSigningIntent,
) -> Result<
    crate::runtime_bridge::DeviceEnrollmentStart,
    aura_invitation::enrollment_setup::EnrollmentIssuanceError,
> {
    approval
        .require_runtime_owner(runtime.as_ref())
        .map_err(
            |source| aura_invitation::enrollment_setup::EnrollmentIssuanceError::Failure {
                stage: aura_invitation::enrollment_setup::EnrollmentIssuanceStage::InvitationExport,
                source,
            },
        )?;
    runtime.resume_device_enrollment_signing(approval).await
}

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

/// Resolve an observed-shell poll interval without letting UI crates branch on
/// harness mode directly.
pub fn harness_observed_poll_interval(env_key: &str, default_ms: u64) -> Duration {
    harness_observed_poll_interval_for_mode(harness_mode_enabled(), env_key, default_ms)
}

/// Resolve an observed-shell poll interval for an explicit execution lane.
pub fn harness_observed_poll_interval_for_mode(
    harness_mode: bool,
    env_key: &str,
    default_ms: u64,
) -> Duration {
    if harness_mode {
        let poll_ms = std::env::var(env_key)
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|poll_ms| *poll_ms > 0)
            .unwrap_or(default_ms);
        Duration::from_millis(poll_ms)
    } else {
        Duration::from_millis(default_ms)
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
        .physical_time_provider()
        .physical_time()
        .await
        .map_err(TimeoutBudgetError::time_source_failure)?;
    let scaled = scaled_workflow_duration(duration)?;
    TimeoutBudget::from_start_and_timeout(&started_at, scaled)
}

async fn runtime_observation_with_budget(
    runtime: &Arc<dyn RuntimeBridge>,
    parent: &TimeoutBudget,
) -> Result<PhysicalTime, TimeoutBudgetError> {
    aura_core::time::timeout::observe_with_timeout_budget(&runtime.physical_time_provider(), parent)
        .await
}

/// Clamp a stage policy to the original workflow endpoint and clock observation.
pub async fn workflow_child_timeout_budget(
    runtime: &Arc<dyn RuntimeBridge>,
    parent: &TimeoutBudget,
    duration: Duration,
) -> Result<TimeoutBudget, TimeoutBudgetError> {
    let now = runtime_observation_with_budget(runtime, parent).await?;
    parent.child_budget(&now, scaled_workflow_duration(duration)?)
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
    aura_core::time::timeout::execute_with_timeout_budget(
        &runtime.physical_time_provider(),
        budget,
        operation,
    )
    .await
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
    timeout_runtime_call_under_budget(runtime, &budget, operation, stage, call).await
}

/// Execute a nested runtime stage without replacing its owner's endpoint.
pub async fn timeout_runtime_call_with_budget<T, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    parent: &TimeoutBudget,
    operation: &'static str,
    stage: &'static str,
    duration: Duration,
    call: F,
) -> Result<T, AuraError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let budget = workflow_child_timeout_budget(runtime, parent, duration).await?;
    timeout_runtime_call_under_budget(runtime, &budget, operation, stage, call).await
}

async fn timeout_runtime_call_under_budget<T, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    budget: &TimeoutBudget,
    operation: &'static str,
    stage: &'static str,
    call: F,
) -> Result<T, AuraError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    match execute_with_runtime_timeout_budget(runtime, budget, || async {
        Ok::<T, AuraError>(call().await)
    })
    .await
    {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Timeout(source @ TimeoutBudgetError::DeadlineExceeded { .. })) => {
            warn_workflow_timeout(operation, stage, budget.timeout_ms());
            let cause = crate::workflows::error::WorkflowError::TimedOut {
                operation,
                stage,
                timeout_ms: budget.timeout_ms(),
            };
            Err(AuraError::Internal {
                message: cause.to_string(),
                source: Some(Arc::new(source)),
            })
        }
        Err(TimeoutRunError::Timeout(error)) => Err(error.into()),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

/// Bound for one best-effort committed-fact send.
const COMMITTED_FACT_SEND_TIMEOUT: Duration = Duration::from_millis(5_000);

/// One best-effort send of an already committed relational fact to `peer`.
/// It only cuts latency: relational-context sync (docs/111 §11.4) delivers a
/// fact this send loses, so callers do not retry it and a failure never
/// turns the committed action into a reported failure.
pub(crate) async fn send_committed_fact(
    runtime: &Arc<dyn RuntimeBridge>,
    operation: &'static str,
    peer: aura_core::types::identifiers::AuthorityId,
    context: aura_core::types::identifiers::ContextId,
    fact: &aura_journal::fact::RelationalFact,
) -> Result<(), AuraError> {
    let budget = workflow_timeout_budget(runtime, COMMITTED_FACT_SEND_TIMEOUT).await?;
    send_committed_fact_with_budget(runtime, &budget, operation, peer, context, fact).await
}

/// Send an already committed fact within an enclosing delivery owner's window.
pub(crate) async fn send_committed_fact_with_budget(
    runtime: &Arc<dyn RuntimeBridge>,
    parent: &TimeoutBudget,
    operation: &'static str,
    peer: aura_core::types::identifiers::AuthorityId,
    context: aura_core::types::identifiers::ContextId,
    fact: &aura_journal::fact::RelationalFact,
) -> Result<(), AuraError> {
    timeout_runtime_call_with_budget(
        runtime,
        parent,
        operation,
        "send_chat_fact",
        COMMITTED_FACT_SEND_TIMEOUT,
        || runtime.send_chat_fact(peer, context, fact),
    )
    .await?
    .map_err(|error| crate::workflows::error::runtime_call(operation, error).into())
}

/// Causal metadata for a new order-independent fact (docs/105 §4.2.1): the
/// runtime advances its logical clock past the facts of `key`'s family and
/// records what the new fact revokes or supersedes.
pub(crate) async fn runtime_causal_stamp(
    runtime: &Arc<dyn RuntimeBridge>,
    operation: &'static str,
    stage: &'static str,
    duration: Duration,
    key: crate::runtime_bridge::CausalStampKey,
) -> Result<aura_core::time::CausalMetadata, AuraError> {
    Ok(
        timeout_runtime_call(runtime, operation, "causal_stamp", duration, || {
            runtime.causal_stamp(key)
        })
        .await
        .map_err(|e| crate::workflows::error::runtime_call(stage, e))?
        .map_err(|e| crate::workflows::error::runtime_call(stage, e))?,
    )
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

/// Execute bounded attempts and backoff under the original owner endpoint.
pub async fn execute_with_runtime_retry_budget<T, E, F, Fut>(
    runtime: &Arc<dyn RuntimeBridge>,
    parent: &TimeoutBudget,
    policy: &RetryBudgetPolicy,
    operation: F,
) -> Result<T, RetryRunError<E>>
where
    E: std::error::Error + 'static,
    F: FnMut(u32, TimeoutBudget) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempts = policy.attempt_budget();
    let mut operation = operation;

    loop {
        let observed = runtime_observation_with_budget(runtime, parent)
            .await
            .map_err(RetryRunError::Timeout)?;
        let remaining = parent
            .remaining_at(&observed)
            .map_err(RetryRunError::Timeout)?;
        let attempt = attempts.record_attempt().map_err(RetryRunError::Timeout)?;
        let child = parent
            .child_budget(&observed, policy.per_attempt_timeout().unwrap_or(remaining))
            .map_err(RetryRunError::Timeout)?;
        let result = execute_with_runtime_timeout_budget(runtime, &child, || {
            operation(attempt, child.clone())
        })
        .await;

        match result {
            Ok(value) => return Ok(value),
            Err(TimeoutRunError::Timeout(error)) => return Err(RetryRunError::Timeout(error)),
            Err(TimeoutRunError::Operation(error)) => {
                // A child may have retained a clock/timer failure through a
                // domain error. That failure terminates the same owner; it is
                // never evidence that a later attempt is safe.
                let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(&error);
                while let Some(source) = cause {
                    if let Some(timeout) = source.downcast_ref::<TimeoutBudgetError>() {
                        return Err(RetryRunError::Timeout(timeout.clone()));
                    }
                    cause = source.source();
                }
                if !attempts.can_attempt() {
                    return Err(RetryRunError::AttemptsExhausted {
                        attempts_used: attempts.attempts_used(),
                        last_error: error,
                    });
                }

                let delay_ms = duration_to_ms(policy.delay_for_attempt(attempt))
                    .map_err(RetryRunError::Timeout)?;
                execute_with_runtime_timeout_budget(runtime, parent, || runtime.sleep_ms(delay_ms))
                    .await
                    .map_err(|error| {
                        RetryRunError::Timeout(match error {
                            TimeoutRunError::Timeout(error) => error,
                            TimeoutRunError::Operation(error) => {
                                TimeoutBudgetError::time_source_failure(error)
                            }
                        })
                    })?;
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

fn duration_to_ms(duration: Duration) -> Result<u64, TimeoutBudgetError> {
    u64::try_from(duration.as_millis()).map_err(|_| {
        TimeoutBudgetError::invalid_policy("duration does not fit in u64 milliseconds")
    })
}

/// Run required convergence under the caller's original operation endpoint.
/// Each step may use a shorter local policy, but cannot extend its parent.
pub async fn converge_runtime(
    runtime: &Arc<dyn RuntimeBridge>,
    budget: &TimeoutBudget,
) -> Result<(), AuraError> {
    converge_runtime_cycle(
        runtime,
        budget,
        Duration::from_millis(DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS),
    )
    .await
}

async fn converge_runtime_cycle(
    runtime: &Arc<dyn RuntimeBridge>,
    budget: &TimeoutBudget,
    step_timeout: Duration,
) -> Result<(), AuraError> {
    async fn run_step<T>(
        runtime: &Arc<dyn RuntimeBridge>,
        parent: &TimeoutBudget,
        requested: Duration,
        operation: &'static str,
        future: impl Future<Output = Result<T, IntentError>>,
    ) -> Result<(), AuraError> {
        let child = workflow_child_timeout_budget(runtime, parent, requested).await?;
        execute_with_runtime_timeout_budget(runtime, &child, || future)
            .await
            .map(|_| ())
            .map_err(|error| match error {
                TimeoutRunError::Timeout(error) => AuraError::from(error),
                TimeoutRunError::Operation(error) => {
                    AuraError::from(super::error::runtime_call(operation, error))
                }
            })
    }

    run_step(
        runtime,
        budget,
        step_timeout,
        "converge sync",
        runtime.trigger_sync(),
    )
    .await?;
    run_step(
        runtime,
        budget,
        step_timeout,
        "converge ceremony inbox",
        runtime.process_ceremony_messages(),
    )
    .await?;
    cooperative_yield().await;
    Ok(())
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
    let harness = harness_mode_enabled();
    let rounds = if harness {
        harness_convergence_rounds()
    } else {
        1
    };
    let rounds = u64::try_from(rounds).map_err(|_| TimeoutBudgetError::InvalidPolicy {
        detail: "maintenance convergence round count exceeds u64".into(),
    })?;
    let step_ms = if harness {
        harness_convergence_step_timeout_ms()
    } else {
        DEFAULT_HARNESS_CONVERGENCE_STEP_TIMEOUT_MS
    };
    let steps = 2;
    let backoff_ms = if harness {
        harness_convergence_backoff_ms()
    } else {
        0
    };
    let timeout_ms = rounds
        .checked_mul(steps)
        .and_then(|steps| steps.checked_mul(step_ms))
        .and_then(|steps| {
            rounds
                .saturating_sub(1)
                .checked_mul(backoff_ms)
                .and_then(|backoff| steps.checked_add(backoff))
        })
        .ok_or_else(|| TimeoutBudgetError::InvalidPolicy {
            detail: "maintenance convergence policy overflows u64 milliseconds".into(),
        })?;
    let budget = workflow_timeout_budget(runtime, Duration::from_millis(timeout_ms)).await?;
    execute_with_runtime_timeout_budget(runtime, &budget, || async {
        for round in 0..rounds {
            converge_runtime_cycle(runtime, &budget, Duration::from_millis(step_ms)).await?;
            if round + 1 < rounds && backoff_ms > 0 {
                execute_with_runtime_timeout_budget(runtime, &budget, || {
                    runtime.sleep_ms(backoff_ms)
                })
                .await
                .map_err(|error| match error {
                    TimeoutRunError::Timeout(error) => AuraError::from(error),
                    TimeoutRunError::Operation(error) => AuraError::from(
                        super::error::runtime_call("maintenance convergence backoff", error),
                    ),
                })?;
            }
        }
        super::system::refresh_account(app_core).await
    })
    .await
    .map_err(|error| match error {
        TimeoutRunError::Timeout(error) => error.into(),
        TimeoutRunError::Operation(error) => error,
    })
}

/// Process newly-delivered browser harness transport mailbox work without
/// immediately re-running full discovery/sync convergence on the browser main
/// thread.
///
/// Browser harness transport polling already delivers remote envelopes into the
/// local runtime inbox. The immediate follow-up work is therefore limited to
/// inbox/ceremony processing plus projection refresh so observed UI state can
/// converge without creating a browser-owned discovery/sync feedback loop.
pub async fn run_harness_runtime_mailbox_pass(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
) -> Result<(), AuraError> {
    let budget = workflow_timeout_budget(runtime, Duration::from_secs(3)).await?;
    execute_with_runtime_timeout_budget(runtime, &budget, || async {
        timeout_runtime_call_with_budget(
            runtime,
            &budget,
            "web_harness_transport_tick",
            "process_ceremony_messages",
            Duration::from_secs(3),
            || runtime.process_ceremony_messages(),
        )
        .await?
        .map_err(|error| {
            AuraError::from(super::error::runtime_call("process runtime mailbox", error))
        })?;
        cooperative_yield().await;
        super::system::refresh_account(app_core).await
    })
    .await
    .map_err(|error| match error {
        TimeoutRunError::Timeout(error) => error.into(),
        TimeoutRunError::Operation(error) => error,
    })
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
    if connected_peers > 0 {
        return Ok(());
    }
    let sync_peers = runtime
        .try_get_sync_peers()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get sync peers", e)))?;
    let discovered_peers = runtime
        .try_get_discovered_peers()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get discovered peers", e)))?;
    let bootstrap_candidates = runtime
        .try_get_bootstrap_candidates()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("get bootstrap candidates", e)))?;

    Err(super::error::WorkflowError::ConnectivityRequired {
        flow: flow.to_string(),
        connected_peers,
        sync_peers: sync_peers.len(),
        discovered_peers: discovered_peers.len(),
        lan_peers: bootstrap_candidates.len(),
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::{ensure_runtime_peer_connectivity, workflow_best_effort};
    use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
    use aura_core::{types::identifiers::AuthorityId, AuraError};
    use std::future::Future;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut context = Context::from_waker(Waker::noop());
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
    async fn verified_connection_does_not_require_optional_discovery_diagnostics() {
        let bridge = OfflineRuntimeBridge::new(AuthorityId::new_from_entropy([61; 32]));
        bridge.queue_sync_status_answers(vec![Box::pin(async {
            Ok(crate::runtime_bridge::SyncStatus {
                connected_peers: 1,
                ..Default::default()
            })
        })]);
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(bridge);
        assert!(runtime.try_get_discovered_peers().await.is_err());
        ensure_runtime_peer_connectivity(&runtime, "verified_connection")
            .await
            .expect("an authoritative connected peer suffices");
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod clock_owner_regressions {
    use super::*;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use futures::FutureExt;

    struct WitnessClock;
    #[async_trait::async_trait]
    impl aura_core::effects::PhysicalTimeEffects for WitnessClock {
        async fn physical_time(&self) -> Result<PhysicalTime, aura_core::effects::TimeError> {
            Ok(PhysicalTime {
                ts_ms: 100,
                uncertainty: Some(7),
            })
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), aura_core::effects::TimeError> {
            panic!("original physical endpoint is required");
        }
        async fn wait_until_physical_deadline(
            &self,
            _: aura_core::types::window::WindowPosition<aura_core::types::window::PhysicalMillis>,
        ) -> Result<PhysicalTime, aura_core::effects::TimeError> {
            futures::future::pending().await
        }
    }

    #[tokio::test]
    async fn runtime_observation_retains_original_selected_provider_and_uncertainty() {
        let mut bridge =
            OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([82; 32]));
        bridge.use_time_provider(Arc::new(WitnessClock));
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(bridge);
        assert!(Arc::ptr_eq(
            &runtime.physical_time_provider(),
            &runtime.physical_time_provider()
        ));
        let budget = workflow_timeout_budget(&runtime, Duration::from_millis(100))
            .await
            .unwrap();
        assert_eq!(
            runtime_observation_with_budget(&runtime, &budget)
                .await
                .unwrap(),
            PhysicalTime {
                ts_ms: 100,
                uncertainty: Some(7)
            }
        );
        execute_with_runtime_timeout_budget(&runtime, &budget, || async { Ok::<_, AuraError>(()) })
            .await
            .unwrap();
    }

    struct StalledReadClock {
        clock: Arc<aura_testkit::time::ManualPhysicalClock>,
        ready_reads: std::sync::atomic::AtomicUsize,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }
    struct StalledReadLease(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for StalledReadLease {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    #[async_trait::async_trait]
    impl aura_core::effects::PhysicalTimeEffects for StalledReadClock {
        async fn physical_time(&self) -> Result<PhysicalTime, aura_core::effects::TimeError> {
            if self
                .ready_reads
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |remaining| remaining.checked_sub(1),
                )
                .is_ok()
            {
                return aura_core::effects::PhysicalTimeEffects::physical_time(self.clock.as_ref())
                    .await;
            }
            let _lease = StalledReadLease(self.dropped.clone());
            futures::future::pending().await
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), aura_core::effects::TimeError> {
            panic!("required observation retains the original absolute endpoint")
        }
        async fn wait_until_physical_deadline(
            &self,
            endpoint: aura_core::types::window::WindowPosition<
                aura_core::types::window::PhysicalMillis,
            >,
        ) -> Result<PhysicalTime, aura_core::effects::TimeError> {
            aura_core::effects::PhysicalTimeEffects::wait_until_physical_deadline(
                self.clock.as_ref(),
                endpoint,
            )
            .await
        }
    }

    #[tokio::test]
    async fn runtime_required_clock_reads_are_bounded_for_executor_child_and_retry() {
        // Each case owns one original provider and window. Success observation
        // permits exactly the executor's first read, then stalls its postcheck.
        for stage in ["initial", "success", "child", "retry"] {
            let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
            let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let provider = Arc::new(StalledReadClock {
                clock: clock.clone(),
                ready_reads: std::sync::atomic::AtomicUsize::new(usize::from(stage == "success")),
                dropped: dropped.clone(),
            });
            let mut bridge =
                OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([83; 32]));
            bridge.use_time_provider(provider);
            let runtime: Arc<dyn RuntimeBridge> = Arc::new(bridge);
            let budget = TimeoutBudget::from_start_and_timeout(
                &PhysicalTime::exact(100),
                Duration::from_millis(100),
            )
            .unwrap();
            let polls = std::cell::Cell::new(0);
            let operation = async {
                match stage {
                    "child" => {
                        workflow_child_timeout_budget(&runtime, &budget, Duration::from_millis(50))
                            .await
                            .map(|_| ())
                    }
                    "retry" => {
                        let policy = RetryBudgetPolicy::new(
                            2,
                            ExponentialBackoffPolicy::new(
                                Duration::from_millis(1),
                                Duration::from_millis(1),
                                aura_core::JitterMode::None,
                            )
                            .unwrap(),
                        );
                        match execute_with_runtime_retry_budget(
                            &runtime,
                            &budget,
                            &policy,
                            |_, _| async {
                                polls.set(polls.get() + 1);
                                Ok::<_, AuraError>(())
                            },
                        )
                        .await
                        {
                            Err(RetryRunError::Timeout(error)) => Err(error),
                            other => {
                                panic!("stalled initial observation must not run retry: {other:?}")
                            }
                        }
                    }
                    _ => match execute_with_runtime_timeout_budget(&runtime, &budget, || async {
                        polls.set(polls.get() + 1);
                        Ok::<_, AuraError>(())
                    })
                    .await
                    {
                        Err(TimeoutRunError::Timeout(error)) => Err(error),
                        other => panic!("stalled required observation must fail: {other:?}"),
                    },
                }
            };
            futures::pin_mut!(operation);
            assert!(futures::poll!(operation.as_mut()).is_pending());
            clock.advance(100);
            assert!(matches!(
                operation.await,
                Err(TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 200,
                    observed_at_ms: 200
                })
            ));
            assert_eq!(polls.get(), usize::from(stage == "success"));
            assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        }
    }

    fn pending_sync() -> (
        Arc<aura_testkit::time::ManualPhysicalClock>,
        Arc<dyn RuntimeBridge>,
        futures::channel::oneshot::Sender<()>,
    ) {
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
        let mut bridge =
            OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([82; 32]));
        bridge.use_time_provider(clock.clone());
        let (ready, receiver) = futures::channel::oneshot::channel();
        bridge.queue_sync_answers(vec![async move {
            receiver.await.expect("sync readiness sender remains owned");
            Ok(())
        }
        .boxed()]);
        bridge.set_process_ceremony_result(Ok(
            crate::runtime_bridge::CeremonyProcessingOutcome::NoProgress,
        ));
        (clock, Arc::new(bridge), ready)
    }

    #[tokio::test]
    async fn absolute_wait_retains_endpoint_when_operation_advances_provider_before_registration() {
        let (clock, runtime, _ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let operation_clock = clock.clone();
        let execution = execute_with_runtime_timeout_budget(&runtime, &budget, || async move {
            operation_clock.advance(80);
            futures::future::pending::<Result<(), AuraError>>().await
        });
        futures::pin_mut!(execution);
        assert!(futures::poll!(execution.as_mut()).is_pending());
        assert_eq!(clock.now_ms(), 180);
        clock.advance(20);
        assert!(matches!(
            futures::poll!(execution.as_mut()),
            std::task::Poll::Ready(Err(TimeoutRunError::Timeout(
                TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 200,
                    observed_at_ms: 200
                }
            )))
        ));
    }

    #[tokio::test]
    async fn absolute_wait_preserves_relative_only_provider_failure() {
        struct RelativeOnly;
        #[async_trait::async_trait]
        impl aura_core::effects::PhysicalTimeEffects for RelativeOnly {
            async fn physical_time(&self) -> Result<PhysicalTime, aura_core::effects::TimeError> {
                Ok(PhysicalTime::exact(100))
            }
            async fn sleep_ms(&self, _: u64) -> Result<(), aura_core::effects::TimeError> {
                panic!("required endpoint cannot fall back to relative sleep")
            }
        }
        let mut bridge =
            OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([84; 32]));
        bridge.use_time_provider(Arc::new(RelativeOnly));
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(bridge);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let error = execute_with_runtime_timeout_budget(&runtime, &budget, || {
            futures::future::pending::<Result<(), AuraError>>()
        })
        .await
        .unwrap_err();
        let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
        while let Some(error) = source {
            if matches!(
                error.downcast_ref::<aura_core::effects::TimeError>(),
                Some(aura_core::effects::TimeError::AbsoluteDeadlineUnsupported)
            ) {
                return;
            }
            source = error.source();
        }
        panic!("original absolute-wait unsupported cause was lost");
    }

    #[tokio::test]
    async fn retry_without_an_attempt_timeout_is_bounded_by_original_endpoint() {
        let (clock, runtime, _ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let policy = RetryBudgetPolicy::new(
            2,
            ExponentialBackoffPolicy::new(
                Duration::from_millis(10),
                Duration::from_millis(10),
                aura_core::JitterMode::None,
            )
            .unwrap(),
        );
        assert_eq!(policy.per_attempt_timeout(), None);
        let retry =
            execute_with_runtime_retry_budget(&runtime, &budget, &policy, |attempt, child| {
                assert_eq!(attempt, 0);
                assert_eq!(child.deadline_at_ms(), 200);
                std::future::pending::<Result<(), AuraError>>()
            });
        futures::pin_mut!(retry);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        clock.advance(99);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        clock.advance(1);
        assert!(matches!(
            retry.await,
            Err(RetryRunError::Timeout(
                TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 200,
                    observed_at_ms: 200
                }
            ))
        ));
    }

    #[tokio::test]
    async fn retry_backoff_and_second_attempt_share_original_remaining_window() {
        let (clock, runtime, _ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let policy = RetryBudgetPolicy::new(
            2,
            ExponentialBackoffPolicy::new(
                Duration::from_millis(50),
                Duration::from_millis(50),
                aura_core::JitterMode::None,
            )
            .unwrap(),
        );
        let attempts = std::cell::RefCell::new(Vec::new());
        let retry =
            execute_with_runtime_retry_budget(&runtime, &budget, &policy, |attempt, child| {
                attempts
                    .borrow_mut()
                    .push((attempt, child.deadline_at_ms()));
                async move {
                    if attempt == 0 {
                        Err(AuraError::agent("first attempt rejected"))
                    } else {
                        std::future::pending::<Result<(), AuraError>>().await
                    }
                }
            });
        futures::pin_mut!(retry);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        assert_eq!(*attempts.borrow(), vec![(0, 200)]);
        clock.advance(50);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        assert_eq!(*attempts.borrow(), vec![(0, 200), (1, 200)]);
        clock.advance(49);
        assert!(futures::poll!(retry.as_mut()).is_pending());
        clock.advance(1);
        assert!(matches!(
            retry.await,
            Err(RetryRunError::Timeout(
                TimeoutBudgetError::DeadlineExceeded {
                    deadline_at_ms: 200,
                    observed_at_ms: 200
                }
            ))
        ));
    }

    #[tokio::test]
    async fn required_convergence_waits_for_owned_sync_readiness_on_a_frozen_clock() {
        let (clock, runtime, ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let convergence = converge_runtime(&runtime, &budget);
        futures::pin_mut!(convergence);
        assert!(futures::poll!(convergence.as_mut()).is_pending());
        assert_eq!(clock.now_ms(), 100);
        ready.send(()).unwrap();
        convergence.await.unwrap();
        assert_eq!(clock.now_ms(), 100);
        assert_eq!(budget.deadline_at_ms(), 200);
    }

    #[tokio::test]
    async fn required_convergence_child_expires_at_the_original_endpoint() {
        let (clock, runtime, _ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let convergence = converge_runtime(&runtime, &budget);
        futures::pin_mut!(convergence);
        assert!(futures::poll!(convergence.as_mut()).is_pending());
        clock.advance(99);
        assert!(futures::poll!(convergence.as_mut()).is_pending());
        clock.advance(1);
        let error = convergence.await.unwrap_err();
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if let Some(error) = cause.downcast_ref::<TimeoutBudgetError>() {
                assert!(matches!(
                    error,
                    TimeoutBudgetError::DeadlineExceeded {
                        deadline_at_ms: 200,
                        observed_at_ms: 200
                    }
                ));
                break;
            }
            cause = cause
                .source()
                .expect("original child deadline remains typed");
        }
    }

    #[tokio::test]
    async fn required_convergence_retains_an_original_timer_failure() {
        let (clock, runtime, _ready) = pending_sync();
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let convergence = converge_runtime(&runtime, &budget);
        futures::pin_mut!(convergence);
        assert!(futures::poll!(convergence.as_mut()).is_pending());
        clock
            .fail_next_sleep(aura_core::effects::TimeError::OperationFailed {
                reason: "original convergence timer failed".into(),
            })
            .await;
        let error = convergence.await.unwrap_err();
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if let Some(aura_core::effects::TimeError::OperationFailed { reason }) =
                cause.downcast_ref::<aura_core::effects::TimeError>()
            {
                assert_eq!(reason, "original convergence timer failed");
                break;
            }
            cause = cause.source().expect("original timer cause remains typed");
        }
        assert_eq!(clock.now_ms(), 100);
    }
    fn bridge(times: &[u64]) -> (Arc<OfflineRuntimeBridge>, Arc<dyn RuntimeBridge>) {
        let mut bridge =
            OfflineRuntimeBridge::new(aura_core::AuthorityId::new_from_entropy([81; 32]));
        bridge.use_time_provider(Arc::new(aura_testkit::time::ManualPhysicalClock::new(100)));
        let bridge = Arc::new(bridge);
        bridge.queue_clock_answers(times.iter().copied().map(Ok).collect());
        let runtime: Arc<dyn RuntimeBridge> = bridge.clone();
        (bridge, runtime)
    }

    #[tokio::test]
    async fn required_convergence_preserves_the_original_exhausted_endpoint() {
        let (_bridge, runtime) = bridge(&[200]);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let error = converge_runtime(&runtime, &budget).await.unwrap_err();
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if let Some(error) = cause.downcast_ref::<TimeoutBudgetError>() {
                assert!(matches!(
                    error,
                    TimeoutBudgetError::DeadlineExceeded {
                        deadline_at_ms: 200,
                        observed_at_ms: 200,
                    }
                ));
                break;
            }
            cause = cause.source().expect("typed original endpoint error");
        }
        assert_eq!(budget.deadline_at_ms(), 200);
    }

    #[tokio::test]
    async fn required_convergence_preserves_clock_failure_without_fallback() {
        #[derive(Debug, thiserror::Error)]
        #[error("injected convergence clock failure")]
        struct ClockFault;
        let (bridge, runtime) = bridge(&[]);
        bridge.queue_clock_answers(vec![Err(
            crate::runtime_bridge::RuntimeBridgeError::with_source(
                crate::IntentError::service_error("convergence clock failed"),
                ClockFault,
            ),
        )]);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .unwrap();
        let error = converge_runtime(&runtime, &budget).await.unwrap_err();
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        loop {
            if cause.downcast_ref::<ClockFault>().is_some() {
                break;
            }
            cause = cause
                .source()
                .expect("original provider fault remains in source chain");
        }
    }
    #[tokio::test]
    async fn runtime_success_observation_rejects_rollback_after_progress() {
        let (_bridge, runtime) = bridge(&[150, 140]);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .expect("valid test budget");
        let result = execute_with_runtime_timeout_budget(&runtime, &budget, || async {
            Ok::<_, AuraError>(())
        })
        .await;
        assert!(matches!(
            result,
            Err(TimeoutRunError::Timeout(
                TimeoutBudgetError::ClockRollback {
                    previous_observed_at_ms: 150,
                    observed_at_ms: 140,
                }
            ))
        ));
    }
    #[tokio::test]
    async fn runtime_required_sleep_failure_retains_concrete_original_source() {
        use std::error::Error;
        #[derive(Debug, thiserror::Error)]
        #[error("injected required timer failure")]
        struct TimerFault;
        let (bridge, runtime) = bridge(&[150]);
        bridge.queue_deadline_answers(vec![Err(
            crate::runtime_bridge::RuntimeBridgeError::with_source(
                crate::IntentError::service_error("required timer failed"),
                TimerFault,
            ),
        )]);
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(100),
        )
        .expect("valid test budget");
        let error = execute_with_runtime_timeout_budget(&runtime, &budget, || {
            futures::future::pending::<Result<(), AuraError>>()
        })
        .await
        .expect_err("required timer failure must fail the owner");
        let mut cause: &(dyn Error + 'static) = &error;
        loop {
            if cause.downcast_ref::<TimerFault>().is_some() {
                break;
            }
            cause = cause
                .source()
                .expect("original timer fault remains in standard source chain");
        }
        assert!(matches!(
            error,
            TimeoutRunError::Timeout(TimeoutBudgetError::TimeSourceUnavailable { .. })
        ));
    }
}
