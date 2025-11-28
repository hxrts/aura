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
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

/// Error type for time operations.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
pub enum TimeError {
    #[error("Timeout after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },
    #[error("Timeout handle not found: {handle}")]
    TimeoutNotFound { handle: TimeoutHandle },
    #[error("Clock sync failed: {reason}")]
    ClockSyncFailed { reason: String },
    #[error("Time service unavailable")]
    ServiceUnavailable,
    #[error("Operation failed: {reason}")]
    OperationFailed { reason: String },
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
    ThresholdEvents { threshold: usize, timeout_ms: u64 },
    Custom(String),
}

#[async_trait]
pub trait PhysicalTimeEffects: Send + Sync {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError>;
    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError>;
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

/// Compatibility shim for legacy time accessors.
///
/// New code should prefer the domain-specific traits above, but many callers
/// still expect helper methods like `current_timestamp()` and `now_instant()`.
/// This trait delegates to `PhysicalTimeEffects` and should be blanket
/// implemented for any physical clock provider.
#[async_trait]
pub trait TimeEffects: PhysicalTimeEffects {
    /// Monotonic clock instant - must be implemented by effect handlers.
    /// This cannot have a default implementation as it violates effect system architecture.
    async fn now_instant(&self) -> Instant;

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

#[async_trait]
impl<T> TimeEffects for T
where
    T: PhysicalTimeEffects + ?Sized,
{
    /// Default implementation that provides Instant::now() for compatibility.
    /// This is a blanket implementation for all PhysicalTimeEffects implementors.
    #[allow(clippy::disallowed_methods)] // Effect trait implementation needs Instant::now
    async fn now_instant(&self) -> Instant {
        Instant::now()
    }
}

/// Blanket implementation for Arc<T> where T: PhysicalTimeEffects
#[async_trait]
impl<T: PhysicalTimeEffects + ?Sized> PhysicalTimeEffects for std::sync::Arc<T> {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        (**self).physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        (**self).sleep_ms(ms).await
    }
}

/// Blanket implementation for Arc<T> where T: LogicalClockEffects
#[async_trait]
impl<T: LogicalClockEffects + ?Sized> LogicalClockEffects for std::sync::Arc<T> {
    async fn logical_advance(
        &self,
        observed: Option<&crate::time::VectorClock>,
    ) -> Result<crate::time::LogicalTime, TimeError> {
        (**self).logical_advance(observed).await
    }

    async fn logical_now(&self) -> Result<crate::time::LogicalTime, TimeError> {
        (**self).logical_now().await
    }
}

/// Blanket implementation for Arc<T> where T: OrderClockEffects
#[async_trait]
impl<T: OrderClockEffects + ?Sized> OrderClockEffects for std::sync::Arc<T> {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        (**self).order_time().await
    }
}

/// Blanket implementation for Arc<T> where T: TimeComparison
#[async_trait]
impl<T: TimeComparison + ?Sized> TimeComparison for std::sync::Arc<T> {
    async fn compare(
        &self,
        a: &crate::time::TimeStamp,
        b: &crate::time::TimeStamp,
    ) -> Result<TimeOrdering, TimeError> {
        (**self).compare(a, b).await
    }
}
