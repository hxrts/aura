//! Mock time effect handlers for testing
//!
//! This module contains stateful time handlers that were moved from aura-effects
//! to fix architectural violations. The SimulatedTimeHandler uses Arc<Mutex<>>
//! for controllable time progression in tests.

use async_trait::async_trait;
use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Mock time handler for deterministic testing
#[derive(Debug, Clone)]
pub struct SimulatedTimeHandler {
    current_time: Arc<Mutex<u64>>,
    base_instant: Arc<Mutex<Instant>>,
    timeout_counter: Arc<Mutex<u64>>,
    active_timeouts: Arc<Mutex<HashMap<Uuid, u64>>>,
    registered_contexts: Arc<Mutex<Vec<Uuid>>>,
    time_scale: f64,
}

impl Default for SimulatedTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatedTimeHandler {
    /// Create a new simulated time handler
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            current_time: Arc::new(Mutex::new(now)),
            base_instant: Arc::new(Mutex::new(Instant::now())),
            timeout_counter: Arc::new(Mutex::new(0)),
            active_timeouts: Arc::new(Mutex::new(HashMap::new())),
            registered_contexts: Arc::new(Mutex::new(Vec::new())),
            time_scale: 1.0,
        }
    }

    /// Create a new simulated time handler with specified time and scale
    pub fn new_deterministic(start_time_ms: u64, time_scale: f64) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(start_time_ms)),
            base_instant: Arc::new(Mutex::new(Instant::now())),
            timeout_counter: Arc::new(Mutex::new(0)),
            active_timeouts: Arc::new(Mutex::new(HashMap::new())),
            registered_contexts: Arc::new(Mutex::new(Vec::new())),
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
    /// Get the current timestamp in epoch milliseconds
    async fn current_epoch(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    /// Get current timestamp in seconds
    async fn current_timestamp(&self) -> u64 {
        *self.current_time.lock().unwrap() / 1000
    }

    /// Get current timestamp in milliseconds
    async fn current_timestamp_millis(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    /// Get the current monotonic time instant
    async fn now_instant(&self) -> Instant {
        *self.base_instant.lock().unwrap()
    }

    /// Sleep for a specified number of milliseconds
    async fn sleep_ms(&self, ms: u64) {
        self.advance_time(ms);
    }

    /// Sleep until a specific epoch timestamp
    async fn sleep_until(&self, epoch: u64) {
        let current = *self.current_time.lock().unwrap();
        if epoch > current {
            self.advance_time(epoch - current);
        }
    }

    /// Delay execution for a specified duration
    async fn delay(&self, duration: Duration) {
        let duration_ms = duration.as_millis() as u64;
        self.advance_time(duration_ms);
    }

    /// Sleep for specified duration in milliseconds
    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.advance_time(duration_ms);
        Ok(())
    }

    /// Yield execution until a condition is met
    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::EpochReached { target } => {
                let current = *self.current_time.lock().unwrap();
                if current < target {
                    self.advance_time(target - current);
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(target) => {
                let current = *self.current_time.lock().unwrap();
                if current < target {
                    self.advance_time(target - current);
                }
                Ok(())
            }
            WakeCondition::TimeoutExpired { timeout_id } => {
                let timeouts = self.active_timeouts.lock().unwrap();
                if let Some(&expires_at) = timeouts.get(&timeout_id) {
                    let current = *self.current_time.lock().unwrap();
                    if current < expires_at {
                        drop(timeouts);
                        self.advance_time(expires_at - current);
                    }
                }
                Ok(())
            }
            _ => {
                // For other conditions, just return immediately in simulation
                Ok(())
            }
        }
    }

    /// Wait until a condition is met (alias for yield_until with AuraError)
    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            AuraError::Internal {
                message: format!("Simulated time operation failed: {}", e),
            }
        })
    }

    /// Set a timeout and return a handle
    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let handle = Uuid::new_v4();
        let expires_at = *self.current_time.lock().unwrap() + timeout_ms;
        
        let mut timeouts = self.active_timeouts.lock().unwrap();
        timeouts.insert(handle, expires_at);
        
        handle
    }

    /// Cancel a timeout by handle
    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut timeouts = self.active_timeouts.lock().unwrap();
        timeouts.remove(&handle);
        Ok(())
    }

    /// Check if this is a simulated time handler
    fn is_simulated(&self) -> bool {
        true
    }

    /// Register a context for time events
    fn register_context(&self, context_id: Uuid) {
        let mut contexts = self.registered_contexts.lock().unwrap();
        if !contexts.contains(&context_id) {
            contexts.push(context_id);
        }
    }

    /// Unregister a context from time events
    fn unregister_context(&self, context_id: Uuid) {
        let mut contexts = self.registered_contexts.lock().unwrap();
        contexts.retain(|&id| id != context_id);
    }

    /// Notify that events are available for waiting contexts
    async fn notify_events_available(&self) {
        // In simulation, this is a no-op as we don't have real event queues
    }

    /// Get time resolution in milliseconds
    fn resolution_ms(&self) -> u64 {
        1 // 1ms resolution for simulation
    }
}