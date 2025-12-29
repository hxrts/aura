//! Simulation Effect Traits
//!
//! This module defines effect traits specifically for simulation and testing environments.
//! These effects enable deterministic, controllable simulation of distributed systems
//! behavior while following the stateless effect pattern.
//!
//! # Effect Classification
//!
//! - **Category**: Testing/Simulation Effect
//! - **Implementation**: `aura-simulator` (Layer 8)
//! - **Usage**: Simulation control, time advancement, checkpointing, fault injection
//!
//! This is a testing/simulation effect for deterministic simulation control. Provides
//! time manipulation, scenario management, checkpointing, and observation capabilities
//! for reproducible distributed systems testing. Handlers in `aura-simulator` manage
//! the simulation lifecycle.
//!
//! ## Core Principles
//!
//! - **Deterministic Control**: All simulation effects are deterministic and reproducible
//! - **Time Manipulation**: Ability to control and advance simulation time
//! - **Fault Injection**: Systematic injection of faults for resilience testing
//! - **State Inspection**: Ability to inspect and checkpoint simulation state
//! - **Scenario Management**: Support for complex test scenarios and configurations
//!
//! ## Effect Categories
//!
//! - **Simulation Control**: Time advancement, scenario management, checkpointing
//! - **Fault Injection**: Network, storage, and computation fault simulation
//! - **Observation**: Metrics collection, state inspection, and event monitoring
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use aura_core::effects::{SimulationControlEffects, FaultInjectionEffects};
//!
//! async fn test_protocol_resilience<E>(
//!     scenario: TestScenario,
//!     effects: &E,
//! ) -> Result<TestResults, AuraError>
//! where
//!     E: SimulationControlEffects + FaultInjectionEffects,
//! {
//!     // Setup scenario
//!     let checkpoint = effects.create_checkpoint("before_fault").await?;
//!     
//!     // Inject fault
//!     effects.inject_network_partition(&["node1", "node2"]).await?;
//!     
//!     // Advance time and observe
//!     effects.advance_time(Duration::from_secs(30)).await?;
//!     
//!     // Restore and verify
//!     effects.restore_checkpoint(&checkpoint).await?;
//!     Ok(TestResults::new())
//! }
//! ```

use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

/// Unique identifier for simulation scenarios
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScenarioId(pub String);

impl std::fmt::Display for ScenarioId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for simulation checkpoints
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheckpointId(pub String);

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Simulation time state and control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationTime {
    /// Current simulation timestamp
    pub current: SystemTime,
    /// Simulation start time
    pub start: SystemTime,
    /// Time advancement rate (1.0 = real-time, 0.0 = paused)
    pub rate: f64,
    /// Whether time advancement is manual or automatic
    pub manual_control: bool,
}

impl SimulationTime {
    /// Create new simulation time starting at the given instant
    pub fn new(start: SystemTime) -> Self {
        Self {
            current: start,
            start,
            rate: 1.0,
            manual_control: false,
        }
    }

    /// Get elapsed simulation time
    pub fn elapsed(&self) -> Duration {
        self.current
            .duration_since(self.start)
            .unwrap_or(Duration::ZERO)
    }
}

/// Simulation scenario configuration and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationScenario {
    /// Unique scenario identifier
    pub id: ScenarioId,
    /// Human-readable scenario name
    pub name: String,
    /// Scenario description
    pub description: String,
    /// Initial simulation parameters
    pub parameters: HashMap<String, serde_json::Value>,
    /// Expected duration for this scenario
    pub duration: Option<Duration>,
    /// Current scenario state
    pub state: ScenarioState,
}

/// Current state of a simulation scenario
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ScenarioState {
    /// Scenario is being initialized
    Initializing,
    /// Scenario is actively running
    Running,
    /// Scenario is paused
    Paused,
    /// Scenario completed successfully
    Completed,
    /// Scenario failed with error
    Failed { reason: String },
    /// Scenario was cancelled
    Cancelled,
}

/// Checkpoint containing simulation state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationCheckpoint {
    /// Unique checkpoint identifier
    pub id: CheckpointId,
    /// Timestamp when checkpoint was created
    pub timestamp: SystemTime,
    /// Simulation time when checkpoint was created
    pub simulation_time: SimulationTime,
    /// Associated scenario
    pub scenario_id: Option<ScenarioId>,
    /// Checkpoint metadata
    pub metadata: HashMap<String, serde_json::Value>,
    /// Size of checkpoint data in bytes
    pub size_bytes: u64,
}

