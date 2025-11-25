use aura_core::effects::time::{
    LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects, TimeError,
};
use aura_core::time::{LogicalTime, OrderTime, PhysicalTime, VectorClock};
use aura_core::DeviceId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Controllable time source for deterministic testing (PhysicalTimeEffects)
#[derive(Clone)]
pub struct ControllableTimeSource {
    current_time: Arc<Mutex<u64>>,
    time_scale: Arc<Mutex<f64>>,
    frozen: Arc<Mutex<bool>>,
    #[allow(dead_code)]
    timeouts: Arc<Mutex<HashMap<Uuid, u64>>>, // timeout_id -> expiry_time
    #[allow(dead_code)]
    contexts: Arc<Mutex<HashMap<Uuid, bool>>>, // context_id -> active
    logical_clock: Arc<Mutex<VectorClock>>,
    order_counter: Arc<Mutex<u64>>, // for deterministic order generation
}

impl ControllableTimeSource {
    /// Create new controllable time source starting at given timestamp
    pub fn new(initial_timestamp: u64) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(initial_timestamp)),
            time_scale: Arc::new(Mutex::new(1.0)),
            frozen: Arc::new(Mutex::new(false)),
            timeouts: Arc::new(Mutex::new(HashMap::new())),
            contexts: Arc::new(Mutex::new(HashMap::new())),
            logical_clock: Arc::new(Mutex::new(VectorClock::new())),
            order_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Create controllable time source starting at current system time
    pub fn now() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        Self::new(now)
    }

    /// Advance time by given number of milliseconds
    pub fn advance_time(&self, millis: u64) {
        let mut current = self.current_time.lock().unwrap();
        *current += millis;
    }

    /// Set absolute time
    pub fn set_time(&self, timestamp: u64) {
        let mut current = self.current_time.lock().unwrap();
        *current = timestamp;
    }

    /// Freeze time (no automatic advancement)
    pub fn freeze(&self) {
        let mut frozen = self.frozen.lock().unwrap();
        *frozen = true;
    }

    /// Unfreeze time
    pub fn unfreeze(&self) {
        let mut frozen = self.frozen.lock().unwrap();
        *frozen = false;
    }

    /// Set time scale (1.0 = normal, 2.0 = double speed, 0.5 = half speed)
    pub fn set_time_scale(&self, scale: f64) {
        let mut time_scale = self.time_scale.lock().unwrap();
        *time_scale = scale;
    }

    /// Get current time
    pub fn current_timestamp(&self) -> u64 {
        let current = self.current_time.lock().unwrap();
        *current
    }

    /// Check if time is frozen
    pub fn is_frozen(&self) -> bool {
        let frozen = self.frozen.lock().unwrap();
        *frozen
    }
}

#[async_trait::async_trait]
impl PhysicalTimeEffects for ControllableTimeSource {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        Ok(PhysicalTime {
            ts_ms: *self.current_time.lock().unwrap(),
            uncertainty: None,
        })
    }

    async fn sleep_ms(&self, ms: u64) -> Result<(), TimeError> {
        if self.is_frozen() {
            return Ok(());
        }
        let scale = *self.time_scale.lock().unwrap();
        let scaled = (ms as f64 * scale).round() as u64;
        self.advance_time(scaled);
        Ok(())
    }
}

#[async_trait::async_trait]
impl LogicalClockEffects for ControllableTimeSource {
    async fn logical_now(&self) -> Result<LogicalTime, TimeError> {
        let clock = self.logical_clock.lock().unwrap();
        let lamport = clock.iter().map(|(_, counter)| *counter).max().unwrap_or(0);
        Ok(LogicalTime {
            vector: clock.clone(),
            lamport,
        })
    }

    async fn logical_advance(
        &self,
        observed: Option<&VectorClock>,
    ) -> Result<LogicalTime, TimeError> {
        let mut clock = self.logical_clock.lock().unwrap();

        // Merge observed clocks if provided
        if let Some(obs) = observed {
            for (device, counter) in obs.iter() {
                let current = clock.get(device).copied().unwrap_or(0);
                clock.insert(*device, current.max(*counter));
            }
        }

        // Increment local counter using placeholder device id for tests
        let test_device = DeviceId::new(); // Create new device ID for testing
        let current = clock.get(&test_device).copied().unwrap_or(0);
        clock.insert(test_device, current + 1);
        let lamport = clock.iter().map(|(_, counter)| *counter).max().unwrap_or(0);

        Ok(LogicalTime {
            vector: clock.clone(),
            lamport,
        })
    }
}

#[async_trait::async_trait]
impl OrderClockEffects for ControllableTimeSource {
    async fn order_time(&self) -> Result<OrderTime, TimeError> {
        let mut counter = self.order_counter.lock().unwrap();
        *counter += 1;

        // Create deterministic order token from counter and current time
        let mut token = [0u8; 32];
        let counter_bytes = counter.to_be_bytes();
        let time_bytes = self.current_time.lock().unwrap().to_be_bytes();

        // Mix counter and time for deterministic but unique tokens
        token[..8].copy_from_slice(&counter_bytes);
        token[8..16].copy_from_slice(&time_bytes);

        // Fill remaining bytes with deterministic pattern
        for (i, item) in token.iter_mut().enumerate().skip(16) {
            *item = ((i as u64 + *counter + *self.current_time.lock().unwrap()) % 256) as u8;
        }

        Ok(OrderTime(token))
    }
}

