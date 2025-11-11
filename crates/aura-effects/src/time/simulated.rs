//! Simulated time effect handler for testing

use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// Simulated time handler for testing and simulation
#[derive(Debug, Clone)]
pub struct SimulatedTimeHandler {
    /// Current simulated time in milliseconds
    current_time: Arc<Mutex<u64>>,
    /// Simulated time scale (1.0 = real time, 2.0 = 2x speed, 0.5 = half speed)
    time_scale: f64,
}

impl SimulatedTimeHandler {
    /// Create a new simulated time handler starting at the given time
    pub fn new(start_time_ms: u64) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(start_time_ms)),
            time_scale: 1.0,
        }
    }

    /// Create a simulated time handler starting at Unix epoch
    pub fn new_at_epoch() -> Self {
        Self::new(0)
    }

    /// Create a simulated time handler with custom time scale
    pub fn with_time_scale(start_time_ms: u64, time_scale: f64) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(start_time_ms)),
            time_scale,
        }
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

impl Default for SimulatedTimeHandler {
    fn default() -> Self {
        Self::new_at_epoch()
    }
}

#[async_trait]
impl TimeEffects for SimulatedTimeHandler {
    async fn current_epoch(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn current_timestamp(&self) -> u64 {
        *self.current_time.lock().unwrap() / 1000 // Convert millis to seconds
    }

    async fn current_timestamp_millis(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    async fn sleep_ms(&self, ms: u64) {
        // In simulation, we can either advance time immediately or actually sleep scaled time
        let scaled_duration = Duration::from_millis((ms as f64 / self.time_scale) as u64);
        
        if self.time_scale > 100.0 {
            // For very fast simulation, just advance time immediately
            self.advance_time(ms);
        } else {
            // For realistic simulation, actually sleep the scaled duration
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
        // In simulation, we can immediately satisfy conditions or yield briefly
        if self.time_scale > 10.0 {
            // Fast simulation: advance time immediately
            self.advance_time(1);
        } else {
            tokio::task::yield_now().await;
        }
        Ok(())
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            AuraError::internal(format!("Wait condition failed: {}", e))
        })
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        // Generate a unique handle
        Uuid::new_v4()
    }

    async fn cancel_timeout(&self, _handle: TimeoutHandle) -> Result<(), TimeError> {
        // In simulation, we can always "cancel" timeouts
        Ok(())
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, _context_id: Uuid) {
        // In simulation, we can track registered contexts
    }

    fn unregister_context(&self, _context_id: Uuid) {
        // In simulation, we can track unregistered contexts
    }

    async fn notify_events_available(&self) {
        // In simulation, this would notify waiting contexts
        // For now, just advance time slightly
        self.advance_time(1);
    }

    fn resolution_ms(&self) -> u64 {
        1 // 1ms resolution in simulation
    }
}