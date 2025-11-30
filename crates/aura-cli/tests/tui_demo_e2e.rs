//! TUI Demo E2E Tests
//!
//! End-to-end tests for the TUI demo functionality.
//! These tests verify that the demo system initializes correctly
//! and runs without panicking (particularly the nested runtime issue).

#![cfg(feature = "development")]

use anyhow::Result;
use aura_macros::aura_test;

/// Test that the human-agent demo handler runs without panicking
///
/// This specifically tests the fix for the nested tokio runtime issue
/// that occurred when trying to create a new runtime inside an existing one.
#[aura_test]
async fn test_human_agent_demo_runs_without_panic() -> Result<()> {
    use aura_cli::demo::{setup_and_run_human_agent_demo, DemoSetupConfig, HumanAgentDemoConfig};

    let setup_config = DemoSetupConfig {
        participant_count: 3,
        guardian_threshold: 2,
        setup_chat_history: false, // Skip chat for faster test
        initial_message_count: 0,
        verbose_logging: false,
        simulate_network_activity: false, // Skip network sim for faster test
    };

    let demo_config = HumanAgentDemoConfig {
        auto_advance: true,
        agent_delay_ms: 100, // Fast for testing
        verbose_logging: false,
        guardian_response_time_ms: 100,
        max_demo_duration_minutes: 1,
    };

    // This should complete without panicking
    let result = setup_and_run_human_agent_demo(setup_config, demo_config, 42).await;

    // The demo should complete successfully
    assert!(
        result.is_ok(),
        "Demo should complete successfully: {:?}",
        result.err()
    );

    Ok(())
}

/// Test scenario setup creates valid environment
#[aura_test]
async fn test_scenario_setup_creates_valid_environment() -> Result<()> {
    use aura_cli::demo::{DemoScenarioBridge, DemoSetupConfig};

    let setup_config = DemoSetupConfig {
        participant_count: 3,
        guardian_threshold: 2,
        setup_chat_history: true,
        initial_message_count: 3,
        verbose_logging: false,
        simulate_network_activity: false,
    };

    let bridge = DemoScenarioBridge::new(42, setup_config);
    let result = bridge.setup_demo_environment().await;

    assert!(result.is_ok(), "Setup should succeed: {:?}", result.err());

    let setup_result = result.unwrap();

    // Verify all authorities were created
    assert!(
        setup_result.bob_authority != setup_result.alice_authority,
        "Bob and Alice should have different authorities"
    );
    assert!(
        setup_result.bob_authority != setup_result.charlie_authority,
        "Bob and Charlie should have different authorities"
    );
    assert!(
        setup_result.alice_authority != setup_result.charlie_authority,
        "Alice and Charlie should have different authorities"
    );

    // Verify setup metrics
    assert!(
        setup_result.setup_metrics.scenarios_executed > 0,
        "At least one scenario should have been executed"
    );
    assert!(
        setup_result.setup_metrics.guardian_registrations >= 2,
        "At least 2 guardians should be registered"
    );

    Ok(())
}

/// Test simulation effect composer async initialization
///
/// This tests the core fix for the nested runtime issue - ensuring
/// that SimulationEffectComposer::for_simulation_async works correctly
/// when called from within an existing tokio runtime.
#[aura_test]
async fn test_simulation_composer_async_initialization() -> Result<()> {
    use aura_core::DeviceId;
    use aura_simulator::SimulationEffectComposer;

    let device_id = DeviceId::new();
    let seed = 42u64;

    // This should work within an async context (no nested runtime panic)
    let result = SimulationEffectComposer::for_simulation_async(device_id, seed).await;

    assert!(
        result.is_ok(),
        "Async simulation composer should initialize: {:?}",
        result.err()
    );

    let env = result.unwrap();

    // Verify environment is properly configured
    assert_eq!(env.device_id(), device_id);
    assert_eq!(env.seed(), seed);
    assert!(env.is_deterministic());

    // Verify handlers are available
    assert!(
        env.time_effects().is_some(),
        "Time effects should be available"
    );
    assert!(
        env.chaos_effects().is_some(),
        "Chaos effects should be available"
    );
    assert!(
        env.testing_effects().is_some(),
        "Testing effects should be available"
    );

    Ok(())
}

/// Test guardian agent creation within async context
#[aura_test]
async fn test_guardian_agent_creation_async() -> Result<()> {
    use aura_cli::demo::GuardianAgentFactory;

    // This should work within async context (the original bug)
    let result = GuardianAgentFactory::create_demo_guardians(42).await;

    assert!(
        result.is_ok(),
        "Guardian creation should succeed: {:?}",
        result.err()
    );

    let (alice, charlie) = result.unwrap();

    assert_eq!(alice.name(), "Alice");
    assert_eq!(charlie.name(), "Charlie");

    // Verify they have different authority IDs
    assert!(
        alice.authority_id() != charlie.authority_id(),
        "Guardians should have different authority IDs"
    );

    Ok(())
}

/// Test demo stats command (smoke test)
#[aura_test]
async fn test_demo_stats_command_runs() -> Result<()> {
    use aura_cli::commands::DemoCommands;
    use aura_cli::handlers::demo::DemoHandler;

    // Stats command should run without error even with no prior demos
    let result = DemoHandler::handle_demo_command(DemoCommands::Stats {
        detailed: false,
        export_to: None,
    })
    .await;

    assert!(
        result.is_ok(),
        "Stats command should succeed: {:?}",
        result.err()
    );

    Ok(())
}

/// Test deterministic demo execution produces consistent results
#[aura_test]
async fn test_demo_determinism() -> Result<()> {
    use aura_cli::demo::{DemoScenarioBridge, DemoSetupConfig};

    let setup_config = DemoSetupConfig {
        participant_count: 3,
        guardian_threshold: 2,
        setup_chat_history: false,
        initial_message_count: 0,
        verbose_logging: false,
        simulate_network_activity: false,
    };

    // Run setup twice with same seed
    let bridge1 = DemoScenarioBridge::new(42, setup_config.clone());
    let result1 = bridge1.setup_demo_environment().await?;

    let bridge2 = DemoScenarioBridge::new(42, setup_config);
    let result2 = bridge2.setup_demo_environment().await?;

    // Both runs should produce same number of scenarios executed
    assert_eq!(
        result1.setup_metrics.scenarios_executed, result2.setup_metrics.scenarios_executed,
        "Deterministic runs should execute same number of scenarios"
    );

    // Both should have same guardian registration count
    assert_eq!(
        result1.setup_metrics.guardian_registrations, result2.setup_metrics.guardian_registrations,
        "Deterministic runs should register same number of guardians"
    );

    Ok(())
}
