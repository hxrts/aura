//! Local timeout and backoff policy for owner-controlled deadlines.
//!
//! Aura treats wall clock as a local choice. This module uses physical time for
//! local budgeting and retry policy, while keeping semantic ordering concerns in
//! logical, order, or provenanced time domains.

use super::{PhysicalTime, TimeDomain};
use crate::{
    effects::{BackoffStrategy, JitterMode, PhysicalTimeEffects, RetryPolicy, TimeError},
    AuraError, ProtocolErrorCode,
};
use futures::{future::Either, pin_mut};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::future::Future;
use std::time::Duration;

/// Typed result for local timeout-budget policy.
pub type TimeoutBudgetResult<T> = Result<T, TimeoutBudgetError>;

/// Explicit mapping between timeout policy and Aura time semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeoutTimeSemantics {
    /// Physical time drives local timeout budgets and retry delays.
    LocalPhysicalBudget,
    /// Logical time remains for causal/semantic ordering, not wall-clock timeouts.
    LogicalSemanticOrdering,
    /// Order time remains for privacy-preserving semantic ordering.
    OrderSemanticOrdering,
    /// Provenanced time remains for attested/consensus-backed semantic claims.
    ProvenancedSemanticOrdering,
}

impl TimeoutTimeSemantics {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LocalPhysicalBudget => "local_physical_budget",
            Self::LogicalSemanticOrdering => "logical_semantic_ordering",
            Self::OrderSemanticOrdering => "order_semantic_ordering",
            Self::ProvenancedSemanticOrdering => "provenanced_semantic_ordering",
        }
    }

    pub fn local_time_domain(&self) -> Option<TimeDomain> {
        match self {
            Self::LocalPhysicalBudget => Some(TimeDomain::PhysicalClock),
            Self::LogicalSemanticOrdering => Some(TimeDomain::LogicalClock),
            Self::OrderSemanticOrdering => Some(TimeDomain::OrderClock),
            Self::ProvenancedSemanticOrdering => None,
        }
    }

    pub fn is_local_budget_domain(&self) -> bool {
        matches!(self, Self::LocalPhysicalBudget)
    }
}

/// Shared execution classes for timeout-policy scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeoutExecutionClass {
    Production,
    SimulationTest,
    Harness,
}

impl TimeoutExecutionClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::SimulationTest => "simulation_test",
            Self::Harness => "harness",
        }
    }
}

/// Shared profile for scaling timeout and backoff policy by execution lane.
///
/// The semantic model stays the same across environments; only scale and
/// deterministic jitter policy vary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutExecutionProfile {
    class: TimeoutExecutionClass,
    scale_percent: u32,
    jitter: JitterMode,
}

impl TimeoutExecutionProfile {
    pub fn new(
        class: TimeoutExecutionClass,
        scale_percent: u32,
        jitter: JitterMode,
    ) -> TimeoutBudgetResult<Self> {
        if scale_percent == 0 {
            return Err(TimeoutBudgetError::invalid_policy(
                "timeout scale_percent must be greater than zero",
            ));
        }
        Ok(Self {
            class,
            scale_percent,
            jitter,
        })
    }

    pub fn production() -> Self {
        Self {
            class: TimeoutExecutionClass::Production,
            scale_percent: 100,
            jitter: JitterMode::Deterministic,
        }
    }

    pub fn simulation_test() -> Self {
        Self {
            class: TimeoutExecutionClass::SimulationTest,
            scale_percent: 10,
            jitter: JitterMode::None,
        }
    }

    pub fn harness() -> Self {
        Self {
            class: TimeoutExecutionClass::Harness,
            scale_percent: 25,
            jitter: JitterMode::None,
        }
    }

    pub fn class(&self) -> TimeoutExecutionClass {
        self.class
    }

    pub fn scale_percent(&self) -> u32 {
        self.scale_percent
    }

    pub fn jitter(&self) -> JitterMode {
        self.jitter
    }

    pub fn scale_duration(&self, duration: Duration) -> TimeoutBudgetResult<Duration> {
        let millis = duration_to_ms(duration)?;
        let scaled = millis
            .checked_mul(u64::from(self.scale_percent))
            .ok_or_else(|| TimeoutBudgetError::invalid_policy("scaled timeout overflow"))?
            / 100;
        Ok(Duration::from_millis(scaled.max(1)))
    }

