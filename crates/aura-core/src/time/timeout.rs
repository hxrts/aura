//! Local timeout and backoff policy for owner-controlled deadlines.
//!
//! Aura treats wall clock as a local choice. This module uses physical time for
//! local budgeting and retry policy, while keeping semantic ordering concerns in
//! logical, order, or provenanced time domains.

use super::{PhysicalTime, TimeDomain};
use crate::types::window::{PhysicalMillis, WindowInterval, WindowPosition};
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
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
pub enum TimeoutBudgetError {
    #[error("required physical clock rolled back from {previous_observed_at_ms}ms to {observed_at_ms}ms")]
    ClockRollback {
        previous_observed_at_ms: u64,
        observed_at_ms: u64,
    },
    #[error("timeout owner observation is concurrently borrowed")]
    ObservationUnavailable,
    #[error("timeout checkpoint ownership discontinuity: {detail}")]
    CheckpointDiscontinuity { detail: String },
    #[error("required timeout checkpoint failed: {detail}")]
    CheckpointFailure {
        detail: String,
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraError>,
    },
    #[error("invalid timeout policy: {detail}")]
    InvalidPolicy { detail: String },
    #[error("time source unavailable: {detail}")]
    TimeSourceUnavailable {
        detail: String,
        #[source]
        #[serde(skip_serializing, skip_deserializing, default)]
        source: Option<AuraError>,
    },
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
            source: None,
        }
    }

    /// Preserve the actual required-clock failure through native source traversal.
    pub fn time_source_failure(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        let detail = error.to_string();
        Self::TimeSourceUnavailable {
            detail: detail.clone(),
            source: Some(AuraError::Internal {
                message: detail,
                source: Some(std::sync::Arc::new(error)),
            }),
        }
    }

    /// Preserve required storage/codec failures without labeling them clock failures.
    pub fn checkpoint_failure(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        let detail = error.to_string();
        Self::CheckpointFailure {
            detail: detail.clone(),
            source: Some(AuraError::Internal {
                message: detail,
                source: Some(std::sync::Arc::new(error)),
            }),
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
            Self::ClockRollback { .. } => "clock_rollback",
            Self::ObservationUnavailable => "timeout_observation_unavailable",
            Self::CheckpointDiscontinuity { .. } => "timeout_checkpoint_discontinuity",
            Self::CheckpointFailure { .. } => "timeout_checkpoint_failure",
            Self::InvalidPolicy { .. } => "invalid_timeout_policy",
            Self::TimeSourceUnavailable { .. } => "time_source_unavailable",
            Self::DeadlineExceeded { .. } => "deadline_exceeded",
            Self::AttemptBudgetExhausted { .. } => "attempt_budget_exhausted",
        }
    }
}

impl From<TimeoutBudgetError> for AuraError {
    fn from(value: TimeoutBudgetError) -> Self {
        let (message, invalid) = match &value {
            TimeoutBudgetError::ClockRollback { .. } | TimeoutBudgetError::ObservationUnavailable | TimeoutBudgetError::CheckpointDiscontinuity { .. } | TimeoutBudgetError::CheckpointFailure { .. } => (value.to_string(), false),
            TimeoutBudgetError::InvalidPolicy { detail } =>
                (format!("invalid_timeout_policy: {detail}"), true),
            TimeoutBudgetError::TimeSourceUnavailable { detail, .. } =>
                (format!("time_source_unavailable: {detail}"), false),
            TimeoutBudgetError::DeadlineExceeded { deadline_at_ms, observed_at_ms } =>
                (format!("deadline_exceeded: observed_at_ms={observed_at_ms} deadline_at_ms={deadline_at_ms}"), false),
            TimeoutBudgetError::AttemptBudgetExhausted { max_attempts, attempts_used } =>
                (format!("attempt_budget_exhausted: attempts_used={attempts_used} max_attempts={max_attempts}"), false),
        };
        let source =
            Some(std::sync::Arc::new(value) as std::sync::Arc<dyn std::error::Error + Send + Sync>);
        if invalid {
            AuraError::Invalid { message, source }
        } else {
            AuraError::Internal { message, source }
        }
    }
}

/// Local operation deadline budget.
///
/// This uses physical time as a local owner choice for budgeting and timeout
/// policy. It does not represent distributed semantic ordering.
/// Owner observation of required physical time. Clones and child deadlines
/// share a high-water mark; a detected rollback is a latched failure.
///
/// Pure snapshot guards are released before await. Async observations additionally
/// hold a nonblocking owner lease across the required read, update, and checkpoint.
#[derive(Debug, Clone)]
pub struct TimeoutClockObservation {
    state: std::sync::Arc<futures::lock::Mutex<TimeoutClockSnapshot>>,
    observation_gate: std::sync::Arc<futures::lock::Mutex<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeoutClockSnapshot {
    max_observed_at_ms: u64,
    rollback: TimeoutClockRollbackSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TimeoutClockRollbackSnapshot {
    Absent,
    Detected {
        previous_observed_at_ms: u64,
        observed_at_ms: u64,
    },
}
#[derive(Serialize, Deserialize)]
enum TimeoutExpirationSnapshot {
    Active,
    Expired { observed_at_ms: u64 },
}

/// An asynchronous observation lease orders required clock reads with their
/// high-water updates and durable acknowledgments. It is not a time sample or
/// an admission capability. Dropping it releases only the observation owner.
pub struct TimeoutObservationLease {
    _guard: futures::lock::OwnedMutexGuard<()>,
}

impl TimeoutClockObservation {
    async fn acquire_observation(&self) -> TimeoutObservationLease {
        TimeoutObservationLease {
            _guard: self.observation_gate.clone().lock_owned().await,
        }
    }

    pub fn new(started_at: &PhysicalTime) -> Self {
        Self {
            observation_gate: std::sync::Arc::new(futures::lock::Mutex::new(())),
            state: std::sync::Arc::new(futures::lock::Mutex::new(TimeoutClockSnapshot {
                max_observed_at_ms: started_at.ts_ms,
                rollback: TimeoutClockRollbackSnapshot::Absent,
            })),
        }
    }

    /// Validate one required observation without increasing a prior allowance.
    pub fn observe(&self, now: &PhysicalTime) -> TimeoutBudgetResult<()> {
        let mut state = self
            .state
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        Self::observe_state(&mut state, now)
    }

    fn observe_state(
        state: &mut TimeoutClockSnapshot,
        now: &PhysicalTime,
    ) -> TimeoutBudgetResult<()> {
        if let TimeoutClockRollbackSnapshot::Detected {
            previous_observed_at_ms,
            observed_at_ms,
        } = state.rollback
        {
            return Err(TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms,
                observed_at_ms,
            });
        }
        if now.ts_ms < state.max_observed_at_ms {
            let previous_observed_at_ms = state.max_observed_at_ms;
            state.rollback = TimeoutClockRollbackSnapshot::Detected {
                previous_observed_at_ms,
                observed_at_ms: now.ts_ms,
            };
            return Err(TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms,
                observed_at_ms: now.ts_ms,
            });
        }
        state.max_observed_at_ms = now.ts_ms;
        Ok(())
    }

