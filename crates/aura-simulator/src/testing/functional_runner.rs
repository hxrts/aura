//! Functional simulation runner
//!
//! This module provides simulation runners that use the pure functional tick() approach.
//! The complex stateful logic of looping, running, and managing scenarios is cleanly
//! separated from the core state transformation logic.

use crate::config::{traits::ConfigDefaults, SimulationConfig};
use crate::metrics::{MetricsCollector, MetricsProvider};
use crate::results::{
    ExecutionStatus, PerformanceMetrics, SimulationRunResult, SimulationStateSnapshot, StopReason,
};
use crate::simulation_engine::tick;
use crate::state::UnifiedStateManager;
use crate::world_state::WorldState;
use crate::{AuraError, Result};
use aura_console_types::trace::CheckpointRef;
use aura_console_types::{SimulationTrace, TraceEvent, TraceMetadata};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// State checkpoint for functional runner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateCheckpoint {
    /// Checkpoint ID
    pub id: String,
    /// Optional checkpoint label
    pub label: Option<String>,
    /// Tick when checkpoint was created
    pub tick: u64,
    /// World state at checkpoint
    pub world_state: WorldState,
    /// Events up to this checkpoint
    pub events_up_to_here: Vec<TraceEvent>,
    /// When checkpoint was created
    pub created_at: u64,
}

/// Functional simulation runner that executes pure state transitions
///
/// This runner demonstrates the clean separation of concerns:
/// - WorldState contains only data
/// - tick() function contains only transition logic
/// - Runner contains only execution harness logic
pub struct FunctionalRunner {
    /// Current world state
    world: WorldState,
    /// Unified simulation configuration
    config: SimulationConfig,
    /// Complete trace of all events
    event_trace: Vec<TraceEvent>,
    /// Unified state manager for checkpoints and snapshots
    _state_manager: UnifiedStateManager,
    /// Metrics collector for performance tracking
    metrics: MetricsCollector,
    /// State checkpoints for restoration
    checkpoints: VecDeque<StateCheckpoint>,
}

// StateCheckpoint removed - using unified state management system

// Using unified SimulationRunResult and StopReason from results module

impl FunctionalRunner {
    /// Create a new functional runner with default configuration
    pub fn new(seed: u64) -> Self {
        let config = SimulationConfig::testing_defaults();
        Self {
            world: WorldState::new(seed),
            config,
            event_trace: Vec::new(),
            _state_manager: UnifiedStateManager::new(),
            metrics: MetricsCollector::new(),
            checkpoints: VecDeque::new(),
        }
    }

    /// Create a functional runner with custom configuration
    pub fn with_config(config: SimulationConfig) -> Self {
        Self {
            world: WorldState::new(config.simulation.seed),
            config,
            event_trace: Vec::new(),
            _state_manager: UnifiedStateManager::new(),
            metrics: MetricsCollector::new(),
            checkpoints: VecDeque::new(),
        }
    }

    /// Enable automatic checkpointing every N ticks
    pub fn with_auto_checkpoints(mut self, interval: u64) -> Self {
        self.config.performance.checkpoint_interval_ticks = interval;
        self
    }

    /// Set maximum number of checkpoints to keep
    pub fn with_max_checkpoints(mut self, max_checkpoints: usize) -> Self {
        self.config.performance.max_checkpoints = max_checkpoints;
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

        // Record metrics for this step
        self.metrics.counter("simulation_steps_total", 1);
        self.metrics
            .counter("events_generated", events.len() as u64);

        // Handle automatic checkpointing
        let checkpoint_interval = self.config.performance.checkpoint_interval_ticks;
        if checkpoint_interval > 0 && self.world.current_tick.is_multiple_of(checkpoint_interval) {
            let label = format!("auto_checkpoint_tick_{}", self.world.current_tick);
            let _ = self.create_checkpoint(Some(label));
        }

        // Step completed

        Ok(events)
    }