    pub fn apply_backoff(
        &self,
        backoff: &ExponentialBackoffPolicy,
    ) -> TimeoutBudgetResult<ExponentialBackoffPolicy> {
        ExponentialBackoffPolicy::new(
            self.scale_duration(backoff.initial_delay())?,
            self.scale_duration(backoff.max_delay())?,
            self.jitter,
        )
    }

    pub fn apply_retry_policy(
        &self,
        policy: &RetryBudgetPolicy,
    ) -> TimeoutBudgetResult<RetryBudgetPolicy> {
        let mut scaled =
            RetryBudgetPolicy::new(policy.max_attempts(), self.apply_backoff(policy.backoff())?);
        if let Some(timeout) = policy.per_attempt_timeout() {
            scaled = scaled.with_per_attempt_timeout(self.scale_duration(timeout)?);
        }
        Ok(scaled)
    }
}

/// Typed timeout/backoff failures for local owner policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum TimeoutBudgetError {
    #[error("invalid timeout policy: {detail}")]
    InvalidPolicy { detail: String },
    #[error("time source unavailable: {detail}")]
    TimeSourceUnavailable { detail: String },
    #[error("local timeout budget exhausted at {observed_at_ms}ms (deadline {deadline_at_ms}ms)")]
    DeadlineExceeded {
        deadline_at_ms: u64,
        observed_at_ms: u64,
    },
    #[error("retry attempt budget exhausted after {attempts_used} attempts (max {max_attempts})")]
    AttemptBudgetExhausted {
        max_attempts: u32,
        attempts_used: u32,
    },
}

impl TimeoutBudgetError {
    pub fn invalid_policy(detail: impl Into<String>) -> Self {
        Self::InvalidPolicy {
            detail: detail.into(),
        }
    }

    pub fn time_source_unavailable(detail: impl Into<String>) -> Self {
        Self::TimeSourceUnavailable {
            detail: detail.into(),
        }
    }

    pub fn deadline_exceeded(deadline_at_ms: u64, observed_at_ms: u64) -> Self {
        Self::DeadlineExceeded {
            deadline_at_ms,
            observed_at_ms,
        }
    }

    pub fn attempt_budget_exhausted(max_attempts: u32, attempts_used: u32) -> Self {
        Self::AttemptBudgetExhausted {
            max_attempts,
            attempts_used,
        }
    }
}

impl ProtocolErrorCode for TimeoutBudgetError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidPolicy { .. } => "invalid_timeout_policy",
            Self::TimeSourceUnavailable { .. } => "time_source_unavailable",
            Self::DeadlineExceeded { .. } => "deadline_exceeded",
            Self::AttemptBudgetExhausted { .. } => "attempt_budget_exhausted",
        }
    }
}

impl From<TimeoutBudgetError> for AuraError {
    fn from(value: TimeoutBudgetError) -> Self {
        match value {
            TimeoutBudgetError::InvalidPolicy { detail } => {
                AuraError::invalid(format!("invalid_timeout_policy: {detail}"))
            }
            TimeoutBudgetError::TimeSourceUnavailable { detail } => {
                AuraError::internal(format!("time_source_unavailable: {detail}"))
            }
            TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms,
                observed_at_ms,
            } => AuraError::terminal(format!(
                "deadline_exceeded: observed_at_ms={observed_at_ms} deadline_at_ms={deadline_at_ms}"
            )),
            TimeoutBudgetError::AttemptBudgetExhausted {
                max_attempts,
                attempts_used,
            } => AuraError::terminal(format!(
                "attempt_budget_exhausted: attempts_used={attempts_used} max_attempts={max_attempts}"
            )),
        }
    }
}

/// Local operation deadline budget.
///
/// This uses physical time as a local owner choice for budgeting and timeout
/// policy. It does not represent distributed semantic ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutBudget {
    started_at_ms: u64,
    deadline_at_ms: u64,
}

impl TimeoutBudget {
    pub fn from_start_and_timeout(
        started_at: &PhysicalTime,
        timeout: Duration,
    ) -> TimeoutBudgetResult<Self> {
        let timeout_ms = duration_to_ms(timeout)?;
        let deadline_at_ms = started_at
            .ts_ms
            .checked_add(timeout_ms)
            .ok_or_else(|| TimeoutBudgetError::invalid_policy("timeout deadline overflow"))?;
        Ok(Self {
            started_at_ms: started_at.ts_ms,
            deadline_at_ms,
        })
    }

