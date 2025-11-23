//! Layer 3: Time Effect Handlers - Production Only
//!
//! Stateless single-party implementation of TimeEffects from aura-core (Layer 1).
//! This handler provides production time operations delegating to system time APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::time;
use uuid::Uuid;

/// Real time handler for production use
///
/// This handler provides access to system time and sleep functionality.
/// It is stateless and delegates all time operations to the operating system.
///
/// **Note**: Multi-context coordination methods (set_timeout with registry, register_context,
/// notify_events_available) have been moved to `TimeoutCoordinator` in aura-protocol (Layer 4).
/// This handler now provides only stateless time operations. For coordination capabilities,
/// wrap this handler with `aura_protocol::handlers::TimeoutCoordinator`.
#[derive(Debug, Clone, Default)]
pub struct RealTimeHandler;

impl RealTimeHandler {
    /// Create a new real time handler
    pub fn new() -> Self {
        Self
    }

    /// Create a new real time handler
    pub fn new_real() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimeEffects for RealTimeHandler {
    #[allow(clippy::disallowed_methods)]
    async fn current_epoch(&self) -> u64 {
        // SystemTime::now() is allowed in production handlers (Layer 3) that implement effect traits.
        // This handler bridges from the pure effect interface to actual system time operations.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    #[allow(clippy::disallowed_methods)]
    async fn current_timestamp(&self) -> u64 {
        // SystemTime::now() is allowed in production handlers (Layer 3) that implement effect traits.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }

    #[allow(clippy::disallowed_methods)]
    async fn current_timestamp_millis(&self) -> u64 {
        // SystemTime::now() is allowed in production handlers (Layer 3) that implement effect traits.
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    #[allow(clippy::disallowed_methods)]
    async fn now_instant(&self) -> Instant {
        // Instant::now() is allowed in production handlers (Layer 3) that implement effect traits.
        Instant::now()
    }

    async fn sleep_ms(&self, ms: u64) {
        time::sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let now = self.current_timestamp_millis().await;
        if epoch > now {
            let diff = epoch - now;
            time::sleep(Duration::from_millis(diff)).await;
        }
    }

    async fn delay(&self, duration: Duration) {
        time::sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        time::sleep(Duration::from_millis(duration_ms)).await;
        Ok(())
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        // Simple cooperative yield
        tokio::task::yield_now().await;
        Ok(())
    }

    async fn wait_until(&self, _condition: WakeCondition) -> Result<(), AuraError> {
        tokio::task::yield_now().await;
        Ok(())
    }

    #[allow(clippy::disallowed_methods)]
    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        // Uuid::new_v4() is allowed in production handlers (Layer 3) that need to generate unique identifiers.
        // This is a legitimate use case for timeout handle generation.
        let handle = Uuid::new_v4();
        let _ = timeout_ms;
        handle
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, _context_id: Uuid) {}

    fn unregister_context(&self, _context_id: Uuid) {}

    async fn notify_events_available(&self) {
        tokio::task::yield_now().await;
    }

    fn resolution_ms(&self) -> u64 {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_time_handler_creation() {
        let handler = RealTimeHandler::new();
        // RealTimeHandler is a unit struct, no fields to check
        let _ = handler;
    }

    #[tokio::test]
    async fn test_current_timestamp() {
        let handler = RealTimeHandler::new();
        let timestamp1 = handler.current_timestamp().await;

        // Sleep for a small amount to ensure time progresses
        tokio::time::sleep(Duration::from_millis(10)).await;

        let timestamp2 = handler.current_timestamp().await;
        assert!(timestamp2 >= timestamp1);
    }

    #[tokio::test]
    async fn test_current_timestamp_millis() {
        let handler = RealTimeHandler::new();
        let timestamp1 = handler.current_timestamp_millis().await;

        // Sleep for a small amount to ensure time progresses
        tokio::time::sleep(Duration::from_millis(10)).await;

        let timestamp2 = handler.current_timestamp_millis().await;
        assert!(timestamp2 >= timestamp1);
    }

    #[tokio::test]
    async fn test_sleep_ms() {
        let handler = RealTimeHandler::new();
        let start = Instant::now();

        handler.sleep_ms(50).await;

        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(40)); // Allow some variance
    }

    #[tokio::test]
    async fn test_set_timeout() {
        let handler = RealTimeHandler::new();

        // Test setting timeout - returns a TimeoutHandle (UUID)
        let handle = handler.set_timeout(1000).await;
        assert!(!uuid::Uuid::nil().eq(&handle));
    }

    #[tokio::test]
    async fn test_cancel_timeout() {
        let handler = RealTimeHandler::new();

        // Test setting and canceling timeout
        let handle = handler.set_timeout(1000).await;
        let result = handler.cancel_timeout(handle).await;
        assert!(result.is_ok());
    }
}
