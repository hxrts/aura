//! Time control middleware for managing simulation time and temporal operations

use super::{
    Result, SimulatorContext, SimulatorError, SimulatorHandler, SimulatorMiddleware,
    SimulatorOperation, TimeControlAction,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Middleware for controlling simulation time flow and temporal operations
pub struct TimeControlMiddleware {
    /// Shared state with interior mutability
    state: Arc<Mutex<TimeControlState>>,
}

/// Internal state for time control with thread-safe access
#[derive(Debug)]
struct TimeControlState {
    /// Current time acceleration factor
    acceleration_factor: f64,
    /// Whether time is currently paused
    is_paused: bool,
    /// Time checkpoints for restoration
    checkpoints: HashMap<String, TimeCheckpoint>,
    /// Time travel debugging state
    time_travel_state: Option<TimeTravelState>,
    /// Minimum allowed acceleration factor
    min_acceleration: f64,
    /// Maximum allowed acceleration factor
    max_acceleration: f64,
    /// Enable precise timing control
    precise_timing: bool,
    /// Real-time synchronization settings
    realtime_sync: RealtimeSync,
    /// Current simulation time
    current_time: Duration,
    /// Time when simulation was last updated
    last_update: Instant,
}

impl TimeControlMiddleware {
    /// Create new time control middleware
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TimeControlState {
                acceleration_factor: 1.0,
                is_paused: false,
                checkpoints: HashMap::new(),
                time_travel_state: None,
                min_acceleration: 0.1,
                max_acceleration: 100.0,
                precise_timing: false,
                realtime_sync: RealtimeSync::default(),
                current_time: Duration::from_secs(0),
                last_update: Instant::now(),
            })),
        }
    }

    /// Set acceleration factor bounds
    pub fn with_acceleration_bounds(self, min: f64, max: f64) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.min_acceleration = min.max(0.01); // Minimum sensible value
            state.max_acceleration = max.min(1000.0); // Maximum sensible value
        }
        self
    }

    /// Enable precise timing control
    pub fn with_precise_timing(self, enable: bool) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.precise_timing = enable;
        }
        self
    }

    /// Configure real-time synchronization
    pub fn with_realtime_sync(self, sync: RealtimeSync) -> Self {
        if let Ok(mut state) = self.state.lock() {
            state.realtime_sync = sync;
        }
        self
    }

    /// Enable time travel debugging
    pub fn with_time_travel(self, enable: bool) -> Self {
        if let Ok(mut state) = self.state.lock() {
            if enable {
                state.time_travel_state = Some(TimeTravelState::new());
            } else {
                state.time_travel_state = None;
            }
        }
        self
    }

    /// Control simulation time
    fn control_time(
        &self,
        action: TimeControlAction,
        parameters: &HashMap<String, Value>,
        context: &SimulatorContext,
    ) -> Result<Value> {
        if let Ok(mut state) = self.state.lock() {
            match action {
                TimeControlAction::Pause => {
                    state.is_paused = true;
                    Ok(json!({
                        "action": "pause",
                        "timestamp": context.timestamp.as_millis(),
                        "tick": context.tick,
                        "status": "paused"
                    }))
                }

                TimeControlAction::Resume => {
                    state.is_paused = false;
                    Ok(json!({
                        "action": "resume",
                        "timestamp": context.timestamp.as_millis(),
                        "tick": context.tick,
                        "status": "resumed"
                    }))
                }

                TimeControlAction::SetAcceleration { factor } => {
                    let clamped_factor = factor.clamp(state.min_acceleration, state.max_acceleration);
                    state.acceleration_factor = clamped_factor;

                    Ok(json!({
                        "action": "set_acceleration",
                        "requested_factor": factor,
                        "actual_factor": clamped_factor,
                        "timestamp": context.timestamp.as_millis(),
                        "status": "set"
                    }))
                }

                TimeControlAction::JumpTo { timestamp } => {
                    let old_time = state.current_time;
                    state.current_time = timestamp;
                    state.last_update = Instant::now();
                    
                    Ok(json!({
                        "action": "jump_to",
                        "target_timestamp": timestamp.as_millis(),
                        "previous_timestamp": old_time.as_millis(),
                        "status": "jumped",
                        "time_delta": (timestamp.as_millis() as i64) - (old_time.as_millis() as i64)
                    }))
                }

                TimeControlAction::Checkpoint { id } => {
                    let checkpoint = TimeCheckpoint {
                        id: id.clone(),
                        timestamp: context.timestamp,
                        tick: context.tick,
                        acceleration_factor: state.acceleration_factor,
                        created_at: Instant::now(),
                        metadata: parameters.clone(),
                    };

                    state.checkpoints.insert(id.clone(), checkpoint);

                    Ok(json!({
                        "action": "checkpoint",
                        "checkpoint_id": id,
                        "timestamp": context.timestamp.as_millis(),
                        "tick": context.tick,
                        "status": "created"
                    }))
                }

                TimeControlAction::Restore { id } => {
                    if let Some(checkpoint) = state.checkpoints.get(&id) {
                        let checkpoint_clone = checkpoint.clone();
                        state.acceleration_factor = checkpoint_clone.acceleration_factor;

                        Ok(json!({
                            "action": "restore",
                            "checkpoint_id": id,
                            "restored_timestamp": checkpoint_clone.timestamp.as_millis(),
                            "restored_tick": checkpoint_clone.tick,
                            "current_timestamp": context.timestamp.as_millis(),
                            "status": "restored"
                        }))
                    } else {
                        Err(SimulatorError::TimeControlError(format!(
                            "Checkpoint not found: {}",
                            id
                        )))
                    }
                }
            }
        } else {
            Err(SimulatorError::OperationFailed("Failed to acquire time control lock".to_string()))
        }
    }

    /// Calculate adjusted delta time based on acceleration
    fn calculate_adjusted_delta(&self, original_delta: Duration) -> Duration {
        if let Ok(state) = self.state.lock() {
            if state.is_paused {
                Duration::from_millis(0)
            } else {
                let adjusted_millis =
                    (original_delta.as_millis() as f64 * state.acceleration_factor) as u64;
                Duration::from_millis(adjusted_millis)
            }
        } else {
            original_delta // Fallback to original if lock fails
        }
    }

    /// Check if operation should be processed (not paused)
    fn should_process_operation(&self, operation: &SimulatorOperation) -> bool {
        if let Ok(state) = self.state.lock() {
            if !state.is_paused {
                return true;
            }
        } else {
            return true; // Allow processing if lock fails
        }

        // Always allow time control operations when paused
        matches!(operation, SimulatorOperation::ControlTime { .. })
    }

    /// Update time travel state if enabled
    fn update_time_travel_state(
        &self,
        operation: &SimulatorOperation,
        context: &SimulatorContext,
    ) {
        if let Ok(mut state) = self.state.lock() {
            // Extract the acceleration_factor before borrowing time_travel mutably
            let acceleration_factor = state.acceleration_factor;
            if let Some(ref mut time_travel) = state.time_travel_state {
                let entry = TimeTravelEntry {
                    tick: context.tick,
                    timestamp: context.timestamp,
                    operation: format!("{:?}", operation),
                    acceleration_factor,
                    recorded_at: Instant::now(),
                };

                time_travel.add_entry(entry);
            }
        }
    }

    /// Get time travel history
    fn get_time_travel_history(&self) -> Value {
        if let Ok(state) = self.state.lock() {
            if let Some(ref time_travel) = state.time_travel_state {
                json!({
                    "enabled": true,
                    "entry_count": time_travel.entries.len(),
                    "max_entries": time_travel.max_entries,
                    "oldest_entry": time_travel.entries.front().map(|e| json!({
                        "tick": e.tick,
                        "timestamp": e.timestamp.as_millis()
                    })),
                    "newest_entry": time_travel.entries.back().map(|e| json!({
                        "tick": e.tick,
                        "timestamp": e.timestamp.as_millis()
                    }))
                })
            } else {
                json!({
                    "enabled": false
                })
            }
        } else {
            json!({
                "enabled": false,
                "error": "Failed to acquire state lock"
            })
        }
    }

    /// Perform real-time synchronization if enabled
    fn sync_realtime(&self, context: &SimulatorContext) -> Result<Value> {
        if let Ok(state) = self.state.lock() {
            if !state.realtime_sync.enabled {
                return Ok(json!({"realtime_sync": "disabled"}));
            }

            let expected_real_time = Duration::from_millis(
                (context.timestamp.as_millis() as f64 / state.acceleration_factor) as u64,
            );

            Ok(json!({
                "realtime_sync": {
                    "enabled": true,
                    "simulation_time": context.timestamp.as_millis(),
                    "expected_real_time": expected_real_time.as_millis(),
                    "acceleration_factor": state.acceleration_factor,
                    "sync_tolerance": state.realtime_sync.tolerance.as_millis()
                }
            }))
        } else {
            Ok(json!({
                "realtime_sync": {
                    "enabled": false,
                    "error": "Failed to acquire state lock"
                }
            }))
        }
    }
}

