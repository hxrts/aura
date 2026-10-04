//! Domain-specific time trait definitions (v2).
//!
//! These traits correspond to the semantic time types defined in `crate::time`.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: All crates needing time operations (physical timestamps, logical clocks, ordering tokens)
//!
//! This module provides multiple time-related traits:
//! - `PhysicalTimeEffects`: Wall-clock time for timestamps, expiration, cooldowns
//! - `LogicalClockEffects`: Vector + Lamport clocks for causal ordering
//! - `OrderClockEffects`: Privacy-preserving deterministic ordering tokens
//!
//! All are infrastructure effects implemented in `aura-effects` with stateless handlers.

use crate::time::{OrderTime, PhysicalTime, TimeOrdering};
use crate::types::window::{PhysicalMillis, WindowPosition};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Error type for time operations.
#[derive(Debug, Serialize, Deserialize)]
pub enum TimeError {
    Timeout {
        timeout_ms: u64,
    },
    TimeoutNotFound {
        handle: TimeoutHandle,
    },
    ClockSyncFailed {
        reason: String,
    },
    ServiceUnavailable,
    OperationFailed {
        reason: String,
    },
    AbsoluteDeadlineUnsupported,
    PhysicalClockRollback {
        previous_ms: u64,
        observed_ms: u64,
    },
    InvalidPhysicalClockValue {
        observed_bits: u64,
    },
    ProviderFailure {
        operation: TimeProviderOperation,
        #[serde(skip)]
        source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
    },
}

impl std::fmt::Display for TimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { timeout_ms } => write!(f, "Timeout after {timeout_ms}ms"),
            Self::TimeoutNotFound { handle } => write!(f, "Timeout handle not found: {handle}"),
            Self::ClockSyncFailed { reason } => write!(f, "Clock sync failed: {reason}"),
            Self::ServiceUnavailable => f.write_str("Time service unavailable"),
            Self::OperationFailed { reason } => write!(f, "Operation failed: {reason}"),
            Self::AbsoluteDeadlineUnsupported => {
                f.write_str("physical deadline waiting is unsupported by the selected provider")
            }
            Self::PhysicalClockRollback {
                previous_ms,
                observed_ms,
            } => write!(
                f,
                "physical clock rolled back from {previous_ms}ms to {observed_ms}ms"
            ),
            Self::InvalidPhysicalClockValue { observed_bits } => write!(
                f,
                "physical clock returned an invalid numeric value (bits {observed_bits})"
            ),
            Self::ProviderFailure { operation, .. } => {
                write!(f, "time provider {operation:?} failed")
            }
        }
    }
}
impl std::error::Error for TimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            // Dereference the Arc: exposing its container hides the concrete
            // native cause from downcasting when that cause has no own source.
            Self::ProviderFailure { source, .. } => source
                .as_ref()
                .map(|source| source.as_ref() as &(dyn std::error::Error + 'static)),
            Self::Timeout { .. }
            | Self::TimeoutNotFound { .. }
            | Self::ClockSyncFailed { .. }
            | Self::ServiceUnavailable
            | Self::OperationFailed { .. }
            | Self::AbsoluteDeadlineUnsupported
            | Self::PhysicalClockRollback { .. }
            | Self::InvalidPhysicalClockValue { .. } => None,
        }
    }
}

/// The failing provider operation; error sources remain native in process.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TimeProviderOperation {
    ReadPhysicalClock,
    RegisterTimer,
    WaitTimer,
}

/// Handle for timeout operations.
pub type TimeoutHandle = Uuid;

/// Wake conditions for cooperative scheduling.
#[derive(Debug, Clone)]
pub enum WakeCondition {
    Immediate,
    NewEvents,
    EpochReached { target: u64 },
    TimeoutAt(u64),
    TimeoutExpired { timeout_id: TimeoutHandle },
    EventMatching(String),
    ThresholdEvents { threshold: u32, timeout_ms: u64 },
    Custom(String),
}

