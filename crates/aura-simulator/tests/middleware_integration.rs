//! Integration tests for simulator middleware composition
//!
//! These tests verify that simulator middleware can be composed together
//! and work harmoniously.

use std::sync::Arc;
use std::time::Duration;

use aura_simulator::{
    SimulatorStackBuilder, 
    ScenarioInjectionMiddleware,
    FaultSimulationMiddleware,
    TimeControlMiddleware,
    StateInspectionMiddleware,
    PropertyCheckingMiddleware,
    ChaosCoordinationMiddleware,
    CoreSimulatorHandler,
    SimulatorContext,
    SimulatorOperation,
};

#[test]
fn test_full_middleware_stack_composition() {
    // Test that all simulator middleware can be composed together
    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(ScenarioInjectionMiddleware::new()))
        .with_middleware(Arc::new(FaultSimulationMiddleware::new()))
        .with_middleware(Arc::new(TimeControlMiddleware::new()))
        .with_middleware(Arc::new(StateInspectionMiddleware::new()))
        .with_middleware(Arc::new(PropertyCheckingMiddleware::new()))
        .with_middleware(Arc::new(ChaosCoordinationMiddleware::new()))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build();

    assert!(stack.is_ok(), "Full middleware stack should build successfully");
    
    let stack = stack.unwrap();
    assert_eq!(stack.layer_count(), 6, "Should have 6 middleware layers");
    
    let expected_names = vec![
        "scenario_injection",
        "fault_simulation", 
        "time_control",
        "state_inspection",
        "property_checking",
        "chaos_coordination"
    ];
    
    let actual_names = stack.middleware_names();
    assert_eq!(actual_names, expected_names, "Middleware names should match expected order");
}

#[test]
fn test_middleware_operation_execution() {
    // Test executing operations through the simulator middleware stack
    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(TimeControlMiddleware::new()))
        .with_middleware(Arc::new(StateInspectionMiddleware::new()))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build()
        .unwrap();

    let context = SimulatorContext::new("test_scenario".to_string(), "run_1".to_string())
        .with_participants(5, 3)
        .with_seed(42);

    let result = stack.process(
        SimulatorOperation::InitializeScenario { 
            scenario_id: "test_scenario".to_string() 
        },
        &context,
    );

    assert!(result.is_ok(), "Scenario initialization should succeed");
    
    let value = result.unwrap();
    assert_eq!(value["scenario_id"], "test_scenario");
    assert_eq!(value["status"], "initialized");
    
    // Verify middleware added their information
    assert!(value.get("time_status").is_some(), "Time control middleware should add status");
    assert!(value.get("state_inspection").is_some(), "State inspection middleware should add info");
}

#[test]
fn test_middleware_configuration() {
    // Test that middleware can be configured properly
    let scenario_middleware = ScenarioInjectionMiddleware::new()
        .with_randomization(true, 0.5)
        .with_max_concurrent(3);

    let fault_middleware = FaultSimulationMiddleware::new()
        .with_auto_injection(true, 0.1)
        .with_max_concurrent_faults(5);

    let time_middleware = TimeControlMiddleware::new()
        .with_acceleration_bounds(0.1, 10.0)
        .with_precise_timing(true);

    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(scenario_middleware))
        .with_middleware(Arc::new(fault_middleware))
        .with_middleware(Arc::new(time_middleware))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build();

    assert!(stack.is_ok(), "Configured middleware stack should build successfully");
}

#[test]
fn test_middleware_context_enhancement() {
    // Test that middleware properly enhances context with metadata
    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(FaultSimulationMiddleware::new()))
        .with_middleware(Arc::new(ChaosCoordinationMiddleware::new()))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build()
        .unwrap();

    let context = SimulatorContext::new("test".to_string(), "run1".to_string())
        .with_participants(3, 2);

    let result = stack.process(
        SimulatorOperation::ExecuteTick {
            tick_number: 1,
            delta_time: Duration::from_millis(100),
        },
        &context,
    );

    assert!(result.is_ok(), "Tick execution should succeed");
    
    let value = result.unwrap();
    
    // Verify that middleware layers added their information
    assert!(value.get("fault_simulation").is_some(), "Fault simulation should add info");
    assert!(value.get("chaos_coordination").is_some(), "Chaos coordination should add info");
    
    // Check that the operation was executed
    assert_eq!(value["tick"], 1);
    assert_eq!(value["delta_time_ms"], 100);
}