/// Builder for creating test scenarios with controlled time
pub struct TimeScenarioBuilder {
    source: ControllableTimeSource,
    events: Vec<TimeEvent>,
}

#[derive(Debug)]
struct TimeEvent {
    at_time: u64,
    action: TimeAction,
}

#[derive(Debug)]
enum TimeAction {
    AdvanceBy(u64),
    SetTime(u64),
    Freeze,
    Unfreeze,
    #[allow(dead_code)]
    SetScale(f64),
}

impl Default for TimeScenarioBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl TimeScenarioBuilder {
    /// Create new scenario starting at given time
    pub fn new() -> Self {
        Self {
            source: ControllableTimeSource::new(0),
            events: Vec::new(),
        }
    }

    /// Set initial time for the scenario
    pub fn with_initial_time(self, start_time: u64) -> Self {
        self.source.set_time(start_time);
        self
    }

    /// Configure devices for logical clock testing (placeholder)
    pub fn with_devices(self, _devices: &[DeviceId]) -> Self {
        // In practice, would configure device-specific logical clocks
        self
    }

    /// Configure time skew tolerance for testing
    pub fn with_time_skew(self, _skew_ms: u64) -> Self {
        // In practice, would configure uncertainty ranges
        self
    }

    /// Add time advancement event
    pub fn advance_at(mut self, at_time: u64, advance_by: u64) -> Self {
        self.events.push(TimeEvent {
            at_time,
            action: TimeAction::AdvanceBy(advance_by),
        });
        self
    }

    /// Add absolute time setting event
    pub fn set_time_at(mut self, at_time: u64, new_time: u64) -> Self {
        self.events.push(TimeEvent {
            at_time,
            action: TimeAction::SetTime(new_time),
        });
        self
    }

    /// Add freeze event
    pub fn freeze_at(mut self, at_time: u64) -> Self {
        self.events.push(TimeEvent {
            at_time,
            action: TimeAction::Freeze,
        });
        self
    }

    /// Add unfreeze event
    pub fn unfreeze_at(mut self, at_time: u64) -> Self {
        self.events.push(TimeEvent {
            at_time,
            action: TimeAction::Unfreeze,
        });
        self
    }

    /// Build the scenario and get the time scenario wrapper
    pub fn build(mut self) -> TimeScenario {
        // Sort events by time
        self.events.sort_by_key(|e| e.at_time);

        // Execute events in order
        for event in self.events {
            self.source.set_time(event.at_time);
            match event.action {
                TimeAction::AdvanceBy(millis) => self.source.advance_time(millis),
                TimeAction::SetTime(time) => self.source.set_time(time),
                TimeAction::Freeze => self.source.freeze(),
                TimeAction::Unfreeze => self.source.unfreeze(),
                TimeAction::SetScale(scale) => self.source.set_time_scale(scale),
            }
        }

        TimeScenario {
            time_source: self.source,
        }
    }
}

/// Time scenario wrapper for test scenarios
pub struct TimeScenario {
    time_source: ControllableTimeSource,
}

impl TimeScenario {
    /// Get the underlying time source
    pub fn time_source(&self) -> &ControllableTimeSource {
        &self.time_source
    }

    /// Advance all clocks by the given amount
    pub fn advance_all_clocks(&self, millis: u64) {
        self.time_source.advance_time(millis);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_controllable_time_advancement() {
        let time_source = ControllableTimeSource::new(1000);

        assert_eq!(time_source.current_timestamp(), 1000);

        time_source.advance_time(100);
        assert_eq!(time_source.current_timestamp(), 1100);

        time_source.set_time(2000);
        assert_eq!(time_source.current_timestamp(), 2000);
    }

    #[tokio::test]
    async fn test_freeze_and_sleep() {
        let time_source = ControllableTimeSource::new(1000);

        // Normal sleep should advance time
        let _ = time_source.sleep_ms(5000).await;
        assert_eq!(time_source.current_timestamp(), 6000);

        // Frozen sleep should not advance time
        time_source.freeze();
        let _ = time_source.sleep_ms(5000).await;
        assert_eq!(time_source.current_timestamp(), 6000);
    }

    #[tokio::test]
    async fn test_scenario_builder() {
        let scenario = TimeScenarioBuilder::new()
            .with_initial_time(1000)
            .advance_at(1000, 100)
            .freeze_at(1100)
            .set_time_at(1100, 2000)
            .build();
        let time_source = scenario.time_source();

        assert_eq!(time_source.current_timestamp(), 2000);
        assert!(time_source.is_frozen());
    }

    #[tokio::test]
    async fn test_time_effects_traits() {
        let time_source = ControllableTimeSource::new(1000);

        // Test PhysicalTimeEffects trait
        let physical_time = time_source.physical_time().await.unwrap();
        assert_eq!(physical_time.ts_ms, 1000);

        // Test LogicalClockEffects trait
        let logical_time = time_source.logical_now().await.unwrap();
        assert_eq!(logical_time.lamport, 0);

        // Test OrderClockEffects trait
        let order_time = time_source.order_time().await.unwrap();
        assert_eq!(order_time.0.len(), 32); // 32-byte order token
    }
}
