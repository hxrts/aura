//! Core simulator types following algebraic effects architecture
//!
//! This module contains the foundational types for the Aura simulator,
//! following pure algebraic effect patterns without legacy middleware concepts.

use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

/// Simulator execution context for effect handlers
#[derive(Debug, Clone)]
pub struct SimulatorContext {
    /// Current simulation timestamp
    pub timestamp: Duration,
    /// Simulation seed for deterministic execution
    pub seed: u64,
    /// Current scenario being executed
    pub scenario_id: String,
    /// Simulation run identifier
    pub run_id: String,
    /// Current tick number
    pub tick: u64,
    /// Number of participants in simulation
    pub participant_count: usize,
    /// Threshold for protocols requiring minimum participants
    pub threshold: usize,
    /// Working directory for simulation artifacts
    pub working_dir: std::path::PathBuf,
    /// Environment variables for simulation
    pub env: HashMap<String, String>,
    /// Simulation configuration
    pub config: SimulatorConfig,
    /// Debug mode flag
    pub debug_mode: bool,
    /// Verbose logging flag
    pub verbose: bool,
    /// Metadata for effect communication
    pub metadata: HashMap<String, String>,
}

impl SimulatorContext {
    /// Create new simulator context
    pub fn new(scenario_id: String, run_id: String) -> Self {
        Self {
            timestamp: Duration::from_secs(0),
            seed: 0,
            scenario_id,
            run_id,
            tick: 0,
            participant_count: 0,
            threshold: 0,
            working_dir: std::env::current_dir().unwrap_or_default(),
            env: std::env::vars().collect(),
            config: SimulatorConfig::default(),
            debug_mode: false,
            verbose: false,
            metadata: HashMap::new(),
        }
    }

    /// Builder pattern for context modification
    pub fn with_timestamp(mut self, timestamp: Duration) -> Self {
        self.timestamp = timestamp;
        self
    }

    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    pub fn with_participants(mut self, count: usize, threshold: usize) -> Self {
        self.participant_count = count;
        self.threshold = threshold;
        self
    }

    pub fn with_config(mut self, config: SimulatorConfig) -> Self {
        self.config = config;
        self
    }

    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug_mode = debug;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Increment tick counter
    pub fn advance_tick(&mut self) {
        self.tick += 1;
    }

    /// Update timestamp
    pub fn advance_time(&mut self, delta: Duration) {
        self.timestamp += delta;
    }
}

/// Simulator configuration
#[derive(Debug, Clone)]
pub struct SimulatorConfig {
    /// Maximum simulation ticks
    pub max_ticks: u64,
    /// Maximum simulation duration
    pub max_duration: Duration,
    /// Enable deterministic mode
    pub deterministic: bool,
    /// Enable fault injection
    pub enable_faults: bool,
    /// Enable chaos testing
    pub enable_chaos: bool,
    /// Enable property checking
    pub enable_property_checking: bool,
    /// Working directory for artifacts
    pub artifacts_dir: std::path::PathBuf,
    /// Checkpoint interval (ticks)
    pub checkpoint_interval: u64,
    /// Log level for simulation
    pub log_level: LogLevel,
    /// Network simulation settings
    pub network_config: NetworkConfig,
    /// Time control settings
    pub time_config: TimeConfig,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            max_ticks: 1000,
            max_duration: Duration::from_secs(300), // 5 minutes
            deterministic: true,
            enable_faults: false,
            enable_chaos: false,
            enable_property_checking: true,
            artifacts_dir: std::path::PathBuf::from("./test_artifacts"),
            checkpoint_interval: 100,
            log_level: LogLevel::Info,
            network_config: NetworkConfig::default(),
            time_config: TimeConfig::default(),
        }
    }
}

/// Logging levels for simulation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

/// Network simulation configuration
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Base latency between nodes
    pub base_latency: Duration,
    /// Latency variance
    pub latency_variance: f64,
    /// Message loss probability (0.0 to 1.0)
    pub loss_probability: f64,
    /// Enable network partitions
    pub enable_partitions: bool,
    /// Maximum partition duration
    pub max_partition_duration: Duration,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base_latency: Duration::from_millis(10),
            latency_variance: 0.1,
            loss_probability: 0.0,
            enable_partitions: false,
            max_partition_duration: Duration::from_secs(30),
        }
    }
}

/// Time control configuration
#[derive(Debug, Clone)]
pub struct TimeConfig {
    /// Time acceleration factor
    pub acceleration_factor: f64,
    /// Enable time travel debugging
    pub enable_time_travel: bool,
    /// Checkpoint creation frequency
    pub checkpoint_frequency: Duration,
    /// Maximum time drift tolerance
    pub max_drift: Duration,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            acceleration_factor: 1.0,
            enable_time_travel: false,
            checkpoint_frequency: Duration::from_secs(10),
            max_drift: Duration::from_millis(100),
        }
    }
}

/// Fault injection strategies
#[derive(Debug, Clone)]
pub enum FaultType {
    /// Drop messages
    MessageDrop { probability: f64 },
    /// Delay messages
    MessageDelay { delay: Duration },
    /// Corrupt message content
    MessageCorruption { probability: f64 },
    /// Byzantine behavior
    Byzantine { strategy: ByzantineStrategy },
    /// Network partition
    NetworkPartition {
        participants: Vec<String>,
        duration: Duration,
    },
    /// Node crash
    NodeCrash {
        node_id: String,
        duration: Option<Duration>,
    },
    /// Resource exhaustion
    ResourceExhaustion { resource: String, factor: f64 },
}

