//! End-to-end integration test demonstrating simulator basic functionality
//!
//! This test validates the basic simulator effect system

use aura_core::DeviceId;
use aura_macros::aura_test;
use aura_simulator::{Duration, Result, SimulationEffectComposer, SimulatorContext};

#[aura_test]
async fn test_simulator_effect_composition_basic() -> Result<()> {
    // Create a basic simulation environment
    let device_id = DeviceId::new();
    let environment = SimulationEffectComposer::for_testing(device_id)
        .map_err(|e| aura_simulator::SimulatorError::OperationFailed(e.to_string()))?;

    // Test basic time effect
    let timestamp = environment
        .current_timestamp()
        .await
        .map_err(|e| aura_simulator::SimulatorError::TimeControlError(e.to_string()))?;

    assert!(timestamp >= 0);

    println!("[OK] Simulator effect composition test completed");

    Ok(())
}

#[aura_test]
async fn test_simulator_full_effect_composition() -> Result<()> {
    // Test all effect handlers together
    let device_id = DeviceId::new();
    let environment = SimulationEffectComposer::for_simulation(device_id, 123)
        .map_err(|e| aura_simulator::SimulatorError::OperationFailed(e.to_string()))?;

    // Test time effects
    let timestamp = environment
        .current_timestamp()
        .await
        .map_err(|e| aura_simulator::SimulatorError::TimeControlError(e.to_string()))?;
    assert!(timestamp >= 0);

    // Test fault injection
    environment
        .inject_network_delay((Duration::from_millis(10), Duration::from_millis(50)), None)
        .await
        .map_err(|e| aura_simulator::SimulatorError::FaultInjectionFailed(e.to_string()))?;

    // Test scenario management
    let mut event_data = std::collections::HashMap::new();
    event_data.insert("test_key".to_string(), "test_value".to_string());
    environment
        .record_test_event("integration_test", event_data)
        .await
        .map_err(|e| aura_simulator::SimulatorError::OperationFailed(e.to_string()))?;

    println!("[OK] Full effect composition test completed");
    Ok(())
}
