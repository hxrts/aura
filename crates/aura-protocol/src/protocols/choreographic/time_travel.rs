//! Time-travel debugging for choreographic protocols
//!
//! This module provides checkpoint-based time-travel debugging capabilities,
//! allowing developers to step forward and backward through protocol execution.

use super::{BridgedRole, ChoreoEvent, SimulationConfig};
use crate::{
    context::BaseContext, effects::ProtocolEffects, middleware::handler::AuraProtocolHandler,
};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Checkpoint of protocol state at a specific point in time
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreoCheckpoint {
    /// Checkpoint ID
    pub id: Uuid,

    /// Simulation time when checkpoint was taken
    pub timestamp: u64,

    /// Protocol ID this checkpoint belongs to
    pub protocol_id: Uuid,

    /// Events up to this point
    pub events: Vec<ChoreoEvent>,

    /// Message queue states
    pub message_queues: HashMap<DeviceId, Vec<MessageSnapshot>>,

    /// Protocol-specific state
    pub protocol_state: HashMap<String, serde_json::Value>,

    /// Active participants
    pub participants: Vec<BridgedRole>,

    /// Current step/phase
    pub current_step: usize,
}

/// Snapshot of a queued message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSnapshot {
    pub from: DeviceId,
    pub to: DeviceId,
    pub message_type: String,
    pub payload: Vec<u8>,
    pub delivery_time: u64,
}

/// Time-travel debugging controller
pub struct TimeTravelDebugger {
    /// All checkpoints for a protocol
    checkpoints: VecDeque<ChoreoCheckpoint>,

    /// Maximum number of checkpoints to keep
    max_checkpoints: usize,

    /// Current checkpoint index
    current_index: Option<usize>,

    /// Checkpoint interval (in simulation time units)
    checkpoint_interval: u64,

    /// Last checkpoint time
    last_checkpoint_time: u64,
}

impl TimeTravelDebugger {
    pub fn new(max_checkpoints: usize, checkpoint_interval: u64) -> Self {
        Self {
            checkpoints: VecDeque::with_capacity(max_checkpoints),
            max_checkpoints,
            current_index: None,
            checkpoint_interval,
            last_checkpoint_time: 0,
        }
    }

    /// Create a checkpoint if enough time has passed
    pub fn maybe_checkpoint(
        &mut self,
        current_time: u64,
        protocol_id: Uuid,
        events: &[ChoreoEvent],
        message_queues: &HashMap<DeviceId, Vec<MessageSnapshot>>,
        protocol_state: HashMap<String, serde_json::Value>,
        participants: Vec<BridgedRole>,
        current_step: usize,
    ) {
        if current_time - self.last_checkpoint_time >= self.checkpoint_interval {
            self.create_checkpoint(
                current_time,
                protocol_id,
                events,
                message_queues,
                protocol_state,
                participants,
                current_step,
            );
            self.last_checkpoint_time = current_time;
        }
    }

    /// Create a new checkpoint
    pub fn create_checkpoint(
        &mut self,
        timestamp: u64,
        protocol_id: Uuid,
        events: &[ChoreoEvent],
        message_queues: &HashMap<DeviceId, Vec<MessageSnapshot>>,
        protocol_state: HashMap<String, serde_json::Value>,
        participants: Vec<BridgedRole>,
        current_step: usize,
    ) {
        let checkpoint = ChoreoCheckpoint {
            id: Uuid::new_v4(),
            timestamp,
            protocol_id,
            events: events.to_vec(),
            message_queues: message_queues.clone(),
            protocol_state,
            participants,
            current_step,
        };

        self.checkpoints.push_back(checkpoint);

        // Remove old checkpoints if we exceed the limit
        while self.checkpoints.len() > self.max_checkpoints {
            self.checkpoints.pop_front();
        }

        // Update current index to point to the new checkpoint
        self.current_index = Some(self.checkpoints.len() - 1);
    }

    /// Move to a specific checkpoint by index
    pub fn goto_checkpoint(&mut self, index: usize) -> Option<&ChoreoCheckpoint> {
        if index < self.checkpoints.len() {
            self.current_index = Some(index);
            self.checkpoints.get(index)
        } else {
            None
        }
    }

    /// Move to the previous checkpoint
    pub fn step_backward(&mut self) -> Option<&ChoreoCheckpoint> {
        if let Some(current) = self.current_index {
            if current > 0 {
                self.current_index = Some(current - 1);
                return self.checkpoints.get(current - 1);
            }
        }
        None
    }

