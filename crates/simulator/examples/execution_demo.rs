//! Unified Test Execution Model Demonstration
//!
//! This example showcases the unified testing framework that serves as the canonical
//! entry point for all tests, demonstrating how imperative helpers from runners/
//! have been converted to declarative choreography actions.

use aura_simulator::{
    choreography_actions::register_standard_choreographies,
    unified_scenario_engine::{
        ChoreographyAction, ExpectedOutcome, PropertyCheck, ScenarioPhaseWithActions,
        ScenarioSetupConfig, UnifiedScenario,
    },
    Result, UnifiedEngineConfig, UnifiedScenarioEngine, UnifiedScenarioLoader,
};
use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Unified Test Execution Model Demo ===\n");

    // Demonstrate the unified approach
    demonstrate_toml_scenario_execution()?;
    println!();

    demonstrate_programmatic_scenario_creation()?;
    println!();

    demonstrate_choreography_actions()?;
    println!();

    demonstrate_debugging_integration()?;
    println!();

    demonstrate_scenario_inheritance()?;

    Ok(())
}

/// Demonstrate loading and executing TOML scenarios
fn demonstrate_toml_scenario_execution() -> Result<(), Box<dyn std::error::Error>> {
    println!("1. TOML Scenario Execution");
    println!("==========================");

    let temp_dir = TempDir::new()?;

    // Create a scenario loader and generate sample TOML
    let loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario_path = temp_dir.path().join("dkd_test.toml");
    loader.generate_sample_toml(&scenario_path)?;

    println!(
        "[OK] Generated sample TOML scenario at: {}",
        scenario_path.display()
    );

    // Load the scenario
    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario = loader.load_scenario(&scenario_path)?;

    println!("[OK] Loaded scenario: '{}'", scenario.name);
    println!("  - Description: {}", scenario.description);
    println!("  - Participants: {}", scenario.setup.participants);
    println!("  - Phases: {}", scenario.phases.len());

    // Create unified engine and register choreographies
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())?;
    register_standard_choreographies(&mut engine);

    println!("[OK] Created unified engine with standard choreographies");

    // Execute the scenario
    let result = engine.execute_scenario(&scenario)?;

    println!("[OK] Scenario execution completed:");
    println!("  - Success: {}", result.success);
    println!("  - Phases executed: {}", result.phase_results.len());
    println!("  - Properties checked: {}", result.property_results.len());
    println!("  - Execution time: {:?}", result.execution_time);
    println!("  - Artifacts generated: {}", result.artifacts.len());

    println!("[OK] Benefits demonstrated:");
    println!("  - Single entry point for all tests");
    println!("  - Declarative TOML scenario definition");
    println!("  - Automatic choreography action resolution");
    println!("  - Integrated debugging and analysis");

    Ok(())
}

/// Demonstrate programmatic scenario creation
fn demonstrate_programmatic_scenario_creation() -> Result<(), Box<dyn std::error::Error>> {
    println!("2. Programmatic Scenario Creation");
    println!("=================================");

    let temp_dir = TempDir::new()?;

    // Create a scenario programmatically
    let scenario = UnifiedScenario {
        name: "Programmatic DKD Test".to_string(),
        description: "DKD protocol test created programmatically".to_string(),
        setup: ScenarioSetupConfig {
            participants: 3,
            threshold: 2,
            seed: 12345,
            participant_configs: None,
        },
        phases: vec![
            ScenarioPhaseWithActions {
                name: "dkd_execution".to_string(),
                description: Some("Execute DKD choreography".to_string()),
                actions: vec![
                    ChoreographyAction::RunChoreography {
                        choreography_type: "dkd".to_string(),
                        participants: Some(vec![
                            "alice".to_string(),
                            "bob".to_string(),
                            "charlie".to_string(),
                        ]),
                        parameters: {
                            let mut params = HashMap::new();
                            params.insert("threshold".to_string(), toml::Value::Integer(2));
                            params.insert(
                                "app_id".to_string(),
                                toml::Value::String("programmatic_test".to_string()),
                            );
                            params.insert(
                                "context".to_string(),
                                toml::Value::String("demo_context".to_string()),
                            );
                            params
                        },
                    },
                    ChoreographyAction::CreateCheckpoint {
                        label: "after_dkd".to_string(),
                    },
                ],
                checkpoints: None,
                verify_properties: None,
                timeout: Some(Duration::from_secs(30)),
            },
            ScenarioPhaseWithActions {
                name: "network_testing".to_string(),
                description: Some("Test under network conditions".to_string()),
                actions: vec![
                    ChoreographyAction::ApplyNetworkCondition {
                        condition_type: "partition".to_string(),
                        participants: vec!["alice".to_string(), "bob".to_string()],
                        duration_ticks: Some(10),
                        parameters: HashMap::new(),
                    },
                    ChoreographyAction::WaitTicks { ticks: 15 },
                    ChoreographyAction::VerifyProperty {
                        property: "threshold_security".to_string(),
                        expected: true,
                    },
                ],
                checkpoints: Some(vec!["network_test_complete".to_string()]),
                verify_properties: Some(vec!["threshold_security".to_string()]),
                timeout: None,
            },
        ],
        network: None,
        byzantine: None,
        properties: vec![PropertyCheck {
            name: "threshold_security".to_string(),
            property_type: "byzantine_tolerance".to_string(),
            parameters: None,
            check_in_phases: Some(vec!["network_testing".to_string()]),
        }],
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    };

    println!(
        "[OK] Created scenario programmatically: '{}'",
        scenario.name
    );
    println!("  - Phases: {}", scenario.phases.len());
    println!(
        "  - Total actions: {}",
        scenario
            .phases
            .iter()
            .map(|p| p.actions.len())
            .sum::<usize>()
    );

    // Execute with debugging enabled
    let config = UnifiedEngineConfig {
        enable_debugging: true,
        verbose: true,
        export_reports: true,
        ..Default::default()
    };

    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())?.configure(config);
    register_standard_choreographies(&mut engine);

    let result = engine.execute_scenario(&scenario)?;

    println!("[OK] Programmatic scenario execution completed:");
    println!("  - Success: {}", result.success);
    for (i, phase_result) in result.phase_results.iter().enumerate() {
        println!(
            "  - Phase {}: '{}' - Success: {}, Actions: {}",
            i + 1,
            phase_result.phase_name,
            phase_result.success,
            phase_result.action_results.len()
        );
    }

    println!("[OK] Programming benefits:");
    println!("  - Type-safe scenario construction");
    println!("  - IDE support and autocompletion");
    println!("  - Runtime validation");
    println!("  - Seamless integration with TOML scenarios");

    Ok(())
}