impl Default for TimeControlMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl SimulatorMiddleware for TimeControlMiddleware {
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value> {
        // Check if operation should be processed
        if !self.should_process_operation(&operation) {
            return Ok(json!({
                "status": "paused",
                "message": "Simulation is paused",
                "timestamp": context.timestamp.as_millis()
            }));
        }

        match &operation {
            SimulatorOperation::ControlTime { action, parameters } => {
                // Handle time control operations with real implementation
                let control_result = self.control_time(action.clone(), parameters, context)?;

                // Add time control info to context
                let mut enhanced_context = context.clone();
                enhanced_context
                    .metadata
                    .insert("time_control_action".to_string(), format!("{:?}", action));
                
                if let Ok(state) = self.state.lock() {
                    enhanced_context.metadata.insert(
                        "acceleration_factor".to_string(),
                        state.acceleration_factor.to_string(),
                    );
                }

                // Call next handler
                let mut result = next.handle(operation, &enhanced_context)?;

                // Add time control results
                if let Some(obj) = result.as_object_mut() {
                    obj.insert("time_control".to_string(), control_result);
                }

                Ok(result)
            }

            SimulatorOperation::ExecuteTick {
                tick_number,
                delta_time,
            } => {
                // Adjust delta time based on acceleration
                let adjusted_delta = self.calculate_adjusted_delta(*delta_time);

                // Create modified operation with adjusted time
                let adjusted_operation = SimulatorOperation::ExecuteTick {
                    tick_number: *tick_number,
                    delta_time: adjusted_delta,
                };

                // Add time control info to context
                let mut enhanced_context = context.clone();
                enhanced_context.metadata.insert(
                    "original_delta_ms".to_string(),
                    delta_time.as_millis().to_string(),
                );
                enhanced_context.metadata.insert(
                    "adjusted_delta_ms".to_string(),
                    adjusted_delta.as_millis().to_string(),
                );
                
                if let Ok(state) = self.state.lock() {
                    enhanced_context.metadata.insert(
                        "acceleration_factor".to_string(),
                        state.acceleration_factor.to_string(),
                    );
                }

                // Perform real-time sync
                let sync_result = self.sync_realtime(&enhanced_context)?;

                // Update time travel state
                self.update_time_travel_state(&operation, &enhanced_context);

                // Call next handler
                let mut result = next.handle(adjusted_operation, &enhanced_context)?;

                // Add time control information
                if let Some(obj) = result.as_object_mut() {
                    if let Ok(state) = self.state.lock() {
                        obj.insert(
                            "time_control".to_string(),
                            json!({
                                "acceleration_factor": state.acceleration_factor,
                                "is_paused": state.is_paused,
                                "original_delta_ms": delta_time.as_millis(),
                                "adjusted_delta_ms": adjusted_delta.as_millis(),
                                "time_travel": self.get_time_travel_history()
                            }),
                        );
                    }
                    obj.insert("realtime_sync".to_string(), sync_result);
                }

                Ok(result)
            }

            _ => {
                // For other operations, just add time control metadata
                let mut enhanced_context = context.clone();
                if let Ok(state) = self.state.lock() {
                    enhanced_context.metadata.insert(
                        "acceleration_factor".to_string(),
                        state.acceleration_factor.to_string(),
                    );
                    enhanced_context
                        .metadata
                        .insert("time_paused".to_string(), state.is_paused.to_string());
                }

                let mut result = next.handle(operation, &enhanced_context)?;

                // Add time control status to result
                if let Some(obj) = result.as_object_mut() {
                    if let Ok(state) = self.state.lock() {
                        obj.insert(
                            "time_status".to_string(),
                            json!({
                                "acceleration_factor": state.acceleration_factor,
                                "is_paused": state.is_paused
                            }),
                        );
                    }
                }

                Ok(result)
            }
        }
    }

