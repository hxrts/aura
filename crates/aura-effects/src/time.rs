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
    TimeProviderOperation,
};
use aura_core::time::{
    LogicalTime, OrderTime, OrderingPolicy, TimeOrdering, TimeStamp, VectorClock,
};
use aura_core::types::window::{PhysicalMillis, WindowPosition};
use cfg_if::cfg_if;
use rand::RngCore;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

#[cfg(target_arch = "wasm32")]
type MonotonicInstant = web_time::Instant;
#[cfg(not(target_arch = "wasm32"))]
type MonotonicInstant = std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use tokio::time;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{closure::Closure, JsCast};

cfg_if! {
    if #[cfg(target_arch = "wasm32")] {
        use js_sys::Date;
        use web_sys::window;
    } else {
        use std::time::{SystemTime, UNIX_EPOCH};
    }
}

/// Monotonic timestamp helper for layers that need batching or scheduling.
#[allow(clippy::disallowed_methods)] // Monotonic clock access is permitted in effect handlers
pub fn monotonic_now() -> MonotonicInstant {
    MonotonicInstant::now()
}

/// Production physical clock handler backed by the system clock.
#[derive(Debug, Clone, Default)]
pub struct PhysicalTimeHandler;

impl PhysicalTimeHandler {
    /// Create a new physical clock handler.
    pub fn new() -> Self {
        Self
    }

    /// Required physical observation; clock faults never become epoch zero.
    #[allow(clippy::disallowed_methods)] // Native clock access belongs to this effect implementation.
    fn required_physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        #[cfg(not(target_arch = "wasm32"))]
        let ts_ms = {
            let elapsed = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|source| TimeError::ProviderFailure {
                    operation: TimeProviderOperation::ReadPhysicalClock,
                    source: Some(std::sync::Arc::new(source)),
                })?;
            u64::try_from(elapsed.as_millis()).map_err(|source| TimeError::ProviderFailure {
                operation: TimeProviderOperation::ReadPhysicalClock,
                source: Some(std::sync::Arc::new(source)),
            })?
        };
        #[cfg(target_arch = "wasm32")]
        let ts_ms = {
            let value = Date::now();
            if !value.is_finite() || value < 0.0 || value >= u64::MAX as f64 {
                return Err(TimeError::InvalidPhysicalClockValue {
                    observed_bits: value.to_bits(),
                });
            }
            value as u64
        };
        Ok(aura_core::time::PhysicalTime {
            ts_ms,
            uncertainty: None,
        })
    }

    async fn wait_with_observer<C>(
        &self,
        deadline: WindowPosition<PhysicalMillis>,
        mut read: C,
    ) -> Result<aura_core::time::PhysicalTime, TimeError>
    where
        C: FnMut() -> Result<aura_core::time::PhysicalTime, TimeError> + Send,
    {
        use std::task::Poll;
        let mut previous = None;
        let mut timer = None;
        futures::future::poll_fn(|context| loop {
            let now = match read() {
                Ok(now) => now,
                Err(source) => return Poll::Ready(Err(source)),
            };
            if let Some(previous_ms) = previous {
                if now.ts_ms < previous_ms {
                    return Poll::Ready(Err(TimeError::PhysicalClockRollback {
                        previous_ms,
                        observed_ms: now.ts_ms,
                    }));
                }
            }
            if now.ts_ms >= deadline.value() {
                return Poll::Ready(Ok(now));
            }
            previous = Some(now.ts_ms);
            let registration = timer
                .get_or_insert_with(|| self.sleep_ms((deadline.value() - now.ts_ms).min(1000)));
            match registration.as_mut().poll(context) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(source)) => return Poll::Ready(Err(source)),
                Poll::Ready(Ok(())) => {
                    timer = None;
                }
            }
        })
        .await
    }

    /// Synchronous physical time helper (ms since epoch).
    ///
    /// This is intended for UI/frontend call sites that are not async and need
    /// a best-effort timestamp without spawning a runtime. It still sources time
    /// from the system clock, so simulator-driven tests should prefer the async
    /// `physical_time` trait method for full control.
    #[allow(clippy::disallowed_methods)] // Effect implementation reads wall clock directly
    pub fn physical_time_now_ms(&self) -> u64 {
        #[cfg(target_arch = "wasm32")]
        {
            Date::now() as u64
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO);
            now.as_millis() as u64
        }
    }

    /// Sleep until a target epoch in seconds (best-effort).
    pub async fn sleep_until(&self, target_epoch_secs: u64) {
        if let Ok(now) = self.physical_time().await {
            let now_secs = now.ts_ms / 1000;
            if target_epoch_secs > now_secs {
                let delta = target_epoch_secs - now_secs;
                let _ = self.sleep_ms(delta.saturating_mul(1000)).await;
            }
        }
    }
}

#[async_trait]
impl PhysicalTimeEffects for PhysicalTimeHandler {
    #[tracing::instrument(name = "physical_time", level = "trace")]
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        let result = self.required_physical_time()?;

        // Record latency metrics
        #[cfg(not(target_arch = "wasm32"))]
        {
            let start = monotonic_now();
            let latency = start.elapsed();
            tracing::trace!(
                latency_ns = latency.as_nanos(),
                "physical_time_access_latency"
            );
        }

        Ok(result)
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        #[cfg(target_arch = "wasm32")]
        {
            sleep_browser_owned(ms).await?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            time::sleep(Duration::from_millis(ms)).await;
        }
        Ok(())
    }

    async fn wait_until_physical_deadline(
        &self,
        deadline: WindowPosition<PhysicalMillis>,
    ) -> Result<aura_core::time::PhysicalTime, TimeError> {
        // Recheck on every poll, including a competing checkpoint's wake while
        // the monotonic timer is pending. A wall-clock jump must not let that
        // checkpoint publish after the fixed physical endpoint.
        self.wait_with_observer(deadline, || self.required_physical_time())
            .await
    }
}

