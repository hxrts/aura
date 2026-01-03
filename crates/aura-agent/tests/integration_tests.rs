//! Agent Runtime Integration Tests
//!
//! Tests that validate the agent runtime system with the new architectural improvements.
//! These tests focus on:
//! - Effect system composition and delegation
//! - Runtime builder patterns
//! - Authority-based identity model
//! - Guard chain enforcement (when implemented)
#![allow(clippy::expect_used)] // Test code uses expect for clarity
#![allow(clippy::uninlined_format_args)] // Test code uses explicit format args for clarity

use aura_composition::{CompositeHandler, Handler};
use aura_core::{
    hash::hash,
    identifiers::{AuthorityId, DeviceId},
    AuraResult, ExecutionMode,
};
use aura_macros::aura_test;
use aura_testkit::DeviceTestFixture;
use std::time::Duration;
use tokio::time::timeout;

/// Test basic agent runtime composition
#[aura_test]
async fn test_agent_runtime_composition() -> AuraResult<()> {
    let fixture = DeviceTestFixture::new(0);
    let device_id = fixture.device_id();
    let authority_id = AuthorityId::from_uuid(device_id.0);

    // Create a composite handler using the testing factory
    let handler = CompositeHandler::for_testing(device_id);

    // Test that handler creation works
    assert_eq!(handler.device_id(), device_id);
    assert_eq!(handler.execution_mode(), ExecutionMode::Testing);

    println!(
        "Agent runtime composition test passed for authority: {:?}",
        authority_id
    );
    Ok(())
}

/// Test effect system delegation works correctly
#[aura_test]
async fn test_effect_system_delegation() -> AuraResult<()> {
    let fixture = DeviceTestFixture::new(1);
    let handler = CompositeHandler::for_testing(fixture.device_id());

    // Test that different execution modes produce different handlers
    let production_handler = CompositeHandler::for_production(fixture.device_id());
    let simulation_handler = CompositeHandler::for_simulation(fixture.device_id(), 42);

    assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
    assert_eq!(
        production_handler.execution_mode(),
        ExecutionMode::Production
    );
    assert_eq!(
        simulation_handler.execution_mode(),
        ExecutionMode::Simulation { seed: 42 }
    );

    println!("Effect system delegation test passed");
    Ok(())
}

/// Test agent runtime with multiple authorities (authority model validation)
#[aura_test]
async fn test_multiple_authority_runtime() -> AuraResult<()> {
    let fixture1 = DeviceTestFixture::new(100);
    let fixture2 = DeviceTestFixture::new(101);

    let authority1 = AuthorityId::from_uuid(fixture1.device_id().0);
    let authority2 = AuthorityId::from_uuid(fixture2.device_id().0);

    let handler1 = CompositeHandler::for_testing(fixture1.device_id());
    let handler2 = CompositeHandler::for_testing(fixture2.device_id());

    // Each authority should have independent device IDs and execution contexts
    assert_eq!(handler1.device_id(), fixture1.device_id());
    assert_eq!(handler2.device_id(), fixture2.device_id());
    assert_ne!(handler1.device_id(), handler2.device_id());

    // Authorities should be distinct
    assert_ne!(authority1, authority2);

    println!(
        "Multiple authority runtime test passed: {:?} != {:?}",
        authority1, authority2
    );
    Ok(())
}

/// Test runtime determinism in testing mode
#[aura_test]
async fn test_runtime_determinism() -> AuraResult<()> {
    let device_id = DeviceId::from_bytes([42u8; 32]);

    // Create two handlers with same device ID
    let handler1 = CompositeHandler::for_testing(device_id);
    let handler2 = CompositeHandler::for_testing(device_id);

    // Both handlers should have same device ID and execution mode
    assert_eq!(handler1.device_id(), device_id);
    assert_eq!(handler2.device_id(), device_id);
    assert_eq!(handler1.execution_mode(), handler2.execution_mode());

    // Test that the core hash function is deterministic
    let test_data = b"determinism_test";
    let hash1 = hash(test_data);
    let hash2 = hash(test_data);
    assert_eq!(hash1, hash2);
    assert_eq!(hash1.len(), 32);

    println!("Runtime determinism test passed: hash consistent");
    Ok(())
}

/// Test runtime performance and timeout behavior
#[aura_test]
async fn test_runtime_performance() -> AuraResult<()> {
    let fixture = DeviceTestFixture::new(2);
    let handler = CompositeHandler::for_testing(fixture.device_id());

    // Test that handler operations complete within reasonable time (no deadlocks)
    let result = timeout(Duration::from_secs(5), async {
        for i in 0..1000 {
            let test_data = format!("performance_test_data_{}", i);
            let _hash_result = hash(test_data.as_bytes());

            // Test handler introspection methods
            let _device_id = handler.device_id();
            let _exec_mode = handler.execution_mode();
        }
        Ok::<(), aura_core::AuraError>(())
    })
    .await;

    assert!(result.is_ok(), "Operations should complete without timeout");
    assert!(result.unwrap().is_ok(), "Operations should succeed");

    println!("Runtime performance test passed");
    Ok(())
}

/// Test authority model and identity consistency
#[aura_test]
async fn test_authority_identity_model() -> AuraResult<()> {
    let fixture1 = DeviceTestFixture::new(200);
    let fixture2 = DeviceTestFixture::new(201);

    let device_id1 = fixture1.device_id();
    let device_id2 = fixture2.device_id();

    // Create authorities from device IDs
    let authority1 = AuthorityId::from_uuid(device_id1.0);
    let authority2 = AuthorityId::from_uuid(device_id2.0);

    // Test authority properties
    assert_ne!(authority1.uuid(), authority2.uuid());
    assert_eq!(authority1.uuid(), device_id1.0);
    assert_eq!(authority2.uuid(), device_id2.0);

    // Test that authorities can be converted to bytes consistently
    let bytes1 = authority1.to_bytes();
    let bytes2 = authority2.to_bytes();
    assert_ne!(bytes1, bytes2);
    assert_eq!(bytes1.len(), 16);
    assert_eq!(bytes2.len(), 16);

    // Test that the same device always produces the same authority
    let authority1_again = AuthorityId::from_uuid(device_id1.0);
    assert_eq!(authority1, authority1_again);

    println!("Authority identity model test passed");
    Ok(())
}
