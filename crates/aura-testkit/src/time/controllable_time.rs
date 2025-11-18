use aura_core::{
    effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition},
    AuraError,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use uuid::Uuid;

/// Controllable time source for deterministic testing
#[derive(Clone)]
pub struct ControllableTimeSource {
    current_time: Arc<Mutex<u64>>,
    time_scale: Arc<Mutex<f64>>,
    frozen: Arc<Mutex<bool>>,
    timeouts: Arc<Mutex<HashMap<Uuid, u64>>>, // timeout_id -> expiry_time
    contexts: Arc<Mutex<HashMap<Uuid, bool>>>, // context_id -> active
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
        }
    }

    /// Create controllable time source starting at current system time
    pub fn now() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Self::new(now)
    }

    /// Advance time by given number of seconds
    pub fn advance_time(&self, seconds: u64) {
        let mut current = self.current_time.lock().unwrap();
        *current += seconds;
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
impl TimeEffects for ControllableTimeSource {
    async fn current_epoch(&self) -> u64 {
        let current = self.current_time.lock().unwrap();
        *current
    }

    async fn current_timestamp(&self) -> u64 {
        let current = self.current_time.lock().unwrap();
        *current
    }

    async fn current_timestamp_millis(&self) -> u64 {
        let current = self.current_time.lock().unwrap();
        *current * 1000
    }

    async fn now_instant(&self) -> std::time::Instant {
        // For controllable time, we use a fixed instant based on current timestamp
        let base = std::time::Instant::now();
        let current = self.current_timestamp();
        base + Duration::from_secs(current)
    }

    async fn sleep_ms(&self, ms: u64) {
        // In controllable time, sleep just advances time if not frozen
        if !self.is_frozen() {
            let seconds = ms / 1000;
            if seconds > 0 {
                self.advance_time(seconds);
            }
        }
        // Don't actually sleep in tests - return immediately
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = self.current_timestamp();
        if epoch > current && !self.is_frozen() {
            let wait_time = epoch - current;
            self.advance_time(wait_time);
        }
    }

    async fn delay(&self, duration: Duration) {
        let ms = duration.as_millis() as u64;
        self.sleep_ms(ms).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::NewEvents => {
                // In test mode, assume events are always available
                Ok(())
            }
            WakeCondition::EpochReached { target } => {
                let current = self.current_timestamp() * 1000; // convert to millis
                if target > current && !self.is_frozen() {
                    let wait_time = (target - current) / 1000; // convert back to seconds
                    self.advance_time(wait_time);
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(timestamp) => {
                self.sleep_until(timestamp).await;
                Ok(())
            }
            WakeCondition::TimeoutExpired { timeout_id } => {
                // Check if timeout exists and is expired
                let timeouts = self.timeouts.lock().unwrap();
                let current = self.current_timestamp();
                if let Some(&expiry) = timeouts.get(&timeout_id) {
                    if current >= expiry {
                        Ok(())
                    } else {
                        Err(TimeError::TimeoutNotFound {
                            handle: timeout_id.to_string(),
                        })
                    }
                } else {
                    Err(TimeError::TimeoutNotFound {
                        handle: timeout_id.to_string(),
                    })
                }
            }
            WakeCondition::Immediate => Ok(()),
            WakeCondition::Custom(_) => Ok(()),
            WakeCondition::EventMatching(_) => Ok(()),
            WakeCondition::ThresholdEvents { timeout_ms, .. } => {
                // Simulate waiting for the timeout
                self.sleep_ms(timeout_ms).await;
                Ok(())
            }
        }
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition)
            .await
            .map_err(|e| AuraError::internal(format!("Time operation failed: {}", e)))
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let timeout_id = Uuid::new_v4();
        let current = self.current_timestamp();
        let expiry_time = current + (timeout_ms / 1000); // convert to seconds

        let mut timeouts = self.timeouts.lock().unwrap();
        timeouts.insert(timeout_id, expiry_time);

        timeout_id
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut timeouts = self.timeouts.lock().unwrap();
        if timeouts.remove(&handle).is_some() {
            Ok(())
        } else {
            Err(TimeError::TimeoutNotFound {
                handle: handle.to_string(),
            })
        }
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, context_id: Uuid) {
        let mut contexts = self.contexts.lock().unwrap();
        contexts.insert(context_id, true);
    }

    fn unregister_context(&self, context_id: Uuid) {
        let mut contexts = self.contexts.lock().unwrap();
        contexts.remove(&context_id);
    }

    async fn notify_events_available(&self) {
        // In test mode, this is a no-op since we control time directly
    }

    fn resolution_ms(&self) -> u64 {
        1000 // 1 second resolution for tests
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
    SetScale(f64),
}

impl TimeScenarioBuilder {
    /// Create new scenario starting at given time
    pub fn new(start_time: u64) -> Self {
        Self {
            source: ControllableTimeSource::new(start_time),
            events: Vec::new(),
        }
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

    /// Build the scenario and get the controllable time source
    pub fn build(mut self) -> ControllableTimeSource {
        // Sort events by time
        self.events.sort_by_key(|e| e.at_time);

        // Execute events in order
        for event in self.events {
            self.source.set_time(event.at_time);
            match event.action {
                TimeAction::AdvanceBy(seconds) => self.source.advance_time(seconds),
                TimeAction::SetTime(time) => self.source.set_time(time),
                TimeAction::Freeze => self.source.freeze(),
                TimeAction::Unfreeze => self.source.unfreeze(),
                TimeAction::SetScale(scale) => self.source.set_time_scale(scale),
            }
        }

        self.source
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
        time_source.sleep_ms(5000).await;
        assert_eq!(time_source.current_timestamp(), 1005);

        // Frozen sleep should not advance time
        time_source.freeze();
        time_source.sleep_ms(5000).await;
        assert_eq!(time_source.current_timestamp(), 1005);
    }

    #[tokio::test]
    async fn test_scenario_builder() {
        let time_source = TimeScenarioBuilder::new(1000)
            .advance_at(1000, 100)
            .freeze_at(1100)
            .set_time_at(1100, 2000)
            .build();

        assert_eq!(time_source.current_timestamp(), 2000);
        assert!(time_source.is_frozen());
    }

    #[tokio::test]
    async fn test_sleep_until() {
        let time_source = ControllableTimeSource::new(1000);

        time_source.sleep_until(1500).await;
        assert_eq!(time_source.current_timestamp(), 1500);

        // Sleep until past time should not change current time
        time_source.sleep_until(1200).await;
        assert_eq!(time_source.current_timestamp(), 1500);
    }
}