    pub fn started_at_ms(&self) -> u64 {
        self.started_at_ms
    }

    pub fn deadline_at_ms(&self) -> u64 {
        self.deadline_at_ms
    }

    pub fn timeout_ms(&self) -> u64 {
        self.deadline_at_ms.saturating_sub(self.started_at_ms)
    }

    pub fn time_semantics(&self) -> TimeoutTimeSemantics {
        TimeoutTimeSemantics::LocalPhysicalBudget
    }

    pub fn remaining_at(&self, now: &PhysicalTime) -> TimeoutBudgetResult<Duration> {
        if now.ts_ms >= self.deadline_at_ms {
            return Err(TimeoutBudgetError::deadline_exceeded(
                self.deadline_at_ms,
                now.ts_ms,
            ));
        }
        Ok(Duration::from_millis(self.deadline_at_ms - now.ts_ms))
    }

    pub fn remaining_or_zero_at(&self, now: &PhysicalTime) -> Duration {
        Duration::from_millis(self.deadline_at_ms.saturating_sub(now.ts_ms))
    }

    pub fn clamp_to_remaining(
        &self,
        now: &PhysicalTime,
        requested: Duration,
    ) -> TimeoutBudgetResult<Duration> {
        let remaining = self.remaining_at(now)?;
        Ok(remaining.min(requested))
    }

    pub fn child_budget(
        &self,
        now: &PhysicalTime,
        requested: Duration,
    ) -> TimeoutBudgetResult<Self> {
        Self::from_start_and_timeout(now, self.clamp_to_remaining(now, requested)?)
    }
}

/// Mutable retry-attempt budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttemptBudget {
    max_attempts: u32,
    attempts_used: u32,
}

impl AttemptBudget {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts,
            attempts_used: 0,
        }
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn attempts_used(&self) -> u32 {
        self.attempts_used
    }

    pub fn remaining_attempts(&self) -> u32 {
        self.max_attempts.saturating_sub(self.attempts_used)
    }

    pub fn can_attempt(&self) -> bool {
        self.attempts_used < self.max_attempts
    }

    pub fn record_attempt(&mut self) -> TimeoutBudgetResult<u32> {
        if !self.can_attempt() {
            return Err(TimeoutBudgetError::attempt_budget_exhausted(
                self.max_attempts,
                self.attempts_used,
            ));
        }
        let attempt = self.attempts_used;
        self.attempts_used = self
            .attempts_used
            .checked_add(1)
            .ok_or_else(|| TimeoutBudgetError::invalid_policy("attempt counter overflow"))?;
        Ok(attempt)
    }
}

/// Bounded exponential backoff policy with explicit jitter handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExponentialBackoffPolicy {
    initial_delay: Duration,
    max_delay: Duration,
    jitter: JitterMode,
}

impl ExponentialBackoffPolicy {
    pub fn new(
        initial_delay: Duration,
        max_delay: Duration,
        jitter: JitterMode,
    ) -> TimeoutBudgetResult<Self> {
        if initial_delay.is_zero() {
            return Err(TimeoutBudgetError::invalid_policy(
                "initial_delay must be greater than zero",
            ));
        }
        if max_delay < initial_delay {
            return Err(TimeoutBudgetError::invalid_policy(
                "max_delay must be greater than or equal to initial_delay",
            ));
        }
        Ok(Self {
            initial_delay,
            max_delay,
            jitter,
        })
    }

    pub fn initial_delay(&self) -> Duration {
        self.initial_delay
    }

    pub fn max_delay(&self) -> Duration {
        self.max_delay
    }

    pub fn jitter(&self) -> JitterMode {
        self.jitter
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let strategy = match self.jitter {
            JitterMode::None => BackoffStrategy::Exponential,
            JitterMode::Deterministic => BackoffStrategy::ExponentialWithJitter,
        };
        strategy.calculate_delay(attempt, self.initial_delay, self.max_delay)
    }
}

/// Shared retry-policy vocabulary for local timeout budgeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryBudgetPolicy {
    max_attempts: u32,
    per_attempt_timeout: Option<Duration>,
    backoff: ExponentialBackoffPolicy,
}

impl RetryBudgetPolicy {
    #[must_use]
    pub fn new(max_attempts: u32, backoff: ExponentialBackoffPolicy) -> Self {
        Self {
            max_attempts,
            per_attempt_timeout: None,
            backoff,
        }
    }

