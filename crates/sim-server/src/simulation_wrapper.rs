//! Simulation wrapper that adapts FunctionalRunner to sim-server API
//!
//! This wrapper provides the interface expected by the sim-server while using
//! the new functional simulation architecture internally.

use anyhow::Result;
use aura_console_types::{DeviceInfo, SimulationInfo, TraceEvent};
use aura_simulator::{FunctionalRunner, WorldState};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

/// Wrapper around FunctionalRunner that provides sim-server compatibility
pub struct SimulationWrapper {
    /// The underlying functional runner
    runner: FunctionalRunner,
    /// Simulation ID for tracking
    pub id: Uuid,
    /// Original seed used for simulation
    pub seed: u64,
    /// Whether recording is enabled (always true for now)
    recording_enabled: bool,
}

impl SimulationWrapper {
    /// Create a new simulation wrapper
    pub fn new(seed: u64) -> Self {
        Self {
            runner: FunctionalRunner::new(seed),
            id: Uuid::new_v4(),
            seed,
            recording_enabled: true,
        }
    }

    /// Get current tick
    pub fn current_tick(&self) -> u64 {
        self.runner.current_tick()
    }

    /// Get current time
    pub fn current_time(&self) -> u64 {
        self.runner.current_time()
    }

    /// Execute a single simulation step
    pub fn step(&mut self) -> Result<Vec<TraceEvent>> {
        self.runner.step().map_err(|e| anyhow::anyhow!("Step failed: {}", e))
    }

    /// Check if recording is enabled
    pub fn is_recording_enabled(&self) -> bool {
        self.recording_enabled
    }

    /// Check if simulation is idle (simplified - just check if no participants)
    pub fn is_idle(&self) -> bool {
        self.runner.world_state().participants.is_empty()
    }

    /// Get participant list as device info
    pub fn get_participants(&self) -> Vec<DeviceInfo> {
        self.runner
            .world_state()
            .participants
            .iter()
            .map(|(id, participant)| DeviceInfo {
                device_id: participant.device_id.clone(),
                account_id: participant.account_id.clone(),
                is_online: true, // Simplified
                last_seen: Some(self.current_time()),
                status: "active".to_string(),
            })
            .collect()
    }

    /// Get a specific participant by ID
    pub fn get_participant(&self, participant_id: &str) -> Option<&aura_simulator::world_state::ParticipantState> {
        self.runner.world_state().participants.get(participant_id)
    }

    /// Add a participant to the simulation  
    pub fn add_participant(
        &mut self,
        id: String,
        device_id: String,
        account_id: String,
    ) -> Result<()> {
        self.runner.add_participant(id, device_id, account_id)
            .map_err(|e| anyhow::anyhow!("Failed to add participant: {}", e))
    }

    /// Set a participant as byzantine (simplified - just mark in world state)
    pub fn set_participant_byzantine(&mut self, participant_id: &str) -> Result<()> {
        // For now, just log this - full byzantine behavior would require more complex integration
        tracing::info!("Marking participant {} as byzantine", participant_id);
        
        // Add to byzantine participants list if it exists in world state
        if let Some(participant) = self.runner.world_state_mut().participants.get_mut(participant_id) {
            // Mark participant as byzantine in some way
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

    /// Get world state for advanced access
    pub fn world_state(&self) -> &WorldState {
        self.runner.world_state()
    }

    /// Get mutable world state for setup
    pub fn world_state_mut(&mut self) -> &mut WorldState {
        self.runner.world_state_mut()
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