/// Fault injection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultInjectionConfig {
    /// Type of fault to inject
    pub fault_type: FaultType,
    /// Fault injection parameters
    pub parameters: HashMap<String, serde_json::Value>,
    /// Duration for which fault should be active
    pub duration: Option<Duration>,
    /// Probability of fault occurring (0.0 to 1.0)
    pub probability: f64,
}

/// Types of faults that can be injected
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FaultType {
    /// Network-related faults
    Network(NetworkFault),
    /// Storage-related faults
    Storage(StorageFault),
    /// Computation-related faults
    Computation(ComputationFault),
    /// Time-related faults
    Time(TimeFault),
    /// Byzantine behavior simulation
    Byzantine(ByzantineFault),
}

/// Network fault types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NetworkFault {
    /// Partition network between specified groups
    Partition { groups: Vec<Vec<String>> },
    /// Drop packets with specified probability
    PacketLoss { probability: f64 },
    /// Add latency to network operations
    Latency { delay: Duration },
    /// Corrupt packets with specified probability
    Corruption { probability: f64 },
    /// Simulate network congestion
    Congestion { bandwidth_limit: u64 },
    /// Complete network failure
    Outage,
}

/// Storage fault types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StorageFault {
    /// Storage operations fail with specified probability
    Failure { probability: f64 },
    /// Storage operations are slow
    Slowness { delay: Duration },
    /// Storage corruption
    Corruption { probability: f64 },
    /// Storage capacity exhaustion
    CapacityExhausted,
    /// Complete storage unavailability
    Unavailable,
}

/// Computation fault types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ComputationFault {
    /// CPU intensive operations are slow
    CpuSlowness { factor: f64 },
    /// Memory allocation failures
    MemoryExhaustion,
    /// Computation results are corrupted
    ResultCorruption { probability: f64 },
    /// Operations timeout
    Timeout { duration: Duration },
}

/// Time-related fault types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TimeFault {
    /// Clock drift simulation
    ClockDrift { rate: f64 },
    /// Clock skew between nodes
    ClockSkew { offset: Duration },
    /// Time jumps forward or backward
    TimeJump { delta: Duration },
}

/// Byzantine behavior types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ByzantineFault {
    /// Send different messages to different peers
    Equivocation,
    /// Send invalid signatures
    InvalidSignatures,
    /// Refuse to participate in protocols
    Silence,
    /// Send messages out of protocol order
    ProtocolViolation,
}

/// Simulation observation and metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationMetrics {
    /// Number of operations executed
    pub operations_count: u64,
    /// Total simulation runtime
    pub total_runtime: Duration,
    /// Average operation latency
    pub avg_operation_latency: Duration,
    /// Number of faults injected
    pub faults_injected: u64,
    /// Number of checkpoints created
    pub checkpoints_created: u64,
    /// Memory usage statistics
    pub memory_usage_bytes: u64,
    /// Custom metrics
    pub custom_metrics: HashMap<String, f64>,
}

impl Default for SimulationMetrics {
    fn default() -> Self {
        Self {
            operations_count: 0,
            total_runtime: Duration::ZERO,
            avg_operation_latency: Duration::ZERO,
            faults_injected: 0,
            checkpoints_created: 0,
            memory_usage_bytes: 0,
            custom_metrics: HashMap::new(),
        }
    }
}

/// Effect trait for simulation control operations
#[async_trait::async_trait]
pub trait SimulationControlEffects {
    /// Create a new simulation scenario
    async fn create_scenario(
        &self,
        name: String,
        description: String,
        parameters: HashMap<String, serde_json::Value>,
    ) -> Result<ScenarioId>;

    /// Start a simulation scenario
    async fn start_scenario(&self, scenario_id: &ScenarioId) -> Result<()>;

    /// Pause a running scenario
    async fn pause_scenario(&self, scenario_id: &ScenarioId) -> Result<()>;

    /// Resume a paused scenario
    async fn resume_scenario(&self, scenario_id: &ScenarioId) -> Result<()>;

    /// Stop a scenario
    async fn stop_scenario(&self, scenario_id: &ScenarioId) -> Result<()>;

    /// Get current scenario state
    async fn get_scenario(&self, scenario_id: &ScenarioId) -> Result<Option<SimulationScenario>>;

    /// List all scenarios
    async fn list_scenarios(&self) -> Result<Vec<SimulationScenario>>;

    /// Advance simulation time by the specified duration
    async fn advance_time(&self, duration: Duration) -> Result<()>;

    /// Set simulation time advancement rate (1.0 = real-time, 0.0 = paused)
    async fn set_time_rate(&self, rate: f64) -> Result<()>;