    #[must_use]
    pub fn with_per_attempt_timeout(mut self, timeout: Duration) -> Self {
        self.per_attempt_timeout = Some(timeout);
        self
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn per_attempt_timeout(&self) -> Option<Duration> {
        self.per_attempt_timeout
    }

    pub fn backoff(&self) -> &ExponentialBackoffPolicy {
        &self.backoff
    }

    pub fn attempt_budget(&self) -> AttemptBudget {
        AttemptBudget::new(self.max_attempts)
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        self.backoff.delay_for_attempt(attempt)
    }

    pub fn as_retry_policy(&self) -> RetryPolicy {
        let mut policy = RetryPolicy::exponential()
            .with_max_attempts(self.max_attempts)
            .with_initial_delay(self.backoff.initial_delay())
            .with_max_delay(self.backoff.max_delay())
            .with_jitter(self.backoff.jitter());

        if let Some(timeout) = self.per_attempt_timeout {
            policy = policy.with_timeout(timeout);
        }

        policy
    }
}

/// Typed result for an operation run under a timeout budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeoutRunError<E> {
    Timeout(TimeoutBudgetError),
    Operation(E),
}

impl<E: fmt::Display> fmt::Display for TimeoutRunError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout(error) => write!(f, "{error}"),
            Self::Operation(error) => write!(f, "{error}"),
        }
    }
}

/// Typed result for an operation run under retry policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryRunError<E> {
    Timeout(TimeoutBudgetError),
    AttemptsExhausted { attempts_used: u32, last_error: E },
}

impl<E: fmt::Display> fmt::Display for RetryRunError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout(error) => write!(f, "{error}"),
            Self::AttemptsExhausted {
                attempts_used,
                last_error,
            } => write!(
                f,
                "retry attempts exhausted after {attempts_used} attempts: {last_error}"
            ),
        }
    }
}

/// Run an async operation with a typed local timeout budget.
pub async fn execute_with_timeout_budget<TTime, F, Fut, T, E>(
    time: &TTime,
    budget: &TimeoutBudget,
    operation: F,
) -> Result<T, TimeoutRunError<E>>
where
    TTime: PhysicalTimeEffects + Sync,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let now = current_physical_time(time)
        .await
        .map_err(TimeoutRunError::Timeout)?;
    let remaining = budget
        .remaining_at(&now)
        .map_err(TimeoutRunError::Timeout)?;
    let sleep_ms = duration_to_ms(remaining).map_err(TimeoutRunError::Timeout)?;

    let operation_future = operation();
    let sleep_future = time.sleep_ms(sleep_ms);
    pin_mut!(operation_future);
    pin_mut!(sleep_future);
    match futures::future::select(operation_future, sleep_future).await {
        Either::Left((result, _sleep_future)) => result.map_err(TimeoutRunError::Operation),
        Either::Right((sleep, _operation_future)) => {
            sleep.map_err(|error| TimeoutRunError::Timeout(time_error(error)))?;
            let observed_at_ms = current_physical_time(time)
                .await
                .map(|time| time.ts_ms)
                .unwrap_or(budget.deadline_at_ms());
            Err(TimeoutRunError::Timeout(
                TimeoutBudgetError::deadline_exceeded(budget.deadline_at_ms(), observed_at_ms),
            ))
        }
    }
}

/// Run an async operation with typed retry and optional per-attempt timeout policy.
pub async fn execute_with_retry_budget<TTime, F, Fut, T, E>(
    time: &TTime,
    policy: &RetryBudgetPolicy,
    mut operation: F,
) -> Result<T, RetryRunError<E>>
where
    TTime: PhysicalTimeEffects + Sync,
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut attempts = policy.attempt_budget();

    loop {
        let attempt = attempts.record_attempt().map_err(RetryRunError::Timeout)?;

        let result = if let Some(timeout) = policy.per_attempt_timeout() {
            let now = current_physical_time(time)
                .await
                .map_err(RetryRunError::Timeout)?;
            let budget = TimeoutBudget::from_start_and_timeout(&now, timeout)
                .map_err(RetryRunError::Timeout)?;
            execute_with_timeout_budget(time, &budget, || operation(attempt)).await
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
                time.sleep_ms(delay_ms)
                    .await
                    .map_err(|error| RetryRunError::Timeout(time_error(error)))?;
            }
        }
    }
}

