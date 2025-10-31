//! Analysis and debugging tools for simulation failures
//!
//! This module provides tools for analyzing simulation failures, generating
//! minimal reproductions, and producing detailed debug reports.

#![allow(ambiguous_glob_reexports)]

pub mod debug_reporter;
pub mod failure_analyzer;
pub mod focused_tester;
pub mod minimal_reproduction;
pub mod trace_recorder;

pub use debug_reporter::*;
pub use failure_analyzer::*;
pub use focused_tester::*;
pub use minimal_reproduction::*;
pub use trace_recorder::*;

// Re-export unified types
pub use crate::metrics::{MetricsSnapshot, SimulationMetrics};
pub use crate::results::{PropertyViolation, SimulationExecutionResult, SimulationRunResult};

use crate::{world_state::WorldState, Result};
use aura_console_types::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Duplicate SimulationResult and SimulationMetrics removed - using unified types from results and metrics modules

/// Debug result from violation analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationDebugResult {
    /// The violation that was analyzed
    pub violation_type: String,
    /// Context when violation occurred
    pub context: DebugContext,
    /// State information at violation time
    pub state_snapshot: WorldState,
    /// Causal chain leading to violation
    pub causal_chain: Vec<TraceEvent>,
    /// Related participants
    pub participants: Vec<String>,
    /// Failure analysis result
    pub failure_analysis: failure_analyzer::FailureAnalysisResult,
    /// Analysis metadata
    pub metadata: HashMap<String, String>,
}

/// Debug context information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugContext {
    /// Tick when violation occurred
    pub tick: u64,
    /// Time when violation occurred
    pub time: u64,
    /// Active protocol sessions
    pub active_sessions: Vec<String>,
    /// Network conditions
    pub network_state: String,
}

// Re-export DebugSession from observability module to avoid conflicts
pub use crate::observability::DebugSession;

/// Position in debug session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugPosition {
    /// Current tick
    pub tick: u64,
    /// Current event index
    pub event_index: usize,
}

/// Debug checkpoint for time travel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugCheckpoint {
    /// Checkpoint name
    pub name: String,
    /// World state at checkpoint
    pub state: WorldState,
    /// Tick when checkpoint was taken
    pub tick: u64,
}

/// Simulation with checkpoint support
#[derive(Debug, Clone)]
pub struct CheckpointSimulation {
    /// Current world state
    pub world_state: WorldState,
    /// Simulation configuration
    pub config: CheckpointConfig,
    /// Checkpoints taken during simulation
    pub checkpoints: Vec<DebugCheckpoint>,
    /// Event history
    pub event_history: Vec<TraceEvent>,
}

/// Configuration for checkpoint simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Maximum number of checkpoints to keep
    pub max_checkpoints: usize,
    /// Interval between automatic checkpoints
    pub checkpoint_interval: u64,
    /// Whether to enable detailed logging
    pub detailed_logging: bool,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            max_checkpoints: 100,
            checkpoint_interval: 100,
            detailed_logging: false,
        }
    }
}

impl CheckpointSimulation {
    /// Create a new checkpoint simulation
    pub fn new(world_state: WorldState) -> Self {
        Self {
            world_state,
            config: CheckpointConfig::default(),
            checkpoints: Vec::new(),
            event_history: Vec::new(),
        }
    }

    /// Create simulation from scenario
    pub fn create_simulation_from_scenario(
        _scenario: crate::scenario::Scenario,
        seed: u64,
    ) -> Result<Self> {
        let world_state = WorldState::new(seed);
        Ok(Self::new(world_state))
    }

    /// Create simulation from scenario reference
    pub fn from_scenario(_scenario: &crate::scenario::Scenario) -> Result<Self> {
        let world_state = WorldState::new(42);
        Ok(Self::new(world_state))
    }

    /// Run simulation until completion
    pub fn run_until_completion(&mut self) -> Result<SimulationExecutionResult> {
        let mut events = Vec::new();
        let start_time = std::time::Instant::now();

        while self.world_state.should_continue() {
            let tick_events = crate::simulation_engine::tick(&mut self.world_state)?;
            events.extend(tick_events);

            // Take checkpoint if needed
            if self
                .world_state
                .current_tick
                .is_multiple_of(self.config.checkpoint_interval)
            {
                self.take_checkpoint(format!("auto_{}", self.world_state.current_tick));
            }
        }

        self.event_history.extend(events.clone());

        Ok(SimulationExecutionResult {
            success: true,
            final_state: self.world_state.clone(),
            events,
            metrics: {
                let mut metrics = SimulationMetrics::new();
                metrics.simulation.current_tick = self.world_state.current_tick;
                metrics.simulation.total_duration_ms = start_time.elapsed().as_millis() as u64;
                metrics.network.messages_sent = 0; // Default placeholder
                metrics.protocol.completed_sessions =
                    self.world_state.protocols.completed_sessions.len() as u64;
                metrics
            },
            error: None,
        })
    }

    /// Run simulation with monitoring
    pub fn run_with_monitoring(
        &mut self,
        _monitor: &mut crate::testing::PropertyMonitor,
    ) -> Result<SimulationExecutionResult> {
        self.run_until_completion()
    }

    /// Take a checkpoint
    pub fn take_checkpoint(&mut self, name: String) {
        let checkpoint = DebugCheckpoint {
            name,
            state: self.world_state.clone(),
            tick: self.world_state.current_tick,
        };

        self.checkpoints.push(checkpoint);

        // Limit number of checkpoints
        if self.checkpoints.len() > self.config.max_checkpoints {
            self.checkpoints.remove(0);
        }
    }

    /// Get participants from the world state
    pub fn get_participants(&self) -> &HashMap<String, crate::world_state::ParticipantState> {
        &self.world_state.participants
    }

    /// Get current simulation state
    pub fn get_simulation_state(&self) -> &crate::world_state::WorldState {
        &self.world_state
    }
}
