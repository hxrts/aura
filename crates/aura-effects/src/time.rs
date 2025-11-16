//! Time effect handlers
//!
//! This module provides standard implementations of the `TimeEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time;
use uuid::Uuid;

/// Mock time handler for deterministic testing
#[derive(Debug, Clone)]
pub struct SimulatedTimeHandler {
    current_time: Arc<Mutex<u64>>,
    time_scale: f64,
}

impl SimulatedTimeHandler {
    /// Create a new simulated time handler
    pub fn new() -> Self {
        Self {
            current_time: Arc::new(Mutex::new(0)),
            time_scale: 1.0,
        }
    }

    /// Create a new simulated time handler with specified time and scale
    pub fn new_deterministic(start_time_ms: u64, time_scale: f64) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(start_time_ms)),
            time_scale,
        }
    }

    /// Create a new simulated time handler starting at the given time
    pub fn new_with_time(start_time_ms: u64) -> Self {
        Self::new_deterministic(start_time_ms, 1.0)
    }

    /// Create a simulated time handler starting at Unix epoch
    pub fn new_at_epoch() -> Self {
        Self::new_with_time(0)
    }

    /// Create a simulated time handler with custom time scale
    pub fn with_time_scale(start_time_ms: u64, time_scale: f64) -> Self {
        Self::new_deterministic(start_time_ms, time_scale)
    }

    /// Advance simulated time by the given duration
    pub fn advance_time(&self, duration_ms: u64) {
        let mut time = self.current_time.lock().unwrap();
        *time += duration_ms;
    }

    /// Set the absolute simulated time
    pub fn set_time(&self, time_ms: u64) {
        let mut time = self.current_time.lock().unwrap();
        *time = time_ms;
    }

    /// Get the current simulated time (for testing)
    pub fn get_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    /// Reset time to epoch (for testing)
    pub fn reset(&self) {
        self.set_time(0);
    }

    /// Set the time scale for simulation speed
    pub fn set_time_scale(&mut self, scale: f64) {
        self.time_scale = scale;
    }
}

#[async_trait]
impl TimeEffects for SimulatedTimeHandler {
    async fn current_epoch(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn current_timestamp(&self) -> u64 {
        *self.current_time.lock().unwrap() / 1000
    }

    async fn current_timestamp_millis(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep_ms(&self, ms: u64) {
        let scaled_duration = Duration::from_millis((ms as f64 / self.time_scale) as u64);
        if self.time_scale > 100.0 {
            self.advance_time(ms);
        } else {
            tokio::time::sleep(scaled_duration).await;
            self.advance_time(ms);
        }
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = self.current_timestamp_millis().await;
        if epoch > current {
            let wait_time = epoch - current;
            self.sleep_ms(wait_time).await;
        }
    }

    async fn delay(&self, duration: Duration) {
        self.sleep_ms(duration.as_millis() as u64).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, _condition: WakeCondition) -> Result<(), TimeError> {
        if self.time_scale > 10.0 {
            self.advance_time(1);
        } else {
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| AuraError::internal(format!("Wait condition failed: {}", e)))
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        Uuid::new_v4()
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, _context_id: Uuid) {
        // In simulation, track registered contexts
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // In simulation, track unregistered contexts  
    }

    async fn notify_events_available(&self) {
        self.advance_time(1);
    }

    fn resolution_ms(&self) -> u64 {
        1
    }
}

/// Real time handler using actual system time
///
/// **Layer 3 (aura-effects)**: Stateless time operations only.
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
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    async fn current_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }

    async fn current_timestamp_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    async fn sleep_ms(&self, ms: u64) {
        time::sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = self.current_timestamp_millis().await;
        if epoch > current {
            let wait_time = epoch - current;
            time::sleep(Duration::from_millis(wait_time)).await;
        }
    }

    async fn delay(&self, duration: Duration) {
        time::sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        time::sleep(Duration::from_millis(duration_ms)).await;
        Ok(())
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::NewEvents => {
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::EpochReached { target } => {
                let current_time = self.current_timestamp().await;
                if target > current_time {
                    let delay = target - current_time;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(target_time) => {
                let current_time = self.current_timestamp().await;
                if target_time > current_time {
                    let delay = target_time - current_time;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                Ok(())
            }
            WakeCondition::EventMatching(_) => {
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::ThresholdEvents { threshold: _, timeout_ms } => {
                tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms)).await;
                Ok(())
            }
            WakeCondition::Immediate => Ok(()),
            WakeCondition::Custom(_) => {
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::TimeoutExpired { timeout_id: _ } => {
                tokio::task::yield_now().await;
                Ok(())
            }
        }
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| AuraError::internal(format!("Wait condition failed: {}", e)))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        // Simple stateless timeout - creates a task but doesn't track it in a registry
        // For coordinated timeout management, use TimeoutCoordinator from aura-protocol
        let handle = Uuid::new_v4();
        let _timeout_task = tokio::spawn(async move {
            time::sleep(Duration::from_millis(timeout_ms)).await;
        });
        handle
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        // Stateless handler cannot track or cancel timeouts
        // Use TimeoutCoordinator from aura-protocol for cancellation support
        Err(TimeError::TimeoutNotFound {
            handle: "Stateless handler - use TimeoutCoordinator for cancellation".to_string(),
        })
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, _context_id: Uuid) {
        // Stateless handler - no context registry
        // Use TimeoutCoordinator from aura-protocol for multi-context coordination
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Stateless handler - no context registry
        // Use TimeoutCoordinator from aura-protocol for multi-context coordination
    }

    async fn notify_events_available(&self) {
        // Stateless handler - no registered contexts to notify
        // Use TimeoutCoordinator from aura-protocol for event broadcasting
    }

    fn resolution_ms(&self) -> u64 {
        1
    }
}