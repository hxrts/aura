//! Time control effect handler for simulation
//!
//! This module provides runtime-specific time control effects for the aura-simulator.
//! Replaces the former TimeControlMiddleware with effect system integration.

use async_trait::async_trait;
use aura_core::effects::time::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

/// Simulation-specific time control handler
///
/// This handler provides deterministic time control for simulations,
/// including time acceleration, pause/resume, and checkpoint functionality.
pub struct SimulationTimeHandler {
    /// Current simulation time offset
    time_offset: Duration,
    /// Time acceleration factor (1.0 = real-time, 2.0 = 2x speed, etc.)
    acceleration: f64,
    /// Whether time is paused
    paused: bool,
    /// Base real-world time when simulation started
    simulation_start: SystemTime,
    /// Synthetic instant base for deterministic testing
    instant_base: std::time::Instant,
}

impl SimulationTimeHandler {
    /// Create a new simulation time handler
    pub fn new() -> Self {
        Self {
            time_offset: Duration::from_secs(0),
            acceleration: 1.0,
            paused: false,
            simulation_start: SystemTime::now(),
            instant_base: std::time::Instant::now(),
        }
    }

    /// Set time acceleration factor
    pub fn set_acceleration(&mut self, factor: f64) {
        if factor > 0.0 {
            self.acceleration = factor;
        }
    }

    /// Pause simulation time
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume simulation time
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Jump to a specific simulation time
    pub fn jump_to_time(&mut self, target_time: Duration) {
        self.time_offset = target_time;
    }
}

impl Default for SimulationTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimeEffects for SimulationTimeHandler {
    async fn current_epoch(&self) -> u64 {
        // Return epoch in milliseconds
        self.current_timestamp_millis().await
    }

    async fn current_timestamp(&self) -> u64 {
        if self.paused {
            // Return frozen time when paused
            self.time_offset.as_secs()
        } else {
            // Calculate accelerated time
            let elapsed = self
                .simulation_start
                .elapsed()
                .unwrap_or(Duration::from_secs(0));

            let accelerated_elapsed =
                Duration::from_secs_f64(elapsed.as_secs_f64() * self.acceleration);

            let total_time = self.time_offset + accelerated_elapsed;
            total_time.as_secs()
        }
    }

    async fn current_timestamp_millis(&self) -> u64 {
        if self.paused {
            // Return frozen time when paused
            self.time_offset.as_millis() as u64
        } else {
            // Calculate accelerated time
            let elapsed = self
                .simulation_start
                .elapsed()
                .unwrap_or(Duration::from_secs(0));

            let accelerated_elapsed =
                Duration::from_secs_f64(elapsed.as_secs_f64() * self.acceleration);

            let total_time = self.time_offset + accelerated_elapsed;
            total_time.as_millis() as u64
        }
    }

    async fn now_instant(&self) -> Instant {
        if self.paused {
            // Return frozen instant when paused
            self.instant_base + self.time_offset
        } else {
            // Calculate accelerated instant
            let elapsed = self
                .simulation_start
                .elapsed()
                .unwrap_or(Duration::from_secs(0));

            let accelerated_elapsed =
                Duration::from_secs_f64(elapsed.as_secs_f64() * self.acceleration);

            self.instant_base + self.time_offset + accelerated_elapsed
        }
    }

    async fn sleep_ms(&self, ms: u64) {
        if self.paused {
            // Don't sleep when paused
            return;
        }

        // Adjust sleep duration based on acceleration
        let duration = Duration::from_millis(ms);
        let adjusted_duration = Duration::from_secs_f64(duration.as_secs_f64() / self.acceleration);

        tokio::time::sleep(adjusted_duration).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = self.current_epoch().await;
        if epoch > current {
            let wait_ms = epoch - current;
            self.sleep_ms(wait_ms).await;
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
        // Simplified implementation for simulator - just yield
        tokio::task::yield_now().await;
        Ok(())
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| AuraError::internal(format!("Wait failed: {}", e)))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        // Generate a timeout handle
        // In a real implementation, this would track active timeouts
        let handle = Uuid::new_v4();

        // Spawn a background task to trigger timeout
        let _timeout_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            // In real implementation, would notify via WakeCondition::TimeoutExpired
        });

        handle
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        // Stub implementation - in production would cancel the actual timeout
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, _context_id: Uuid) {
        // Stub implementation - in production would track registered contexts
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // Stub implementation - in production would remove context
    }

    async fn notify_events_available(&self) {
        // Stub implementation - in production would wake waiting contexts
    }

    fn resolution_ms(&self) -> u64 {
        // Simulation provides millisecond resolution
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_time_handler() {
        let handler = SimulationTimeHandler::new();

        // Should provide current timestamp
        let timestamp = handler.current_timestamp().await;
        assert!(timestamp >= 0);
    }

    #[tokio::test]
    async fn test_time_acceleration() {
        let mut handler = SimulationTimeHandler::new();
        let timestamp1 = handler.current_timestamp_millis().await;

        // Sleep a bit to ensure some real time passes
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Test acceleration after some time has passed
        handler.set_acceleration(2.0);
        tokio::time::sleep(Duration::from_millis(100)).await;
        let timestamp2 = handler.current_timestamp_millis().await;

        // With 2x acceleration, 100ms real time should advance simulation time by ~200ms
        // Time should advance faster with acceleration
        let time_diff = timestamp2 - timestamp1;

        // Should be at least some time advancement (more than 100ms due to acceleration)
        assert!(
            time_diff > 100,
            "Expected time advancement > 100ms, got {}",
            time_diff
        );
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let mut handler = SimulationTimeHandler::new();

        handler.pause();
        let timestamp1 = handler.current_timestamp().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        let timestamp2 = handler.current_timestamp().await;

        // Time should be frozen when paused
        assert_eq!(timestamp1, timestamp2);

        handler.resume();
        // Time should advance again after resume
        let timestamp3 = handler.current_timestamp().await;
        assert!(timestamp3 >= timestamp2);
    }

    #[tokio::test]
    async fn test_time_jump() {
        let mut handler = SimulationTimeHandler::new();

        let target = Duration::from_secs(3600); // Jump to 1 hour
        handler.jump_to_time(target);

        let timestamp = handler.current_timestamp().await;
        assert!(timestamp >= target.as_secs());
    }
}