    fn name(&self) -> &str {
        "time_control"
    }
}

/// Time checkpoint for restoration
#[derive(Debug, Clone)]
struct TimeCheckpoint {
    id: String,
    timestamp: Duration,
    tick: u64,
    acceleration_factor: f64,
    created_at: Instant,
    metadata: HashMap<String, Value>,
}

/// Time travel debugging state
#[derive(Debug)]
struct TimeTravelState {
    entries: std::collections::VecDeque<TimeTravelEntry>,
    max_entries: usize,
}

impl TimeTravelState {
    fn new() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
            max_entries: 1000,
        }
    }

    fn add_entry(&mut self, entry: TimeTravelEntry) {
        if self.entries.len() >= self.max_entries {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }
}

/// Entry in time travel log
#[derive(Debug, Clone)]
struct TimeTravelEntry {
    tick: u64,
    timestamp: Duration,
    operation: String,
    acceleration_factor: f64,
    recorded_at: Instant,
}

/// Real-time synchronization settings
#[derive(Debug, Clone)]
pub struct RealtimeSync {
    /// Enable real-time synchronization
    pub enabled: bool,
    /// Tolerance for real-time drift
    pub tolerance: Duration,
    /// Target real-time factor (1.0 = real-time)
    pub target_factor: f64,
}