cfg_if! {
if #[cfg(target_arch = "wasm32")] {
struct BrowserTimer {
    window: web_sys::Window,
    id: i32,
    _callback: Closure<dyn FnMut()>,
}
impl Drop for BrowserTimer {
    fn drop(&mut self) {
        self.window.clear_timeout_with_handle(self.id);
    }
}

#[derive(Debug)]
struct BrowserTimerRegistrationError(send_wrapper::SendWrapper<wasm_bindgen::JsValue>);
impl std::fmt::Display for BrowserTimerRegistrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "browser timer registration failed: {:?}", &*self.0)
    }
}
impl std::error::Error for BrowserTimerRegistrationError {}

async fn sleep_browser_owned(ms: u64) -> Result<(), TimeError> {
    let (timer, receiver) = {
        let window = window().ok_or(TimeError::ServiceUnavailable)?;
        let (sender, receiver) = futures::channel::oneshot::channel();
        let callback = Closure::once(move || {
            let _ = sender.send(());
        });
        let id = window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                callback.as_ref().unchecked_ref(),
                i32::try_from(ms.min(i32::MAX as u64)).unwrap_or(i32::MAX),
            )
            .map_err(|source| TimeError::ProviderFailure {
                operation: TimeProviderOperation::RegisterTimer,
                source: Some(std::sync::Arc::new(BrowserTimerRegistrationError(
                    send_wrapper::SendWrapper::new(source),
                ))),
            })?;
        // Browser execution is thread-confined. SendWrapper checks this on
        // access and Drop while retaining the closure until timer cancellation.
        (
            send_wrapper::SendWrapper::new(BrowserTimer {
                window,
                id,
                _callback: callback,
            }),
            receiver,
        )
    };
    let outcome = receiver.await.map_err(|source| TimeError::ProviderFailure {
        operation: TimeProviderOperation::WaitTimer,
        source: Some(std::sync::Arc::new(source)),
    });
    drop(timer);
    outcome
}
}
}

/// Implement TimeEffects for PhysicalTimeHandler using default implementations
/// (current_timestamp derives from physical_time via the trait default)
#[async_trait]
impl aura_core::effects::TimeEffects for PhysicalTimeHandler {}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod absolute_deadline_tests {
    use super::*;

    #[tokio::test]
    async fn required_absolute_deadline_rechecks_clock_before_pending_timer() {
        use futures::FutureExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        let handler = PhysicalTimeHandler::new();
        let clock = AtomicU64::new(100);
        let wait = handler.wait_with_observer(WindowPosition::new(500), || {
            Ok(aura_core::time::PhysicalTime::exact(
                clock.load(Ordering::SeqCst),
            ))
        });
        tokio::pin!(wait);
        assert!(
            wait.as_mut().now_or_never().is_none(),
            "actual native timer remains pending"
        );
        // Represents the competing observation becoming runnable after a
        // physical clock jump, before the already registered timer has fired.
        clock.store(500, Ordering::SeqCst);
        assert_eq!(
            wait.as_mut()
                .now_or_never()
                .expect("endpoint checked on competing wake")
                .expect("actual current endpoint")
                .ts_ms,
            500
        );
    }

    #[tokio::test]
    async fn required_absolute_deadline_at_epoch_returns_actual_native_clock() {
        let clock = PhysicalTimeHandler::new();
        let before = clock
            .physical_time()
            .await
            .expect("actual required native read");
        let endpoint = clock
            .wait_until_physical_deadline(WindowPosition::new(0))
            .await
            .expect("already elapsed absolute endpoint");
        assert!(endpoint.ts_ms >= before.ts_ms);
        assert_ne!(
            endpoint.ts_ms, 0,
            "returned witness is an actual observation, not requested endpoint"
        );
    }

    #[tokio::test]
    async fn required_absolute_deadline_waits_for_actual_native_endpoint() {
        let clock = PhysicalTimeHandler::new();
        let start = clock
            .physical_time()
            .await
            .expect("native timer registration observation");
        let deadline = WindowPosition::new(
            start
                .ts_ms
                .checked_add(2)
                .expect("native test endpoint fits"),
        );
        let endpoint = clock
            .wait_until_physical_deadline(deadline)
            .await
            .expect("original native timer wake");
        assert!(endpoint.ts_ms >= deadline.value());
        assert!(endpoint.ts_ms > start.ts_ms);
    }
}

/// Simple logical clock handler - stateless pure functions for logical clock operations.
#[deprecated(
    note = "Use the runtime-owned LogicalClockService (aura-agent) for stateful logical clocks. \
            This handler remains as a pure helper."
)]
#[derive(Debug, Clone, Default)]
pub struct LogicalClockHandler;

#[allow(deprecated)]
impl LogicalClockHandler {
    /// Create a new logical clock handler.
    pub fn new() -> Self {
        Self
    }

    /// Pure function to advance logical time based on observed vector clock.
    pub fn advance_logical_time(
        current_vector: &VectorClock,
        current_scalar: u64,
        authority: Option<aura_core::types::identifiers::DeviceId>,
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
#[allow(deprecated)]
impl LogicalClockEffects for LogicalClockHandler {
    #[tracing::instrument(name = "logical_advance", level = "trace", skip(observed))]
    #[allow(clippy::disallowed_methods)] // Effect implementation uses Instant for metrics
    async fn logical_advance(
        &self,
        observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let start = monotonic_now();

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
        let start = monotonic_now();

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
        let start = monotonic_now();

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
