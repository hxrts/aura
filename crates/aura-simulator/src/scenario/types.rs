use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export ByzantineStrategy from world_state
pub use crate::world_state::ByzantineStrategy;

/// Top-level scenario file containing multiple scenario definitions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioFile {
    /// List of scenario definitions in the file
    pub scenario: Vec<Scenario>,
}

/// Complete scenario definition for testing distributed protocols
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    /// Unique name for this scenario
    pub name: String,
    /// Human-readable description of what this scenario tests
    pub description: String,
    /// Initial setup configuration
    pub setup: ScenarioSetup,
    /// Optional network conditions to simulate
    pub network: Option<NetworkConditions>,
    /// Optional Byzantine behavior conditions
    pub byzantine: Option<ByzantineConditions>,
    /// Optional phases for multi-stage scenarios
    pub phases: Option<Vec<ScenarioPhase>>,
    /// Optional protocol executions to run
    pub protocols: Option<Vec<ProtocolExecution>>,
    /// Assertions to verify during execution
    pub assertions: Vec<ScenarioAssertion>,
    /// Expected outcome of the scenario
    pub expected_outcome: ExpectedOutcome,
    /// Optional base scenario to extend from
    pub extends: Option<String>,
    /// Optional Quint specification metadata
    pub quint_source: Option<QuintMetadata>,
}

/// Initial setup configuration for a scenario
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ScenarioSetup {
    /// Number of participants in the protocol
    pub participants: usize,
    /// Threshold for quorum (M in M-of-N)
    pub threshold: usize,
    /// Random seed for deterministic execution
    pub seed: u64,
    /// Optional network conditions
    pub network_conditions: Option<NetworkConditions>,
    /// Optional Byzantine conditions
    pub byzantine_conditions: Option<ByzantineConditions>,
}

/// Network conditions to simulate
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConditions {
    /// Min and max network latency in milliseconds
    pub latency_range: [u64; 2],
    /// Probability of message drops (0.0 to 1.0)
    pub drop_rate: f64,
    /// Network partitions as groups of participant indices
    pub partitions: Vec<Vec<usize>>,
}

/// Byzantine behavior conditions
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ByzantineConditions {
    /// Number of Byzantine participants
    pub count: usize,
    /// Indices of Byzantine participants
    pub participants: Vec<usize>,
    /// Byzantine strategies to employ
    pub strategies: Vec<LegacyByzantineStrategy>,
}

/// Legacy Byzantine strategy specification for backwards compatibility
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyByzantineStrategy {
    /// Type of Byzantine strategy to employ
    #[serde(rename = "type")]
    pub strategy_type: String,
    /// Optional human-readable description of what this strategy does
    pub description: Option<String>,
    /// Optional phase name after which to abort execution
    pub abort_after: Option<String>,
}

/// Single phase in a multi-phase scenario execution
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioPhase {
    /// Unique name identifying this phase
    pub name: String,
    /// Optional setup changes to apply when entering this phase
    pub setup: Option<ScenarioSetup>,
    /// Optional protocol executions to run during this phase
    pub protocols: Option<Vec<ProtocolExecution>>,
    /// Optional simulation events to trigger during this phase
    pub simulate: Option<Vec<SimulationEvent>>,
    /// Optional assertions to check at the end of this phase
    pub assertions: Option<Vec<ScenarioAssertion>>,
    /// Optional checkpoint labels to create during this phase
    pub checkpoints: Option<Vec<String>>,
}

/// Specification for executing a protocol during scenario
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolExecution {
    /// Type of protocol to execute
    #[serde(rename = "type")]
    pub protocol_type: ProtocolType,
    /// Optional timeout in epochs before protocol is considered failed
    pub timeout_epochs: Option<u64>,
    /// Optional context string passed to the protocol
    pub context: Option<String>,
    /// Optional protocol-specific configuration parameters
    pub parameters: Option<HashMap<String, toml::Value>>,
    /// Optional new threshold for resharing protocols
    pub new_threshold: Option<usize>,
    /// Optional guardian device indices for recovery protocols
    pub guardian_devices: Option<Vec<usize>>,
}

/// Types of protocols that can be executed
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ProtocolType {
    /// Deterministic Key Derivation protocol
    #[serde(rename = "dkd")]
    Dkd,
    /// Key resharing protocol
    #[serde(rename = "resharing")]
    Resharing,
    /// Account recovery protocol
    #[serde(rename = "recovery")]
    Recovery,
    /// Locking protocol
    #[serde(rename = "locking")]
    Locking,
    /// Counter coordination protocol
    #[serde(rename = "counter_coordination")]
    CounterCoordination,
}

/// Simulation event to trigger during scenario execution
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationEvent {
    /// Type of simulation event to trigger
    #[serde(rename = "type")]
    pub event_type: String,
    /// Optional device index to simulate as lost or unavailable
    pub lost_device: Option<usize>,
}

/// Assertion to verify during scenario execution
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioAssertion {
    /// Type of assertion to check
    #[serde(rename = "type")]
    pub assertion_type: String,
    /// Optional list of participant indices expected to be honest
    pub honest_participants: Option<Vec<usize>>,
    /// Optional list of participant indices expected to be detected as Byzantine
    pub expected_detected: Option<Vec<usize>>,
    /// Optional property name that should hold
    pub expected_property: Option<String>,
    /// Optional timeout multiplier to extend default assertion timeout
    pub timeout_multiplier: Option<f64>,
}

/// Expected outcome of scenario execution
#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ExpectedOutcome {
    /// Scenario should complete successfully
    #[serde(rename = "success")]
    Success,
    /// Honest majority should achieve success
    #[serde(rename = "honest_majority_success")]
    HonestMajoritySuccess,
    /// Safety violation should be prevented
    #[serde(rename = "safety_violation_prevented")]
    SafetyViolationPrevented,
    /// Scenario should fail
    #[serde(rename = "failure")]
    Failure,
    /// Property violation should be detected
    #[serde(rename = "property_violation")]
    PropertyViolation {
        /// Name of the property that should be violated
        property: String,
    },
}

/// Metadata about Quint specification source
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuintMetadata {
    /// Path to Quint specification file
    pub specification: String,
    /// Property name in the specification
    pub property: String,
    /// Pattern describing the violation
    pub violation_pattern: String,
}

impl Scenario {
    /// Inherit configuration from a base scenario
    ///
    /// Merges this scenario's configuration with a base scenario, allowing for
    /// scenario inheritance and composition. Base values are used only when
    /// this scenario doesn't specify them.
    pub fn inherit_from(&mut self, base: &Scenario) {
        if self.setup.participants == 0 {
            self.setup = base.setup.clone();
        }
        if self.network.is_none() {
            self.network = base.network.clone();
        }
        if self.byzantine.is_none() {
            self.byzantine = base.byzantine.clone();
        }
        if self.protocols.is_none() || self.protocols.as_ref().unwrap().is_empty() {
            self.protocols = base.protocols.clone();
        }
        let mut inherited_assertions = base.assertions.clone();
        inherited_assertions.extend(self.assertions.clone());
        self.assertions = inherited_assertions;
    }
}