/// Demonstrate individual choreography actions
fn demonstrate_choreography_actions() -> Result<(), Box<dyn std::error::Error>> {
    println!("3. Choreography Actions (Refactored Runners)");
    println!("=============================================");

    let temp_dir = TempDir::new()?;

    // Create engine with verbose logging
    let config = UnifiedEngineConfig {
        verbose: true,
        enable_debugging: true,
        ..Default::default()
    };

    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())?.configure(config);
    register_standard_choreographies(&mut engine);

    println!("[OK] Available choreography actions (refactored from runners/):");
    println!("  [key] dkd - Deterministic Key Derivation");
    println!("  [reload] resharing - Key Resharing Protocol");
    println!("  [shield] recovery - Guardian-based Recovery");
    println!("  [lock] locking - Distributed Locking");

    // Demonstrate each choreography action type
    let choreography_demo = UnifiedScenario {
        name: "Choreography Actions Demo".to_string(),
        description: "Demonstrates all standard choreography actions".to_string(),
        setup: ScenarioSetupConfig {
            participants: 4,
            threshold: 3,
            seed: 42,
            participant_configs: None,
        },
        phases: vec![
            ScenarioPhaseWithActions {
                name: "dkd_demo".to_string(),
                description: Some("DKD choreography demonstration".to_string()),
                actions: vec![ChoreographyAction::RunChoreography {
                    choreography_type: "dkd".to_string(),
                    participants: None, // Use all participants
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("threshold".to_string(), toml::Value::Integer(3));
                        params.insert(
                            "app_id".to_string(),
                            toml::Value::String("demo_app".to_string()),
                        );
                        params.insert(
                            "context".to_string(),
                            toml::Value::String("choreography_demo".to_string()),
                        );
                        params
                    },
                }],
                checkpoints: Some(vec!["dkd_complete".to_string()]),
                verify_properties: None,
                timeout: None,
            },
            ScenarioPhaseWithActions {
                name: "resharing_demo".to_string(),
                description: Some("Resharing choreography demonstration".to_string()),
                actions: vec![ChoreographyAction::RunChoreography {
                    choreography_type: "resharing".to_string(),
                    participants: None,
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("old_threshold".to_string(), toml::Value::Integer(3));
                        params.insert("new_threshold".to_string(), toml::Value::Integer(2));
                        params
                    },
                }],
                checkpoints: None,
                verify_properties: None,
                timeout: None,
            },
            ScenarioPhaseWithActions {
                name: "recovery_demo".to_string(),
                description: Some("Recovery choreography demonstration".to_string()),
                actions: vec![ChoreographyAction::RunChoreography {
                    choreography_type: "recovery".to_string(),
                    participants: Some(vec!["alice".to_string(), "bob".to_string()]),
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("guardian_threshold".to_string(), toml::Value::Integer(2));
                        params.insert("cooldown_hours".to_string(), toml::Value::Integer(24));
                        params
                    },
                }],
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
    };

    let result = engine.execute_scenario(&choreography_demo)?;

    println!("[OK] Choreography actions demonstration completed:");
    println!(
        "  - All phases: {}",
        result.phase_results.iter().all(|p| p.success)
    );
    println!(
        "  - DKD action: {}",
        result.phase_results[0].action_results[0].success
    );
    println!(
        "  - Resharing action: {}",
        result.phase_results[1].action_results[0].success
    );
    println!(
        "  - Recovery action: {}",
        result.phase_results[2].action_results[0].success
    );

    println!("[OK] Refactoring benefits:");
    println!("  - Imperative runners/ helpers → declarative actions");
    println!("  - Unified execution model for all test types");
    println!("  - TOML-configurable protocol parameters");
    println!("  - Consistent error handling and reporting");

    Ok(())
}

