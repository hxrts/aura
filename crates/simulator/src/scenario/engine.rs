//! Unified Scenario Engine - Canonical Entry Point for All Tests
//!
//! This engine unifies declarative TOML scenarios and imperative choreography helpers
//! into a single, powerful testing framework. All tests, from simple protocol checks
//! to complex multi-phase chaos scenarios, are defined declaratively.

use crate::{tick, AuraError, PassiveTraceRecorder, Result, TimeTravelDebugger, WorldState};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Unified scenario engine that serves as the canonical entry point for all tests
pub struct UnifiedScenarioEngine {
    /// Base directory for artifacts and debugging data
    base_dir: PathBuf,
    /// Debugging tools integration
    _debug_engine: crate::observability::ScenarioEngine,
    /// Choreography action registry
    choreography_registry: ChoreographyActionRegistry,
    /// Configuration for scenario execution
    config: UnifiedEngineConfig,
    /// Active debugging tools
    trace_recorder: Option<PassiveTraceRecorder>,
    checkpoint_manager: Option<crate::observability::checkpoint_manager::CheckpointManager>,
    _time_travel_debugger: Option<TimeTravelDebugger>,
}

/// Configuration for the unified engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedEngineConfig {
    /// Enable debugging tools and trace recording
    pub enable_debugging: bool,
    /// Auto-checkpoint interval in ticks
    pub auto_checkpoint_interval: Option<u64>,
    /// Maximum execution time before timeout
    pub max_execution_time: Duration,
    /// Enable verbose logging
    pub verbose: bool,
    /// Export detailed reports after execution
    pub export_reports: bool,
    /// Base name for generated artifacts
    pub artifact_prefix: String,
}

/// Extended scenario definition with choreography actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedScenario {
    /// Basic scenario metadata
    pub name: String,
    pub description: String,

    /// Setup configuration
    pub setup: ScenarioSetupConfig,

    /// Phases with choreography actions
    pub phases: Vec<ScenarioPhaseWithActions>,

    /// Network conditions
    pub network: Option<NetworkConfig>,

    /// Byzantine behavior configuration
    pub byzantine: Option<ByzantineConfig>,

    /// Properties to check during execution
    pub properties: Vec<PropertyCheck>,

    /// Expected final outcome
    pub expected_outcome: ExpectedOutcome,

    /// Scenario inheritance
    pub extends: Option<String>,
}

/// Setup configuration for scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioSetupConfig {
    /// Number of participants
    pub participants: usize,
    /// Threshold for protocols
    pub threshold: usize,
    /// Random seed for reproducibility
    pub seed: u64,
    /// Participant roles and configurations
    pub participant_configs: Option<HashMap<String, ParticipantConfig>>,
}

/// Configuration for individual participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantConfig {
    /// Device ID for this participant
    pub device_id: String,
    /// Account ID
    pub account_id: String,
    /// Whether this participant is byzantine
    pub is_byzantine: bool,
    /// Role in the scenario (optional)
    pub role: Option<String>,
}

/// Scenario phase with choreography actions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioPhaseWithActions {
    /// Phase name
    pub name: String,
    /// Description of what this phase tests
    pub description: Option<String>,
    /// Actions to execute in this phase
    pub actions: Vec<ChoreographyAction>,
    /// Checkpoints to create during this phase
    pub checkpoints: Option<Vec<String>>,
    /// Properties to verify after this phase
    pub verify_properties: Option<Vec<String>>,
    /// Maximum time to spend in this phase
    pub timeout: Option<Duration>,
}