    /// Move to the next checkpoint
    pub fn step_forward(&mut self) -> Option<&ChoreoCheckpoint> {
        if let Some(current) = self.current_index {
            if current < self.checkpoints.len() - 1 {
                self.current_index = Some(current + 1);
                return self.checkpoints.get(current + 1);
            }
        }
        None
    }

    /// Get the current checkpoint
    pub fn current_checkpoint(&self) -> Option<&ChoreoCheckpoint> {
        self.current_index.and_then(|idx| self.checkpoints.get(idx))
    }

    /// Get all checkpoints
    pub fn all_checkpoints(&self) -> Vec<&ChoreoCheckpoint> {
        self.checkpoints.iter().collect()
    }

    /// Find checkpoint closest to a specific timestamp
    pub fn find_checkpoint_near_time(&self, target_time: u64) -> Option<&ChoreoCheckpoint> {
        self.checkpoints
            .iter()
            .min_by_key(|cp| (cp.timestamp as i64 - target_time as i64).abs())
    }

    /// Clear all checkpoints
    pub fn clear(&mut self) {
        self.checkpoints.clear();
        self.current_index = None;
        self.last_checkpoint_time = 0;
    }
}

/// Time-travel enabled simulation handler
pub struct TimeTravelSimulationHandler<H: AuraProtocolHandler, E: ProtocolEffects> {
    /// Base simulation handler
    inner: super::SimulationChoreoHandler<H, E>,

    /// Time travel debugger
    debugger: Arc<Mutex<TimeTravelDebugger>>,

    /// Whether time-travel is enabled
    enabled: bool,
}

impl<H: AuraProtocolHandler, E: ProtocolEffects> TimeTravelSimulationHandler<H, E> {
    pub fn new(
        handler: H,
        effects: E,
        context: BaseContext,
        config: SimulationConfig,
        max_checkpoints: usize,
        checkpoint_interval: u64,
    ) -> Self {
        let inner = super::SimulationChoreoHandler::new(handler, effects, context, config);
        let debugger = Arc::new(Mutex::new(TimeTravelDebugger::new(
            max_checkpoints,
            checkpoint_interval,
        )));

        Self {
            inner,
            debugger,
            enabled: true,
        }
    }

    /// Enable or disable time-travel debugging
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Create a checkpoint manually
    pub fn checkpoint(&mut self, protocol_id: Uuid, current_step: usize) {
        if !self.enabled {
            return;
        }

        let events = self.inner.get_events();
        let message_queues = self.extract_message_queues();
        let protocol_state = self.extract_protocol_state();
        let participants = vec![]; // Would be extracted from handler

        self.debugger.lock().unwrap().create_checkpoint(
            self.inner.current_time(),
            protocol_id,
            &events,
            &message_queues,
            protocol_state,
            participants,
            current_step,
        );
    }

    /// Restore from a checkpoint
    pub fn restore_checkpoint(&mut self, checkpoint: &ChoreoCheckpoint) -> Result<(), String> {
        if !self.enabled {
            return Err("Time-travel debugging is disabled".to_string());
        }

        // Clear current state
        self.inner.clear_events();

        // Restore events
        for event in &checkpoint.events {
            self.inner.record_event(event.clone());
        }

        // Restore time
        self.inner.set_current_time(checkpoint.timestamp);

        // Restore message queues
        self.restore_message_queues(&checkpoint.message_queues);

        Ok(())
    }

    /// Step backward in time
    pub fn step_backward(&mut self) -> Result<(), String> {
        let checkpoint = self.debugger.lock().unwrap().step_backward().cloned();

        if let Some(cp) = checkpoint {
            self.restore_checkpoint(&cp)
        } else {
            Err("No previous checkpoint available".to_string())
        }
    }

    /// Step forward in time
    pub fn step_forward(&mut self) -> Result<(), String> {
        let checkpoint = self.debugger.lock().unwrap().step_forward().cloned();

        if let Some(cp) = checkpoint {
            self.restore_checkpoint(&cp)
        } else {
            Err("No next checkpoint available".to_string())
        }
    }

    /// Get debugging information
    pub fn debug_info(&self) -> TimeTravelDebugInfo {
        let debugger = self.debugger.lock().unwrap();
        let total_checkpoints = debugger.checkpoints.len();
        let current_index = debugger.current_index;

        TimeTravelDebugInfo {
            total_checkpoints,
            current_index,
            current_time: self.inner.current_time(),
            events_count: self.inner.get_events().len(),
            enabled: self.enabled,
        }
    }

    /// Extract message queues for checkpointing
    fn extract_message_queues(&self) -> HashMap<DeviceId, Vec<MessageSnapshot>> {
        // In a real implementation, this would extract from inner handler
        HashMap::new()
    }