    fn snapshot(&self) -> TimeoutBudgetResult<TimeoutClockSnapshot> {
        self.state
            .try_lock()
            .map(|state| state.clone())
            .ok_or(TimeoutBudgetError::ObservationUnavailable)
    }

    fn restore(snapshot: TimeoutClockSnapshot) -> TimeoutBudgetResult<Self> {
        if let TimeoutClockRollbackSnapshot::Detected {
            previous_observed_at_ms: previous,
            observed_at_ms: observed,
        } = snapshot.rollback
        {
            if previous != snapshot.max_observed_at_ms || observed >= previous {
                return Err(TimeoutBudgetError::invalid_policy(
                    "invalid persisted rollback observation",
                ));
            }
        }
        Ok(Self {
            state: std::sync::Arc::new(futures::lock::Mutex::new(snapshot)),
            observation_gate: std::sync::Arc::new(futures::lock::Mutex::new(())),
        })
    }
}

impl Serialize for TimeoutClockObservation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.snapshot()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TimeoutClockObservation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::restore(TimeoutClockSnapshot::deserialize(deserializer)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Fixed local deadline with shared owner observation and expiration state.
///
/// ```compile_fail
/// fn require_copy<T: Copy>() {}
/// require_copy::<aura_core::TimeoutBudget>();
/// ```
///
/// Clone preserves observation and exhaustion; it cannot reset an allowance.
#[derive(Debug, Clone)]
pub struct TimeoutBudget {
    interval: WindowInterval<PhysicalMillis>,
    clock: TimeoutClockObservation,
    expired_at_ms: std::sync::Arc<futures::lock::Mutex<Option<u64>>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TimeoutBudgetSnapshot {
    started_at_ms: u64,
    deadline_at_ms: u64,
    clock: TimeoutClockSnapshot,
    expiration: TimeoutExpirationSnapshot,
}

impl Serialize for TimeoutBudget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let snapshot = {
            let clock = self.clock.state.try_lock().ok_or_else(|| {
                serde::ser::Error::custom(TimeoutBudgetError::ObservationUnavailable)
            })?;
            let expired = self.expired_at_ms.try_lock().ok_or_else(|| {
                serde::ser::Error::custom(TimeoutBudgetError::ObservationUnavailable)
            })?;
            TimeoutBudgetSnapshot {
                started_at_ms: self.started_at_ms(),
                deadline_at_ms: self.deadline_at_ms(),
                clock: clock.clone(),
                expiration: match *expired {
                    Some(observed_at_ms) => TimeoutExpirationSnapshot::Expired { observed_at_ms },
                    None => TimeoutExpirationSnapshot::Active,
                },
            }
        };
        snapshot.serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TimeoutBudget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let snapshot = TimeoutBudgetSnapshot::deserialize(deserializer)?;
        let expired_at_ms = match snapshot.expiration {
            TimeoutExpirationSnapshot::Active => None,
            TimeoutExpirationSnapshot::Expired { observed_at_ms } => Some(observed_at_ms),
        };
        let interval = WindowInterval::<PhysicalMillis>::from_bounds(
            WindowPosition::new(snapshot.started_at_ms),
            WindowPosition::new(snapshot.deadline_at_ms),
        )
        .map_err(serde::de::Error::custom)?;
        if interval.is_empty()
            || snapshot.clock.max_observed_at_ms < snapshot.started_at_ms
            || expired_at_ms.is_some_and(|expired| {
                expired < snapshot.started_at_ms || expired > snapshot.clock.max_observed_at_ms
            })
        {
            return Err(serde::de::Error::custom(
                "invalid persisted timeout budget observation",
            ));
        }
        Ok(Self {
            interval,
            clock: TimeoutClockObservation::restore(snapshot.clock)
                .map_err(serde::de::Error::custom)?,
            expired_at_ms: std::sync::Arc::new(futures::lock::Mutex::new(expired_at_ms)),
        })
    }
}
impl TimeoutBudget {
    pub fn from_start_and_timeout(
        started_at: &PhysicalTime,
        timeout: Duration,
    ) -> TimeoutBudgetResult<Self> {
        Self::from_start_and_timeout_with_observation(
            started_at,
            timeout,
            TimeoutClockObservation::new(started_at),
        )
    }
    pub fn from_start_and_timeout_with_observation(
        started_at: &PhysicalTime,
        timeout: Duration,
        clock: TimeoutClockObservation,
    ) -> TimeoutBudgetResult<Self> {
        let extent = duration_to_ms(timeout)?;
        if extent == 0 {
            return Err(TimeoutBudgetError::invalid_policy(
                "physical timeout must be at least one millisecond",
            ));
        }
        let interval =
            WindowInterval::<PhysicalMillis>::new(WindowPosition::new(started_at.ts_ms), extent)
                .map_err(|_| TimeoutBudgetError::invalid_policy("timeout deadline overflow"))?;
        clock.observe(started_at)?;
        Ok(Self {
            interval,
            clock,
            expired_at_ms: std::sync::Arc::new(futures::lock::Mutex::new(None)),
        })
    }
    /// Compare opaque observation/exhaustion ownership without exposing either lock.
    pub fn shares_observation_owner_with(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.clock.state, &other.clock.state)
            && std::sync::Arc::ptr_eq(&self.expired_at_ms, &other.expired_at_ms)
    }
    fn checkpoint_history(
        &self,
    ) -> TimeoutBudgetResult<(u64, TimeoutClockRollbackSnapshot, Option<u64>)> {
        let clock = self
            .clock
            .state
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        let expiration = self
            .expired_at_ms
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        Ok((clock.max_observed_at_ms, clock.rollback, *expiration))
    }
    /// Require continuation of an exact durable window without resetting its history.
    /// This validates arithmetic/observation continuity, not storage provenance.
    pub fn validate_checkpoint_continuation_from(&self, durable: &Self) -> TimeoutBudgetResult<()> {
        let discontinuity = |detail: &str| TimeoutBudgetError::CheckpointDiscontinuity {
            detail: detail.into(),
        };
        if self.interval != durable.interval {
            return Err(discontinuity("original interval changed"));
        }
        let previous = durable.checkpoint_history()?;
        let current = self.checkpoint_history()?;
        if current.0 < previous.0 {
            return Err(discontinuity(
                "live observation precedes retained highwater",
            ));
        }
        if matches!(previous.1, TimeoutClockRollbackSnapshot::Detected { .. })
            && current.1 != previous.1
        {
            return Err(discontinuity("retained rollback evidence was replaced"));
        }
        if previous.2.is_some() && current.2 != previous.2 {
            return Err(discontinuity("retained exhaustion evidence was replaced"));
        }
        Ok(())
    }

    /// Serialize asynchronous physical read/update/checkpoint sequences through
    /// this original owner. Clones and children share the same lease gate.
    pub async fn acquire_observation(&self) -> TimeoutObservationLease {
        self.clock.acquire_observation().await
    }

    pub fn started_at_ms(&self) -> u64 {
        self.interval.start().value()
    }
    pub fn deadline_at_ms(&self) -> u64 {
        self.interval.end().value()
    }
    pub fn timeout_ms(&self) -> u64 {
        self.interval.extent()
    }
    pub fn time_semantics(&self) -> TimeoutTimeSemantics {
        TimeoutTimeSemantics::LocalPhysicalBudget
    }
    pub fn remaining_at(&self, now: &PhysicalTime) -> TimeoutBudgetResult<Duration> {
        let mut clock = self
            .clock
            .state
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        let mut expired = self
            .expired_at_ms
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        TimeoutClockObservation::observe_state(&mut clock, now)?;
        if let Some(observed_at_ms) = *expired {
            return Err(TimeoutBudgetError::deadline_exceeded(
                self.deadline_at_ms(),
                observed_at_ms,
            ));
        }
        if !self.interval.contains(WindowPosition::new(now.ts_ms)) {
            *expired = Some(now.ts_ms);
            return Err(TimeoutBudgetError::deadline_exceeded(
                self.deadline_at_ms(),
                now.ts_ms,
            ));
        }
        Ok(Duration::from_millis(self.deadline_at_ms() - now.ts_ms))
    }
    /// Successful timers latch expiration after validating required clock time.
    pub fn expire_at(&self, now: &PhysicalTime) -> TimeoutBudgetResult<TimeoutBudgetError> {
        let mut clock = self
            .clock
            .state
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        let mut expired = self
            .expired_at_ms
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        TimeoutClockObservation::observe_state(&mut clock, now)?;
        let observed_at_ms = *expired.get_or_insert(now.ts_ms);
        Ok(TimeoutBudgetError::deadline_exceeded(
            self.deadline_at_ms(),
            observed_at_ms,
        ))
    }
    /// Validate a recorded observation against an immutable restored snapshot.
    /// This pure check neither observes a new clock nor grants authorization.
    pub fn validate_recorded_observation_at(
        &self,
        recorded_at: &PhysicalTime,
    ) -> TimeoutBudgetResult<()> {
        let clock = self
            .clock
            .state
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        let expired = self
            .expired_at_ms
            .try_lock()
            .ok_or(TimeoutBudgetError::ObservationUnavailable)?;
        if let TimeoutClockRollbackSnapshot::Detected {
            previous_observed_at_ms,
            observed_at_ms,
        } = clock.rollback
        {
            return Err(TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms,
                observed_at_ms,
            });
        }
        if let Some(observed_at_ms) = *expired {
            return Err(TimeoutBudgetError::deadline_exceeded(
                self.deadline_at_ms(),
                observed_at_ms,
            ));
        }
        if recorded_at.ts_ms < self.started_at_ms() || recorded_at.ts_ms > clock.max_observed_at_ms
        {
            return Err(TimeoutBudgetError::invalid_policy(
                "recorded observation is outside acknowledged clock history",
            ));
        }
        if recorded_at.ts_ms >= self.deadline_at_ms() {
            return Err(TimeoutBudgetError::deadline_exceeded(
                self.deadline_at_ms(),
                recorded_at.ts_ms,
            ));
        }
        Ok(())
    }

    pub fn clamp_to_remaining(
        &self,
        now: &PhysicalTime,
        requested: Duration,
    ) -> TimeoutBudgetResult<Duration> {
        Ok(self.remaining_at(now)?.min(requested))
    }
    pub fn child_budget(
        &self,
        now: &PhysicalTime,
        requested: Duration,
    ) -> TimeoutBudgetResult<Self> {
        Self::from_start_and_timeout_with_observation(
            now,
            self.clamp_to_remaining(now, requested)?,
            self.clock.clone(),
        )
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
#[derive(Debug, Clone)]
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

impl<E: std::error::Error + 'static> std::error::Error for TimeoutRunError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout(error) => Some(error),
            Self::Operation(error) => Some(error),
        }
    }
}

