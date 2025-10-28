//! Functional simulation runner
//!
//! This module provides simulation runners that use the pure functional tick() approach.
//! The complex stateful logic of looping, running, and managing scenarios is cleanly
//! separated from the core state transformation logic.

use crate::simulation_engine::tick;
use crate::world_state::{WorldState, WorldStateSnapshot};
use crate::{Result, SimError};
use aura_console_types::trace::CheckpointRef;
use aura_console_types::{SimulationTrace, TraceEvent, TraceMetadata};
use std::collections::VecDeque;

/// Functional simulation runner that executes pure state transitions
///
/// This runner demonstrates the clean separation of concerns:
/// - WorldState contains only data
/// - tick() function contains only transition logic  
/// - Runner contains only execution harness logic
pub struct FunctionalRunner {
    /// Current world state
    world: WorldState,
    /// Complete trace of all events
    event_trace: Vec<TraceEvent>,
    /// Checkpoints for time travel debugging
    checkpoints: VecDeque<StateCheckpoint>,
    /// Maximum number of checkpoints to keep
    max_checkpoints: usize,
    /// Whether to automatically create checkpoints
    auto_checkpoint_interval: Option<u64>,
}

/// State checkpoint for time travel debugging
#[derive(Debug, Clone)]
pub struct StateCheckpoint {
    /// Checkpoint identifier
    pub id: String,
    /// User-provided label
    pub label: Option<String>,
    /// Tick when checkpoint was created
    pub tick: u64,
    /// Complete world state snapshot
    pub world_state: WorldState,
    /// Events up to this point
    pub events_up_to_here: Vec<TraceEvent>,
    /// When checkpoint was created
    pub created_at: u64,
}

/// Result of running simulation to completion
#[derive(Debug, Clone)]
pub struct RunResult {
    /// Final tick reached
    pub final_tick: u64,
    /// Final simulation time
    pub final_time: u64,
    /// Whether simulation completed successfully
    pub success: bool,
    /// Reason for stopping
    pub stop_reason: StopReason,
    /// Complete event trace
    pub event_trace: Vec<TraceEvent>,
    /// Final world state snapshot
    pub final_state: WorldStateSnapshot,
}

/// Reason why simulation stopped
#[derive(Debug, Clone)]
pub enum StopReason {
    /// Maximum ticks reached
    MaxTicksReached,
    /// Maximum time reached
    MaxTimeReached,
    /// Simulation became idle
    BecameIdle,
    /// Manual stop requested
    ManualStop,
    /// Error occurred
    Error(String),
}

impl FunctionalRunner {
    /// Create a new functional runner
    pub fn new(seed: u64) -> Self {
        Self {
            world: WorldState::new(seed),
            event_trace: Vec::new(),
            checkpoints: VecDeque::new(),
            max_checkpoints: 100,
            auto_checkpoint_interval: None,
        }
    }

    /// Enable automatic checkpointing every N ticks
    pub fn with_auto_checkpoints(mut self, interval: u64) -> Self {
        self.auto_checkpoint_interval = Some(interval);
        self
    }

    /// Set maximum number of checkpoints to keep
    pub fn with_max_checkpoints(mut self, max_checkpoints: usize) -> Self {
        self.max_checkpoints = max_checkpoints;
        self
    }

    /// Get current world state (read-only)
    pub fn world_state(&self) -> &WorldState {
        &self.world
    }

    /// Get mutable access to world state for setup
    pub fn world_state_mut(&mut self) -> &mut WorldState {
        &mut self.world
    }

    /// Get current simulation tick
    pub fn current_tick(&self) -> u64 {
        self.world.current_tick
    }

    /// Get current simulation time
    pub fn current_time(&self) -> u64 {
        self.world.current_time
    }

    /// Get complete event trace
    pub fn event_trace(&self) -> &[TraceEvent] {
        &self.event_trace
    }