    /// Extract protocol state for checkpointing
    fn extract_protocol_state(&self) -> HashMap<String, serde_json::Value> {
        let mut state = HashMap::new();
        state.insert(
            "current_time".to_string(),
            serde_json::json!(self.inner.current_time()),
        );
        state
    }

    /// Restore message queues from checkpoint
    fn restore_message_queues(&mut self, _queues: &HashMap<DeviceId, Vec<MessageSnapshot>>) {
        // In a real implementation, this would restore to inner handler
    }
}

/// Debug information for time-travel state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeTravelDebugInfo {
    pub total_checkpoints: usize,
    pub current_index: Option<usize>,
    pub current_time: u64,
    pub events_count: usize,
    pub enabled: bool,
}

/// Export time-travel session for analysis
pub fn export_time_travel_session(
    debugger: &TimeTravelDebugger,
    output_path: &str,
) -> Result<(), std::io::Error> {
    let session = TimeTravelSession {
        checkpoints: debugger.checkpoints.iter().cloned().collect(),
        metadata: HashMap::from([
            ("version".to_string(), "1.0".to_string()),
            ("created_at".to_string(), chrono::Utc::now().to_rfc3339()),
        ]),
    };

    let json = serde_json::to_string_pretty(&session)?;
    std::fs::write(output_path, json)?;

    Ok(())
}

/// Complete time-travel session data
#[derive(Debug, Serialize, Deserialize)]
pub struct TimeTravelSession {
    pub checkpoints: Vec<ChoreoCheckpoint>,
    pub metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checkpoint_creation() {
        let mut debugger = TimeTravelDebugger::new(10, 100);

        debugger.create_checkpoint(
            100,
            Uuid::new_v4(),
            &[],
            &HashMap::new(),
            HashMap::new(),
            vec![],
            0,
        );

        assert_eq!(debugger.checkpoints.len(), 1);
        assert_eq!(debugger.current_index, Some(0));
    }

    #[test]
    fn test_checkpoint_limits() {
        let mut debugger = TimeTravelDebugger::new(3, 100);

        // Create 5 checkpoints
        for i in 0..5 {
            debugger.create_checkpoint(
                i * 100,
                Uuid::new_v4(),
                &[],
                &HashMap::new(),
                HashMap::new(),
                vec![],
                i,
            );
        }

        // Should only keep the last 3
        assert_eq!(debugger.checkpoints.len(), 3);
        assert_eq!(debugger.checkpoints[0].current_step, 2);
        assert_eq!(debugger.checkpoints[2].current_step, 4);
    }

    #[test]
    fn test_time_travel_navigation() {
        let mut debugger = TimeTravelDebugger::new(10, 100);

        // Create 3 checkpoints
        for i in 0..3 {
            debugger.create_checkpoint(
                i * 100,
                Uuid::new_v4(),
                &[],
                &HashMap::new(),
                HashMap::new(),
                vec![],
                i,
            );
        }

        // Should be at the last checkpoint
        assert_eq!(debugger.current_index, Some(2));

        // Step backward
        let cp = debugger.step_backward();
        assert!(cp.is_some());
        assert_eq!(debugger.current_index, Some(1));

        // Step backward again
        let cp = debugger.step_backward();
        assert!(cp.is_some());
        assert_eq!(debugger.current_index, Some(0));

        // Can't go further back
        let cp = debugger.step_backward();
        assert!(cp.is_none());
        assert_eq!(debugger.current_index, Some(0));

        // Step forward
        let cp = debugger.step_forward();
        assert!(cp.is_some());
        assert_eq!(debugger.current_index, Some(1));
    }

    #[test]
    fn test_find_checkpoint_near_time() {
        let mut debugger = TimeTravelDebugger::new(10, 100);

        debugger.create_checkpoint(
            100,
            Uuid::new_v4(),
            &[],
            &HashMap::new(),
            HashMap::new(),
            vec![],
            0,
        );
        debugger.create_checkpoint(
            300,
            Uuid::new_v4(),
            &[],
            &HashMap::new(),
            HashMap::new(),
            vec![],
            0,
        );
        debugger.create_checkpoint(
            500,
            Uuid::new_v4(),
            &[],
            &HashMap::new(),
            HashMap::new(),
            vec![],
            0,
        );

        // Find closest to 250
        let cp = debugger.find_checkpoint_near_time(250);
        assert!(cp.is_some());
        assert_eq!(cp.unwrap().timestamp, 300);

        // Find closest to 450
        let cp = debugger.find_checkpoint_near_time(450);
        assert!(cp.is_some());
        assert_eq!(cp.unwrap().timestamp, 500);
    }
}