#[async_trait]
pub trait PhysicalTimeEffects: Send + Sync {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError>;
    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError>;

    /// Wait for an existing fixed physical endpoint and return its actual observation.
    ///
    /// Implementations must register against their own configured clock without
    /// awaiting `physical_time` first. A relative sleep derived from a caller's
    /// cached observation does not satisfy this contract. Cancellation releases
    /// timer custody. Unsupported providers fail explicitly without a fallback.
    /// Receipt generations cannot be used as physical deadline coordinates.
    ///
    /// ```compile_fail
    /// use aura_core::effects::PhysicalTimeEffects;
    /// use aura_core::types::window::{ReceiptGeneration, WindowPosition};
    /// async fn wrong_domain(clock: &dyn PhysicalTimeEffects) {
    ///     let _ = clock.wait_until_physical_deadline(
    ///         WindowPosition::<ReceiptGeneration>::new(500)
    ///     ).await;
    /// }
    /// ```
    async fn wait_until_physical_deadline(
        &self,
        _deadline: WindowPosition<PhysicalMillis>,
    ) -> Result<PhysicalTime, TimeError> {
        Err(TimeError::AbsoluteDeadlineUnsupported)
    }
}

#[async_trait]
pub trait LogicalClockEffects: Send + Sync {
    async fn logical_advance(
        &self,
        observed: Option<&crate::time::VectorClock>,
    ) -> Result<crate::time::LogicalTime, TimeError>;
    async fn logical_now(&self) -> Result<crate::time::LogicalTime, TimeError>;
}

#[async_trait]
pub trait OrderClockEffects: Send + Sync {
    async fn order_time(&self) -> Result<OrderTime, TimeError>;
}

#[async_trait]
pub trait TimeComparison: Send + Sync {
    async fn compare(
        &self,
        a: &crate::time::TimeStamp,
        b: &crate::time::TimeStamp,
    ) -> Result<TimeOrdering, TimeError>;
}

/// Convenience trait for common timestamp accessors.
///
/// Delegates to `PhysicalTimeEffects` for underlying time operations.
/// New code should prefer the domain-specific traits above, but this trait
/// provides helper methods like `current_timestamp()` for simpler use cases.
#[async_trait]
pub trait TimeEffects: PhysicalTimeEffects {
    /// Current Unix timestamp in seconds.
    async fn current_timestamp(&self) -> u64 {
        self.physical_time()
            .await
            .map(|t| t.ts_ms / 1000)
            .unwrap_or(0)
    }

    /// Current Unix timestamp in milliseconds.
    async fn current_timestamp_ms(&self) -> u64 {
        self.physical_time().await.map(|t| t.ts_ms).unwrap_or(0)
    }

    /// Alias for current epoch seconds.
    async fn current_epoch(&self) -> u64 {
        self.current_timestamp().await
    }
}

impl_arc_effect!(PhysicalTimeEffects {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        (**self).physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        (**self).sleep_ms(ms).await
    }

    async fn wait_until_physical_deadline(
        &self,
        deadline: WindowPosition<PhysicalMillis>,
    ) -> Result<PhysicalTime, TimeError> {
        (**self).wait_until_physical_deadline(deadline).await
    }
});

impl_arc_effect!(LogicalClockEffects {
    async fn logical_advance(
        &self,
        observed: Option<&crate::time::VectorClock>,
    ) -> Result<crate::time::LogicalTime, TimeError> {
        (**self).logical_advance(observed).await
    }

    async fn logical_now(&self) -> Result<crate::time::LogicalTime, TimeError> {
        (**self).logical_now().await
    }
});

impl_arc_effect!(OrderClockEffects {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        (**self).order_time().await
    }
});

impl_arc_effect!(TimeComparison {
    async fn compare(
        &self,
        a: &crate::time::TimeStamp,
        b: &crate::time::TimeStamp,
    ) -> Result<TimeOrdering, TimeError> {
        (**self).compare(a, b).await
    }
});
