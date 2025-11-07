//! Simulation wrapper that adapts simulator middleware to sim-server API
//!
//! This wrapper provides the interface expected by the sim-server while using
//! the new middleware-based simulation architecture internally.

use anyhow::Result;
use app_console_types::{
    trace::{ParticipantStatus, ParticipantType},
    DeviceInfo, TraceEvent,
};
use aura_simulator::{
    CoreSimulatorHandler, SimulatorContext, SimulatorMiddlewareStack, SimulatorOperation,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

/// Simulation participant state
#[derive(Debug, Clone)]
pub struct ParticipantState {
    pub device_id: String,
    pub account_id: String,
    pub status: ParticipantStatus,
    pub message_count: u32,
}

/// Simulation state tracker
#[derive(Debug)]
pub struct SimulationState {
    pub participants: HashMap<String, ParticipantState>,
    pub current_tick: u64,
    pub current_time: u64,
    pub byzantine_participants: Vec<String>,
}

impl Default for SimulationState {
    fn default() -> Self {
        Self {
            participants: HashMap::new(),
            current_tick: 0,
            current_time: 0,
            byzantine_participants: Vec::new(),
        }
    }
}

/// Wrapper around simulator middleware stack that provides sim-server compatibility
pub struct SimulationWrapper {
    /// The underlying middleware stack
    stack: SimulatorMiddlewareStack,
    /// Simulation context
    context: SimulatorContext,
    /// Simulation state
    state: SimulationState,
    /// Simulation ID for tracking
    pub id: Uuid,
    /// Original seed used for simulation
    pub seed: u64,
    /// Whether recording is enabled (always true for now)
    recording_enabled: bool,
}

#[allow(dead_code)]
impl SimulationWrapper {
    /// Create a new simulation wrapper
    pub fn new(seed: u64) -> Self {
        // Create core handler
        let handler = Arc::new(CoreSimulatorHandler::new());

        // Build middleware stack with basic components
        let stack = SimulatorMiddlewareStack::new(handler);

        let context =
            SimulatorContext::new(format!("sim_{}", Uuid::new_v4()), format!("run_{}", seed))
                .with_seed(seed);

        Self {
            stack,
            context,
            state: SimulationState::default(),
            id: Uuid::new_v4(),
            seed,
            recording_enabled: true,
        }
    }

    /// Get current tick
    pub fn current_tick(&self) -> u64 {
        self.state.current_tick
    }

    /// Get current time
    pub fn current_time(&self) -> u64 {
        self.state.current_time
    }

    /// Execute a single simulation step
    pub fn step(&mut self) -> Result<Vec<TraceEvent>> {
        // Execute a tick operation through the middleware stack
        let operation = SimulatorOperation::ExecuteTick {
            tick_number: self.state.current_tick + 1,
            delta_time: Duration::from_millis(100),
        };

        match self.stack.process(operation, &self.context) {
            Ok(_result) => {
                // Update internal state
                self.state.current_tick += 1;
                self.state.current_time += 100; // ms

                // For now, return empty trace events
                // In a full implementation, this would extract events from the result
                Ok(vec![])
            }
            Err(e) => Err(anyhow::anyhow!("Step failed: {}", e)),
        }
    }

    /// Check if recording is enabled
    pub fn is_recording_enabled(&self) -> bool {
        self.recording_enabled
    }

    /// Check if simulation is idle (simplified - just check if no participants)
    pub fn is_idle(&self) -> bool {
        self.state.participants.is_empty()
    }

    /// Get participant list as device info
    pub fn get_participants(&self) -> Vec<DeviceInfo> {
        self.state
            .participants
            .iter()
            .map(|(id, participant)| DeviceInfo {
                id: id.clone(),
                device_id: participant.device_id.clone(),
                account_id: participant.account_id.clone(),
                participant_type: if self.state.byzantine_participants.contains(id) {
                    ParticipantType::Byzantine
                } else {
                    ParticipantType::Honest
                },
                status: participant.status,
                message_count: participant.message_count as u64,
            })
            .collect()
    }

    /// Get a specific participant by ID
    pub fn get_participant(&self, participant_id: &str) -> Option<&ParticipantState> {
        self.state.participants.get(participant_id)
    }

    /// Add a participant to the simulation  
    pub fn add_participant(
        &mut self,
        id: String,
        device_id: String,
        account_id: String,
    ) -> Result<()> {
        // Add to local state
        let participant = ParticipantState {
            device_id: device_id.clone(),
            account_id: account_id.clone(),
            status: ParticipantStatus::Online,
            message_count: 0,
        };

        self.state.participants.insert(id.clone(), participant);

        // For now, just use local state management
        // In a full implementation, this would trigger protocol initialization
        tracing::debug!("Added participant {} to simulation", id);
        Ok(())
    }

    /// Set a participant as byzantine (simplified - just mark in state)
    pub fn set_participant_byzantine(&mut self, participant_id: &str) -> Result<()> {
        tracing::info!("Marking participant {} as byzantine", participant_id);

        // Add to byzantine participants list if participant exists
        if self.state.participants.contains_key(participant_id) {
            if !self
                .state
                .byzantine_participants
                .contains(&participant_id.to_string())
            {
                self.state
                    .byzantine_participants
                    .push(participant_id.to_string());
            }
            tracing::debug!("Participant {} marked as byzantine", participant_id);
        }

        Ok(())
    }

    /// Record a state transition (simplified logging)
    pub fn record_state_transition(
        &mut self,
        from_state: &str,
        to_state: &str,
        event_type: &str,
        metadata: Value,
    ) {
        tracing::debug!(
            "State transition: {} -> {} (event: {}, metadata: {})",
            from_state,
            to_state,
            event_type,
            metadata
        );
    }

    /// Record an effect execution (simplified logging)  
    pub fn record_effect_executed(
        &mut self,
        effect_type: &str,
        effect_data: Value,
        participant_id: Option<String>,
    ) {
        tracing::debug!(
            "Effect executed: {} by {} (data: {})",
            effect_type,
            participant_id.unwrap_or_else(|| "system".to_string()),
            effect_data
        );
    }

    /// Get simulation state for advanced access
    pub fn simulation_state(&self) -> &SimulationState {
        &self.state
    }

    /// Get mutable simulation state for setup
    pub fn simulation_state_mut(&mut self) -> &mut SimulationState {
        &mut self.state
    }
}

impl std::fmt::Debug for SimulationWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulationWrapper")
            .field("id", &self.id)
            .field("seed", &self.seed)
            .field("current_tick", &self.current_tick())
            .field("recording_enabled", &self.recording_enabled)
            .finish()
    }
}