/// Choreography action that can be invoked declaratively
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChoreographyAction {
    /// Run a specific choreography
    #[serde(rename = "run_choreography")]
    RunChoreography {
        choreography_type: String,
        participants: Option<Vec<String>>,
        parameters: HashMap<String, toml::Value>,
    },

    /// Execute a protocol
    #[serde(rename = "execute_protocol")]
    ExecuteProtocol {
        protocol_type: String,
        participants: Vec<String>,
        timeout_ticks: Option<u64>,
        parameters: Option<HashMap<String, toml::Value>>,
    },

    /// Apply network conditions
    #[serde(rename = "apply_network_condition")]
    ApplyNetworkCondition {
        condition_type: String,
        participants: Vec<String>,
        duration_ticks: Option<u64>,
        parameters: HashMap<String, toml::Value>,
    },

    /// Inject Byzantine behavior
    #[serde(rename = "inject_byzantine")]
    InjectByzantine {
        participant: String,
        behavior_type: String,
        parameters: HashMap<String, toml::Value>,
    },

    /// Wait for a specific number of ticks
    #[serde(rename = "wait_ticks")]
    WaitTicks { ticks: u64 },

    /// Create a checkpoint
    #[serde(rename = "create_checkpoint")]
    CreateCheckpoint { label: String },

    /// Verify a property
    #[serde(rename = "verify_property")]
    VerifyProperty { property: String, expected: bool },

    /// Custom action with arbitrary parameters
    #[serde(rename = "custom")]
    Custom {
        action_name: String,
        parameters: HashMap<String, toml::Value>,
    },
}

/// Network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Latency range in milliseconds
    pub latency_range: Option<[u64; 2]>,
    /// Message drop rate (0.0 to 1.0)
    pub drop_rate: Option<f64>,
    /// Network partitions
    pub partitions: Option<Vec<Vec<String>>>,
}

/// Byzantine behavior configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineConfig {
    /// Number of byzantine participants
    pub count: usize,
    /// Specific participants to make byzantine
    pub participants: Option<Vec<String>>,
    /// Default byzantine strategies
    pub default_strategies: Option<Vec<String>>,
}

/// Property check configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheck {
    /// Property name
    pub name: String,
    /// Property implementation type
    pub property_type: String,
    /// Parameters for the property checker
    pub parameters: Option<HashMap<String, toml::Value>>,
    /// When to check this property (phase names)
    pub check_in_phases: Option<Vec<String>>,
}

/// Expected outcome of scenario execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpectedOutcome {
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "failure")]
    Failure,
    #[serde(rename = "property_violation")]
    PropertyViolation { property: String },
    #[serde(rename = "timeout")]
    Timeout,
    #[serde(rename = "byzantine_detected")]
    ByzantineDetected,
}

/// Registry for choreography actions
pub struct ChoreographyActionRegistry {
    /// Registered choreography executors
    choreographies: HashMap<String, Box<dyn ChoreographyExecutor>>,
    /// Registered protocol executors
    protocols: HashMap<String, Box<dyn ProtocolExecutor>>,
    /// Registered network condition handlers
    _network_conditions: HashMap<String, Box<dyn NetworkConditionHandler>>,
    /// Registered byzantine behavior injectors
    _byzantine_behaviors: HashMap<String, Box<dyn ByzantineBehaviorInjector>>,
}

/// Trait for executing choreographies
pub trait ChoreographyExecutor: Send + Sync {
    /// Execute the choreography with given parameters
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult>;

    /// Get description of this choreography
    fn description(&self) -> &str;
}

/// Trait for executing protocols
pub trait ProtocolExecutor: Send + Sync {
    /// Execute the protocol
    fn execute(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &Option<HashMap<String, toml::Value>>,
    ) -> Result<ProtocolResult>;

    /// Get description of this protocol
    fn description(&self) -> &str;
}

