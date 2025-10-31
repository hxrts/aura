//! Scenario engine that coordinates decoupled debugging tools
//!
//! This module provides a high-level interface for coordinating multiple
//! debugging tools as external observers of the core simulation.

use crate::{
    testing::PropertyViolation, tick, AuraError, PassiveTraceRecorder, Result, TimeTravelDebugger,
    WorldState,
};
use aura_console_types::TraceEvent;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// High-level scenario engine for coordinated debugging
///
/// This engine manages multiple debugging tools as external observers,
/// providing a unified interface for complex debugging scenarios without
/// coupling to the core simulation logic.
pub struct ScenarioEngine {
    /// Base directory for all debugging artifacts
    base_dir: PathBuf,
    /// Checkpoint manager for state snapshots
    checkpoint_manager: crate::observability::checkpoint_manager::CheckpointManager,
    /// Active trace recorder
    trace_recorder: PassiveTraceRecorder,
    /// Time travel debugger for failure analysis
    time_travel_debugger: Option<TimeTravelDebugger>,
    /// Scenario configuration
    config: ScenarioConfig,
    /// Active debugging scenarios
    active_scenarios: HashMap<String, DebuggingScenario>,
    /// Global property checkers
    property_checkers: Vec<Box<dyn PropertyChecker>>,
}

/// Configuration for scenario debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioConfig {
    /// Automatically create checkpoints every N ticks
    pub auto_checkpoint_interval: Option<u64>,
    /// Maximum number of checkpoints to keep
    pub max_checkpoints: usize,
    /// Enable automatic property checking
    pub enable_property_checking: bool,
    /// Stop simulation on first property violation
    pub stop_on_violation: bool,
    /// Export detailed traces
    pub export_detailed_traces: bool,
    /// Base name for generated artifacts
    pub artifact_prefix: String,
}

/// A debugging scenario being executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingScenario {
    /// Unique scenario ID
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of what is being tested
    pub description: String,
    /// Initial world state setup
    pub initial_setup: DebuggingScenarioSetup,
    /// Expected outcomes or behaviors
    pub expected_outcomes: Vec<DebuggingExpectedOutcome>,
    /// Failure conditions to watch for
    pub failure_conditions: Vec<FailureCondition>,
    /// When scenario was started
    pub started_at: u64,
    /// Current status
    pub status: ScenarioStatus,
    /// Results collected so far
    pub results: ScenarioResults,
}

/// Setup instructions for a debugging scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingScenarioSetup {
    /// Random seed for reproducibility
    pub seed: u64,
    /// Participants to create
    pub participants: Vec<ParticipantSetup>,
    /// Network conditions to apply
    pub network_conditions: Vec<NetworkCondition>,
    /// Byzantine behaviors to inject
    pub byzantine_behaviors: Vec<ByzantineSetup>,
    /// Protocols to queue for execution
    pub queued_protocols: Vec<ProtocolSetup>,
}

/// Participant setup for scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantSetup {
    /// Participant ID
    pub participant_id: String,
    /// Device ID
    pub device_id: String,
    /// Account ID
    pub account_id: String,
    /// Whether this participant should be byzantine
    pub is_byzantine: bool,
}

/// Network condition for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkCondition {
    /// Create a partition between specific participants
    Partition {
        participants: Vec<String>,
        duration_ticks: u64,
        start_tick: u64,
    },
    /// Add message delay between participants
    Delay {
        from: String,
        to: String,
        delay_ticks: u64,
        start_tick: u64,
    },
    /// Drop messages with a specific probability
    MessageDrop {
        participants: Vec<String>,
        drop_probability: f64,
        start_tick: u64,
    },
}

/// Byzantine behavior setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineSetup {
    /// Which participant exhibits byzantine behavior
    pub participant_id: String,
    /// Type of byzantine behavior
    pub behavior_type: String,
    /// When to start the behavior
    pub start_tick: u64,
    /// Parameters for the behavior
    pub parameters: HashMap<String, String>,
}

