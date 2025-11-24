//! Mock time effect handlers for testing
//!
//! This module contains stateful time handlers that were moved from aura-effects
//! to fix architectural violations. The SimulatedTimeHandler uses Arc<Mutex<>>
//! for controllable time progression in tests.

use async_trait::async_trait;
use aura_core::effects::{
    time::TimeComparison, LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeError,
};
use aura_core::time::{LogicalTime, OrderTime, TimeStamp, VectorClock};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
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
impl PhysicalTimeEffects for SimulatedTimeHandler {
    async fn physical_time(&self) -> Result<aura_core::time::PhysicalTime, TimeError> {
        Ok(aura_core::time::PhysicalTime {
            ts_ms: *self.current_time.lock().unwrap(),
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        let scaled_ms = (ms as f64 * self.time_scale) as u64;
        self.advance_time(scaled_ms);
        Ok(())
    }
}

#[async_trait]
impl LogicalClockEffects for SimulatedTimeHandler {
    async fn logical_advance(
        &self,
        observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let mut clock = VectorClock::new();
        if let Some(obs) = observed {
            for (actor, val) in obs.iter() {
                let current = clock.get(actor).copied().unwrap_or(0);
                clock.insert(*actor, current.max(*val));
            }
        }
        Ok(LogicalTime {
            vector: clock,
            lamport: 0,
        })
    }

    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        Ok(LogicalTime {
            vector: VectorClock::new(),
            lamport: 0,
        })
    }
}

#[async_trait]
impl OrderClockEffects for SimulatedTimeHandler {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        Ok(OrderTime([0u8; 32]))
    }
}

#[async_trait]
impl TimeComparison for SimulatedTimeHandler {
    async fn compare(
        &self,
        a: &TimeStamp,
        b: &TimeStamp,
    ) -> Result<aura_core::time::TimeOrdering, TimeError> {
        Ok(a.compare(b, aura_core::time::OrderingPolicy::Native))
    }
}
