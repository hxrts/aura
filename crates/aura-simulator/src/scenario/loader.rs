//! Unified Scenario Loader - TOML Parsing for Declarative Scenarios
//!
//! This module provides parsing and loading capabilities for the unified TOML
//! scenario format, supporting inheritance, validation, and schema checking.

use crate::{
    scenario::engine::{
        ByzantineConfig, ChoreographyAction, ExpectedOutcome, NetworkConfig, ParticipantConfig,
        PropertyCheck, ScenarioPhaseWithActions, ScenarioSetupConfig, UnifiedScenario,
    },
    AuraError, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// TOML file structure for unified scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedScenarioFile {
    /// Scenario metadata
    pub metadata: ScenarioMetadata,
    /// Setup configuration
    pub setup: TomlScenarioSetup,
    /// Network configuration
    pub network: Option<TomlNetworkConfig>,
    /// Byzantine configuration
    pub byzantine: Option<TomlByzantineConfig>,
    /// Scenario phases with actions
    pub phases: Vec<TomlScenarioPhase>,
    /// Properties to check
    pub properties: Option<Vec<TomlPropertyCheck>>,
    /// Expected outcome
    #[serde(default = "default_expected_outcome")]
    pub expected_outcome: String,
    /// Inheritance
    pub extends: Option<String>,
}

fn default_expected_outcome() -> String {
    "success".to_string()
}

/// Scenario metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioMetadata {
    /// Scenario name
    pub name: String,
    /// Description
    pub description: String,
    /// Version of scenario format
    pub version: Option<String>,
    /// Author information
    pub author: Option<String>,
    /// Tags for categorization
    pub tags: Option<Vec<String>>,
}

/// TOML setup configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlScenarioSetup {
    /// Number of participants
    pub participants: Option<usize>,
    /// Threshold for protocols
    pub threshold: Option<usize>,
    /// Random seed
    pub seed: Option<u64>,
    /// Specific participant configurations
    pub participant_configs: Option<HashMap<String, TomlParticipantConfig>>,
}

/// TOML participant configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlParticipantConfig {
    /// Device ID
    pub device_id: String,
    /// Account ID
    pub account_id: String,
    /// Byzantine flag
    pub is_byzantine: Option<bool>,
    /// Role designation
    pub role: Option<String>,
}

/// TOML network configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlNetworkConfig {
    /// Latency range in milliseconds
    pub latency_range: Option<[u64; 2]>,
    /// Message drop rate
    pub drop_rate: Option<f64>,
    /// Network partitions
    pub partitions: Option<Vec<Vec<String>>>,
}

/// TOML byzantine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlByzantineConfig {
    /// Number of byzantine participants
    pub count: Option<usize>,
    /// Specific participants
    pub participants: Option<Vec<String>>,
    /// Default strategies
    pub default_strategies: Option<Vec<String>>,
}

/// TOML scenario phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlScenarioPhase {
    /// Phase name
    pub name: String,
    /// Description
    pub description: Option<String>,
    /// Actions in this phase
    pub actions: Vec<TomlChoreographyAction>,
    /// Checkpoints to create
    pub checkpoints: Option<Vec<String>>,
    /// Properties to verify
    pub verify_properties: Option<Vec<String>>,
    /// Phase timeout
    pub timeout_seconds: Option<u64>,
}

/// TOML choreography action
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TomlChoreographyAction {
    /// Run choreography action
    #[serde(rename = "run_choreography")]
    RunChoreography {
        choreography: String,
        participants: Option<Vec<String>>,
        threshold: Option<usize>,
        app_id: Option<String>,
        context: Option<String>,
        #[serde(flatten)]
        extra_params: HashMap<String, toml::Value>,
    },

    /// Execute protocol action
    #[serde(rename = "execute_protocol")]
    ExecuteProtocol {
        protocol: String,
        participants: Vec<String>,
        timeout_ticks: Option<u64>,
        #[serde(flatten)]
        extra_params: HashMap<String, toml::Value>,
    },

    /// Apply network condition
    #[serde(rename = "apply_network_condition")]
    ApplyNetworkCondition {
        condition: String,
        participants: Vec<String>,
        duration_ticks: Option<u64>,
        #[serde(flatten)]
        extra_params: HashMap<String, toml::Value>,
    },

    /// Inject byzantine behavior
    #[serde(rename = "inject_byzantine")]
    InjectByzantine {
        participant: String,
        behavior: String,
        #[serde(flatten)]
        extra_params: HashMap<String, toml::Value>,
    },

    /// Wait for ticks
    #[serde(rename = "wait_ticks")]
    WaitTicks { ticks: u64 },

    /// Create checkpoint
    #[serde(rename = "create_checkpoint")]
    CreateCheckpoint { label: String },

    /// Verify property
    #[serde(rename = "verify_property")]
    VerifyProperty { property: String, expected: bool },
}