    /// Run multiple steps
    pub fn step_n(&mut self, count: u64) -> Result<Vec<TraceEvent>> {
        self.step_n_with_idle_check(count, true)
    }

    /// Run multiple steps with optional idle check
    pub fn step_n_with_idle_check(
        &mut self,
        count: u64,
        check_idle: bool,
    ) -> Result<Vec<TraceEvent>> {
        let mut all_events = Vec::new();

        for _ in 0..count {
            let events = self.step()?;
            all_events.extend(events);

            if check_idle && !self.world.should_continue() {
                break;
            }
        }

        Ok(all_events)
    }

    /// Run exactly N steps without idle checking
    pub fn step_exactly(&mut self, count: u64) -> Result<Vec<TraceEvent>> {
        self.step_n_with_idle_check(count, false)
    }

    /// Run simulation until completion or stopping condition
    // SAFETY: timing measurement for simulation diagnostics
    #[allow(clippy::disallowed_methods)]
    pub fn run_until_complete(&mut self) -> Result<SimulationRunResult> {
        let start_time = std::time::SystemTime::now();
        let mut _total_events = 0;

        while self.world.should_continue() {
            let events = self.step()?;
            _total_events += events.len();

            // Check for various stopping conditions
            if self.world.current_tick >= self.config.simulation.max_ticks {
                return Ok(self.build_run_result(
                    ExecutionStatus::Success,
                    StopReason::MaxTicksReached,
                    start_time,
                ));
            }

            if self.world.current_time >= self.config.simulation.max_time_ms {
                return Ok(self.build_run_result(
                    ExecutionStatus::Success,
                    StopReason::MaxTimeReached,
                    start_time,
                ));
            }

            if self.world.is_idle() {
                return Ok(self.build_run_result(
                    ExecutionStatus::Success,
                    StopReason::BecameIdle,
                    start_time,
                ));
            }
        }

        Ok(self.build_run_result(ExecutionStatus::Success, StopReason::BecameIdle, start_time))
    }

    /// Run simulation until idle (no more work to do)
    // SAFETY: timing measurement for simulation diagnostics
    #[allow(clippy::disallowed_methods)]
    pub fn run_until_idle(&mut self) -> Result<SimulationRunResult> {
        let start_time = std::time::SystemTime::now();

        while !self.world.is_idle() && self.world.should_continue() {
            self.step()?;
        }

        let stop_reason = if self.world.is_idle() {
            StopReason::BecameIdle
        } else {
            StopReason::MaxTicksReached
        };

        Ok(self.build_run_result(ExecutionStatus::Success, stop_reason, start_time))
    }

    /// Run simulation for a specific number of ticks
    // SAFETY: timing measurement for simulation diagnostics
    #[allow(clippy::disallowed_methods)]
    pub fn run_for_ticks(&mut self, tick_count: u64) -> Result<SimulationRunResult> {
        let start_time = std::time::SystemTime::now();
        let target_tick = self.world.current_tick + tick_count;

        // Run for exact tick count, even if idle
        while self.world.current_tick < target_tick {
            self.step()?;
        }

        Ok(self.build_run_result(ExecutionStatus::Success, StopReason::ManualStop, start_time))
    }

    /// Helper method to build consistent simulation run results
    fn build_run_result(
        &self,
        status: ExecutionStatus,
        stop_reason: StopReason,
        start_time: std::time::SystemTime,
    ) -> SimulationRunResult {
        use crate::results::builder::SimulationRunResultBuilder;

        let duration_ms = start_time.elapsed().unwrap_or_default().as_millis() as u64;

        let final_state = SimulationStateSnapshot {
            tick: self.world.current_tick,
            time: self.world.current_time,
            participant_count: self.world.participants.len(),
            active_sessions: self.world.protocols.active_sessions.len(),
            completed_sessions: self.world.protocols.completed_sessions.len(),
            state_hash: self.world.snapshot().state_hash,
        };

        let performance_summary = PerformanceMetrics::with_duration_ms(duration_ms)
            .with_items_processed(self.event_trace.len())
            .with_counter("total_ticks", self.world.current_tick)
            .with_counter("total_events", self.event_trace.len() as u64);

        SimulationRunResultBuilder::new(self.world.current_tick, self.world.current_time)
            .status(status)
            .stop_reason(stop_reason)
            .total_events(self.event_trace.len())
            .final_state(final_state)
            .performance_summary(performance_summary)
            .build()
    }