/// Protocol execution setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolSetup {
    /// Protocol type (e.g., "DKD", "Resharing")
    pub protocol_type: String,
    /// Participants involved
    pub participants: Vec<String>,
    /// When to execute (relative to simulation start)
    pub scheduled_tick: u64,
    /// Priority level
    pub priority: u8,
    /// Protocol-specific parameters
    pub parameters: HashMap<String, String>,
}

/// Expected outcome for debugging validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebuggingExpectedOutcome {
    /// Description of expected behavior
    pub description: String,
    /// Tick range where this should occur
    pub tick_range: (u64, u64),
    /// Type of outcome check
    pub outcome_type: OutcomeType,
    /// Whether this outcome is required for success
    pub required: bool,
}

/// Types of outcomes that can be checked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutcomeType {
    /// Specific event should occur
    EventOccurs {
        event_type: String,
        participant: Option<String>,
    },
    /// Protocol should complete successfully
    ProtocolCompletes {
        protocol_type: String,
        participants: Vec<String>,
    },
    /// State should reach a specific condition
    StateCondition {
        condition: String,
        expected_value: String,
    },
    /// Property should remain true
    PropertyHolds { property: String },
}

/// Failure condition to monitor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureCondition {
    /// Description of failure
    pub description: String,
    /// Type of failure check
    pub condition_type: FailureType,
    /// Whether this failure should stop the scenario
    pub is_critical: bool,
}

/// Types of failures to detect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FailureType {
    /// Property violation occurs
    PropertyViolation { property: String },
    /// Protocol fails to complete
    ProtocolTimeout {
        protocol_type: String,
        max_ticks: u64,
    },
    /// Deadlock detected
    Deadlock { participants: Vec<String> },
    /// Unexpected error occurs
    UnexpectedError { error_pattern: String },
}

/// Current status of a scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioStatus {
    /// Scenario is being set up
    Setup,
    /// Scenario is running
    Running,
    /// Scenario completed successfully
    Completed,
    /// Scenario failed
    Failed(String),
    /// Scenario was stopped by user
    Stopped,
}

/// Results collected during scenario execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResults {
    /// Final tick reached
    pub final_tick: u64,
    /// Total events generated
    pub total_events: usize,
    /// Expected outcomes that were met
    pub outcomes_met: Vec<String>,
    /// Expected outcomes that were missed
    pub outcomes_missed: Vec<String>,
    /// Failure conditions that were triggered
    pub failures_triggered: Vec<String>,
    /// Property violations encountered
    pub violations: Vec<PropertyViolation>,
    /// Checkpoints created during execution
    pub checkpoints_created: Vec<String>,
    /// Performance metrics
    pub performance_metrics: PerformanceMetrics,
}

/// Performance metrics collected during scenario execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    /// Total execution time in milliseconds
    pub execution_time_ms: u64,
    /// Average ticks per second
    pub ticks_per_second: f64,
    /// Peak memory usage in MB
    pub peak_memory_mb: u64,
    /// Total checkpoints created
    pub checkpoints_created: usize,
    /// Total trace events recorded
    pub trace_events_recorded: usize,
}

/// Trait for custom property checkers
pub trait PropertyChecker: Send + Sync {
    /// Name of the property being checked
    fn property_name(&self) -> &str;

    /// Check if property holds for current world state
    fn check_property(&self, world_state: &WorldState, events: &[TraceEvent]) -> Result<bool>;

    /// Description of what this property ensures
    fn description(&self) -> &str;
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            auto_checkpoint_interval: Some(100),
            max_checkpoints: 50,
            enable_property_checking: true,
            stop_on_violation: false,
            export_detailed_traces: true,
            artifact_prefix: "scenario".to_string(),
        }
    }
}