/// Typed result for an operation run under retry policy.
#[derive(Debug, Clone)]
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

impl<E: std::error::Error + 'static> std::error::Error for RetryRunError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Timeout(error) => Some(error),
            Self::AttemptsExhausted { last_error, .. } => Some(last_error),
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
    execute_with_timeout_budget_and_checkpoint(time, budget, || async { Ok(()) }, operation).await
}

/// Execute under a deadline whose owner acknowledges every observation before
/// continuing. The checkpoint also runs for latched rollback and expiration.
/// Runtime enrolled owners seal this hook behind their persistence capability.
///
/// ```compile_fail
/// use aura_core::{AuraError, TimeoutBudget};
/// use aura_core::effects::PhysicalTimeEffects;
/// async fn missing_ack<T: PhysicalTimeEffects + Sync>(time: &T, budget: &TimeoutBudget) {
///     let _ = aura_core::time::timeout::execute_with_timeout_budget_and_checkpoint(
///         time, budget, || async {}, || async { Ok::<_, AuraError>(()) },
///     ).await;
/// }
/// ```
pub async fn execute_with_timeout_budget_and_checkpoint<TTime, C, CFut, F, Fut, T, E>(
    time: &TTime,
    budget: &TimeoutBudget,
    mut checkpoint: C,
    operation: F,
) -> Result<T, TimeoutRunError<E>>
where
    TTime: PhysicalTimeEffects + Sync,
    C: FnMut() -> CFut,
    CFut: Future<Output = TimeoutBudgetResult<()>>,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let remaining = {
        let _observation = budget.acquire_observation().await;
        let now = current_physical_time(time)
            .await
            .map_err(TimeoutRunError::Timeout)?;
        let remaining = budget.remaining_at(&now);
        checkpoint().await.map_err(TimeoutRunError::Timeout)?;
        remaining.map_err(TimeoutRunError::Timeout)?
    };
    let sleep_ms = duration_to_ms(remaining).map_err(TimeoutRunError::Timeout)?;

    let operation_future = operation();
    let sleep_future = time.sleep_ms(sleep_ms);
    pin_mut!(operation_future);
    pin_mut!(sleep_future);
    match futures::future::select(operation_future, sleep_future).await {
        Either::Left((result, _sleep_future)) => {
            let _observation = budget.acquire_observation().await;
            let observed = current_physical_time(time)
                .await
                .map_err(TimeoutRunError::Timeout)?;
            let observation = budget.remaining_at(&observed);
            checkpoint().await.map_err(TimeoutRunError::Timeout)?;
            observation.map_err(TimeoutRunError::Timeout)?;
            result.map_err(TimeoutRunError::Operation)
        }
        Either::Right((sleep, _operation_future)) => {
            sleep.map_err(|error| TimeoutRunError::Timeout(time_error(error)))?;
            let _observation = budget.acquire_observation().await;
            let observed = current_physical_time(time)
                .await
                .map_err(TimeoutRunError::Timeout)?;
            let expiration = budget.expire_at(&observed);
            checkpoint().await.map_err(TimeoutRunError::Timeout)?;
            Err(TimeoutRunError::Timeout(
                expiration.map_err(TimeoutRunError::Timeout)?,
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
    let initial = current_physical_time(time)
        .await
        .map_err(RetryRunError::Timeout)?;
    let clock = TimeoutClockObservation::new(&initial);
    let mut attempts = policy.attempt_budget();

    loop {
        let observed = {
            let _observation = clock.acquire_observation().await;
            let observed = current_physical_time(time)
                .await
                .map_err(RetryRunError::Timeout)?;
            clock.observe(&observed).map_err(RetryRunError::Timeout)?;
            observed
        };
        let attempt = attempts.record_attempt().map_err(RetryRunError::Timeout)?;

        let result = if let Some(timeout) = policy.per_attempt_timeout() {
            let budget = TimeoutBudget::from_start_and_timeout_with_observation(
                &observed,
                timeout,
                clock.clone(),
            )
            .map_err(RetryRunError::Timeout)?;
            execute_with_timeout_budget(time, &budget, || operation(attempt)).await
        } else {
            let result = operation(attempt).await;
            let _observation = clock.acquire_observation().await;
            let observed = current_physical_time(time)
                .await
                .map_err(RetryRunError::Timeout)?;
            clock.observe(&observed).map_err(RetryRunError::Timeout)?;
            result.map_err(TimeoutRunError::Operation)
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
                let _observation = clock.acquire_observation().await;
                let observed = current_physical_time(time)
                    .await
                    .map_err(RetryRunError::Timeout)?;
                clock.observe(&observed).map_err(RetryRunError::Timeout)?;
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
    TimeoutBudgetError::time_source_failure(error)
}

#[cfg(test)]
#[allow(clippy::disallowed_types, clippy::expect_used, clippy::redundant_clone)]
mod tests {
    use super::{
        execute_with_retry_budget, execute_with_timeout_budget,
        execute_with_timeout_budget_and_checkpoint, AttemptBudget, ExponentialBackoffPolicy,
        RetryBudgetPolicy, RetryRunError, TimeoutBudget, TimeoutBudgetError, TimeoutExecutionClass,
        TimeoutExecutionProfile, TimeoutRunError, TimeoutTimeSemantics,
    };
    use crate::{
        effects::{JitterMode, PhysicalTimeEffects, TimeError},
        time::{PhysicalTime, TimeDomain},
        AuraError, ProtocolErrorCode,
    };
    use parking_lot::Mutex;
    use std::time::Duration;
    use std::{collections::VecDeque, sync::Arc};

    pub(super) fn physical_time(ts_ms: u64) -> PhysicalTime {
        PhysicalTime::exact(ts_ms)
    }

    #[derive(Debug, Clone, Copy)]
    pub(super) enum SleepBehavior {
        Immediate,
        YieldOnce,
    }

    #[derive(Clone)]
    pub(super) struct ScriptedTimeEffects {
        times: Arc<Mutex<VecDeque<PhysicalTime>>>,
        sleeps: Arc<Mutex<Vec<u64>>>,
        sleep_behavior: SleepBehavior,
    }

    impl ScriptedTimeEffects {
        pub(super) fn new(
            times: impl IntoIterator<Item = PhysicalTime>,
            sleep_behavior: SleepBehavior,
        ) -> Self {
            Self {
                times: Arc::new(Mutex::new(times.into_iter().collect())),
                sleeps: Arc::new(Mutex::new(Vec::new())),
                sleep_behavior,
            }
        }

        pub(super) fn sleep_calls(&self) -> Vec<u64> {
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

    struct InterleavedObservationTime {
        reads: std::sync::atomic::AtomicUsize,
        release_first: futures::lock::Mutex<Option<futures::channel::oneshot::Receiver<()>>>,
    }
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for InterleavedObservationTime {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            let read = self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if read == 0 {
                let receiver = self
                    .release_first
                    .lock()
                    .await
                    .take()
                    .expect("first query owns the deterministic release");
                receiver
                    .await
                    .expect("test releases captured first clock read");
                Ok(physical_time(150))
            } else {
                Ok(physical_time(200))
            }
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), TimeError> {
            futures::future::pending().await
        }
    }

    #[tokio::test]
    async fn async_observation_lease_orders_cloned_child_queries_and_checkpoint_ack() {
        use std::sync::atomic::Ordering;
        let (release, wait) = futures::channel::oneshot::channel();
        let time = InterleavedObservationTime {
            reads: std::sync::atomic::AtomicUsize::new(0),
            release_first: futures::lock::Mutex::new(Some(wait)),
        };
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(400))
                .expect("original physical window");
        let child = budget
            .child_budget(&physical_time(100), Duration::from_millis(300))
            .expect("same original observation owner");
        let checkpoints = std::sync::atomic::AtomicUsize::new(0);
        let checkpoint = || async {
            checkpoints.fetch_add(1, Ordering::SeqCst);
            Ok(())
        };
        let mut first = Box::pin(execute_with_timeout_budget_and_checkpoint(
            &time,
            &budget,
            checkpoint,
            || async { Ok::<_, AuraError>(1) },
        ));
        let mut second = Box::pin(execute_with_timeout_budget_and_checkpoint(
            &time,
            &child,
            checkpoint,
            || async { Ok::<_, AuraError>(2) },
        ));
        assert!(futures::poll!(first.as_mut()).is_pending());
        assert!(futures::poll!(second.as_mut()).is_pending());
        assert_eq!(
            time.reads.load(Ordering::SeqCst),
            1,
            "a newer query must not bypass an earlier captured read awaiting completion"
        );
        release.send(()).expect("release original read");
        assert_eq!(
            first.await.expect("original owner observes before sibling"),
            1
        );
        assert_eq!(
            second
                .await
                .expect("child observes in owner order without false rollback"),
            2
        );
        assert_eq!(
            checkpoints.load(Ordering::SeqCst),
            4,
            "both initial and completed observations require acknowledgments"
        );
        assert_eq!(
            budget
                .remaining_at(&physical_time(200))
                .expect("no renewal"),
            Duration::from_millis(300)
        );
        assert!(
            matches!(
                budget.remaining_at(&physical_time(190)),
                Err(TimeoutBudgetError::ClockRollback { .. })
            ),
            "actual rollback remains sticky"
        );
    }

    #[tokio::test]
    async fn cancelled_required_query_releases_async_observation_lease() {
        use std::sync::atomic::Ordering;
        let (_release, wait) = futures::channel::oneshot::channel();
        let time = InterleavedObservationTime {
            reads: std::sync::atomic::AtomicUsize::new(0),
            release_first: futures::lock::Mutex::new(Some(wait)),
        };
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(400))
                .expect("original physical window");
        let mut cancelled = Box::pin(execute_with_timeout_budget_and_checkpoint(
            &time,
            &budget,
            || async { Ok(()) },
            || async { Ok::<_, AuraError>(()) },
        ));
        assert!(futures::poll!(cancelled.as_mut()).is_pending());
        drop(cancelled);
        assert_eq!(
            execute_with_timeout_budget_and_checkpoint(
                &time,
                &budget,
                || async { Ok(()) },
                || async { Ok::<_, AuraError>(7) }
            )
            .await
            .expect("cancelled query must release observation ownership"),
            7
        );
        assert_eq!(time.reads.load(Ordering::SeqCst), 3);
        assert_eq!(
            budget.deadline_at_ms(),
            500,
            "cancellation never renews the deadline"
        );
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
        assert!(matches!(
            budget.remaining_at(&physical_time(6_500)),
            Err(TimeoutBudgetError::DeadlineExceeded { .. })
        ));
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
        let effects = ScriptedTimeEffects::new(
            std::iter::repeat_n(physical_time(100), 9),
            SleepBehavior::YieldOnce,
        );
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
        let effects = ScriptedTimeEffects::new(
            std::iter::repeat_n(physical_time(100), 6),
            SleepBehavior::YieldOnce,
        );
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
    #[tokio::test]
    async fn required_clock_failure_keeps_actual_source_and_does_not_start_operation() {
        use std::error::Error;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let effects = ScriptedTimeEffects::new([], SleepBehavior::Immediate);
        let calls = AtomicUsize::new(0);
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(50))
                .unwrap();
        let error = execute_with_timeout_budget(&effects, &budget, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(())
        })
        .await
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            &error,
            TimeoutRunError::Timeout(TimeoutBudgetError::TimeSourceUnavailable { .. })
        ));
        let clock = error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<TimeError>()
            .unwrap();
        assert!(matches!(clock, TimeError::ServiceUnavailable));
    }

    #[test]
    fn clock_source_survives_clone_but_is_omitted_from_serialized_diagnostics() {
        use std::error::Error;
        let original = super::time_error(TimeError::ClockSyncFailed {
            reason: "clock drift".into(),
        });
        let cloned = original.clone();
        assert!(
            matches!(cloned.source().unwrap().source().unwrap().downcast_ref::<TimeError>(),
            Some(TimeError::ClockSyncFailed { reason }) if reason == "clock drift")
        );
        let json = serde_json::to_value(&original).unwrap();
        assert!(json["TimeSourceUnavailable"].get("source").is_none());
        let restored: TimeoutBudgetError = serde_json::from_value(json).unwrap();
        assert!(restored.source().is_none());
        assert_eq!(restored.to_string(), original.to_string());
        assert_eq!(restored.code(), original.code());
    }

    #[test]
    fn conversion_retains_every_budget_variant_and_wrapper_operation_causes() {
        use std::error::Error;
        for original in [
            TimeoutBudgetError::invalid_policy("bad"),
            super::time_error(TimeError::ServiceUnavailable),
            TimeoutBudgetError::deadline_exceeded(50, 51),
            TimeoutBudgetError::attempt_budget_exhausted(2, 2),
        ] {
            let expected_code = original.code();
            let outer = crate::AuraError::from(original.clone());
            assert_eq!(
                outer
                    .source()
                    .unwrap()
                    .downcast_ref::<TimeoutBudgetError>()
                    .unwrap()
                    .code(),
                expected_code
            );
        }
        let timeout =
            TimeoutRunError::Operation(std::io::Error::from(std::io::ErrorKind::BrokenPipe));
        assert_eq!(
            timeout
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::BrokenPipe
        );
        let retry = RetryRunError::AttemptsExhausted {
            attempts_used: 2,
            last_error: std::io::Error::from(std::io::ErrorKind::ConnectionReset),
        };
        assert_eq!(
            retry
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::ConnectionReset
        );
    }

    #[tokio::test]
    async fn failed_post_sleep_clock_read_cannot_fabricate_deadline_evidence() {
        use std::error::Error;
        let effects = ScriptedTimeEffects::new([physical_time(100)], SleepBehavior::Immediate);
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(50))
                .unwrap();
        let error = execute_with_timeout_budget(&effects, &budget, || async {
            futures::future::pending::<Result<(), std::io::Error>>().await
        })
        .await
        .unwrap_err();
        assert!(matches!(
            &error,
            TimeoutRunError::Timeout(TimeoutBudgetError::TimeSourceUnavailable { .. })
        ));
        assert!(error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<TimeError>());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::disallowed_types)]