    /// Get current simulation time
    async fn get_simulation_time(&self) -> Result<SimulationTime>;

    /// Enable or disable manual time control
    async fn set_manual_time_control(&self, enabled: bool) -> Result<()>;

    /// Create a checkpoint of current simulation state
    async fn create_checkpoint(&self, name: String) -> Result<CheckpointId>;

    /// Restore simulation state from a checkpoint
    async fn restore_checkpoint(&self, checkpoint_id: &CheckpointId) -> Result<()>;

    /// Get checkpoint information
    async fn get_checkpoint(
        &self,
        checkpoint_id: &CheckpointId,
    ) -> Result<Option<SimulationCheckpoint>>;

    /// List all available checkpoints
    async fn list_checkpoints(&self) -> Result<Vec<SimulationCheckpoint>>;

    /// Delete a checkpoint
    async fn delete_checkpoint(&self, checkpoint_id: &CheckpointId) -> Result<()>;

    /// Get current simulation metrics
    async fn get_metrics(&self) -> Result<SimulationMetrics>;

    /// Reset simulation metrics
    async fn reset_metrics(&self) -> Result<()>;
}

/// Effect trait for fault injection operations
#[async_trait::async_trait]
pub trait FaultInjectionEffects {
    /// Inject a network partition between specified node groups
    async fn inject_network_partition(&self, groups: Vec<Vec<String>>) -> Result<()>;

    /// Inject packet loss with specified probability
    async fn inject_packet_loss(&self, probability: f64) -> Result<()>;

    /// Inject network latency
    async fn inject_network_latency(&self, delay: Duration) -> Result<()>;

    /// Inject storage failures
    async fn inject_storage_failure(&self, probability: f64) -> Result<()>;

    /// Inject computation slowness
    async fn inject_computation_slowness(&self, factor: f64) -> Result<()>;

    /// Inject Byzantine behavior
    async fn inject_byzantine_fault(&self, fault: ByzantineFault) -> Result<()>;

    /// Inject a configured fault
    async fn inject_fault(&self, config: FaultInjectionConfig) -> Result<()>;

    /// Clear all active faults
    async fn clear_faults(&self) -> Result<()>;

    /// Clear specific fault type
    async fn clear_fault_type(&self, fault_type: FaultType) -> Result<()>;

    /// Get list of currently active faults
    async fn list_active_faults(&self) -> Result<Vec<FaultInjectionConfig>>;
}

/// Effect trait for simulation observation and monitoring
#[async_trait::async_trait]
pub trait SimulationObservationEffects {
    /// Record a custom metric
    async fn record_metric(&self, name: String, value: f64) -> Result<()>;

    /// Get specific metric value
    async fn get_metric(&self, name: &str) -> Result<Option<f64>>;

    /// Get all custom metrics
    async fn get_all_metrics(&self) -> Result<HashMap<String, f64>>;

    /// Record an operation execution
    async fn record_operation(&self, operation_name: &str, duration: Duration) -> Result<()>;

    /// Get operation statistics
    async fn get_operation_stats(&self, operation_name: &str) -> Result<Option<OperationStats>>;

    /// Export simulation data for analysis
    async fn export_simulation_data(&self, format: ExportFormat) -> Result<Vec<u8>>;
}

/// Statistics for a specific operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationStats {
    /// Operation name
    pub operation_name: String,
    /// Total number of executions
    pub execution_count: u64,
    /// Total execution time
    pub total_duration: Duration,
    /// Average execution time
    pub avg_duration: Duration,
    /// Minimum execution time
    pub min_duration: Duration,
    /// Maximum execution time
    pub max_duration: Duration,
    /// Standard deviation of execution times
    pub std_deviation: Duration,
}

/// Export format for simulation data
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExportFormat {
    /// JSON format
    Json,
    /// CSV format
    Csv,
    /// Binary format
    Binary,
}

/// Comprehensive simulation effects trait combining all simulation capabilities
pub trait SimulationEffects:
    SimulationControlEffects + FaultInjectionEffects + SimulationObservationEffects
{
}

// Blanket implementation for any type that implements all simulation effect traits
impl<T> SimulationEffects for T where
    T: SimulationControlEffects + FaultInjectionEffects + SimulationObservationEffects
{
}

// ============================================================================
// Simulation Environment Factory Abstraction
// ============================================================================