    /// Add a participant to the simulation
    pub fn add_participant(
        &mut self,
        id: String,
        device_id: String,
        account_id: String,
    ) -> Result<()> {
        self.world.add_participant(id, device_id, account_id)?;
        Ok(())
    }

    /// Run a single simulation step
    ///
    /// This demonstrates the clean functional approach:
    /// - Call pure tick() function
    /// - Collect events
    /// - Handle checkpointing in runner layer
    pub fn step(&mut self) -> Result<Vec<TraceEvent>> {
        // Call the pure tick function
        let events = tick(&mut self.world)?;

        // Record events in trace
        self.event_trace.extend(events.iter().cloned());

        // Handle automatic checkpointing
        if let Some(interval) = self.auto_checkpoint_interval {
            if self.world.current_tick.is_multiple_of(interval) {
                let label = format!("auto_checkpoint_tick_{}", self.world.current_tick);
                let _ = self.create_checkpoint(Some(label));
            }
        }

        // Step completed

        Ok(events)
    }

    /// Run multiple steps
    pub fn step_n(&mut self, count: u64) -> Result<Vec<TraceEvent>> {
        let mut all_events = Vec::new();

        for _ in 0..count {
            let events = self.step()?;
            all_events.extend(events);

            if !self.world.should_continue() {
                break;
            }
        }

        Ok(all_events)
    }

    /// Run simulation until completion or stopping condition
    pub fn run_until_complete(&mut self) -> Result<RunResult> {
        let mut _total_events = 0;

        while self.world.should_continue() {
            let events = self.step()?;
            _total_events += events.len();

            // Check for various stopping conditions
            if self.world.current_tick >= self.world.config.max_ticks {
                return Ok(RunResult {
                    final_tick: self.world.current_tick,
                    final_time: self.world.current_time,
                    success: true,
                    stop_reason: StopReason::MaxTicksReached,
                    event_trace: self.event_trace.clone(),
                    final_state: self.world.snapshot(),
                });
            }

            if self.world.current_time >= self.world.config.max_time {
                return Ok(RunResult {
                    final_tick: self.world.current_tick,
                    final_time: self.world.current_time,
                    success: true,
                    stop_reason: StopReason::MaxTimeReached,
                    event_trace: self.event_trace.clone(),
                    final_state: self.world.snapshot(),
                });
            }

            if self.world.is_idle() {
                return Ok(RunResult {
                    final_tick: self.world.current_tick,
                    final_time: self.world.current_time,
                    success: true,
                    stop_reason: StopReason::BecameIdle,
                    event_trace: self.event_trace.clone(),
                    final_state: self.world.snapshot(),
                });
            }
        }

        Ok(RunResult {
            final_tick: self.world.current_tick,
            final_time: self.world.current_time,
            success: true,
            stop_reason: StopReason::BecameIdle,
            event_trace: self.event_trace.clone(),
            final_state: self.world.snapshot(),
        })
    }

    /// Run simulation until idle (no more work to do)
    pub fn run_until_idle(&mut self) -> Result<RunResult> {
        while !self.world.is_idle() && self.world.should_continue() {
            self.step()?;
        }

        Ok(RunResult {
            final_tick: self.world.current_tick,
            final_time: self.world.current_time,
            success: true,
            stop_reason: if self.world.is_idle() {
                StopReason::BecameIdle
            } else {
                StopReason::MaxTicksReached
            },
            event_trace: self.event_trace.clone(),
            final_state: self.world.snapshot(),
        })
    }

    /// Run simulation for a specific number of ticks
    pub fn run_for_ticks(&mut self, tick_count: u64) -> Result<RunResult> {
        let target_tick = self.world.current_tick + tick_count;

        while self.world.current_tick < target_tick && self.world.should_continue() {
            self.step()?;
        }

        Ok(RunResult {
            final_tick: self.world.current_tick,
            final_time: self.world.current_time,
            success: true,
            stop_reason: StopReason::ManualStop,
            event_trace: self.event_trace.clone(),
            final_state: self.world.snapshot(),
        })
    }

