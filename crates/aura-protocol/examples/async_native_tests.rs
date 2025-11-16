//! Example demonstrating async-native testing with the new infrastructure
//!
//! This example shows how to use:
//! - The `#[aura_test]` macro for automatic setup/teardown
//! - Time control utilities for deterministic testing
//! - Network simulation for distributed protocol testing
//! - Effect capture and snapshot testing
//! - Test fixtures and harness utilities

// Note: In real tests, you'd import from aura_macros
// use aura_macros::aura_test;

use aura_core::{AuraResult, DeviceId};
use aura_agent::runtime::EffectSystemBuilder;
use aura_protocol::ExecutionMode;
use aura_testkit::{
    network_sim::{NetworkCondition, NetworkSimulator, NetworkTopology},
    test_harness::{snapshot::EffectSnapshot, TestConfig, TestFixture},
    time::{self, TimeGuard},
};
use std::time::Duration;
use tokio::time::sleep;

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Initialize tracing for examples
    tracing_subscriber::fmt()
        .with_target(false)
        .with_timestamp(true)
        .init();

    println!("=== Async-Native Testing Examples ===\n");

    // Run various example tests
    example_basic_test().await?;
    example_time_control().await?;
    example_network_simulation().await?;
    example_test_fixture().await?;
    example_snapshot_testing().await?;

    println!("\n=== All examples completed successfully ===");
    Ok(())
}

// Example 1: Basic test with automatic setup
// In real code: #[aura_test]
async fn example_basic_test() -> AuraResult<()> {
    println!("\n--- Example 1: Basic Test with Automatic Setup ---");

    // With #[aura_test], this setup would be automatic
    let device_id = DeviceId::new();
    let effects = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .build()
        .await?;

    effects.initialize_lifecycle().await?;

    println!("Effect system initialized automatically");
    println!("  Device ID: {:?}", effects.device_id());
    println!("  State: {:?}", effects.lifecycle_state());

    // Your test logic here
    // ...

    // Cleanup would be automatic with #[aura_test]
    effects.shutdown_lifecycle().await?;

    Ok(())
}

// Example 2: Time control for deterministic testing
// In real code: #[aura_test(deterministic_time)]
async fn example_time_control() -> AuraResult<()> {
    println!("\n--- Example 2: Time Control ---");

    // Freeze time at epoch
    let _time_guard = TimeGuard::freeze_at_epoch();

    println!("Time frozen at: {:?}", time::current_test_time());

    // Simulate passage of time
    for i in 1..=3 {
        time::advance_time_by(Duration::from_secs(60));
        println!(
            "Advanced by 1 minute, now at: {:?}",
            time::current_test_time()
        );

        // Your time-dependent logic here
        sleep(Duration::from_millis(100)).await; // This completes instantly in frozen time
    }

    // Time automatically resets when guard is dropped
    drop(_time_guard);

    Ok(())
}

// Example 3: Network simulation for distributed testing
async fn example_network_simulation() -> AuraResult<()> {
    println!("\n--- Example 3: Network Simulation ---");

    // Create devices
    let devices: Vec<DeviceId> = (0..4).map(|_| DeviceId::new()).collect();
    println!("Created {} devices", devices.len());

    // Create network topology
    let simulator = NetworkTopology::new(devices.clone())
        .star(devices[0]) // Device 0 is the center
        .await;

    // Simulate message sending
    println!("\nSimulating message sends:");

    // Good connection (center to edge)
    match simulator.simulate_send(devices[0], devices[1], 1024).await {
        Ok(()) => println!("  ✓ Message from center to edge delivered"),
        Err(e) => println!("  ✗ Message from center to edge failed: {}", e),
    }

    // Poor connection (edge to edge)
    match simulator.simulate_send(devices[1], devices[2], 1024).await {
        Ok(()) => println!("  ✓ Message from edge to edge delivered"),
        Err(e) => println!("  ✗ Message from edge to edge failed: {}", e),
    }

    // Create partition
    println!("\nCreating network partition...");
    simulator
        .partition(vec![devices[0], devices[1]], vec![devices[2], devices[3]])
        .await;

    // Try to send across partition
    match simulator.simulate_send(devices[0], devices[2], 1024).await {
        Ok(()) => println!("  ✗ Message crossed partition (unexpected)"),
        Err(e) => println!("  ✓ Message blocked by partition: {}", e),
    }

    // Heal partition
    simulator.heal_partition().await;
    println!("Partition healed");

    Ok(())
}