    /// Create a checkpoint of current state
    pub fn create_checkpoint(&mut self, label: Option<String>) -> Result<String> {
        let checkpoint_id = uuid::Uuid::from_u128(400 + self.checkpoints.len() as u128).to_string(); // Fixed UUID for deterministic testing

        let checkpoint = StateCheckpoint {
            id: checkpoint_id.clone(),
            label: label.clone(),
            tick: self.world.current_tick,
            world_state: self.world.clone(),
            events_up_to_here: self.event_trace.clone(),
            created_at: crate::utils::time::current_unix_timestamp_secs(),
        };

        self.checkpoints.push_back(checkpoint);

        // Trim old checkpoints if we exceed the limit
        while self.checkpoints.len() > self.config.performance.max_checkpoints {
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
                AuraError::configuration_error(format!("Checkpoint {} not found", checkpoint_id))
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

    /// Get simulation statistics from unified metrics
    pub fn get_metrics_snapshot(&self) -> crate::metrics::MetricsSnapshot {
        self.metrics.snapshot()
    }

    /// Get simulation statistics summary
    pub fn get_statistics_summary(&self) -> crate::metrics::MetricsSummary {
        self.metrics.summary()
    }

    /// Update metrics with current world state
    pub fn update_metrics(&mut self) {
        self.metrics
            .gauge("current_tick", self.world.current_tick as f64);
        self.metrics
            .gauge("current_time", self.world.current_time as f64);
        self.metrics
            .gauge("participant_count", self.world.participants.len() as f64);
        self.metrics
            .gauge("active_sessions", self.world.active_session_count() as f64);
        self.metrics.gauge(
            "queued_protocols",
            self.world.queued_protocol_count() as f64,
        );
        self.metrics.gauge(
            "in_flight_messages",
            self.world.network.in_flight_messages.len() as f64,
        );
        self.metrics
            .gauge("checkpoints_created", self.checkpoints.len() as f64);
        self.metrics.gauge(
            "byzantine_participants",
            self.world.byzantine.byzantine_participants.len() as f64,
        );
        self.metrics.gauge(
            "network_partitions",
            self.world.network.partitions.len() as f64,
        );
    }
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

    /// TODO: Update test to match current checkpointing implementation
    #[test]
    #[ignore]
    fn test_auto_checkpointing() {
        let mut runner = FunctionalRunner::new(42).with_auto_checkpoints(2); // Checkpoint every 2 ticks

        // Run enough steps to trigger auto-checkpointing (without idle check)
        runner.step_exactly(5).unwrap();

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
        assert!(matches!(
            result.status,
            crate::results::ExecutionStatus::Success
        ));
    }

    /// TODO: Update test to match current statistics implementation
    #[test]
    #[ignore]
    fn test_statistics() {
        let mut runner = FunctionalRunner::new(42);

        runner
            .add_participant(
                "alice".to_string(),
                "device_alice".to_string(),
                "account_1".to_string(),
            )
            .unwrap();

        runner.step_exactly(3).unwrap();
        runner.create_checkpoint(Some("test".to_string())).unwrap();

        let stats = runner.get_statistics_summary();
        assert_eq!(stats.total_ticks, 3);
        assert_eq!(stats.completed_sessions, 0);
        assert_eq!(stats.failed_sessions, 0);
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

        runner.step_exactly(2).unwrap();

        let trace = runner.export_trace();
        assert_eq!(trace.metadata.seed, 42);
        assert_eq!(trace.metadata.total_ticks, 2);
        assert!(!trace.timeline.is_empty());
        assert_eq!(trace.participants.len(), 1);
    }
}