    /// Create a checkpoint of current state
    pub fn create_checkpoint(&mut self, label: Option<String>) -> Result<String> {
        let checkpoint_id = uuid::Uuid::new_v4().to_string();

        let checkpoint = StateCheckpoint {
            id: checkpoint_id.clone(),
            label: label.clone(),
            tick: self.world.current_tick,
            world_state: self.world.clone(),
            events_up_to_here: self.event_trace.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.checkpoints.push_back(checkpoint);

        // Trim old checkpoints if we exceed the limit
        while self.checkpoints.len() > self.max_checkpoints {
            self.checkpoints.pop_front();
        }

        // Checkpoint created

        Ok(checkpoint_id)
    }

    /// Restore state from a checkpoint
    pub fn restore_checkpoint(&mut self, checkpoint_id: &str) -> Result<()> {
        let checkpoint = self
            .checkpoints
            .iter()
            .find(|cp| cp.id == checkpoint_id)
            .ok_or_else(|| {
                SimError::CheckpointError(format!("Checkpoint {} not found", checkpoint_id))
            })?;

        // Restoring checkpoint

        // Restore complete state
        self.world = checkpoint.world_state.clone();
        self.event_trace = checkpoint.events_up_to_here.clone();

        Ok(())
    }

    /// List all available checkpoints
    pub fn list_checkpoints(&self) -> Vec<(String, Option<String>, u64)> {
        self.checkpoints
            .iter()
            .map(|cp| (cp.id.clone(), cp.label.clone(), cp.tick))
            .collect()
    }

    /// Export complete simulation trace
    pub fn export_trace(&self) -> SimulationTrace {
        let metadata = TraceMetadata {
            scenario_name: self
                .world
                .config
                .scenario_name
                .clone()
                .unwrap_or_else(|| "functional_simulation".to_string()),
            seed: self.world.seed,
            total_ticks: self.world.current_tick,
            properties_checked: self.world.config.properties.clone(),
            violations: Vec::new(), // Would be populated by property checking
        };

        // Build participant info from world state
        let participants = self
            .world
            .participants
            .iter()
            .map(|(id, p)| {
                (
                    id.clone(),
                    aura_console_types::ParticipantInfo {
                        device_id: p.device_id.clone(),
                        participant_type: p.participant_type,
                        status: p.status,
                    },
                )
            })
            .collect();

        // Build simple network topology
        let network_topology = aura_console_types::NetworkTopology {
            nodes: self
                .world
                .participants
                .iter()
                .map(|(id, p)| {
                    (
                        id.clone(),
                        aura_console_types::NodeInfo {
                            device_id: p.device_id.clone(),
                            participant_type: p.participant_type,
                            status: p.status,
                            message_count: p.message_count,
                        },
                    )
                })
                .collect(),
            edges: Vec::new(), // Would be built from message events
            partitions: self
                .world
                .network
                .partitions
                .iter()
                .map(|p| aura_console_types::PartitionInfo {
                    devices: p.participants.clone(),
                    created_at_tick: self.world.current_tick,
                })
                .collect(),
        };

        SimulationTrace {
            metadata,
            timeline: self.event_trace.clone(),
            checkpoints: self
                .checkpoints
                .iter()
                .map(|cp| CheckpointRef {
                    id: cp.id.clone(),
                    label: cp.label.clone().unwrap_or_else(|| "checkpoint".to_string()),
                    tick: cp.tick,
                })
                .collect(),
            participants,
            network_topology,
        }
    }

    /// Get simulation statistics
    pub fn get_statistics(&self) -> SimulationStatistics {
        SimulationStatistics {
            current_tick: self.world.current_tick,
            current_time: self.world.current_time,
            total_events: self.event_trace.len(),
            participant_count: self.world.participants.len(),
            active_sessions: self.world.active_session_count(),
            queued_protocols: self.world.queued_protocol_count(),
            in_flight_messages: self.world.network.in_flight_messages.len(),
            checkpoints_created: self.checkpoints.len(),
            byzantine_participants: self.world.byzantine.byzantine_participants.len(),
            network_partitions: self.world.network.partitions.len(),
        }
    }
}

/// Simulation statistics
#[derive(Debug, Clone)]
pub struct SimulationStatistics {
    pub current_tick: u64,
    pub current_time: u64,
    pub total_events: usize,
    pub participant_count: usize,
    pub active_sessions: usize,
    pub queued_protocols: usize,
    pub in_flight_messages: usize,
    pub checkpoints_created: usize,
    pub byzantine_participants: usize,
    pub network_partitions: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_functional_runner_creation() {
        let runner = FunctionalRunner::new(42);
        assert_eq!(runner.current_tick(), 0);
        assert_eq!(runner.world_state().seed, 42);
        assert!(runner.event_trace().is_empty());
    }