impl Default for RealtimeSync {
    fn default() -> Self {
        Self {
            enabled: false,
            tolerance: Duration::from_millis(100),
            target_factor: 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpSimulatorHandler;

    #[test]
    fn test_time_control_creation() {
        let middleware = TimeControlMiddleware::new()
            .with_acceleration_bounds(0.5, 10.0)
            .with_precise_timing(true)
            .with_time_travel(true);

        if let Ok(state) = middleware.state.lock() {
            assert_eq!(state.min_acceleration, 0.5);
            assert_eq!(state.max_acceleration, 10.0);
            assert!(state.precise_timing);
            assert!(state.time_travel_state.is_some());
        } else {
            panic!("Failed to acquire state lock for test");
        }
    }

    #[test]
    fn test_time_control_operation() {
        let middleware = TimeControlMiddleware::new();
        let handler = NoOpSimulatorHandler;
        let context = SimulatorContext::new("test".to_string(), "run1".to_string());

        let result = middleware.process(
            SimulatorOperation::ControlTime {
                action: TimeControlAction::SetAcceleration { factor: 2.0 },
                parameters: HashMap::new(),
            },
            &context,
            &handler,
        );

        assert!(result.is_ok());
        let value = result.unwrap();
        assert!(value.get("time_control").is_some());
    }

    #[test]
    fn test_time_acceleration() {
        let middleware = TimeControlMiddleware::new();
        let original_delta = Duration::from_millis(100);
        let adjusted = middleware.calculate_adjusted_delta(original_delta);

        // With default acceleration factor of 1.0, should be unchanged
        assert_eq!(adjusted, original_delta);
    }

    #[test]
    fn test_pause_functionality() {
        let middleware = TimeControlMiddleware::new();
        
        // Set paused state
        if let Ok(mut state) = middleware.state.lock() {
            state.is_paused = true;
        }

        let operation = SimulatorOperation::ExecuteTick {
            tick_number: 1,
            delta_time: Duration::from_millis(100),
        };

        // Should not process when paused
        assert!(!middleware.should_process_operation(&operation));

        // Should process time control operations when paused
        let time_control = SimulatorOperation::ControlTime {
            action: TimeControlAction::Resume,
            parameters: HashMap::new(),
        };
        assert!(middleware.should_process_operation(&time_control));
    }

    #[test]
    fn test_realtime_sync() {
        let sync = RealtimeSync {
            enabled: true,
            tolerance: Duration::from_millis(50),
            target_factor: 1.0,
        };

        assert!(sync.enabled);
        assert_eq!(sync.tolerance, Duration::from_millis(50));
        assert_eq!(sync.target_factor, 1.0);
    }
}
