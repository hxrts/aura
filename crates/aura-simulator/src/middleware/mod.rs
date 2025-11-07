//! Simulator Middleware System
//!
//! This module implements the algebraic effect-style middleware pattern for the Aura simulator,
//! providing composable layers for scenario injection, fault simulation, time control,
//! state inspection, property checking, and chaos coordination.

use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub mod chaos_coordination;
pub mod fault_simulation;
pub mod handler;
pub mod property_checking;
pub mod scenario_injection;
pub mod stack;
pub mod state_inspection;
pub mod time_control;

// Re-export for convenience
pub use handler::{
    ChaosStrategy, NoOpSimulatorHandler, SimulationOutcome, SimulatorHandler, SimulatorOperation,
    StateQuery, TimeControlAction,
};
pub use stack::{SimulatorMiddlewareStack, SimulatorStackBuilder};

// Re-export middleware components
pub use chaos_coordination::{
    ChaosAction, ChaosCoordinationMiddleware, ChaosRecoverySettings, ChaosRule, ChaosRuleAction,
    ChaosRuleCondition, ChaosRuleOperator, ChaosStrategyTemplate,
};
pub use fault_simulation::{
    FaultCondition, FaultInjectionRule, FaultRecoverySettings, FaultSimulationMiddleware,
};
pub use property_checking::{
    PropertyCheckResult, PropertyChecker, PropertyCheckingMiddleware, PropertyType,
    PropertyViolation,
};
pub use scenario_injection::{
    InjectionAction, ScenarioDefinition, ScenarioInjectionMiddleware, TriggerCondition,
};
pub use state_inspection::{
    StateInspectionMiddleware, StateTrigger, StateWatcher, TriggerAction, WatcherCondition,
};
pub use time_control::{RealtimeSync, TimeControlMiddleware};

/// Simulator execution context that flows through middleware layers
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
    /// Metadata for middleware communication
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

/// Middleware trait for simulator operations
pub trait SimulatorMiddleware: Send + Sync {
    /// Process a simulator operation through the middleware layer
    fn process(
        &self,
        operation: SimulatorOperation,
        context: &SimulatorContext,
        next: &dyn SimulatorHandler,
    ) -> Result<Value>;

    /// Get the name of this middleware layer
    fn name(&self) -> &str;

    /// Check if this middleware should process the given operation
    fn handles(&self, _operation: &SimulatorOperation) -> bool {
        true // Default: handle all operations
    }

    /// Initialize middleware (called once at stack creation)
    fn initialize(&mut self, _config: &SimulatorConfig) -> Result<()> {
        Ok(())
    }

    /// Cleanup middleware (called at stack destruction)
    fn cleanup(&mut self) -> Result<()> {
        Ok(())
    }
}

/// Simulator error types
#[derive(Debug, thiserror::Error)]
pub enum SimulatorError {
    #[error("Scenario not found: {0}")]
    ScenarioNotFound(String),

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
}

pub type Result<T> = std::result::Result<T, SimulatorError>;