// Example 4: Test fixtures for common scenarios
async fn example_test_fixture() -> AuraResult<()> {
    println!("\n--- Example 4: Test Fixtures ---");

    // Create test fixture with custom config
    let config = TestConfig {
        name: "example_test".to_string(),
        deterministic_time: true,
        capture_effects: false,
        timeout: Some(Duration::from_secs(5)),
    };

    let fixture = TestFixture::with_config(config).await?;
    println!("Created test fixture");

    // Run test with automatic cleanup
    let result = fixture
        .run_test(|effects| async move {
            println!("  Running test with effect system");
            println!("  Device ID: {:?}", effects.device_id());

            // Your test logic here
            sleep(Duration::from_millis(100)).await;

            println!("  Test completed");
            Ok(42)
        })
        .await?;

    println!("Test result: {}", result);

    // Fixture automatically cleans up when dropped

    Ok(())
}

// Example 5: Snapshot testing for effect assertions
// In real code: #[aura_test(capture)]
async fn example_snapshot_testing() -> AuraResult<()> {
    println!("\n--- Example 5: Snapshot Testing ---");

    // Create a snapshot to capture effects
    let mut snapshot = EffectSnapshot::new();

    // Simulate some effect calls
    use aura_testkit::test_harness::snapshot::EffectCall;

    let calls = vec![
        EffectCall {
            effect_type: "Network".to_string(),
            operation: "send_to_peer".to_string(),
            params: vec![1, 2, 3],
            timestamp: Duration::from_millis(100),
        },
        EffectCall {
            effect_type: "Storage".to_string(),
            operation: "store".to_string(),
            params: vec![4, 5, 6],
            timestamp: Duration::from_millis(200),
        },
    ];

    for call in calls {
        snapshot.record(call);
    }

    println!("Captured {} effect calls", snapshot.calls.len());

    // Create expected snapshot
    let mut expected = EffectSnapshot::new();
    expected.record(EffectCall {
        effect_type: "Network".to_string(),
        operation: "send_to_peer".to_string(),
        params: vec![],
        timestamp: Duration::ZERO,
    });
    expected.record(EffectCall {
        effect_type: "Storage".to_string(),
        operation: "store".to_string(),
        params: vec![],
        timestamp: Duration::ZERO,
    });

    // Assert snapshot matches (ignoring params and timestamps for this example)
    match snapshot.assert_matches(&expected) {
        Ok(()) => println!("✓ Snapshot matches expected"),
        Err(e) => println!("✗ Snapshot mismatch: {}", e),
    }

    Ok(())
}

// Example of how tests would look with the macro
mod macro_examples {
    use super::*;

    // This would use: #[aura_test]
    async fn test_basic() -> AuraResult<()> {
        // _aura_test_effects is automatically available
        // Device ID is _aura_test_device_id
        // Everything is initialized

        Ok(())
    }

    // This would use: #[aura_test(timeout = 10)]
    async fn test_with_timeout() -> AuraResult<()> {
        // Test will timeout after 10 seconds
        Ok(())
    }

    // This would use: #[aura_test(capture)]
    async fn test_with_capture() -> AuraResult<()> {
        // _aura_test_capture is available for assertions
        Ok(())
    }

    // This would use: #[aura_test(no_init)]
    async fn test_manual_init() -> AuraResult<()> {
        // Must manually initialize effect system
        let effects = AuraEffectSystemBuilder::new()
            .with_device_id(DeviceId::new())
            .build_sync()?;
        Ok(())
    }
}