/// Demonstrate debugging integration
fn demonstrate_debugging_integration() -> Result<(), Box<dyn std::error::Error>> {
    println!("4. Debugging Integration");
    println!("========================");

    let temp_dir = TempDir::new()?;

    // Create engine with full debugging enabled
    let config = UnifiedEngineConfig {
        enable_debugging: true,
        auto_checkpoint_interval: Some(5),
        verbose: true,
        export_reports: true,
        artifact_prefix: "debug_demo".to_string(),
        ..Default::default()
    };

    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())?.configure(config);
    register_standard_choreographies(&mut engine);

    println!("[OK] Debugging features enabled:");
    println!("  [stats] Passive trace recording");
    println!("  [checkpoint] Automatic checkpointing every 5 ticks");
    println!("  [search] Property monitoring");
    println!("  [file] Detailed report generation");

    // Create a scenario with debugging features
    let debug_scenario = UnifiedScenario {
        name: "Debug Integration Demo".to_string(),
        description: "Scenario demonstrating integrated debugging features".to_string(),
        setup: ScenarioSetupConfig {
            participants: 3,
            threshold: 2,
            seed: 777,
            participant_configs: None,
        },
        phases: vec![
            ScenarioPhaseWithActions {
                name: "setup_and_checkpoint".to_string(),
                description: Some("Initial setup with manual checkpoint".to_string()),
                actions: vec![
                    ChoreographyAction::CreateCheckpoint {
                        label: "initial_state".to_string(),
                    },
                    ChoreographyAction::RunChoreography {
                        choreography_type: "dkd".to_string(),
                        participants: None,
                        parameters: {
                            let mut params = HashMap::new();
                            params.insert(
                                "app_id".to_string(),
                                toml::Value::String("debug_test".to_string()),
                            );
                            params
                        },
                    },
                    ChoreographyAction::CreateCheckpoint {
                        label: "after_dkd".to_string(),
                    },
                ],
                checkpoints: None,
                verify_properties: None,
                timeout: None,
            },
            ScenarioPhaseWithActions {
                name: "byzantine_injection".to_string(),
                description: Some("Inject Byzantine behavior for testing".to_string()),
                actions: vec![
                    ChoreographyAction::InjectByzantine {
                        participant: "participant_0".to_string(),
                        behavior_type: "drop_messages".to_string(),
                        parameters: HashMap::new(),
                    },
                    ChoreographyAction::WaitTicks { ticks: 10 },
                    ChoreographyAction::VerifyProperty {
                        property: "byzantine_tolerance".to_string(),
                        expected: true,
                    },
                ],
                checkpoints: Some(vec!["byzantine_test_complete".to_string()]),
                verify_properties: Some(vec!["byzantine_tolerance".to_string()]),
                timeout: None,
            },
        ],
        network: None,
        byzantine: None,
        properties: vec![PropertyCheck {
            name: "byzantine_tolerance".to_string(),
            property_type: "safety_under_byzantine".to_string(),
            parameters: None,
            check_in_phases: Some(vec!["byzantine_injection".to_string()]),
        }],
        expected_outcome: ExpectedOutcome::Success,
        extends: None,
    };

    let result = engine.execute_scenario(&debug_scenario)?;

    println!("[OK] Debug scenario execution completed:");
    println!("  - Success: {}", result.success);
    println!("  - Total execution time: {:?}", result.execution_time);
    println!("  - Final world state:");
    println!("    - Tick: {}", result.final_state.current_tick);
    println!(
        "    - Participants: {}",
        result.final_state.participant_count
    );
    println!(
        "    - Byzantine count: {}",
        result.final_state.byzantine_count
    );
    println!("  - Generated artifacts: {}", result.artifacts.len());

    for artifact in &result.artifacts {
        println!("    [file] {}", artifact);
    }

    println!("[OK] Debugging integration benefits:");
    println!("  - Zero-coupling between simulation and debugging");
    println!("  - Automatic artifact generation");
    println!("  - Time-travel debugging capabilities");
    println!("  - Comprehensive trace analysis");

    Ok(())
}

