//! Scenario definitions and types for simulator

pub mod types {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

    /// Expected outcome of a scenario execution
    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
    pub enum ExpectedOutcome {
        /// Scenario should complete successfully
        Success,
        /// Scenario should fail with an error
        Failure,
        /// Scenario should timeout
        Timeout,
        /// Property violation should be detected
        PropertyViolation { property: String },
        /// Safety violation should be prevented
        SafetyViolationPrevented,
        /// Success when honest majority exists
        HonestMajoritySuccess,
    }

    /// Legacy Byzantine strategy placeholder
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LegacyByzantineStrategy {
        pub name: String,
        pub parameters: HashMap<String, String>,
    }

    /// Byzantine conditions for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ByzantineConditions {
        pub strategies: Vec<LegacyByzantineStrategy>,
    }

    /// Network conditions for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct NetworkConditions {
        pub latency_ms: Option<u64>,
        pub packet_loss: Option<f64>,
    }

    /// Scenario assertion
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScenarioAssertion {
        pub property: String,
        pub expected: bool,
    }

    /// Scenario setup
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScenarioSetup {
        pub participants: usize,
        pub threshold: usize,
    }

    /// Complete scenario definition
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Scenario {
        pub id: String,
        pub name: String,
        pub setup: ScenarioSetup,
        pub network_conditions: Option<NetworkConditions>,
        pub byzantine_conditions: Option<ByzantineConditions>,
        pub assertions: Vec<ScenarioAssertion>,
        pub expected_outcome: ExpectedOutcome,
    }
}