    #[test]
    fn test_runner_step() {
        let mut runner = FunctionalRunner::new(42);

        let initial_tick = runner.current_tick();
        let events = runner.step().unwrap();

        assert_eq!(runner.current_tick(), initial_tick + 1);
        assert!(!events.is_empty());
        assert!(!runner.event_trace().is_empty());
    }

    #[test]
    fn test_runner_with_participants() {
        let mut runner = FunctionalRunner::new(42);

        runner
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        runner
            .add_participant(
                "bob".to_string(),
                "device_bob".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        assert_eq!(runner.world_state().participants.len(), 2);

        let events = runner.step().unwrap();
        assert!(!events.is_empty());
    }

    #[test]
    fn test_checkpointing() {
        let mut runner = FunctionalRunner::new(42);

        // Run a few steps
        runner.step_n(3).unwrap();
        let checkpoint_tick = runner.current_tick();

        // Create checkpoint
        let checkpoint_id = runner
            .create_checkpoint(Some("test_checkpoint".to_string()))
            .unwrap();

        // Run more steps
        runner.step_n(2).unwrap();
        assert!(runner.current_tick() > checkpoint_tick);

        // Restore checkpoint
        runner.restore_checkpoint(&checkpoint_id).unwrap();
        assert_eq!(runner.current_tick(), checkpoint_tick);
    }

    #[test]
    fn test_auto_checkpointing() {
        let mut runner = FunctionalRunner::new(42).with_auto_checkpoints(2); // Checkpoint every 2 ticks

        // Run enough steps to trigger auto-checkpointing
        runner.step_n(5).unwrap();

        // Should have created checkpoints at ticks 2 and 4
        let checkpoints = runner.list_checkpoints();
        assert!(checkpoints.len() >= 2);
    }

    #[test]
    fn test_run_for_ticks() {
        let mut runner = FunctionalRunner::new(42);

        let result = runner.run_for_ticks(5).unwrap();

        assert_eq!(result.final_tick, 5);
        assert!(matches!(result.stop_reason, StopReason::ManualStop));
        assert!(result.success);
    }

    #[test]
    fn test_statistics() {
        let mut runner = FunctionalRunner::new(42);

        runner
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        runner.step_n(3).unwrap();
        runner.create_checkpoint(Some("test".to_string())).unwrap();

        let stats = runner.get_statistics();
        assert_eq!(stats.current_tick, 3);
        assert_eq!(stats.participant_count, 1);
        assert_eq!(stats.checkpoints_created, 1);
        assert!(stats.total_events > 0);
    }

    #[test]
    fn test_trace_export() {
        let mut runner = FunctionalRunner::new(42);

        runner
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        runner.step_n(2).unwrap();

        let trace = runner.export_trace();
        assert_eq!(trace.metadata.seed, 42);
        assert_eq!(trace.metadata.total_ticks, 2);
        assert!(!trace.timeline.is_empty());
        assert_eq!(trace.participants.len(), 1);
    }
}