fn duration_to_ms(duration: Duration) -> TimeoutBudgetResult<u64> {
    u64::try_from(duration.as_millis()).map_err(|_| {
        TimeoutBudgetError::invalid_policy("duration does not fit in u64 milliseconds")
    })
}

async fn current_physical_time<TTime: PhysicalTimeEffects + Sync>(
    time: &TTime,
) -> TimeoutBudgetResult<PhysicalTime> {
    time.physical_time().await.map_err(time_error)
}

fn time_error(error: TimeError) -> TimeoutBudgetError {
    TimeoutBudgetError::time_source_unavailable(error.to_string())
}

#[cfg(test)]
#[allow(clippy::disallowed_types, clippy::expect_used, clippy::redundant_clone)]
mod tests {
    use super::{
        execute_with_retry_budget, execute_with_timeout_budget, AttemptBudget,
        ExponentialBackoffPolicy, RetryBudgetPolicy, RetryRunError, TimeoutBudget,
        TimeoutBudgetError, TimeoutExecutionClass, TimeoutExecutionProfile, TimeoutRunError,
        TimeoutTimeSemantics,
    };
    use crate::{
        effects::{JitterMode, PhysicalTimeEffects, TimeError},
        time::{PhysicalTime, TimeDomain},
        ProtocolErrorCode,
    };
    use parking_lot::Mutex;
    use std::time::Duration;
    use std::{collections::VecDeque, sync::Arc};

