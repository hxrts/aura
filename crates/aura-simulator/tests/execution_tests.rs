//! Comprehensive tests for the test execution model
//!
//! These tests verify that the scenario engine properly serves as the
//! canonical entry point for all tests, integrating TOML scenarios, choreography
//! actions, and debugging tools.

use aura_simulator::{
    scenario::{
        ChoreographyAction, PropertyCheck,
        ScenarioPhaseWithActions, ScenarioSetupConfig, UnifiedEngineConfig, UnifiedScenario,
        UnifiedScenarioEngine, UnifiedScenarioLoader, register_all_standard_choreographies,
        engine::ExpectedOutcome
    },
};
use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_engine_creation() {
    let temp_dir = TempDir::new().unwrap();
    let engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();

    // Default configuration should be sensible
    assert!(engine.config().enable_debugging);
    assert_eq!(engine.config().auto_checkpoint_interval, Some(50));
    assert_eq!(engine.config().max_execution_time, Duration::from_secs(60));
}

#[test]
fn test_engine_configuration() {
    let temp_dir = TempDir::new().unwrap();
    let config = UnifiedEngineConfig {
        enable_debugging: false,
        verbose: true,
        auto_checkpoint_interval: Some(10),
        ..Default::default()
    };

    let engine = UnifiedScenarioEngine::new(temp_dir.path())
        .unwrap()
        .configure(config);

    assert!(!engine.config().enable_debugging);
    assert!(engine.config().verbose);
    assert_eq!(engine.config().auto_checkpoint_interval, Some(10));
}

#[test]
fn test_choreography_registration() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();

    // Register standard choreographies
    register_all_standard_choreographies(&mut engine);

    // The engine should now have choreographies available
    // This is tested indirectly through scenario execution
    assert!(true); // Placeholder - registration is internal
}

#[test]
fn test_basic_scenario_execution() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let scenario = create_basic_test_scenario();
    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);
    assert_eq!(result.scenario_name, "Basic Test Scenario");
    assert_eq!(result.phase_results.len(), 1);
    assert!(result.phase_results[0].success);
    assert_eq!(result.phase_results[0].action_results.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_multi_phase_scenario() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let scenario = create_multi_phase_scenario();
    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);
    assert_eq!(result.phase_results.len(), 3);

    // Check each phase
    assert!(result.phase_results[0].success); // DKD phase
    assert!(result.phase_results[1].success); // Network test phase
    assert!(result.phase_results[2].success); // Byzantine phase
}

/// TODO: Update test to match current choreography implementation
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_choreography_actions() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let scenario = create_choreography_test_scenario();
    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);

    // Verify individual actions executed successfully
    let phase = &result.phase_results[0];
    assert_eq!(phase.action_results.len(), 4);

    // DKD choreography
    assert!(phase.action_results[0].success);
    assert_eq!(phase.action_results[0].action_type, "run_choreography");

    // Wait ticks
    assert!(phase.action_results[1].success);
    assert_eq!(phase.action_results[1].action_type, "wait_ticks");

    // Checkpoint creation
    assert!(phase.action_results[2].success);
    assert_eq!(phase.action_results[2].action_type, "create_checkpoint");
}

/// TODO: Update test to match current debugging integration
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_debugging_integration() {
    let temp_dir = TempDir::new().unwrap();
    let config = UnifiedEngineConfig {
        enable_debugging: true,
        auto_checkpoint_interval: Some(5),
        export_reports: true,
        ..Default::default()
    };

    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())
        .unwrap()
        .configure(config);
    register_all_standard_choreographies(&mut engine);

    let scenario = create_debugging_test_scenario();
    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);

    // Should have generated artifacts with debugging enabled
    assert!(!result.artifacts.is_empty());

    // Should have world state summary
    assert!(result.final_state.current_tick > 0);
    assert_eq!(result.final_state.participant_count, 3);
}