/// Trait for applying network conditions
pub trait NetworkConditionHandler: Send + Sync {
    /// Apply the network condition
    fn apply(
        &self,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()>;

    /// Get description of this network condition
    fn description(&self) -> &str;
}

/// Trait for injecting byzantine behaviors
pub trait ByzantineBehaviorInjector: Send + Sync {
    /// Inject byzantine behavior into a participant
    fn inject(
        &self,
        world_state: &mut WorldState,
        participant: &str,
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<()>;

    /// Get description of this byzantine behavior
    fn description(&self) -> &str;
}

/// Result of choreography execution
#[derive(Debug, Clone)]
pub struct ChoreographyResult {
    /// Whether choreography completed successfully
    pub success: bool,
    /// Events generated during choreography
    pub events_generated: usize,
    /// Execution time
    pub execution_time: Duration,
    /// Any additional data produced
    pub data: HashMap<String, String>,
}

/// Result of protocol execution
#[derive(Debug, Clone)]
pub struct ProtocolResult {
    /// Whether protocol completed successfully
    pub success: bool,
    /// Final state of the protocol
    pub final_state: String,
    /// Execution time
    pub execution_time: Duration,
    /// Protocol-specific output data
    pub output_data: Vec<u8>,
}

/// Result of unified scenario execution
#[derive(Debug, Clone)]
pub struct UnifiedScenarioResult {
    /// Scenario name
    pub scenario_name: String,
    /// Overall success status
    pub success: bool,
    /// Results from each phase
    pub phase_results: Vec<PhaseResult>,
    /// Property check results
    pub property_results: Vec<PropertyResult>,
    /// Total execution time
    pub execution_time: Duration,
    /// Debugging artifacts generated
    pub artifacts: Vec<String>,
    /// Final world state summary
    pub final_state: WorldStateSummary,
}

/// Result of executing a single phase
#[derive(Debug, Clone)]
pub struct PhaseResult {
    /// Phase name
    pub phase_name: String,
    /// Whether phase completed successfully
    pub success: bool,
    /// Results from individual actions
    pub action_results: Vec<ActionResult>,
    /// Phase execution time
    pub execution_time: Duration,
    /// Checkpoints created in this phase
    pub checkpoints_created: Vec<String>,
}

/// Result of executing a single action
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// Action type
    pub action_type: String,
    /// Whether action completed successfully
    pub success: bool,
    /// Action execution time
    pub execution_time: Duration,
    /// Events generated by this action
    pub events_generated: usize,
    /// Error message if action failed
    pub error_message: Option<String>,
}

/// Result of property verification
#[derive(Debug, Clone)]
pub struct PropertyResult {
    /// Property name
    pub property_name: String,
    /// Whether property holds
    pub holds: bool,
    /// Violation details if property doesn't hold
    pub violation_details: Option<String>,
    /// When property was checked
    pub checked_at_tick: u64,
}

/// Summary of world state
#[derive(Debug, Clone)]
pub struct WorldStateSummary {
    /// Current tick
    pub current_tick: u64,
    /// Number of participants
    pub participant_count: usize,
    /// Number of active protocols
    pub active_protocols: usize,
    /// Number of byzantine participants
    pub byzantine_count: usize,
    /// Network partition status
    pub network_partitions: usize,
}

impl Default for UnifiedEngineConfig {
    fn default() -> Self {
        Self {
            enable_debugging: true,
            auto_checkpoint_interval: Some(50),
            max_execution_time: Duration::from_secs(60),
            verbose: false,
            export_reports: true,
            artifact_prefix: "unified_scenario".to_string(),
        }
    }
}

impl UnifiedScenarioEngine {
    /// Create a new unified scenario engine
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Result<Self> {
        let base_path = base_dir.as_ref().to_path_buf();

        // Create debugging engine
        let debug_engine = crate::observability::ScenarioEngine::new(&base_path)?;

        // Create choreography registry
        let choreography_registry = ChoreographyActionRegistry::new();

        Ok(Self {
            base_dir: base_path,
            _debug_engine: debug_engine,
            choreography_registry,
            config: UnifiedEngineConfig::default(),
            trace_recorder: None,
            checkpoint_manager: None,
            _time_travel_debugger: None,
        })
    }

    /// Configure the engine
    pub fn configure(mut self, config: UnifiedEngineConfig) -> Self {
        self.config = config;
        self
    }

    /// Get a reference to the engine configuration
    pub fn config(&self) -> &UnifiedEngineConfig {
        &self.config
    }

