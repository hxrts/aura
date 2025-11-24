//! Time control effect handler for simulation
//!
//! Provides a controllable physical clock for simulator runs. Supports simple
//! pause/resume and acceleration by scaling elapsed real time.

use async_trait::async_trait;
use aura_core::effects::time::{PhysicalTimeEffects, TimeError};
use aura_core::time::PhysicalTime;
use std::time::{Duration, Instant, SystemTime};

/// Simulation-specific time control handler
#[derive(Debug, Clone)]
pub struct SimulationTimeHandler {
    /// Base wall-clock timestamp (ms) when simulation started
    base_ms: u64,
    /// Base monotonic instant for measuring elapsed real time
    instant_base: Instant,
    /// Fixed offset to apply to simulated time
    time_offset_ms: u64,
    /// Time acceleration factor (1.0 = real time)
    acceleration: f64,
    /// Whether simulated time is paused
    paused: bool,
}

impl SimulationTimeHandler {
    /// Create a new simulation time handler
    pub fn new() -> Self {
        Self {
            base_ms: Self::now_ms(),
            instant_base: Instant::now(),
            time_offset_ms: 0,
            acceleration: 1.0,
            paused: false,
        }
    }

    /// Create with a specific starting timestamp (milliseconds since UNIX epoch)
    pub fn with_start_ms(base_ms: u64) -> Self {
        Self {
            base_ms,
            instant_base: Instant::now(),
            time_offset_ms: 0,
            acceleration: 1.0,
            paused: false,
        }
    }

    /// Set time acceleration factor
    pub fn set_acceleration(&mut self, factor: f64) {
        if factor > 0.0 {
            self.acceleration = factor;
        }
    }

    /// Pause simulated time
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Resume simulated time
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Jump to a specific simulated offset
    pub fn jump_to_time(&mut self, target_time: Duration) {
        self.time_offset_ms = target_time.as_millis() as u64;
    }

    fn simulated_elapsed_ms(&self) -> u64 {
        if self.paused {
            return 0;
        }
        let real_elapsed = self.instant_base.elapsed();
        (real_elapsed.as_millis() as f64 * self.acceleration) as u64
    }

    fn timestamp_ms(&self) -> u64 {
        self.base_ms + self.time_offset_ms + self.simulated_elapsed_ms()
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for SimulationTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PhysicalTimeEffects for SimulationTimeHandler {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        Ok(PhysicalTime {
            ts_ms: self.timestamp_ms(),
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        // Scale the sleep by acceleration to keep pace with simulated time
        let scaled = if self.acceleration > 0.0 {
            (ms as f64 / self.acceleration).max(0.0)
        } else {
            ms as f64
        };
        tokio::time::sleep(Duration::from_millis(scaled as u64)).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_time_handler_basic() {
        let handler = SimulationTimeHandler::new();
        let t1 = handler.physical_time().await.unwrap().ts_ms;
        tokio::time::sleep(Duration::from_millis(5)).await;
        let t2 = handler.physical_time().await.unwrap().ts_ms;
        assert!(t2 >= t1);
    }

    #[tokio::test]
    async fn test_acceleration() {
        let mut handler = SimulationTimeHandler::new();
        handler.set_acceleration(2.0);
        let t1 = handler.physical_time().await.unwrap().ts_ms;
        handler.sleep_ms(50).await.unwrap();
        let t2 = handler.physical_time().await.unwrap().ts_ms;
        assert!(t2 - t1 >= 50); // accelerated sleep should advance at least requested ms
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let mut handler = SimulationTimeHandler::new();
        handler.pause();
        let t1 = handler.physical_time().await.unwrap().ts_ms;
        tokio::time::sleep(Duration::from_millis(10)).await;
        let t2 = handler.physical_time().await.unwrap().ts_ms;
        assert_eq!(t1, t2);
        handler.resume();
        handler.sleep_ms(5).await.unwrap();
        let t3 = handler.physical_time().await.unwrap().ts_ms;
        assert!(t3 > t2);
    }

    #[tokio::test]
    async fn test_jump() {
        let mut handler = SimulationTimeHandler::new();
        handler.jump_to_time(Duration::from_secs(3600));
        let t = handler.physical_time().await.unwrap().ts_ms;
        assert!(t >= 3_600_000);
    }
}
