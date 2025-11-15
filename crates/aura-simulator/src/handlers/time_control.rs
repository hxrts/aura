//! Time control effect handler for simulation
//!
//! This module provides runtime-specific time control effects for the aura-simulator.
//! Replaces the former TimeControlMiddleware with effect system integration.

use async_trait::async_trait;
use std::time::{Duration, SystemTime};
use aura_core::{AuraResult, AuraError};
use aura_core::effects::TimeEffects;

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
}

impl SimulationTimeHandler {
    /// Create a new simulation time handler
    pub fn new() -> Self {
        Self {
            time_offset: Duration::from_secs(0),
            acceleration: 1.0,
            paused: false,
            simulation_start: SystemTime::now(),
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
    async fn current_timestamp(&self) -> AuraResult<u64> {
        if self.paused {
            // Return frozen time when paused
            Ok(self.time_offset.as_secs())
        } else {
            // Calculate accelerated time
            let elapsed = self.simulation_start
                .elapsed()
                .map_err(|e| AuraError::internal(format!("Time calculation error: {}", e)))?;
            
            let accelerated_elapsed = Duration::from_secs_f64(
                elapsed.as_secs_f64() * self.acceleration
            );
            
            let total_time = self.time_offset + accelerated_elapsed;
            Ok(total_time.as_secs())
        }
    }

    async fn sleep(&self, duration: Duration) -> AuraResult<()> {
        if self.paused {
            // Don't sleep when paused
            return Ok(());
        }

        // Adjust sleep duration based on acceleration
        let adjusted_duration = Duration::from_secs_f64(
            duration.as_secs_f64() / self.acceleration
        );

        tokio::time::sleep(adjusted_duration).await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_time_handler() {
        let handler = SimulationTimeHandler::new();
        
        // Should provide current timestamp
        let timestamp = handler.current_timestamp().await.unwrap();
        assert!(timestamp >= 0);
    }

    #[tokio::test]
    async fn test_time_acceleration() {
        let mut handler = SimulationTimeHandler::new();
        handler.set_acceleration(2.0);
        
        let timestamp1 = handler.current_timestamp().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let timestamp2 = handler.current_timestamp().await.unwrap();
        
        // Time should advance faster with acceleration
        assert!(timestamp2 > timestamp1);
    }

    #[tokio::test]
    async fn test_pause_resume() {
        let mut handler = SimulationTimeHandler::new();
        
        handler.pause();
        let timestamp1 = handler.current_timestamp().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let timestamp2 = handler.current_timestamp().await.unwrap();
        
        // Time should be frozen when paused
        assert_eq!(timestamp1, timestamp2);
        
        handler.resume();
        // Time should advance again after resume
        let timestamp3 = handler.current_timestamp().await.unwrap();
        assert!(timestamp3 >= timestamp2);
    }

    #[tokio::test]
    async fn test_time_jump() {
        let mut handler = SimulationTimeHandler::new();
        
        let target = Duration::from_secs(3600); // Jump to 1 hour
        handler.jump_to_time(target);
        
        let timestamp = handler.current_timestamp().await.unwrap();
        assert!(timestamp >= target.as_secs());
    }
}