/// Byzantine behavior strategies
#[derive(Debug, Clone)]
pub enum ByzantineStrategy {
    /// Send random messages
    RandomMessages,
    /// Send duplicate messages
    DuplicateMessages,
    /// Send delayed messages
    DelayedMessages { delay: Duration },
    /// Send corrupted signatures
    CorruptedSignatures,
    /// Selective message dropping
    SelectiveDrop { targets: Vec<String> },
}

/// Property violation types for checking
#[derive(Debug, Clone)]
pub enum PropertyViolationType {
    /// Safety property violated
    Safety {
        description: String,
        evidence: Value,
    },
    /// Liveness property violated
    Liveness {
        description: String,
        timeout: Duration,
    },
    /// Consistency property violated
    Consistency {
        description: String,
        conflicting_states: Vec<Value>,
    },
    /// Performance property violated
    Performance {
        description: String,
        metric: String,
        value: f64,
        threshold: f64,
    },
    /// Security property violated
    Security { description: String, threat: String },
}

/// Core simulator operations that can be performed via effects
#[derive(Debug, Clone)]
pub enum SimulatorOperation {
    /// Initialize a new simulation scenario
    InitializeScenario { scenario_id: String },

    /// Execute a single simulation tick
    ExecuteTick {
        tick_number: u64,
        delta_time: Duration,
    },

    /// Inject a fault into the simulation
    InjectFault {
        fault_type: FaultType,
        target: String,
        duration: Option<Duration>,
    },

    /// Control simulation time
    ControlTime {
        action: TimeControlAction,
        parameters: HashMap<String, Value>,
    },

    /// Inspect simulation state
    InspectState {
        component: String,
        query: StateQuery,
    },

    /// Check properties
    CheckProperty {
        property_name: String,
        expected: Value,
        actual: Value,
    },

    /// Coordinate chaos testing
    CoordinateChaos {
        strategy: ChaosStrategy,
        intensity: f64,
        duration: Duration,
    },

    /// Run a choreographed protocol
    RunChoreography {
        protocol: String,
        participants: Vec<String>,
        parameters: HashMap<String, Value>,
    },

    /// Create a checkpoint
    CreateCheckpoint {
        checkpoint_id: String,
        description: Option<String>,
    },

    /// Restore from checkpoint
    RestoreCheckpoint { checkpoint_id: String },

    /// Finalize simulation and generate results
    FinalizeSimulation {
        outcome: SimulationOutcome,
        metrics: HashMap<String, Value>,
    },

    /// Execute a raw effect through the stateless system
    ExecuteEffect {
        effect_type: String,
        operation_name: String,
        params: Value,
    },

    /// Set up devices using testkit foundations
    SetupDevices { count: usize, threshold: usize },

    /// Initialize choreography protocols
    InitializeChoreography { protocol: String },

    /// Collect performance metrics
    CollectMetrics,
}

/// Time control actions
#[derive(Debug, Clone)]
pub enum TimeControlAction {
    /// Pause simulation time
    Pause,
    /// Resume simulation time
    Resume,
    /// Set time acceleration factor
    SetAcceleration { factor: f64 },
    /// Jump to specific time
    JumpTo { timestamp: Duration },
    /// Create time checkpoint
    Checkpoint { id: String },
    /// Restore to time checkpoint
    Restore { id: String },
}

/// State inspection queries
#[derive(Debug, Clone)]
pub enum StateQuery {
    /// Get all state
    GetAll,
    /// Get specific field
    GetField { field: String },
    /// Query with filter
    Query { filter: String },
    /// Get state history
    GetHistory { since: Option<Duration> },
    /// Get state diff
    GetDiff { from: String, to: String },
}

/// Chaos testing strategies
#[derive(Debug, Clone)]
pub enum ChaosStrategy {
    /// Random fault injection
    RandomFaults,
    /// Network partitioning
    NetworkPartitions,
    /// Resource exhaustion
    ResourceExhaustion,
    /// Byzantine behavior injection
    ByzantineBehavior,
    /// Combined chaos testing
    Combined { strategies: Vec<ChaosStrategy> },
}

/// Simulation outcome types
#[derive(Debug, Clone)]
pub enum SimulationOutcome {
    /// Simulation completed successfully
    Success,
    /// Simulation failed with error
    Failure { reason: String },
    /// Simulation timed out
    Timeout,
    /// Property violation detected
    PropertyViolation {
        violations: Vec<PropertyViolationType>,
    },
    /// Simulation was cancelled
    Cancelled,
}

/// Simulator error types
#[derive(Debug, thiserror::Error)]
pub enum SimulatorError {
    #[error("Scenario not found: {0}")]
    ScenarioNotFound(String),

    #[error("Checkpoint not found: {0}")]
    CheckpointNotFound(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    #[error("Simulation timeout after {duration:?}")]
    Timeout { duration: Duration },

    #[error("Property violation: {property} - {description}")]
    PropertyViolation {
        property: String,
        description: String,
    },

    #[error("Fault injection failed: {0}")]
    FaultInjectionFailed(String),

    #[error("Time control error: {0}")]
    TimeControlError(String),

    #[error("State inspection failed: {0}")]
    StateInspectionFailed(String),

    #[error("Chaos coordination error: {0}")]
    ChaosCoordinationError(String),

    #[error("File system error: {0}")]
    FileSystem(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Operation failed: {0}")]
    OperationFailed(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SimulatorError>;
