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
        /// Chat group functionality validated
        ChatGroupSuccess,
        /// Recovery demo completed successfully
        RecoveryDemoSuccess,
    }

    /// Legacy Byzantine strategy kept for backward compatibility with older scenarios
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

    /// Chat group configuration for scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChatGroupConfig {
        pub enabled: bool,
        pub multi_actor_support: bool,
        pub message_history_validation: bool,
        pub group_name: Option<String>,
        pub initial_messages: Vec<ChatMessage>,
    }

    /// Chat message for scenario testing
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ChatMessage {
        pub sender: String,
        pub content: String,
        pub timestamp: Option<u64>,
    }

    /// Data loss simulation configuration
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DataLossSimulation {
        pub enabled: bool,
        pub target_participant: String,
        pub loss_type: DataLossType,
        pub recovery_validation: bool,
    }

    /// Types of data loss for simulation
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DataLossType {
        /// Complete device loss with all data
        CompleteDeviceLoss,
        /// Partial key material corruption
        PartialKeyCorruption,
        /// Network partition simulation
        NetworkPartition,
        /// Storage corruption
        StorageCorruption,
    }

    /// Demo configuration for UX scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DemoConfig {
        pub protagonist: Option<String>,
        pub guardians: Vec<String>,
        pub demo_type: DemoType,
        pub validation_steps: Vec<String>,
    }

    /// Types of demo scenarios
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum DemoType {
        /// Bob's recovery journey demo
        RecoveryJourney,
        /// Guardian setup demo
        GuardianSetup,
        /// Chat group demo
        ChatGroupDemo,
        /// Multi-actor coordination demo
        MultiActorDemo,
    }

    /// Scenario setup with extended capabilities
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScenarioSetup {
        pub participants: u32,
        pub threshold: u32,
        pub chat_config: Option<ChatGroupConfig>,
        pub data_loss_config: Option<DataLossSimulation>,
        pub demo_config: Option<DemoConfig>,
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