    /// Register a choreography executor
    pub fn register_choreography<T: ChoreographyExecutor + 'static>(
        &mut self,
        name: String,
        executor: T,
    ) {
        self.choreography_registry
            .register_choreography(name, Box::new(executor));
    }

    /// Register a protocol executor
    pub fn register_protocol<T: ProtocolExecutor + 'static>(&mut self, name: String, executor: T) {
        self.choreography_registry
            .register_protocol(name, Box::new(executor));
    }

    /// Execute a unified scenario
    pub fn execute_scenario(
        &mut self,
        scenario: &UnifiedScenario,
    ) -> Result<UnifiedScenarioResult> {
        let start_time = Instant::now();

        if self.config.verbose {
            println!("Executing unified scenario: {}", scenario.name);
            println!("Description: {}", scenario.description);
        }

        // Initialize debugging tools if enabled
        if self.config.enable_debugging {
            self.initialize_debugging_tools()?;
        }

        // Create initial world state
        let mut world_state = self.create_world_state(&scenario.setup)?;

        // Apply network conditions
        if let Some(network) = &scenario.network {
            self.apply_network_configuration(&mut world_state, network)?;
        }

        // Apply byzantine configuration
        if let Some(byzantine) = &scenario.byzantine {
            self.apply_byzantine_configuration(&mut world_state, byzantine, &scenario.setup)?;
        }

        // Create initial checkpoint
        let _initial_checkpoint = if self.config.enable_debugging {
            Some(self.create_checkpoint(&world_state, "scenario_start".to_string())?)
        } else {
            None
        };

        let mut phase_results = Vec::new();
        let mut all_phases_success = true;

        // Execute each phase
        for phase in &scenario.phases {
            if self.config.verbose {
                println!("Executing phase: {}", phase.name);
            }

            let phase_result = self.execute_phase(&mut world_state, phase)?;

            if !phase_result.success {
                all_phases_success = false;
                if self.config.verbose {
                    println!("[ERROR] Phase '{}' failed", phase.name);
                }
            }

            phase_results.push(phase_result);
        }

        // Verify properties
        let property_results = self.verify_properties(&world_state, &scenario.properties)?;

        // Check if any properties failed
        let properties_success = property_results.iter().all(|r| r.holds);

        // Determine overall success
        let success = all_phases_success
            && properties_success
            && self.check_expected_outcome(&world_state, &scenario.expected_outcome)?;

        // Generate artifacts
        let artifacts = if self.config.export_reports {
            self.generate_artifacts(&scenario.name)?
        } else {
            Vec::new()
        };

        let execution_time = start_time.elapsed();

        if self.config.verbose {
            if success {
                println!(
                    "[OK] Scenario '{}' completed successfully in {:?}",
                    scenario.name, execution_time
                );
            } else {
                println!(
                    "[ERROR] Scenario '{}' failed after {:?}",
                    scenario.name, execution_time
                );
            }
        }

        Ok(UnifiedScenarioResult {
            scenario_name: scenario.name.clone(),
            success,
            phase_results,
            property_results,
            execution_time,
            artifacts,
            final_state: self.create_world_state_summary(&world_state),
        })
    }

    /// Execute multiple scenarios
    pub fn execute_scenario_suite(
        &mut self,
        scenarios: &[UnifiedScenario],
    ) -> Result<Vec<UnifiedScenarioResult>> {
        let mut results = Vec::new();

        for scenario in scenarios {
            let result = self.execute_scenario(scenario)?;
            results.push(result);
        }

        Ok(results)
    }

    // Private implementation methods

    fn initialize_debugging_tools(&mut self) -> Result<()> {
        if self.trace_recorder.is_none() {
            self.trace_recorder = Some(PassiveTraceRecorder::new());
        }

        if self.checkpoint_manager.is_none() {
            let checkpoint_dir = self.base_dir.join("checkpoints");
            self.checkpoint_manager = Some(
                crate::observability::checkpoint_manager::CheckpointManager::new(checkpoint_dir)?,
            );
        }

        Ok(())
    }

    fn create_world_state(&self, setup: &ScenarioSetupConfig) -> Result<WorldState> {
        let mut world = WorldState::new(setup.seed);

        // Add participants
        if let Some(configs) = &setup.participant_configs {
            for (participant_id, config) in configs {
                world.add_participant(
                    participant_id.clone(),
                    config.device_id.clone(),
                    config.account_id.clone(),
                )?;
            }
        } else {
            // Create default participants
            for i in 0..setup.participants {
                let participant_id = format!("participant_{}", i);
                let device_id = format!("device_{}", i);
                let account_id = "default_account".to_string();

                world.add_participant(participant_id, device_id, account_id)?;
            }
        }

        Ok(world)
    }

    fn apply_network_configuration(
        &self,
        world_state: &mut WorldState,
        network: &NetworkConfig,
    ) -> Result<()> {
        // Apply network partitions
        if let Some(partitions) = &network.partitions {
            for partition in partitions {
                let network_partition = crate::NetworkPartition {
                    id: Uuid::new_v4().to_string(),
                    participants: partition.clone(),
                    started_at: world_state.current_time,
                    duration: None, // Persistent partition
                };
                world_state.network.partitions.push(network_partition);
            }
        }

        // Note: latency_range and drop_rate would be applied to the network fabric
        // This would require extending the NetworkFabric structure

        Ok(())
    }

    fn apply_byzantine_configuration(
        &self,
        world_state: &mut WorldState,
        byzantine: &ByzantineConfig,
        setup: &ScenarioSetupConfig,
    ) -> Result<()> {
        let participants_to_corrupt = if let Some(specific) = &byzantine.participants {
            specific.clone()
        } else {
            // Select first N participants to be byzantine
            let mut participants = Vec::new();
            for i in 0..byzantine.count.min(setup.participants) {
                participants.push(format!("participant_{}", i));
            }
            participants
        };

        for participant in participants_to_corrupt {
            world_state
                .byzantine
                .byzantine_participants
                .push(participant.clone());

            // Apply default strategy if specified
            if let Some(strategies) = &byzantine.default_strategies {
                if let Some(first_strategy) = strategies.first() {
                    let strategy = match first_strategy.as_str() {
                        "drop_all_messages" => crate::ByzantineStrategy::DropAllMessages,
                        "delay_messages" => {
                            crate::ByzantineStrategy::DelayMessages { delay_ms: 5000 }
                        }
                        "invalid_signatures" => crate::ByzantineStrategy::InvalidSignatures,
                        "conflicting_messages" => crate::ByzantineStrategy::ConflictingMessages,
                        _ => crate::ByzantineStrategy::DropAllMessages, // Default fallback
                    };
                    world_state
                        .byzantine
                        .active_strategies
                        .insert(participant, strategy);
                }
            }
        }

        Ok(())
    }

    fn execute_phase(
        &mut self,
        world_state: &mut WorldState,
        phase: &ScenarioPhaseWithActions,
    ) -> Result<PhaseResult> {
        let start_time = Instant::now();
        let mut action_results = Vec::new();
        let mut checkpoints_created = Vec::new();
        let mut phase_success = true;

        for action in &phase.actions {
            let action_result = self.execute_action(world_state, action)?;

            if !action_result.success {
                phase_success = false;
            }

            action_results.push(action_result);
        }

        // Create checkpoints if specified
        if let Some(checkpoint_labels) = &phase.checkpoints {
            for label in checkpoint_labels {
                if self.config.enable_debugging {
                    let checkpoint_id = self.create_checkpoint(world_state, label.clone())?;
                    checkpoints_created.push(checkpoint_id);
                }
            }
        }

        // Verify phase-specific properties
        if let Some(property_names) = &phase.verify_properties {
            for property_name in property_names {
                // This would verify specific properties for this phase
                if self.config.verbose {
                    println!(
                        "Verifying property '{}' for phase '{}'",
                        property_name, phase.name
                    );
                }
            }
        }

        Ok(PhaseResult {
            phase_name: phase.name.clone(),
            success: phase_success,
            action_results,
            execution_time: start_time.elapsed(),
            checkpoints_created,
        })
    }

    fn execute_action(
        &mut self,
        world_state: &mut WorldState,
        action: &ChoreographyAction,
    ) -> Result<ActionResult> {
        let start_time = Instant::now();

        match action {
            ChoreographyAction::RunChoreography {
                choreography_type,
                participants,
                parameters,
            } => {
                let participants = participants.as_ref().map(|p| p.as_slice()).unwrap_or(&[]);
                let result = self.choreography_registry.execute_choreography(
                    choreography_type,
                    world_state,
                    participants,
                    parameters,
                )?;

                Ok(ActionResult {
                    action_type: "run_choreography".to_string(),
                    success: result.success,
                    execution_time: start_time.elapsed(),
                    events_generated: result.events_generated,
                    error_message: if result.success {
                        None
                    } else {
                        Some("Choreography execution failed".to_string())
                    },
                })
            }

            ChoreographyAction::ExecuteProtocol {
                protocol_type,
                participants,
                timeout_ticks: _,
                parameters,
            } => {
                let result = self.choreography_registry.execute_protocol(
                    protocol_type,
                    world_state,
                    participants,
                    parameters,
                )?;

                Ok(ActionResult {
                    action_type: "execute_protocol".to_string(),
                    success: result.success,
                    execution_time: start_time.elapsed(),
                    events_generated: 1, // Simplified
                    error_message: if result.success {
                        None
                    } else {
                        Some("Protocol execution failed".to_string())
                    },
                })
            }

            ChoreographyAction::WaitTicks { ticks } => {
                // Execute simulation for specified ticks
                for _ in 0..*ticks {
                    let events = tick(world_state)?;
                    if self.config.enable_debugging {
                        if let Some(recorder) = &mut self.trace_recorder {
                            recorder.record_tick_events(&events);
                        }
                    }
                }

                Ok(ActionResult {
                    action_type: "wait_ticks".to_string(),
                    success: true,
                    execution_time: start_time.elapsed(),
                    events_generated: *ticks as usize,
                    error_message: None,
                })
            }

            ChoreographyAction::CreateCheckpoint { label } => {
                if self.config.enable_debugging {
                    let _checkpoint_id = self.create_checkpoint(world_state, label.clone())?;

                    Ok(ActionResult {
                        action_type: "create_checkpoint".to_string(),
                        success: true,
                        execution_time: start_time.elapsed(),
                        events_generated: 0,
                        error_message: None,
                    })
                } else {
                    Ok(ActionResult {
                        action_type: "create_checkpoint".to_string(),
                        success: true,
                        execution_time: start_time.elapsed(),
                        events_generated: 0,
                        error_message: Some("Debugging not enabled".to_string()),
                    })
                }
            }

            _ => {
                // Placeholder for other action types
                Ok(ActionResult {
                    action_type: "unknown".to_string(),
                    success: true,
                    execution_time: start_time.elapsed(),
                    events_generated: 0,
                    error_message: Some("Action type not yet implemented".to_string()),
                })
            }
        }
    }

    fn verify_properties(
        &self,
        world_state: &WorldState,
        properties: &[PropertyCheck],
    ) -> Result<Vec<PropertyResult>> {
        let mut results = Vec::new();

        for property in properties {
            // This would use the actual property checking framework
            let holds = true; // Placeholder

            results.push(PropertyResult {
                property_name: property.name.clone(),
                holds,
                violation_details: if holds {
                    None
                } else {
                    Some("Property violation detected".to_string())
                },
                checked_at_tick: world_state.current_tick,
            });
        }

        Ok(results)
    }

    fn check_expected_outcome(
        &self,
        world_state: &WorldState,
        expected: &ExpectedOutcome,
    ) -> Result<bool> {
        match expected {
            ExpectedOutcome::Success => Ok(true),  // Placeholder
            ExpectedOutcome::Failure => Ok(false), // Would check for expected failure
            ExpectedOutcome::PropertyViolation { property: _ } => Ok(false), // Would check for specific violation
            ExpectedOutcome::Timeout => Ok(false), // Would check for timeout
            ExpectedOutcome::ByzantineDetected => {
                Ok(!world_state.byzantine.byzantine_participants.is_empty())
            }
        }
    }

    fn create_checkpoint(&mut self, world_state: &WorldState, label: String) -> Result<String> {
        if let Some(checkpoint_manager) = &mut self.checkpoint_manager {
            checkpoint_manager.save(world_state, Some(label))
        } else {
            Err(AuraError::configuration_error(
                "Checkpoint manager not initialized".to_string(),
            ))
        }
    }

    fn generate_artifacts(&self, scenario_name: &str) -> Result<Vec<String>> {
        let mut artifacts = Vec::new();

        // Export trace if available
        if let Some(recorder) = &self.trace_recorder {
            let trace_path = self.base_dir.join(format!("{}_trace.json", scenario_name));
            recorder.save_session(&trace_path)?;
            artifacts.push(trace_path.to_string_lossy().to_string());
        }

        Ok(artifacts)
    }

    fn create_world_state_summary(&self, world_state: &WorldState) -> WorldStateSummary {
        WorldStateSummary {
            current_tick: world_state.current_tick,
            participant_count: world_state.participants.len(),
            active_protocols: world_state.protocols.active_sessions.len(),
            byzantine_count: world_state.byzantine.byzantine_participants.len(),
            network_partitions: world_state.network.partitions.len(),
        }
    }
}

