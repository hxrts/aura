use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Re-export ByzantineStrategy from world_state
pub use crate::world_state::ByzantineStrategy;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioFile {
    pub scenario: Vec<Scenario>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    pub setup: ScenarioSetup,
    pub network: Option<NetworkConditions>,
    pub byzantine: Option<ByzantineConditions>,
    pub phases: Option<Vec<ScenarioPhase>>,
    pub protocols: Option<Vec<ProtocolExecution>>,
    pub assertions: Vec<ScenarioAssertion>,
    pub expected_outcome: ExpectedOutcome,
    pub extends: Option<String>,
    pub quint_source: Option<QuintMetadata>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[derive(Default)]
pub struct ScenarioSetup {
    pub participants: usize,
    pub threshold: usize,
    pub seed: u64,
    pub network_conditions: Option<NetworkConditions>,
    pub byzantine_conditions: Option<ByzantineConditions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NetworkConditions {
    pub latency_range: [u64; 2],
    pub drop_rate: f64,
    pub partitions: Vec<Vec<usize>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ByzantineConditions {
    pub count: usize,
    pub participants: Vec<usize>,
    pub strategies: Vec<LegacyByzantineStrategy>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyByzantineStrategy {
    #[serde(rename = "type")]
    pub strategy_type: String,
    pub description: Option<String>,
    pub abort_after: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioPhase {
    pub name: String,
    pub setup: Option<ScenarioSetup>,
    pub protocols: Option<Vec<ProtocolExecution>>,
    pub simulate: Option<Vec<SimulationEvent>>,
    pub assertions: Option<Vec<ScenarioAssertion>>,
    pub checkpoints: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtocolExecution {
    #[serde(rename = "type")]
    pub protocol_type: ProtocolType,
    pub timeout_epochs: Option<u64>,
    pub context: Option<String>,
    pub parameters: Option<HashMap<String, toml::Value>>,
    pub new_threshold: Option<usize>,
    pub guardian_devices: Option<Vec<usize>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ProtocolType {
    #[serde(rename = "dkd")]
    Dkd,
    #[serde(rename = "resharing")]
    Resharing,
    #[serde(rename = "recovery")]
    Recovery,
    #[serde(rename = "locking")]
    Locking,
    #[serde(rename = "counter_coordination")]
    CounterCoordination,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SimulationEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub lost_device: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ScenarioAssertion {
    #[serde(rename = "type")]
    pub assertion_type: String,
    pub honest_participants: Option<Vec<usize>>,
    pub expected_detected: Option<Vec<usize>>,
    pub expected_property: Option<String>,
    pub timeout_multiplier: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ExpectedOutcome {
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "honest_majority_success")]
    HonestMajoritySuccess,
    #[serde(rename = "safety_violation_prevented")]
    SafetyViolationPrevented,
    #[serde(rename = "failure")]
    Failure,
    #[serde(rename = "property_violation")]
    PropertyViolation { property: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuintMetadata {
    pub specification: String,
    pub property: String,
    pub violation_pattern: String,
}


impl Scenario {
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
