//! Time control effect handler for simulation
//!
//! Provides a controllable physical clock for simulator runs.
//!
//! Key requirements:
//! - Must be deterministic (no direct OS clock reads)
//! - Must obey the effect system contract (PhysicalTimeEffects)
//! - Must be clippy-compliant (direct OS clock access is disallowed)

use async_trait::async_trait;
use aura_core::effects::time::{PhysicalTimeEffects, TimeError};
use aura_core::time::PhysicalTime;
use futures::future::yield_now;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
struct SimTimeState {
    base_ms: u64,
    offset_ms: u64,
    acceleration: f64,
    paused: bool,
}

/// Simulation-specific time control handler.
///
/// This handler is deterministic: time only advances when the simulator calls
/// `sleep_ms` or the explicit control APIs (`jump_to_time`).
#[derive(Debug, Clone)]
pub struct SimulationTimeHandler {
    state: Arc<Mutex<SimTimeState>>,
}

impl SimulationTimeHandler {
    /// Create a new simulation time handler starting at epoch 0.
    pub fn new() -> Self {
        Self::with_start_ms(0)
    }

    /// Create with a specific starting timestamp (milliseconds since UNIX epoch).
    pub fn with_start_ms(base_ms: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(SimTimeState {
                base_ms,
                offset_ms: 0,
                acceleration: 1.0,
                paused: false,
            })),
        }
    }

    /// Set time acceleration factor (1.0 = real time).
    ///
    /// In simulation, this scales the amount of simulated time advanced by `sleep_ms`.
    pub fn set_acceleration(&mut self, factor: f64) {
        if factor <= 0.0 {
            return;
        }
        if let Ok(mut state) = self.state.lock() {
            state.acceleration = factor;
        }
    }

    /// Pause simulated time.
    pub fn pause(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.paused = true;
        }
    }

    /// Resume simulated time.
    pub fn resume(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            state.paused = false;
        }
    }

    /// Jump to a specific simulated offset.
    pub fn jump_to_time(&mut self, target_time: Duration) {
        if let Ok(mut state) = self.state.lock() {
            state.offset_ms = target_time.as_millis() as u64;
        }
    }

    fn timestamp_ms(&self) -> u64 {
        let Ok(state) = self.state.lock() else {
            return 0;
        };
        state.base_ms.saturating_add(state.offset_ms)
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
        // Advance simulated time deterministically without waiting on OS time.
        if let Ok(mut state) = self.state.lock() {
            if !state.paused {
                let scaled = (ms as f64 * state.acceleration).max(0.0);
                state.offset_ms = state.offset_ms.saturating_add(scaled as u64);
            }
        }

        // Yield so callers that expect cooperative scheduling still progress.
        yield_now().await;
        Ok(())
    }
}