impl Default for ChoreographyActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ChoreographyActionRegistry {
    /// Create a new choreography action registry
    pub fn new() -> Self {
        Self {
            choreographies: HashMap::new(),
            protocols: HashMap::new(),
            network_conditions: HashMap::new(),
            byzantine_behaviors: HashMap::new(),
        }
    }

    /// Register a choreography executor
    pub fn register_choreography(&mut self, name: String, executor: Box<dyn ChoreographyExecutor>) {
        self.choreographies.insert(name, executor);
    }

    /// Register a protocol executor
    pub fn register_protocol(&mut self, name: String, executor: Box<dyn ProtocolExecutor>) {
        self.protocols.insert(name, executor);
    }

    /// Execute a choreography by name
    pub fn execute_choreography(
        &self,
        name: &str,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &HashMap<String, toml::Value>,
    ) -> Result<ChoreographyResult> {
        if let Some(executor) = self.choreographies.get(name) {
            executor.execute(world_state, participants, parameters)
        } else {
            Err(AuraError::configuration_error(format!(
                "Unknown choreography: {}",
                name
            )))
        }
    }

    /// Execute a protocol by name
    pub fn execute_protocol(
        &self,
        name: &str,
        world_state: &mut WorldState,
        participants: &[String],
        parameters: &Option<HashMap<String, toml::Value>>,
    ) -> Result<ProtocolResult> {
        if let Some(executor) = self.protocols.get(name) {
            executor.execute(world_state, participants, parameters)
        } else {
            Err(AuraError::configuration_error(format!(
                "Unknown protocol: {}",
                name
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_unified_engine_creation() {
        let temp_dir = TempDir::new().unwrap();
        let engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();

        assert_eq!(engine.config.enable_debugging, true);
        assert_eq!(engine.config.auto_checkpoint_interval, Some(50));
    }

    #[test]
    fn test_engine_configuration() {
        let temp_dir = TempDir::new().unwrap();
        let config = UnifiedEngineConfig {
            enable_debugging: false,
            verbose: true,
            ..Default::default()
        };

        let engine = UnifiedScenarioEngine::new(temp_dir.path())
            .unwrap()
            .configure(config);

        assert_eq!(engine.config.enable_debugging, false);
        assert_eq!(engine.config.verbose, true);
    }
}