/// TOML property check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TomlPropertyCheck {
    /// Property name
    pub name: String,
    /// Property type
    pub property_type: String,
    /// Parameters
    pub parameters: Option<HashMap<String, toml::Value>>,
    /// Phases to check in
    pub check_in_phases: Option<Vec<String>>,
}

/// Unified scenario loader
pub struct UnifiedScenarioLoader {
    /// Base directory for scenario files
    base_dir: PathBuf,
    /// Scenario cache for inheritance resolution
    scenario_cache: HashMap<String, UnifiedScenarioFile>,
    /// Enable validation
    enable_validation: bool,
}

impl UnifiedScenarioLoader {
    /// Create a new scenario loader
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            scenario_cache: HashMap::new(),
            enable_validation: true,
        }
    }

    /// Enable or disable validation
    pub fn with_validation(mut self, enable: bool) -> Self {
        self.enable_validation = enable;
        self
    }

    /// Load a scenario from a TOML file
    pub fn load_scenario<P: AsRef<Path>>(&mut self, file_path: P) -> Result<UnifiedScenario> {
        let path = file_path.as_ref();

        // Read TOML file
        let content = fs::read_to_string(path).map_err(|e| {
            AuraError::configuration_error(format!("Failed to read scenario file: {}", e))
        })?;

        // Parse TOML
        let toml_scenario: UnifiedScenarioFile = toml::from_str(&content).map_err(|e| {
            eprintln!("TOML Parse Error Details:");
            eprintln!("  File: {}", path.display());
            eprintln!("  Error: {:#}", e);
            eprintln!("  Content length: {} bytes", content.len());
            eprintln!("  Full content:\n{}", content);
            AuraError::configuration_error(format!("Failed to parse TOML: {}", e))
        })?;

        // Cache for inheritance resolution
        self.scenario_cache.insert(
            path.file_name().unwrap().to_string_lossy().to_string(),
            toml_scenario.clone(),
        );

        // Resolve inheritance
        let resolved_scenario = self.resolve_inheritance(toml_scenario)?;

        // Validate if enabled
        if self.enable_validation {
            self.validate_scenario(&resolved_scenario)?;
        }

        // Convert to unified scenario
        self.convert_scenario(resolved_scenario)
    }

    /// Load multiple scenarios from a directory
    pub fn load_scenarios_from_directory<P: AsRef<Path>>(
        &mut self,
        dir_path: P,
    ) -> Result<Vec<UnifiedScenario>> {
        let dir = dir_path.as_ref();
        let mut scenarios = Vec::new();

        // Find all TOML files
        let entries = fs::read_dir(dir).map_err(|e| {
            AuraError::configuration_error(format!("Failed to read directory: {}", e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                AuraError::configuration_error(format!("Directory entry error: {}", e))
            })?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                match self.load_scenario(&path) {
                    Ok(scenario) => scenarios.push(scenario),
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to load scenario from {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        Ok(scenarios)
    }

    /// Resolve scenario inheritance
    fn resolve_inheritance(
        &self,
        mut scenario: UnifiedScenarioFile,
    ) -> Result<UnifiedScenarioFile> {
        if let Some(extends) = &scenario.extends {
            // Load parent scenario
            let parent_path = self.base_dir.join(extends);
            let parent_content = fs::read_to_string(&parent_path).map_err(|e| {
                AuraError::configuration_error(format!("Failed to read parent scenario: {}", e))
            })?;

            let parent_scenario: UnifiedScenarioFile =
                toml::from_str(&parent_content).map_err(|e| {
                    AuraError::configuration_error(format!("Failed to parse parent TOML: {}", e))
                })?;

            // Apply inheritance
            scenario = self.merge_scenarios(parent_scenario, scenario)?;
        }

        Ok(scenario)
    }

    /// Merge parent and child scenarios
    fn merge_scenarios(
        &self,
        parent: UnifiedScenarioFile,
        mut child: UnifiedScenarioFile,
    ) -> Result<UnifiedScenarioFile> {
        // Merge setup
        if child.setup.participants.is_none() {
            child.setup.participants = parent.setup.participants;
        }
        if child.setup.threshold.is_none() {
            child.setup.threshold = parent.setup.threshold;
        }
        if child.setup.seed.is_none() {
            child.setup.seed = parent.setup.seed;
        }

        // Merge network config if not specified
        if child.network.is_none() {
            child.network = parent.network;
        }

        // Merge byzantine config if not specified
        if child.byzantine.is_none() {
            child.byzantine = parent.byzantine;
        }

        // Merge properties
        if let Some(parent_properties) = parent.properties {
            if let Some(ref mut child_properties) = child.properties {
                let mut merged_properties = parent_properties;
                merged_properties.extend(child_properties.clone());
                *child_properties = merged_properties;
            } else {
                child.properties = Some(parent_properties);
            }
        }

        Ok(child)
    }

    /// Validate scenario configuration
    fn validate_scenario(&self, scenario: &UnifiedScenarioFile) -> Result<()> {
        // Check required fields
        if scenario.phases.is_empty() {
            return Err(AuraError::configuration_error(
                "Scenario must have at least one phase".to_string(),
            ));
        }

        // Validate setup
        if let Some(participants) = scenario.setup.participants {
            if participants == 0 {
                return Err(AuraError::configuration_error(
                    "Must have at least 1 participant".to_string(),
                ));
            }

            if let Some(threshold) = scenario.setup.threshold {
                if threshold > participants {
                    return Err(AuraError::configuration_error(
                        "Threshold cannot exceed participant count".to_string(),
                    ));
                }
            }
        }

        // Validate phases
        for phase in &scenario.phases {
            if phase.actions.is_empty() {
                return Err(AuraError::configuration_error(format!(
                    "Phase '{}' must have at least one action",
                    phase.name
                )));
            }
        }

        // Validate expected outcome
        match scenario.expected_outcome.as_str() {
            "success" | "failure" | "timeout" | "byzantine_detected" => {}
            outcome if outcome.starts_with("property_violation:") => {}
            _ => {
                return Err(AuraError::configuration_error(format!(
                    "Invalid expected outcome: {}",
                    scenario.expected_outcome
                )))
            }
        }

        Ok(())
    }

    /// Convert TOML scenario to unified scenario
    fn convert_scenario(&self, toml_scenario: UnifiedScenarioFile) -> Result<UnifiedScenario> {
        // Convert setup
        let setup = ScenarioSetupConfig {
            participants: toml_scenario.setup.participants.unwrap_or(2),
            threshold: toml_scenario.setup.threshold.unwrap_or(2),
            seed: toml_scenario.setup.seed.unwrap_or(42),
            participant_configs: toml_scenario.setup.participant_configs.map(|configs| {
                configs
                    .into_iter()
                    .map(|(id, config)| {
                        (
                            id,
                            ParticipantConfig {
                                device_id: config.device_id,
                                account_id: config.account_id,
                                is_byzantine: config.is_byzantine.unwrap_or(false),
                                role: config.role,
                            },
                        )
                    })
                    .collect()
            }),
        };

        // Convert network config
        let network = toml_scenario.network.map(|net| NetworkConfig {
            latency_range: net.latency_range,
            drop_rate: net.drop_rate,
            partitions: net.partitions,
        });

        // Convert byzantine config
        let byzantine = toml_scenario.byzantine.map(|byz| ByzantineConfig {
            count: byz.count.unwrap_or(0),
            participants: byz.participants,
            default_strategies: byz.default_strategies,
        });

        // Convert phases
        let phases: Result<Vec<_>> = toml_scenario
            .phases
            .into_iter()
            .map(|phase| {
                let actions: Result<Vec<_>> = phase
                    .actions
                    .into_iter()
                    .map(|action| self.convert_action(action))
                    .collect();

                Ok(ScenarioPhaseWithActions {
                    name: phase.name,
                    description: phase.description,
                    actions: actions?,
                    checkpoints: phase.checkpoints,
                    verify_properties: phase.verify_properties,
                    timeout: phase.timeout_seconds.map(Duration::from_secs),
                })
            })
            .collect();
        let phases = phases?;

        // Convert properties
        let properties = toml_scenario
            .properties
            .unwrap_or_default()
            .into_iter()
            .map(|prop| PropertyCheck {
                name: prop.name,
                property_type: prop.property_type,
                parameters: prop.parameters,
                check_in_phases: prop.check_in_phases,
            })
            .collect();

        // Convert expected outcome
        let expected_outcome = match toml_scenario.expected_outcome.as_str() {
            "success" => ExpectedOutcome::Success,
            "failure" => ExpectedOutcome::Failure,
            "timeout" => ExpectedOutcome::Timeout,
            "byzantine_detected" => ExpectedOutcome::ByzantineDetected,
            outcome if outcome.starts_with("property_violation:") => {
                let property = outcome
                    .strip_prefix("property_violation:")
                    .unwrap()
                    .to_string();
                ExpectedOutcome::PropertyViolation { property }
            }
            _ => {
                return Err(AuraError::configuration_error(format!(
                    "Invalid expected outcome: {}",
                    toml_scenario.expected_outcome
                )))
            }
        };

        Ok(UnifiedScenario {
            name: toml_scenario.metadata.name,
            description: toml_scenario.metadata.description,
            setup,
            phases,
            network,
            byzantine,
            properties,
            expected_outcome,
            extends: toml_scenario.extends,
        })
    }

    /// Convert TOML action to choreography action
    fn convert_action(&self, action: TomlChoreographyAction) -> Result<ChoreographyAction> {
        match action {
            TomlChoreographyAction::RunChoreography {
                choreography,
                participants,
                threshold,
                app_id,
                context,
                extra_params,
            } => {
                let mut parameters = extra_params;
                if let Some(t) = threshold {
                    parameters.insert("threshold".to_string(), toml::Value::Integer(t as i64));
                }
                if let Some(app) = app_id {
                    parameters.insert("app_id".to_string(), toml::Value::String(app));
                }
                if let Some(ctx) = context {
                    parameters.insert("context".to_string(), toml::Value::String(ctx));
                }

                Ok(ChoreographyAction::RunChoreography {
                    choreography_type: choreography,
                    participants,
                    parameters,
                })
            }

            TomlChoreographyAction::ExecuteProtocol {
                protocol,
                participants,
                timeout_ticks,
                extra_params,
            } => Ok(ChoreographyAction::ExecuteProtocol {
                protocol_type: protocol,
                participants,
                timeout_ticks,
                parameters: Some(extra_params),
            }),

            TomlChoreographyAction::ApplyNetworkCondition {
                condition,
                participants,
                duration_ticks,
                extra_params,
            } => Ok(ChoreographyAction::ApplyNetworkCondition {
                condition_type: condition,
                participants,
                duration_ticks,
                parameters: extra_params,
            }),

            TomlChoreographyAction::InjectByzantine {
                participant,
                behavior,
                extra_params,
            } => Ok(ChoreographyAction::InjectByzantine {
                participant,
                behavior_type: behavior,
                parameters: extra_params,
            }),

            TomlChoreographyAction::WaitTicks { ticks } => {
                Ok(ChoreographyAction::WaitTicks { ticks })
            }

            TomlChoreographyAction::CreateCheckpoint { label } => {
                Ok(ChoreographyAction::CreateCheckpoint { label })
            }

            TomlChoreographyAction::VerifyProperty { property, expected } => {
                Ok(ChoreographyAction::VerifyProperty { property, expected })
            }
        }
    }

    /// Generate a sample TOML scenario file
    pub fn generate_sample_toml<P: AsRef<Path>>(&self, output_path: P) -> Result<()> {
        let sample = r#"[metadata]
name = "dkd_basic_test"
description = "Basic DKD choreography test with network partition"
version = "1.0"
author = "Test Suite"
tags = ["dkd", "basic", "network"]

[setup]
participants = 3
threshold = 2
seed = 42

# Optional: Specific participant configurations
# [setup.participant_configs]
# alice = { device_id = "device_alice", account_id = "account_1" }
# bob = { device_id = "device_bob", account_id = "account_1" }
# charlie = { device_id = "device_charlie", account_id = "account_1", is_byzantine = true }

[network]
latency_range = [10, 100]
drop_rate = 0.05
partitions = [["alice", "bob"]]

[byzantine]
count = 1
default_strategies = ["drop_messages"]

[[phases]]
name = "initial_setup"
description = "Set up participants and run basic DKD"

  [[phases.actions]]
  type = "run_choreography"
  choreography = "dkd"
  participants = ["alice", "bob", "charlie"]
  threshold = 2
  app_id = "test_app_v1"
  context = "user_authentication"

  [[phases.actions]]
  type = "create_checkpoint"
  label = "after_dkd"

[[phases]]
name = "network_failure"
description = "Test behavior under network partition"

  [[phases.actions]]
  type = "apply_network_condition"
  condition = "partition"
  participants = ["alice", "bob"]
  duration_ticks = 10

  [[phases.actions]]
  type = "wait_ticks"
  ticks = 15

  [[phases.actions]]
  type = "verify_property"
  property = "threshold_security"
  expected = true

[[properties]]
name = "threshold_security"
property_type = "byzantine_tolerance"
check_in_phases = ["network_failure"]

expected_outcome = "success"
"#;

        fs::write(output_path, sample).map_err(|e| {
            AuraError::configuration_error(format!("Failed to write sample file: {}", e))
        })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_scenario_loader_creation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = UnifiedScenarioLoader::new(temp_dir.path());

        assert_eq!(loader.base_dir, temp_dir.path());
        assert!(loader.enable_validation);
    }

    #[test]
    fn test_sample_toml_generation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = UnifiedScenarioLoader::new(temp_dir.path());
        let sample_path = temp_dir.path().join("sample.toml");

        loader.generate_sample_toml(&sample_path).unwrap();

        assert!(sample_path.exists());
        let content = fs::read_to_string(&sample_path).unwrap();
        assert!(content.contains("[metadata]"));
        assert!(content.contains("dkd_basic_test"));
    }

    #[test]
    fn test_sample_toml_parsing() {
        let temp_dir = TempDir::new().unwrap();
        let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
        let sample_path = temp_dir.path().join("sample.toml");

        // Generate and load sample
        loader.generate_sample_toml(&sample_path).unwrap();
        let scenario = loader.load_scenario(&sample_path).unwrap();

        assert_eq!(scenario.name, "dkd_basic_test");
        assert_eq!(scenario.phases.len(), 2);
        assert_eq!(scenario.setup.participants, 3);
        assert_eq!(scenario.setup.threshold, 2);
    }

    #[test]
    fn test_validation() {
        let temp_dir = TempDir::new().unwrap();
        let loader = UnifiedScenarioLoader::new(temp_dir.path());

        // Test invalid scenario (no phases)
        let invalid_toml = UnifiedScenarioFile {
            metadata: ScenarioMetadata {
                name: "invalid".to_string(),
                description: "Invalid scenario".to_string(),
                version: None,
                author: None,
                tags: None,
            },
            setup: TomlScenarioSetup {
                participants: Some(2),
                threshold: Some(2),
                seed: Some(42),
                participant_configs: None,
            },
            network: None,
            byzantine: None,
            phases: Vec::new(), // No phases - invalid
            properties: None,
            expected_outcome: "success".to_string(),
            extends: None,
        };

        let result = loader.validate_scenario(&invalid_toml);
        assert!(result.is_err());
    }
}
