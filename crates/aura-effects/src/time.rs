//! Domain time handlers (Layer 3).
//!
//! Provides production implementations for:
//! - PhysicalTimeEffects (system clock + sleep)
//! - LogicalClockEffects (simple scalar + vector tracking)
//! - OrderClockEffects (opaque sortable token)
//! - TimeComparison (delegates to core comparison)

use async_trait::async_trait;
use aura_core::effects::time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeComparison, TimeError,
};
use aura_core::time::{
    LogicalTime, OrderTime, OrderingPolicy, TimeOrdering, TimeStamp, VectorClock,
};
use rand::RngCore;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::{self, Instant};

/// Monotonic timestamp helper for layers that need batching or scheduling.
#[allow(clippy::disallowed_methods)] // Monotonic clock access is permitted in effect handlers
pub fn monotonic_now() -> Instant {
    Instant::now()
}

/// Production physical clock handler backed by the system clock.
#[derive(Debug, Clone, Default)]
pub struct PhysicalTimeHandler;
// Legacy RealTimeHandler alias removed - use PhysicalTimeHandler directly

impl PhysicalTimeHandler {
    /// Create a new physical clock handler.
    pub fn new() -> Self {
        Self
    }

    /// Synchronous physical time helper (ms since epoch).
    ///
    /// This is intended for UI/frontend call sites that are not async and need
    /// a best-effort timestamp without spawning a runtime. It still sources time
    /// from the system clock, so simulator-driven tests should prefer the async
    /// `physical_time` trait method for full control.
    #[allow(clippy::disallowed_methods)] // Effect handler is permitted to read the host clock
    pub fn physical_time_now_ms(&self) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);
        now.as_millis() as u64
    }

    /// Sleep until a target epoch in seconds (best-effort).
    pub async fn sleep_until(&self, target_epoch_secs: u64) {
        if let Ok(now) = self.physical_time().await {
            let now_secs = now.ts_ms / 1000;
            if target_epoch_secs > now_secs {
                let delta = target_epoch_secs - now_secs;
                time::sleep(Duration::from_secs(delta)).await;
            }
        }
    }
}

#[async_trait]
impl PhysicalTimeEffects for PhysicalTimeHandler {
    #[tracing::instrument(name = "physical_time", level = "trace")]
    #[allow(clippy::disallowed_methods)] // Effect implementation legitimately uses system time
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        let start = std::time::Instant::now();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO);

        let result = aura_core::time::PhysicalTime {
            ts_ms: now.as_millis() as u64,
            uncertainty: None,
        };

        // Record latency metrics
        let latency = start.elapsed();
        tracing::trace!(
            latency_ns = latency.as_nanos(),
            "physical_time_access_latency"
        );

        Ok(result)
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        time::sleep(Duration::from_millis(ms)).await;
        Ok(())
    }
}

/// Implement TimeEffects for PhysicalTimeHandler using default implementations
/// (current_timestamp derives from physical_time via the trait default)
#[async_trait]
impl aura_core::effects::TimeEffects for PhysicalTimeHandler {}

/// Simple logical clock handler - stateless pure functions for logical clock operations.
#[derive(Debug, Clone, Default)]
pub struct LogicalClockHandler;

impl LogicalClockHandler {
    /// Create a new logical clock handler.
    pub fn new() -> Self {
        Self
    }

    /// Pure function to advance logical time based on observed vector clock.
    pub fn advance_logical_time(
        current_vector: &VectorClock,
        current_scalar: u64,
        authority: Option<aura_core::identifiers::DeviceId>,
        observed: Option<&VectorClock>,
    ) -> LogicalTime {
        let mut next_vector = current_vector.clone();
        let mut next_scalar = current_scalar;

        if let Some(obs) = observed {
            for (auth, val) in obs.iter() {
                let current_count = next_vector.get(auth).copied().unwrap_or(0);
                next_vector.insert(*auth, current_count.max(*val));
            }
            // Find max value in observed vector clock
            let obs_max = obs.iter().map(|(_, v)| *v).max().unwrap_or(next_scalar);
            next_scalar = next_scalar.max(obs_max);
        }

        // Bump the clock
        next_scalar = next_scalar.saturating_add(1);
        if let Some(auth) = authority {
            let current_count = next_vector.get(&auth).copied().unwrap_or(0);
            next_vector.insert(auth, current_count.saturating_add(1));
        }

        LogicalTime {
            vector: next_vector,
            lamport: next_scalar,
        }
    }
}

#[async_trait]
impl LogicalClockEffects for LogicalClockHandler {
    #[tracing::instrument(name = "logical_advance", level = "trace", skip(observed))]
    #[allow(clippy::disallowed_methods)] // Effect implementation uses Instant for metrics
    async fn logical_advance(
        &self,
        observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let start = std::time::Instant::now();

        // Since this handler is now stateless, return a default logical time
        // that starts from epoch. In a real application, the caller would need to
        // track the current logical clock state and pass it to advance_logical_time().
        let empty_vector = VectorClock::new();
        let result = Self::advance_logical_time(&empty_vector, 0, None, observed);

        // Record latency metrics
        let latency = start.elapsed();
        tracing::trace!(
            latency_ns = latency.as_nanos(),
            vector_size = result.vector.len(),
            "logical_advance_latency"
        );

        Ok(result)
    }

    #[tracing::instrument(name = "logical_now", level = "trace")]
    #[allow(clippy::disallowed_methods)] // Effect implementation uses Instant for metrics
    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        let start = std::time::Instant::now();

        // Since this handler is now stateless, return epoch logical time.
        // In a real application, the caller would manage the current logical clock state.
        let result = LogicalTime {
            vector: VectorClock::new(),
            lamport: 0,
        };

        // Record latency metrics
        let latency = start.elapsed();
        tracing::trace!(
            latency_ns = latency.as_nanos(),
            vector_size = result.vector.len(),
            "logical_now_latency"
        );

        Ok(result)
    }
}

/// Opaque order clock handler that emits sortable random tokens.
#[derive(Debug, Clone, Default)]
pub struct OrderClockHandler;

#[async_trait]
impl OrderClockEffects for OrderClockHandler {
    #[tracing::instrument(name = "order_time", level = "trace")]
    #[allow(clippy::disallowed_methods)] // This IS the time handler implementation
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        let start = std::time::Instant::now();

        // Order clock must be unpredictable but stateless; use OS entropy here (allowed in L3 handler).
        let entropy = rand::rngs::OsRng.next_u64().to_le_bytes();
        let mut hasher = aura_core::hash::hasher();
        hasher.update(b"ORDER_TIME_TOKEN");
        hasher.update(&entropy);
        let hashed = hasher.finalize();
        let result = OrderTime(hashed);

        // Record latency metrics
        let latency = start.elapsed();
        tracing::trace!(latency_ns = latency.as_nanos(), "order_time_latency");

        Ok(result)
    }
}

/// Convenience wrapper for comparing timestamps using core policies.
#[derive(Debug, Clone, Default)]
pub struct TimeComparisonHandler;

#[async_trait]
impl TimeComparison for TimeComparisonHandler {
    async fn compare(&self, a: &TimeStamp, b: &TimeStamp) -> Result<TimeOrdering, TimeError> {
        Ok(a.compare(b, OrderingPolicy::Native))
    }
}