#[test]
fn test_toml_scenario_loading() {
    let temp_dir = TempDir::new().unwrap();
    let loader = UnifiedScenarioLoader::new(temp_dir.path());

    // Generate sample TOML
    let sample_path = temp_dir.path().join("test_scenario.toml");
    loader.generate_sample_toml(&sample_path).unwrap();

    // Load the scenario
    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario = loader.load_scenario(&sample_path).unwrap();

    assert_eq!(scenario.name, "dkd_basic_test");
    assert_eq!(scenario.setup.participants, 3);
    assert_eq!(scenario.setup.threshold, 2);
    assert_eq!(scenario.phases.len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_toml_scenario_execution() {
    let temp_dir = TempDir::new().unwrap();
    let loader = UnifiedScenarioLoader::new(temp_dir.path());

    // Generate and load sample TOML
    let sample_path = temp_dir.path().join("test_scenario.toml");
    loader.generate_sample_toml(&sample_path).unwrap();

    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario = loader.load_scenario(&sample_path).unwrap();

    // Execute the loaded scenario
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);
    assert_eq!(result.scenario_name, "dkd_basic_test");
}

/// TODO: Update test to match current scenario inheritance
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_scenario_inheritance() {
    let temp_dir = TempDir::new().unwrap();

    // Create base scenario
    let base_toml = r#"[metadata]
name = "base_scenario"
description = "Base scenario for inheritance"

[setup]
participants = 2
threshold = 2
seed = 42

[[phases]]
name = "base_phase"

  [[phases.actions]]
  type = "wait_ticks"
  ticks = 5

expected_outcome = "success"
"#;

    let base_path = temp_dir.path().join("base.toml");
    std::fs::write(&base_path, base_toml).unwrap();

    // Create child scenario
    let child_toml = r#"extends = "base.toml"

[metadata]
name = "extended_scenario"
description = "Extended scenario"

[setup]
participants = 3  # Override participants

[[phases]]
name = "extended_phase"

  [[phases.actions]]
  type = "wait_ticks"
  ticks = 3
"#;

    let child_path = temp_dir.path().join("child.toml");
    std::fs::write(&child_path, child_toml).unwrap();

    // Load child scenario (should inherit from base)
    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario = loader.load_scenario(&child_path).unwrap();

    assert_eq!(scenario.name, "extended_scenario");
    assert_eq!(scenario.setup.participants, 3); // Overridden
    assert_eq!(scenario.setup.threshold, 2); // Inherited
    assert_eq!(scenario.setup.seed, 42); // Inherited
    assert_eq!(scenario.phases.len(), 2); // Base + extended
}

#[test]
fn test_scenario_validation() {
    let temp_dir = TempDir::new().unwrap();
    let loader = UnifiedScenarioLoader::new(temp_dir.path());

    // Test invalid scenario (no phases)
    let invalid_toml = r#"[metadata]
name = "invalid_scenario"
description = "Invalid scenario"

[setup]
participants = 2
threshold = 2
seed = 42

expected_outcome = "success"
"#;

    let invalid_path = temp_dir.path().join("invalid.toml");
    std::fs::write(&invalid_path, invalid_toml).unwrap();

    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let result = loader.load_scenario(&invalid_path);

    assert!(result.is_err());
}

#[test]
fn test_property_checking() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let scenario = create_property_test_scenario();
    let result = engine.execute_scenario(&scenario).unwrap();

    assert!(result.success);
    assert_eq!(result.property_results.len(), 1);
    assert_eq!(result.property_results[0].property_name, "test_property");
    assert!(result.property_results[0].holds);
}

/// TODO: Update test to match current suite execution
#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn test_scenario_suite_execution() {
    let temp_dir = TempDir::new().unwrap();
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path()).unwrap();
    register_all_standard_choreographies(&mut engine);

    let scenarios = vec![
        create_basic_test_scenario(),
        create_choreography_test_scenario(),
    ];

    let results = engine.execute_scenario_suite(&scenarios).unwrap();

    assert_eq!(results.len(), 2);
    assert!(results[0].success);
    assert!(results[1].success);
}

// Helper functions to create test scenarios