/// Demonstrate scenario inheritance
fn demonstrate_scenario_inheritance() -> Result<(), Box<dyn std::error::Error>> {
    println!("5. Scenario Inheritance");
    println!("=======================");

    let temp_dir = TempDir::new()?;

    // Create a base scenario TOML file
    let base_scenario = r#"[metadata]
name = "base_dkd_scenario"
description = "Base DKD scenario for inheritance"

[setup]
participants = 3
threshold = 2
seed = 42

[[phases]]
name = "basic_dkd"
description = "Basic DKD execution"

  [[phases.actions]]
  type = "run_choreography"
  choreography = "dkd"
  app_id = "base_app"
  context = "inheritance_test"

expected_outcome = "success"
"#;

    let base_path = temp_dir.path().join("base_scenario.toml");
    std::fs::write(&base_path, base_scenario)?;

    // Create a child scenario that extends the base
    let child_scenario = r#"extends = "base_scenario.toml"

[metadata]
name = "extended_dkd_scenario"
description = "Extended DKD scenario with Byzantine testing"

[byzantine]
count = 1
default_strategies = ["drop_messages"]

[[phases]]
name = "byzantine_testing"
description = "Additional Byzantine testing phase"

  [[phases.actions]]
  type = "inject_byzantine"
  participant = "participant_0"
  behavior = "drop_messages"

  [[phases.actions]]
  type = "wait_ticks"
  ticks = 10

[[properties]]
name = "byzantine_tolerance"
property_type = "safety_under_byzantine"
"#;

    let child_path = temp_dir.path().join("child_scenario.toml");
    std::fs::write(&child_path, child_scenario)?;

    println!("[OK] Created base and child scenario files");

    // Load and execute the child scenario (which inherits from base)
    let mut loader = UnifiedScenarioLoader::new(temp_dir.path());
    let scenario = loader.load_scenario(&child_path)?;

    println!("[OK] Loaded child scenario with inheritance:");
    println!("  - Name: {} (overridden)", scenario.name);
    println!(
        "  - Participants: {} (inherited)",
        scenario.setup.participants
    );
    println!("  - Threshold: {} (inherited)", scenario.setup.threshold);
    println!("  - Phases: {} (base + extended)", scenario.phases.len());
    println!("  - Properties: {} (added)", scenario.properties.len());

    // Execute the inherited scenario
    let mut engine = UnifiedScenarioEngine::new(temp_dir.path())?;
    register_standard_choreographies(&mut engine);

    let result = engine.execute_scenario(&scenario)?;

    println!("[OK] Inherited scenario execution completed:");
    println!("  - Success: {}", result.success);
    println!(
        "  - Base phase (inherited): {}",
        result.phase_results[0].success
    );
    println!(
        "  - Extended phase (new): {}",
        result.phase_results[1].success
    );

    println!("[OK] Inheritance benefits:");
    println!("  - Scenario composition and reuse");
    println!("  - Override specific configurations");
    println!("  - Extend base scenarios with additional phases");
    println!("  - Consistent base testing patterns");

    Ok(())
}

/// Show the architectural transformation
#[allow(dead_code)]
fn show_architectural_transformation() {
    println!("=== Unified Test Execution Model Transformation ===\n");

    println!("BEFORE (Fragmented Testing):");
    println!("- scenarios/engine.rs - TOML scenario execution");
    println!("- runners/protocol.rs - Imperative protocol helpers");
    println!("- runners/choreographic.rs - Imperative choreography helpers");
    println!("- Multiple test entry points and execution models");
    println!("- Inconsistent scenario formats across directories");

    println!();
    println!("AFTER (Unified Testing Framework):");
    println!("- UnifiedScenarioEngine - Single canonical entry point");
    println!("- ChoreographyActions - Declarative action system");
    println!("- TOML scenarios - Unified scenario format");
    println!("- Passive debugging integration");
    println!("- Scenario inheritance and composition");

    println!();
    println!("Key Transformations:");
    println!("[OK] runners/protocol.rs → ChoreographyAction::ExecuteProtocol");
    println!("[OK] runners/choreographic.rs → ChoreographyAction::RunChoreography");
    println!("[OK] Multiple TOML formats → Unified scenario schema");
    println!("[OK] Separate execution paths → Single entry point");
    println!("[OK] Imperative helpers → Declarative actions");

    println!();
    println!("Benefits:");
    println!("[OK] Unified testing framework for all test types");
    println!("[OK] Declarative TOML scenarios with inheritance");
    println!("[OK] Consistent execution model and reporting");
    println!("[OK] Integrated debugging and analysis tools");
    println!("[OK] Easy test discovery and management");
    println!("[OK] Choreography actions that replace imperative helpers");
}