mod rollback_owner_tests {
    use super::tests::{physical_time, ScriptedTimeEffects, SleepBehavior};
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    fn budget() -> TimeoutBudget {
        TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(100))
            .expect("fixed owner budget")
    }
    fn assert_rollback(error: TimeoutBudgetError) {
        assert!(matches!(
            error,
            TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms: 150,
                observed_at_ms: 140
            }
        ));
    }
    #[test]
    fn progressed_clock_rollback_latches_across_clones_children_and_restart() {
        let parent = budget();
        let clone = parent.clone();
        let child = parent
            .child_budget(&physical_time(150), Duration::from_millis(20))
            .expect("bounded child");
        assert_eq!(child.deadline_at_ms(), 170);
        let persisted = serde_json::to_vec(&parent).expect("persist observation");
        let restored: TimeoutBudget =
            serde_json::from_slice(&persisted).expect("restore fixed deadline and highwater");
        assert_eq!(restored.deadline_at_ms(), 200);
        assert_rollback(
            restored
                .remaining_at(&physical_time(140))
                .expect_err("restored progress cannot be reset"),
        );
        assert_rollback(
            clone
                .remaining_at(&physical_time(140))
                .expect_err("clone shares progress"),
        );
        assert_rollback(
            child
                .remaining_at(&physical_time(160))
                .expect_err("rollback remains failed after forward time"),
        );
        assert_rollback(
            parent
                .remaining_at(&physical_time(199))
                .expect_err("parent cannot recover allowance after rollback"),
        );
        let failed = serde_json::to_vec(&parent).expect("persist latched rollback");
        let failed: TimeoutBudget =
            serde_json::from_slice(&failed).expect("restore failed observation");
        assert_rollback(
            failed
                .remaining_at(&physical_time(199))
                .expect_err("failed restart cannot resume"),
        );
    }
    #[test]
    fn child_expiration_does_not_expire_parent_and_timer_expiration_survives_restore() {
        let parent = budget();
        let child = parent
            .child_budget(&physical_time(150), Duration::from_millis(20))
            .expect("child");
        assert!(matches!(
            child.remaining_at(&physical_time(170)),
            Err(TimeoutBudgetError::DeadlineExceeded { .. })
        ));
        assert_eq!(
            parent
                .remaining_at(&physical_time(175))
                .expect("parent has its own deadline"),
            Duration::from_millis(25)
        );
        let clone = parent.clone();
        assert!(matches!(
            parent
                .expire_at(&physical_time(175))
                .expect("timer expiration"),
            TimeoutBudgetError::DeadlineExceeded {
                observed_at_ms: 175,
                ..
            }
        ));
        assert!(matches!(
            clone.remaining_at(&physical_time(180)),
            Err(TimeoutBudgetError::DeadlineExceeded {
                observed_at_ms: 175,
                ..
            })
        ));
        let restored: TimeoutBudget =
            serde_json::from_slice(&serde_json::to_vec(&parent).expect("persist expired owner"))
                .expect("restore expired owner");
        assert!(matches!(
            restored.remaining_at(&physical_time(180)),
            Err(TimeoutBudgetError::DeadlineExceeded {
                observed_at_ms: 175,
                ..
            })
        ));
    }
    #[test]
    fn persisted_budget_rejects_missing_inconsistent_and_legacy_observation_state() {
        let parent = budget();
        parent.remaining_at(&physical_time(150)).expect("progress");
        let valid = serde_json::to_value(&parent).expect("persist budget");
        for field in ["clock", "expiration"] {
            let mut missing = valid.clone();
            missing
                .as_object_mut()
                .expect("budget object")
                .remove(field);
            assert!(
                serde_json::from_value::<TimeoutBudget>(missing).is_err(),
                "missing {field} must fail closed"
            );
        }
        let mut backward = valid.clone();
        backward["clock"]["max_observed_at_ms"] = serde_json::json!(99);
        assert!(serde_json::from_value::<TimeoutBudget>(backward).is_err());
        let mut invalid_deadline = valid.clone();
        invalid_deadline["deadline_at_ms"] = serde_json::json!(99);
        assert!(serde_json::from_value::<TimeoutBudget>(invalid_deadline).is_err());
        let mut missing_latch = valid;
        missing_latch["clock"]
            .as_object_mut()
            .expect("clock object")
            .remove("rollback");
        assert!(serde_json::from_value::<TimeoutBudget>(missing_latch).is_err());
        assert!(serde_json::from_str::<TimeoutBudget>(
            r#"{"started_at_ms":100,"deadline_at_ms":200}"#
        )
        .is_err());
    }
    #[test]
    fn observation_contention_is_typed_owner_failure_without_deadline_or_clock_claim() {
        let parent = budget();
        let child = parent
            .child_budget(&physical_time(150), Duration::from_millis(50))
            .expect("child");
        let guard = parent
            .clock
            .state
            .try_lock()
            .expect("exclusive observation fixture");
        assert!(matches!(
            child.remaining_at(&physical_time(160)),
            Err(TimeoutBudgetError::ObservationUnavailable)
        ));
        drop(guard);
        assert_eq!(
            parent
                .remaining_at(&physical_time(160))
                .expect("original deadline remains bounded"),
            Duration::from_millis(40)
        );
        fn send_sync<T: Send + Sync>() {}
        send_sync::<TimeoutBudget>();
        send_sync::<TimeoutClockObservation>();
    }
    #[test]
    #[allow(clippy::disallowed_methods)]
    fn parallel_children_either_observe_same_clock_or_fail_closed_without_extension() {
        let parent = budget();
        let children: Vec<_> = (0..4)
            .map(|_| {
                parent
                    .child_budget(&physical_time(150), Duration::from_millis(100))
                    .expect("parallel child")
            })
            .collect();
        let threads: Vec<_> = children.into_iter().map(|child| std::thread::spawn(move || {
            for _ in 0..16 {
                match child.remaining_at(&physical_time(150)) {
                    Ok(remaining) => assert_eq!(remaining, Duration::from_millis(50)),
                    Err(TimeoutBudgetError::ObservationUnavailable) => {},
                    Err(other) => panic!("same-clock child must not invent clock/expiration failure: {other}"),
                }
            }
        })).collect();
        for thread in threads {
            thread.join().expect("parallel observation must not panic");
        }
        assert_eq!(
            parent
                .remaining_at(&physical_time(160))
                .expect("shared parent bound"),
            Duration::from_millis(40)
        );
    }
    #[tokio::test]
    async fn success_and_timer_branches_both_detect_rollback_above_original_start() {
        for timer_wins in [false, true] {
            let time = ScriptedTimeEffects::new(
                [physical_time(150), physical_time(140)],
                SleepBehavior::Immediate,
            );
            let result = execute_with_timeout_budget(&time, &budget(), || async move {
                if timer_wins {
                    futures::future::pending::<()>().await;
                }
                Ok::<_, std::io::Error>("operation completed")
            })
            .await
            .expect_err("clock rollback must prevent successful terminal and fake timeout");
            match result {
                TimeoutRunError::Timeout(error) => assert_rollback(error),
                TimeoutRunError::Operation(error) => {
                    panic!("unexpected operation failure: {error}")
                }
            }
        }
    }
    #[tokio::test]
    async fn rollback_before_operation_does_not_poll_operation_or_sleep() {
        let parent = budget();
        parent
            .remaining_at(&physical_time(150))
            .expect("owner progress");
        let time = ScriptedTimeEffects::new([physical_time(140)], SleepBehavior::Immediate);
        let calls = AtomicUsize::new(0);
        let result = execute_with_timeout_budget(&time, &parent, || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, std::io::Error>(())
        })
        .await
        .expect_err("rollback blocks required operation");
        assert!(matches!(
            result,
            TimeoutRunError::Timeout(TimeoutBudgetError::ClockRollback { .. })
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(time.sleep_calls().is_empty());
    }
    #[tokio::test]
    async fn cancelled_wait_preserves_observed_progress_for_next_clone() {
        let parent = budget();
        let time = ScriptedTimeEffects::new([physical_time(150)], SleepBehavior::YieldOnce);
        let mut operation = Box::pin(execute_with_timeout_budget(&time, &parent, || {
            futures::future::pending::<Result<(), std::io::Error>>()
        }));
        assert!(futures::poll!(&mut operation).is_pending());
        drop(operation);
        assert_rollback(
            parent
                .clone()
                .remaining_at(&physical_time(140))
                .expect_err("cancel does not reset owner clock"),
        );
    }
    #[tokio::test]
    async fn retry_clock_progress_is_retained_through_backoff_and_attempt_budget_creation() {
        for per_attempt in [false, true] {
            let times = if per_attempt {
                vec![100, 150, 150, 150, 140]
            } else {
                vec![100, 150, 150, 140]
            };
            let time = ScriptedTimeEffects::new(
                times.into_iter().map(physical_time),
                SleepBehavior::YieldOnce,
            );
            let mut policy = RetryBudgetPolicy::new(
                3,
                ExponentialBackoffPolicy::new(
                    Duration::from_millis(1),
                    Duration::from_millis(1),
                    JitterMode::None,
                )
                .expect("backoff"),
            );
            if per_attempt {
                policy = policy.with_per_attempt_timeout(Duration::from_millis(100));
            }
            let calls = Arc::new(AtomicUsize::new(0));
            let result = execute_with_retry_budget(&time, &policy, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                async { Err::<(), _>("real retryable operation failure") }
            })
            .await
            .expect_err("rollback must prevent second attempt");
            assert!(matches!(
                result,
                RetryRunError::Timeout(TimeoutBudgetError::ClockRollback {
                    previous_observed_at_ms: 150,
                    observed_at_ms: 140
                })
            ));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(time.sleep_calls(), vec![1]);
        }
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_types, clippy::expect_used)]
mod checkpoint_executor_tests {
    use super::tests::{physical_time, ScriptedTimeEffects, SleepBehavior};
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[tokio::test]
    async fn canceled_unacknowledged_initial_checkpoint_never_polls_operation() {
        let time = ScriptedTimeEffects::new([physical_time(150)], SleepBehavior::Immediate);
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(100))
                .expect("original window");
        let polls = AtomicUsize::new(0);
        let mut future = Box::pin(execute_with_timeout_budget_and_checkpoint(
            &time,
            &budget,
            || futures::future::pending::<TimeoutBudgetResult<()>>(),
            || async {
                polls.fetch_add(1, Ordering::SeqCst);
                Ok::<_, std::io::Error>(())
            },
        ));
        assert!(futures::poll!(future.as_mut()).is_pending());
        drop(future);
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            budget.remaining_at(&physical_time(140)),
            Err(TimeoutBudgetError::ClockRollback {
                previous_observed_at_ms: 150,
                observed_at_ms: 140
            })
        ));
    }
    #[tokio::test]
    async fn rollback_is_checkpointed_before_failure_returns() {
        let time = ScriptedTimeEffects::new(
            [physical_time(150), physical_time(140)],
            SleepBehavior::Immediate,
        );
        let budget =
            TimeoutBudget::from_start_and_timeout(&physical_time(100), Duration::from_millis(100))
                .expect("original window");
        let snapshots = std::sync::Mutex::new(Vec::new());
        let error = execute_with_timeout_budget_and_checkpoint(
            &time,
            &budget,
            || {
                snapshots
                    .lock()
                    .expect("test snapshots available")
                    .push(serde_json::to_vec(&budget).expect("checkpoint snapshot"));
                futures::future::ready(Ok(()))
            },
            || async { Ok::<_, std::io::Error>(()) },
        )
        .await
        .expect_err("rollback cannot publish success");
        assert!(matches!(
            error,
            TimeoutRunError::Timeout(TimeoutBudgetError::ClockRollback { .. })
        ));
        let restored: TimeoutBudget = serde_json::from_slice(
            snapshots
                .lock()
                .expect("test snapshots available")
                .last()
                .expect("failure checkpoint exists"),
        )
        .expect("validated rollback checkpoint");
        assert!(matches!(
            restored.remaining_at(&physical_time(180)),
            Err(TimeoutBudgetError::ClockRollback { .. })
        ));
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod physical_interval_policy_tests {
    use super::*;
    #[test]
    fn physical_budgets_reject_empty_submillisecond_and_unrepresentable_windows() {
        let start = PhysicalTime::exact(100);
        for duration in [Duration::ZERO, Duration::from_nanos(1)] {
            assert!(matches!(
                TimeoutBudget::from_start_and_timeout(&start, duration),
                Err(TimeoutBudgetError::InvalidPolicy { .. })
            ));
        }
        assert!(TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(u64::MAX),
            Duration::from_millis(1)
        )
        .is_err());
        let generation = WindowInterval::<crate::types::window::ReceiptGeneration>::new(
            WindowPosition::new(100),
            0,
        )
        .expect("empty generation allowance is valid arithmetic");
        assert!(generation.is_empty());
    }
    #[test]
    fn restore_rejects_empty_physical_window_without_changing_wire_snapshot() {
        let budget = TimeoutBudget::from_start_and_timeout(
            &PhysicalTime::exact(100),
            Duration::from_millis(10),
        )
        .expect("positive original budget");
        let mut snapshot = serde_json::to_value(&budget).expect("serialize owner state");
        assert_eq!(snapshot["started_at_ms"], 100);
        assert_eq!(snapshot["deadline_at_ms"], 110);
        snapshot["deadline_at_ms"] = serde_json::json!(100);
        assert!(serde_json::from_value::<TimeoutBudget>(snapshot).is_err());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod checkpoint_continuity_tests {
    use super::*;
    fn budget() -> TimeoutBudget {
        TimeoutBudget::from_start_and_timeout(&PhysicalTime::exact(100), Duration::from_millis(100))
            .expect("positive original budget")
    }
    #[test]
    fn ownership_identity_distinguishes_same_bounds_and_roundtrip_from_real_clone() {
        let original = budget();
        assert!(original.shares_observation_owner_with(&original.clone()));
        assert!(!original.shares_observation_owner_with(&budget()));
        let restored: TimeoutBudget =
            serde_json::from_slice(&serde_json::to_vec(&original).expect("frozen budget"))
                .expect("validated restored owner");
        assert!(!original.shares_observation_owner_with(&restored));
        assert!(restored
            .validate_checkpoint_continuation_from(&original)
            .is_ok());
    }
    #[test]
    fn durable_progress_and_sticky_failures_cannot_be_replaced_by_fresh_same_bounds() {
        let original = budget();
        original
            .remaining_at(&PhysicalTime::exact(150))
            .expect("actual progress");
        assert!(matches!(
            budget().validate_checkpoint_continuation_from(&original),
            Err(TimeoutBudgetError::CheckpointDiscontinuity { .. })
        ));
        assert!(original.remaining_at(&PhysicalTime::exact(140)).is_err());
        let fresh = budget();
        fresh
            .remaining_at(&PhysicalTime::exact(160))
            .expect("independent higher clock");
        assert!(
            fresh
                .validate_checkpoint_continuation_from(&original)
                .is_err(),
            "higher time cannot erase sticky rollback"
        );
        let exhausted = budget();
        assert!(exhausted.remaining_at(&PhysicalTime::exact(200)).is_err());
        let replacement = budget();
        replacement
            .clock
            .observe(&PhysicalTime::exact(200))
            .expect("same max without exhaustion");
        assert!(replacement
            .validate_checkpoint_continuation_from(&exhausted)
            .is_err());
    }
}

#[cfg(test)]
mod checkpoint_failure_tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn checkpoint_clone_retains_codec_cause_and_wire_omits_it() {
        let codec = serde_json::from_slice::<u64>(b"invalid").expect_err("actual invalid JSON");
        let original = TimeoutBudgetError::checkpoint_failure(codec);
        let clone = original.clone();
        assert!(clone
            .source()
            .expect("retained wrapper")
            .source()
            .expect("actual codec")
            .is::<serde_json::Error>());
        assert_eq!(clone.code(), "timeout_checkpoint_failure");
        let bytes = serde_json::to_vec(&clone).expect("diagnostic serialization");
        let restored: TimeoutBudgetError =
            serde_json::from_slice(&bytes).expect("diagnostic restore");
        assert!(matches!(
            restored,
            TimeoutBudgetError::CheckpointFailure { source: None, .. }
        ));
        assert!(restored.source().is_none());
        let outer: AuraError = original.into();
        assert!(outer
            .source()
            .expect("budget source")
            .is::<TimeoutBudgetError>());
    }
}