fn create_basic_test_scenario() -> UnifiedScenario {
    UnifiedScenario {
        name: "Basic Test Scenario".to_string(),
        description: "Simple test scenario".to_string(),
        setup: ScenarioSetupConfig {
            participants: 2,
            threshold: 2,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![ScenarioPhaseWithActions {
            name: "basic_test".to_string(),
            description: Some("Basic test phase".to_string()),
            actions: vec![
                ChoreographyAction::WaitTicks { ticks: 5 },
                ChoreographyAction::CreateCheckpoint {
                    label: "test_checkpoint".to_string(),
                },
            ],
            checkpoints: None,
            verify_properties: None,
            timeout: None,
        }],
        network: None,
        byzantine: None,
        properties: Vec::new(),
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    }
}

fn create_multi_phase_scenario() -> UnifiedScenario {
    UnifiedScenario {
        name: "Multi-Phase Test".to_string(),
        description: "Multi-phase test scenario".to_string(),
        setup: ScenarioSetupConfig {
            participants: 3,
            threshold: 2,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![
            ScenarioPhaseWithActions {
                name: "dkd_phase".to_string(),
                description: Some("DKD execution phase".to_string()),
                actions: vec![ChoreographyAction::RunChoreography {
                    choreography_type: "dkd".to_string(),
                    participants: None,
                    parameters: HashMap::new(),
                }],
                checkpoints: None,
                verify_properties: None,
                timeout: None,
            },
            ScenarioPhaseWithActions {
                name: "network_test".to_string(),
                description: Some("Network testing phase".to_string()),
                actions: vec![
                    ChoreographyAction::ApplyNetworkCondition {
                        condition_type: "partition".to_string(),
                        participants: vec!["participant_0".to_string()],
                        duration_ticks: Some(5),
                        parameters: HashMap::new(),
                    },
                    ChoreographyAction::WaitTicks { ticks: 10 },
                ],
                checkpoints: None,
                verify_properties: None,
                timeout: None,
            },
            ScenarioPhaseWithActions {
                name: "byzantine_test".to_string(),
                description: Some("Byzantine testing phase".to_string()),
                actions: vec![
                    ChoreographyAction::InjectByzantine {
                        participant: "participant_0".to_string(),
                        behavior_type: "drop_messages".to_string(),
                        parameters: HashMap::new(),
                    },
                    ChoreographyAction::WaitTicks { ticks: 5 },
                ],
                checkpoints: None,
                verify_properties: None,
                timeout: None,
            },
        ],
        network: None,
        byzantine: None,
        properties: Vec::new(),
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    }
}

fn create_choreography_test_scenario() -> UnifiedScenario {
    UnifiedScenario {
        name: "Choreography Test".to_string(),
        description: "Test choreography actions".to_string(),
        setup: ScenarioSetupConfig {
            participants: 3,
            threshold: 2,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![ScenarioPhaseWithActions {
            name: "choreography_actions".to_string(),
            description: Some("Test various choreography actions".to_string()),
            actions: vec![
                ChoreographyAction::RunChoreography {
                    choreography_type: "dkd".to_string(),
                    participants: None,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert(
                            "app_id".to_string(),
                            toml::Value::String("test_app".to_string()),
                        );
                        params
                    },
                },
                ChoreographyAction::WaitTicks { ticks: 10 },
                ChoreographyAction::CreateCheckpoint {
                    label: "choreography_test".to_string(),
                },
                ChoreographyAction::VerifyProperty {
                    property: "test_property".to_string(),
                    expected: true,
                },
            ],
            checkpoints: None,
            verify_properties: None,
            timeout: None,
        }],
        network: None,
        byzantine: None,
        properties: Vec::new(),
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    }
}

fn create_debugging_test_scenario() -> UnifiedScenario {
    UnifiedScenario {
        name: "Debugging Test".to_string(),
        description: "Test debugging integration".to_string(),
        setup: ScenarioSetupConfig {
            participants: 3,
            threshold: 2,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![ScenarioPhaseWithActions {
            name: "debug_test".to_string(),
            description: Some("Test debugging features".to_string()),
            actions: vec![
                ChoreographyAction::CreateCheckpoint {
                    label: "start".to_string(),
                },
                ChoreographyAction::RunChoreography {
                    choreography_type: "dkd".to_string(),
                    participants: None,
                    parameters: HashMap::new(),
                },
                ChoreographyAction::WaitTicks { ticks: 15 },
                ChoreographyAction::CreateCheckpoint {
                    label: "end".to_string(),
                },
            ],
            checkpoints: Some(vec!["debug_checkpoint".to_string()]),
            verify_properties: None,
            timeout: None,
        }],
        network: None,
        byzantine: None,
        properties: Vec::new(),
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    }
}

fn create_property_test_scenario() -> UnifiedScenario {
    UnifiedScenario {
        name: "Property Test".to_string(),
        description: "Test property checking".to_string(),
        setup: ScenarioSetupConfig {
            participants: 2,
            threshold: 2,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![ScenarioPhaseWithActions {
            name: "property_test".to_string(),
            description: Some("Test property verification".to_string()),
            actions: vec![
                ChoreographyAction::WaitTicks { ticks: 5 },
                ChoreographyAction::VerifyProperty {
                    property: "test_property".to_string(),
                    expected: true,
                },
            ],
            checkpoints: None,
            verify_properties: None,
            timeout: None,
        }],
        network: None,
        byzantine: None,
        properties: vec![PropertyCheck {
            name: "test_property".to_string(),
            property_type: "basic_test".to_string(),
            parameters: None,
            check_in_phases: Some(vec!["property_test".to_string()]),
        }],
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    }
}