use crate::effects::transport::TransportEnvelope;
use crate::identifiers::AuthorityId;
use crate::DeviceId;
// Allow parking_lot::RwLock here because:
// 1. The simulation feature is only used by Layer 6-8 (runtime/test layers)
// 2. The shared transport inbox is accessed synchronously for deterministic simulation
// 3. This maintains compatibility with the Layer 6 SharedTransport implementation
#[allow(clippy::disallowed_types)]
use parking_lot::RwLock;
use std::sync::Arc;

/// Configuration for creating a simulation environment
///
/// This structure contains all parameters needed to create an effect system
/// suitable for simulation purposes.
#[derive(Debug, Clone)]
pub struct SimulationEnvironmentConfig {
    /// Random seed for deterministic simulation
    pub seed: u64,
    /// Device ID for this simulated agent
    pub device_id: DeviceId,
    /// Optional authority ID override (derived from device_id if not provided)
    pub authority_id: Option<AuthorityId>,
}

impl SimulationEnvironmentConfig {
    /// Create a new configuration with the given seed and device ID
    pub fn new(seed: u64, device_id: DeviceId) -> Self {
        Self {
            seed,
            device_id,
            authority_id: None,
        }
    }

    /// Set an explicit authority ID
    #[must_use]
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }
}

/// Error type for simulation environment creation failures
#[derive(Debug, thiserror::Error)]
pub enum SimulationEnvironmentError {
    /// Configuration error
    #[error("Invalid configuration: {0}")]
    Configuration(String),

    /// Effect system creation failed
    #[error("Effect system creation failed: {0}")]
    CreationFailed(String),

    /// Required component missing
    #[error("Required component missing: {0}")]
    MissingComponent(String),
}

/// Factory trait for creating effect systems suitable for simulation
///
/// This trait abstracts the creation of effect systems, allowing the simulator
/// to work with any implementation without directly depending on concrete types.
///
/// # Example
///
/// ```rust,ignore
/// use aura_core::effects::simulation::{SimulationEnvironmentFactory, SimulationEnvironmentConfig};
///
/// async fn run_simulation<F>(factory: &F, config: SimulationEnvironmentConfig)
/// where
///     F: SimulationEnvironmentFactory,
/// {
///     let effects = factory.create_simulation_environment(config).await?;
///     // Use effects for simulation...
/// }
/// ```
// Allow parking_lot::RwLock in this trait because:
// 1. The simulation feature is only used by Layer 6-8 (runtime/test layers)
// 2. The shared transport inbox is accessed synchronously for deterministic simulation
// 3. This maintains compatibility with the Layer 6 SharedTransport implementation
#[allow(clippy::disallowed_types)]
#[async_trait::async_trait]
pub trait SimulationEnvironmentFactory: Send + Sync {
    /// The effect system type produced by this factory
    type EffectSystem: RuntimeEffectsBundle + Send + Sync + 'static;

    /// Create an effect system for simulation with the given configuration
    async fn create_simulation_environment(
        &self,
        config: SimulationEnvironmentConfig,
    ) -> std::result::Result<Arc<Self::EffectSystem>, SimulationEnvironmentError>;

    /// Create an effect system with shared transport for multi-agent simulations
    ///
    /// The shared inbox enables message routing between multiple simulated agents
    /// (e.g., Bob, Alice, Carol) in demo mode.
    async fn create_simulation_environment_with_shared_transport(
        &self,
        config: SimulationEnvironmentConfig,
        shared_inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> std::result::Result<Arc<Self::EffectSystem>, SimulationEnvironmentError>;
}

/// Bundle of all effect traits needed for runtime operation
///
/// This supertrait combines all the effect traits that a simulation environment
/// must provide. Effect systems that implement this trait can be used with the
/// simulator without knowing their concrete type.
///
/// Note: This is designed to be object-safe where possible, but the primary
/// usage pattern is through generics with the `SimulationEnvironmentFactory` trait.
pub trait RuntimeEffectsBundle:
    crate::effects::PhysicalTimeEffects
    + crate::effects::LogicalClockEffects
    + crate::effects::OrderClockEffects
    + crate::effects::CryptoEffects
    + crate::effects::StorageEffects
    + crate::effects::RandomEffects
    + crate::effects::TransportEffects
    + crate::effects::JournalEffects
    + crate::effects::ConsoleEffects
    + crate::effects::SystemEffects
{
    /// Check if this effect system is in test/simulation mode
    fn is_simulation_mode(&self) -> bool;

    /// Get the seed used for deterministic simulation (if applicable)
    fn simulation_seed(&self) -> Option<u64>;
}

// Note: Blanket implementation is not provided here because RuntimeEffectsBundle
// requires methods beyond the supertrait bounds. Implementors must explicitly
// implement RuntimeEffectsBundle.