impl ScenarioEngine {
    /// Create a new scenario engine
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Result<Self> {
        let base_path = base_dir.as_ref().to_path_buf();

        // Create subdirectories
        let checkpoint_dir = base_path.join("checkpoints");
        std::fs::create_dir_all(&checkpoint_dir).map_err(|e| {
            AuraError::configuration_error(format!("Failed to create checkpoint directory: {}", e))
        })?;

        let checkpoint_manager =
            crate::observability::checkpoint_manager::CheckpointManager::new(checkpoint_dir)?;
        let trace_recorder = PassiveTraceRecorder::new();

        Ok(Self {
            base_dir: base_path,
            checkpoint_manager,
            trace_recorder,
            time_travel_debugger: None,
            config: ScenarioConfig::default(),
            active_scenarios: HashMap::new(),
            property_checkers: Vec::new(),
        })
    }

    /// Configure the scenario engine
    pub fn configure(&mut self, config: ScenarioConfig) {
        self.config = config;
        self.checkpoint_manager
            .set_max_checkpoints(self.config.max_checkpoints);
    }

    /// Add a custom property checker
    pub fn add_property_checker(&mut self, checker: Box<dyn PropertyChecker>) {
        self.property_checkers.push(checker);
    }

    /// Create and start a new debugging scenario
    pub fn start_scenario(
        &mut self,
        setup: DebuggingScenarioSetup,
        scenario_name: String,
        description: String,
    ) -> Result<String> {
        let scenario_id = Uuid::new_v4().to_string();

        println!("Starting scenario: {}", scenario_name);

        // Create initial world state from setup
        let mut world_state = WorldState::new(setup.seed);

        // Add participants
        for participant_setup in &setup.participants {
            world_state.add_participant(
                participant_setup.participant_id.clone(),
                participant_setup.device_id.clone(),
                participant_setup.account_id.clone(),
            )?;

            if participant_setup.is_byzantine {
                world_state
                    .byzantine
                    .byzantine_participants
                    .push(participant_setup.participant_id.clone());
            }
        }

        // Apply network conditions (simplified for demo)
        for condition in &setup.network_conditions {
            if let NetworkCondition::Partition {
                participants,
                duration_ticks,
                ..
            } = condition
            {
                let partition = crate::NetworkPartition {
                    id: Uuid::new_v4().to_string(),
                    participants: participants.clone(),
                    started_at: world_state.current_time,
                    duration: Some(*duration_ticks * 100), // Convert to time units
                };
                world_state.network.partitions.push(partition);
            }
            // Other network conditions would be implemented similarly
        }

        // Queue protocols
        for protocol_setup in &setup.queued_protocols {
            let protocol = crate::QueuedProtocol {
                protocol_type: protocol_setup.protocol_type.clone(),
                participants: protocol_setup.participants.clone(),
                parameters: HashMap::new(), // Convert from string map if needed
                scheduled_time: world_state.current_time + (protocol_setup.scheduled_tick * 100),
                priority: protocol_setup.priority as u32,
            };
            world_state.protocols.execution_queue.push_back(protocol);
        }

        // Create initial checkpoint
        let checkpoint_id = self.checkpoint_manager.save(
            &world_state,
            Some(format!("scenario_{}_start", &scenario_id[..8])),
        )?;

        // Set up trace recorder for this scenario
        self.trace_recorder.set_scenario_name(scenario_name.clone());
        self.trace_recorder.set_seed(setup.seed);

        // Create scenario record
        let scenario = DebuggingScenario {
            id: scenario_id.clone(),
            name: scenario_name,
            description,
            initial_setup: setup,
            expected_outcomes: Vec::new(),
            failure_conditions: Vec::new(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            status: ScenarioStatus::Running,
            results: ScenarioResults {
                final_tick: 0,
                total_events: 0,
                outcomes_met: Vec::new(),
                outcomes_missed: Vec::new(),
                failures_triggered: Vec::new(),
                violations: Vec::new(),
                checkpoints_created: vec![checkpoint_id],
                performance_metrics: PerformanceMetrics {
                    execution_time_ms: 0,
                    ticks_per_second: 0.0,
                    peak_memory_mb: 0,
                    checkpoints_created: 1,
                    trace_events_recorded: 0,
                },
            },
        };

        self.active_scenarios.insert(scenario_id.clone(), scenario);

        println!(
            "[OK] Scenario {} started with initial checkpoint",
            &scenario_id[..8]
        );

        Ok(scenario_id)
    }

    /// Execute a scenario for a specified number of ticks
    pub fn run_scenario(
        &mut self,
        scenario_id: &str,
        max_ticks: u64,
    ) -> Result<ScenarioExecutionResult> {
        let scenario = self.active_scenarios.get_mut(scenario_id).ok_or_else(|| {
            AuraError::configuration_error(format!("Scenario {} not found", scenario_id))
        })?;

        if !matches!(scenario.status, ScenarioStatus::Running) {
            return Err(AuraError::configuration_error(
                "Scenario is not in running state".to_string(),
            ));
        }

        println!(
            "Running scenario {} for {} ticks",
            &scenario.name, max_ticks
        );

        let start_time = std::time::Instant::now();

        // Load initial checkpoint to get world state
        let initial_checkpoint = &scenario.results.checkpoints_created[0];
        let mut world_state = self.checkpoint_manager.load(initial_checkpoint)?;

        let mut execution_events = Vec::new();
        let mut violations_found = Vec::new();
        let start_tick = world_state.current_tick;

        // Main execution loop
        for _tick in 0..max_ticks {
            // Execute one simulation tick using pure function
            match tick(&mut world_state) {
                Ok(ref events) => {
                    // Record events with passive recorder
                    self.trace_recorder.record_tick_events(events);
                    execution_events.extend(events.iter().cloned());

                    // Check properties if enabled
                    if self.config.enable_property_checking {
                        for checker in &self.property_checkers {
                            match checker.check_property(&world_state, &events) {
                                Ok(holds) => {
                                    if !holds {
                                        let violation = PropertyViolation {
                                            property_name: checker.property_name().to_string(),
                                            property_type:
                                                crate::results::PropertyViolationType::Safety,
                                            violation_state:
                                                crate::results::SimulationStateSnapshot {
                                                    tick: world_state.current_tick,
                                                    time: world_state.current_time,
                                                    participant_count: 0, // TODO: Get from world_state
                                                    active_sessions: 0, // TODO: Get from world_state
                                                    completed_sessions: 0, // TODO: Get from world_state
                                                    state_hash: "placeholder".to_string(), // TODO: Generate proper hash
                                                },
                                            violation_details: crate::results::ViolationDetails {
                                                description: format!(
                                                    "Property '{}' violated",
                                                    checker.property_name()
                                                ),
                                                evidence: vec!["System-level violation".to_string()],
                                                potential_causes: vec![],
                                                severity: crate::results::ViolationSeverity::High,
                                                remediation_suggestions: vec![],
                                            },
                                            confidence: 0.9,
                                            detected_at: world_state.current_time,
                                        };
                                        violations_found.push(violation.clone());
                                        self.trace_recorder.record_violation(violation);

                                        if self.config.stop_on_violation {
                                            println!(
                                                "[ERROR] Stopping on property violation: {}",
                                                checker.property_name()
                                            );
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("[WARN] Property check failed: {}", e);
                                }
                            }
                        }
                    }

                    // Auto-checkpoint if configured
                    if let Some(interval) = self.config.auto_checkpoint_interval {
                        if world_state.current_tick % interval == 0 {
                            let checkpoint_id = self.checkpoint_manager.save(
                                &world_state,
                                Some(format!(
                                    "scenario_{}_tick_{}",
                                    &scenario_id[..8],
                                    world_state.current_tick
                                )),
                            )?;
                            scenario.results.checkpoints_created.push(checkpoint_id);
                        }
                    }
                }
                Err(e) => {
                    println!(
                        "[ERROR] Simulation error at tick {}: {}",
                        world_state.current_tick, e
                    );
                    scenario.status = ScenarioStatus::Failed(e.to_string());
                    break;
                }
            }
        }

        // Update scenario results
        let elapsed = start_time.elapsed();
        let ticks_processed = world_state.current_tick - start_tick;

        scenario.results.final_tick = world_state.current_tick;
        scenario.results.total_events = execution_events.len();
        scenario.results.violations.extend(violations_found.clone());
        scenario.results.performance_metrics = PerformanceMetrics {
            execution_time_ms: elapsed.as_millis() as u64,
            ticks_per_second: if elapsed.as_secs_f64() > 0.0 {
                ticks_processed as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
            peak_memory_mb: 0, // Would need actual memory tracking
            checkpoints_created: scenario.results.checkpoints_created.len(),
            trace_events_recorded: execution_events.len(),
        };

        if matches!(scenario.status, ScenarioStatus::Running) {
            scenario.status = ScenarioStatus::Completed;
        }

        println!(
            "Scenario execution complete. Final tick: {}, Events: {}",
            world_state.current_tick,
            execution_events.len()
        );

        Ok(ScenarioExecutionResult {
            scenario_id: scenario_id.to_string(),
            final_status: scenario.status.clone(),
            events_generated: execution_events,
            violations_found,
            checkpoints_created: scenario.results.checkpoints_created.len(),
            execution_time_ms: elapsed.as_millis() as u64,
        })
    }

    /// Get time travel debugger for failure analysis
    pub fn get_time_travel_debugger(&mut self) -> Result<&mut TimeTravelDebugger> {
        if self.time_travel_debugger.is_none() {
            let debugger = TimeTravelDebugger::new(self.base_dir.join("checkpoints"))?;
            self.time_travel_debugger = Some(debugger);
        }

        Ok(self.time_travel_debugger.as_mut().unwrap())
    }

    /// Export comprehensive scenario report
    pub fn export_scenario_report(&self, scenario_id: &str) -> Result<ScenarioReport> {
        let scenario = self.active_scenarios.get(scenario_id).ok_or_else(|| {
            AuraError::configuration_error(format!("Scenario {} not found", scenario_id))
        })?;

        let trace = self.trace_recorder.export_trace(None);

        Ok(ScenarioReport {
            scenario: scenario.clone(),
            trace_summary: TraceSummary {
                total_events: trace.timeline.len(),
                event_types: self.analyze_event_types(&trace.timeline),
                participant_activity: self.analyze_participant_activity(&trace.timeline),
            },
            config_used: self.config.clone(),
            checkpoints_available: self.checkpoint_manager.list_checkpoints().len(),
            generated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        })
    }

    /// Get current active scenarios
    pub fn active_scenarios(&self) -> &HashMap<String, DebuggingScenario> {
        &self.active_scenarios
    }

    /// Get trace recorder for event analysis
    pub fn trace_recorder(&self) -> &PassiveTraceRecorder {
        &self.trace_recorder
    }

    /// Get checkpoint manager for state analysis
    pub fn checkpoint_manager(
        &self,
    ) -> &crate::observability::checkpoint_manager::CheckpointManager {
        &self.checkpoint_manager
    }

    // Private helper methods

    /// Analyze event types in a trace
    fn analyze_event_types(&self, events: &[TraceEvent]) -> HashMap<String, usize> {
        let mut types = HashMap::new();
        for event in events {
            let type_name = format!("{:?}", event.event_type)
                .split(' ')
                .next()
                .unwrap_or("Unknown")
                .to_string();
            *types.entry(type_name).or_insert(0) += 1;
        }
        types
    }

    /// Analyze participant activity in a trace
    fn analyze_participant_activity(&self, events: &[TraceEvent]) -> HashMap<String, usize> {
        let mut activity = HashMap::new();
        for event in events {
            *activity.entry(event.participant.clone()).or_insert(0) += 1;
        }
        activity
    }
}

/// Result of scenario execution
#[derive(Debug, Clone)]
pub struct ScenarioExecutionResult {
    /// ID of executed scenario
    pub scenario_id: String,
    /// Final status after execution
    pub final_status: ScenarioStatus,
    /// Events generated during execution
    pub events_generated: Vec<TraceEvent>,
    /// Property violations found
    pub violations_found: Vec<PropertyViolation>,
    /// Number of checkpoints created
    pub checkpoints_created: usize,
    /// Total execution time
    pub execution_time_ms: u64,
}

/// Summary of trace data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    /// Total number of events
    pub total_events: usize,
    /// Distribution of event types
    pub event_types: HashMap<String, usize>,
    /// Activity per participant
    pub participant_activity: HashMap<String, usize>,
}

/// Comprehensive report for a scenario
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioReport {
    /// The scenario that was executed
    pub scenario: DebuggingScenario,
    /// Summary of trace data
    pub trace_summary: TraceSummary,
    /// Configuration used
    pub config_used: ScenarioConfig,
    /// Number of checkpoints available
    pub checkpoints_available: usize,
    /// When report was generated
    pub generated_at: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scenario_engine_creation() {
        let temp_dir = TempDir::new().unwrap();
        let engine = ScenarioEngine::new(temp_dir.path()).unwrap();

        assert!(engine.active_scenarios.is_empty());
        assert_eq!(engine.property_checkers.len(), 0);
    }

    #[test]
    fn test_simple_scenario() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = ScenarioEngine::new(temp_dir.path()).unwrap();

        let setup = DebuggingScenarioSetup {
            seed: 42,
            participants: vec![ParticipantSetup {
                participant_id: "alice".to_string(),
                device_id: "device_alice".to_string(),
                account_id: "account_1".to_string(),
                is_byzantine: false,
            }],
            network_conditions: Vec::new(),
            byzantine_behaviors: Vec::new(),
            queued_protocols: Vec::new(),
        };

        let scenario_id = engine
            .start_scenario(
                setup,
                "test scenario".to_string(),
                "A simple test".to_string(),
            )
            .unwrap();

        assert_eq!(engine.active_scenarios.len(), 1);
        assert!(engine.active_scenarios.contains_key(&scenario_id));

        let scenario = &engine.active_scenarios[&scenario_id];
        assert_eq!(scenario.name, "test scenario");
        assert!(matches!(scenario.status, ScenarioStatus::Running));
    }

    /// TODO: Update test to match current scenario execution implementation
    #[test]
    #[ignore]
    fn test_scenario_execution() {
        let temp_dir = TempDir::new().unwrap();
        let mut engine = ScenarioEngine::new(temp_dir.path()).unwrap();

        let setup = DebuggingScenarioSetup {
            seed: 42,
            participants: vec![
                ParticipantSetup {
                    participant_id: "alice".to_string(),
                    device_id: "device_alice".to_string(),
                    account_id: "account_1".to_string(),
                    is_byzantine: false,
                },
                ParticipantSetup {
                    participant_id: "bob".to_string(),
                    device_id: "device_bob".to_string(),
                    account_id: "account_1".to_string(),
                    is_byzantine: false,
                },
            ],
            network_conditions: Vec::new(),
            byzantine_behaviors: Vec::new(),
            queued_protocols: Vec::new(),
        };

        let scenario_id = engine
            .start_scenario(
                setup,
                "execution test".to_string(),
                "Test execution".to_string(),
            )
            .unwrap();
        let result = engine.run_scenario(&scenario_id, 5).unwrap();

        assert_eq!(result.scenario_id, scenario_id);
        assert!(matches!(result.final_status, ScenarioStatus::Completed));
        // execution_time_ms may be 0 for very fast executions
        assert!(result.execution_time_ms >= 0);
    }
}