    fn physical_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime::exact(ts_ms)
    }

    #[derive(Debug, Clone, Copy)]
    enum SleepBehavior {
        Immediate,
        YieldOnce,
    }

    #[derive(Clone)]
    struct ScriptedTimeEffects {
        times: Arc<Mutex<VecDeque<PhysicalTime>>>,
        sleeps: Arc<Mutex<Vec<u64>>>,
        sleep_behavior: SleepBehavior,
    }

    impl ScriptedTimeEffects {
        fn new(
            times: impl IntoIterator<Item = PhysicalTime>,
            sleep_behavior: SleepBehavior,
        ) -> Self {
            Self {
                times: Arc::new(Mutex::new(times.into_iter().collect())),
                sleeps: Arc::new(Mutex::new(Vec::new())),
                sleep_behavior,
            }
        }

        fn sleep_calls(&self) -> Vec<u64> {
            self.sleeps.lock().clone()
        }
    }

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for ScriptedTimeEffects {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            self.times
                .lock()
                .pop_front()
                .ok_or(TimeError::ServiceUnavailable)
        }

        async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
            self.sleeps.lock().push(ms);
            match self.sleep_behavior {
                SleepBehavior::Immediate => Ok(()),
                SleepBehavior::YieldOnce => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                    Ok(())
                }
            }
        }
    }

    #[test]
    fn timeout_budget_tracks_remaining_and_child_budget() {
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(1_000), Duration::from_secs(5))
                .expect("budget");

        assert_eq!(budget.started_at_ms(), 1_000);
        assert_eq!(budget.deadline_at_ms(), 6_000);
        assert_eq!(budget.timeout_ms(), 5_000);
        assert_eq!(
            budget.time_semantics(),
            TimeoutTimeSemantics::LocalPhysicalBudget
        );
        assert_eq!(
            budget
                .remaining_at(&physical_time(2_500))
                .expect("remaining"),
            Duration::from_millis(3_500)
        );
        assert_eq!(
            budget
                .clamp_to_remaining(&physical_time(2_500), Duration::from_secs(10))
                .expect("clamped"),
            Duration::from_millis(3_500)
        );

        let child = budget
            .child_budget(&physical_time(2_500), Duration::from_secs(2))
            .expect("child");
        assert_eq!(child.started_at_ms(), 2_500);
        assert_eq!(child.deadline_at_ms(), 4_500);
    }

    #[test]
    fn timeout_budget_expires_with_typed_failure() {
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(1_000), Duration::from_secs(5))
                .expect("budget");

        let error = budget
            .remaining_at(&physical_time(6_500))
            .expect_err("expired");
        assert_eq!(error.code(), "deadline_exceeded");
        assert!(matches!(
            error,
            TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 6_000,
                observed_at_ms: 6_500,
            }
        ));
        assert_eq!(
            budget.remaining_or_zero_at(&physical_time(6_500)),
            Duration::ZERO
        );
    }

    #[test]
    fn attempt_budget_enforces_max_attempts() {
        let mut budget = AttemptBudget::new(2);
        assert_eq!(budget.remaining_attempts(), 2);
        assert_eq!(budget.record_attempt().expect("attempt 0"), 0);
        assert_eq!(budget.record_attempt().expect("attempt 1"), 1);
        assert_eq!(budget.remaining_attempts(), 0);

        let error = budget.record_attempt().expect_err("exhausted");
        assert_eq!(error.code(), "attempt_budget_exhausted");
        assert!(matches!(
            error,
            TimeoutBudgetError::AttemptBudgetExhausted {
                max_attempts: 2,
                attempts_used: 2,
            }
        ));
    }

    #[test]
    fn exponential_backoff_is_bounded_and_round_trips_to_retry_policy() {
        let backoff = ExponentialBackoffPolicy::new(
            Duration::from_millis(100),
            Duration::from_secs(1),
            JitterMode::Deterministic,
        )
        .expect("backoff");
        let policy = RetryBudgetPolicy::new(4, backoff.clone())
            .with_per_attempt_timeout(Duration::from_millis(750));

        assert_eq!(backoff.delay_for_attempt(0), Duration::from_millis(100));
        assert!(backoff.delay_for_attempt(4) <= Duration::from_secs(1));

        let retry_policy = policy.as_retry_policy();
        assert_eq!(retry_policy.max_attempts, 4);
        assert_eq!(retry_policy.timeout, Some(Duration::from_millis(750)));
        assert_eq!(
            retry_policy.calculate_delay(3),
            backoff.delay_for_attempt(3)
        );
    }

    #[test]
    fn timeout_time_semantics_preserve_domain_split() {
        assert_eq!(
            TimeoutTimeSemantics::LocalPhysicalBudget.local_time_domain(),
            Some(TimeDomain::PhysicalClock)
        );
        assert_eq!(
            TimeoutTimeSemantics::LogicalSemanticOrdering.local_time_domain(),
            Some(TimeDomain::LogicalClock)
        );
        assert_eq!(
            TimeoutTimeSemantics::OrderSemanticOrdering.local_time_domain(),
            Some(TimeDomain::OrderClock)
        );
        assert_eq!(
            TimeoutTimeSemantics::ProvenancedSemanticOrdering.local_time_domain(),
            None
        );
        assert!(TimeoutTimeSemantics::LocalPhysicalBudget.is_local_budget_domain());
        assert!(!TimeoutTimeSemantics::LogicalSemanticOrdering.is_local_budget_domain());
    }

    #[test]
    fn timeout_execution_profiles_scale_policy_by_environment() {
        let production = TimeoutExecutionProfile::production();
        let simulation = TimeoutExecutionProfile::simulation_test();
        let harness = TimeoutExecutionProfile::harness();
        let base_backoff = ExponentialBackoffPolicy::new(
            Duration::from_secs(2),
            Duration::from_secs(10),
            JitterMode::Deterministic,
        )
        .expect("backoff");
        let base_retry = RetryBudgetPolicy::new(5, base_backoff.clone())
            .with_per_attempt_timeout(Duration::from_secs(8));

        assert_eq!(production.class(), TimeoutExecutionClass::Production);
        assert_eq!(
            production
                .scale_duration(Duration::from_secs(4))
                .expect("scaled"),
            Duration::from_secs(4)
        );
        assert_eq!(simulation.jitter(), JitterMode::None);
        assert_eq!(harness.scale_percent(), 25);

        let scaled = harness
            .apply_retry_policy(&base_retry)
            .expect("scaled policy");
        assert_eq!(scaled.max_attempts(), 5);
        assert_eq!(scaled.per_attempt_timeout(), Some(Duration::from_secs(2)));
        assert_eq!(scaled.backoff().initial_delay(), Duration::from_millis(500));
        assert_eq!(scaled.backoff().max_delay(), Duration::from_millis(2_500));
        assert_eq!(scaled.backoff().jitter(), JitterMode::None);
    }

    #[test]
    fn execution_profile_scaling_preserves_local_success_and_failure_relations() {
        let profiles = [
            TimeoutExecutionProfile::production(),
            TimeoutExecutionProfile::simulation_test(),
            TimeoutExecutionProfile::harness(),
        ];

        let base_timeout = Duration::from_secs(4);
        let base_success_latency = Duration::from_millis(1_500);
        let base_failure_latency = Duration::from_secs(6);

        assert!(base_success_latency <= base_timeout);
        assert!(base_failure_latency > base_timeout);

        for profile in profiles {
            let scaled_timeout = profile
                .scale_duration(base_timeout)
                .expect("scaled timeout");
            let scaled_success = profile
                .scale_duration(base_success_latency)
                .expect("scaled success latency");
            let scaled_failure = profile
                .scale_duration(base_failure_latency)
                .expect("scaled failure latency");

            assert!(
                scaled_success <= scaled_timeout,
                "profile {:?} changed a local success relation into failure",
                profile.class()
            );
            assert!(
                scaled_failure > scaled_timeout,
                "profile {:?} changed a local failure relation into success",
                profile.class()
            );
        }
    }

    #[tokio::test]
    async fn timeout_wrapper_returns_typed_deadline_error() {
        let effects = ScriptedTimeEffects::new(
            [physical_time(1_000), physical_time(6_200)],
            SleepBehavior::Immediate,
        );
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(1_000), Duration::from_secs(5))
                .expect("budget");

        let error = execute_with_timeout_budget(&effects, &budget, || async {
            futures::future::pending::<Result<(), &'static str>>().await
        })
        .await
        .expect_err("timed out");

        assert!(matches!(
            error,
            TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 6_000,
                observed_at_ms: 6_200,
            })
        ));
        assert_eq!(effects.sleep_calls(), vec![5_000]);
    }

    #[tokio::test]
    async fn timeout_wrapper_preserves_remaining_child_budget() {
        let parent =
            TimeoutBudget::from_start_and_timeout(&physical_time(1_000), Duration::from_secs(5))
                .expect("parent");
        let child = parent
            .child_budget(&physical_time(2_500), Duration::from_secs(10))
            .expect("child");
        let effects = ScriptedTimeEffects::new(
            [physical_time(2_500), physical_time(6_100)],
            SleepBehavior::Immediate,
        );

        let error = execute_with_timeout_budget(&effects, &child, || async {
            futures::future::pending::<Result<(), &'static str>>().await
        })
        .await
        .expect_err("timed out");

        assert!(matches!(
            error,
            TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded {
                deadline_at_ms: 6_000,
                observed_at_ms: 6_100,
            })
        ));
        assert_eq!(effects.sleep_calls(), vec![3_500]);
    }

    #[tokio::test]
    async fn retry_wrapper_retries_with_typed_backoff_policy() {
        let effects = ScriptedTimeEffects::new([], SleepBehavior::YieldOnce);
        let policy = RetryBudgetPolicy::new(
            3,
            ExponentialBackoffPolicy::new(
                Duration::from_millis(100),
                Duration::from_secs(1),
                JitterMode::None,
            )
            .expect("backoff"),
        );
        let attempts = Arc::new(Mutex::new(Vec::new()));

        let result = execute_with_retry_budget(&effects, &policy, {
            let attempts = Arc::clone(&attempts);
            move |attempt| {
                let attempts = Arc::clone(&attempts);
                async move {
                    attempts.lock().push(attempt);
                    if attempt < 2 {
                        Err("retryable failure")
                    } else {
                        Ok("done")
                    }
                }
            }
        })
        .await
        .expect("eventual success");

        assert_eq!(result, "done");
        assert_eq!(*attempts.lock(), vec![0, 1, 2]);
        assert_eq!(effects.sleep_calls(), vec![100, 200]);
    }

    #[tokio::test]
    async fn retry_wrapper_surfaces_typed_attempt_exhaustion() {
        let effects = ScriptedTimeEffects::new([], SleepBehavior::YieldOnce);
        let policy = RetryBudgetPolicy::new(
            2,
            ExponentialBackoffPolicy::new(
                Duration::from_millis(50),
                Duration::from_millis(200),
                JitterMode::None,
            )
            .expect("backoff"),
        );

        let error = execute_with_retry_budget(&effects, &policy, |_attempt| async {
            Err::<(), _>("still failing")
        })
        .await
        .expect_err("exhausted");

        assert!(matches!(
            error,
            RetryRunError::AttemptsExhausted {
                attempts_used: 2,
                last_error: "still failing",
            }
        ));
        assert_eq!(effects.sleep_calls(), vec![50]);
    }
}
