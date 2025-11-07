//! End-to-end integration test demonstrating simulator basic functionality
//!
//! This test validates the basic simulator middleware stack

use aura_simulator::{
    CoreSimulatorHandler, Duration, Result, ScenarioInjectionMiddleware, SimulatorContext,
    SimulatorOperation, SimulatorStackBuilder,
};
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn test_simulator_stack_basic() -> Result<()> {
    // Create a basic simulator stack
    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(ScenarioInjectionMiddleware::new()))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build()?;

    // Create simulation context
    let context = SimulatorContext::new("test_scenario".to_string(), "run_1".to_string())
        .with_participants(3, 2)
        .with_seed(42);

    // Execute a simple tick operation
    let result = stack.process(
        SimulatorOperation::ExecuteTick {
            tick_number: 1,
            delta_time: Duration::from_millis(100),
        },
        &context,
    )?;

    // Verify basic operation succeeded
    println!("[OK] Simulator stack test completed");

    Ok(())
}

#[tokio::test(flavor = "multi_thread")]
async fn test_simulator_middleware_composition() -> Result<()> {
    // Test multiple middleware layers
    use aura_simulator::{FaultSimulationMiddleware, TimeControlMiddleware};

    let stack = SimulatorStackBuilder::new()
        .with_middleware(Arc::new(ScenarioInjectionMiddleware::new()))
        .with_middleware(Arc::new(FaultSimulationMiddleware::new()))
        .with_middleware(Arc::new(TimeControlMiddleware::new()))
        .with_handler(Arc::new(CoreSimulatorHandler::new()))
        .build()?;

    let context = SimulatorContext::new("middleware_test".to_string(), "run_1".to_string())
        .with_participants(2, 2)
        .with_seed(123);

    // Test tick execution with multiple middleware
    let result = stack.process(
        SimulatorOperation::ExecuteTick {
            tick_number: 1,
            delta_time: Duration::from_millis(50),
        },
        &context,
    )?;

    println!("[OK] Middleware composition test completed");
    Ok(())
}
