//! Example demonstrating various ways to use the EffectRegistry

use aura_core::{effects::ExecutionMode, AuraResult, DeviceId};
use aura_protocol::handlers::EffectRegistry;

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Example 1: Simple test configuration
    println!("Example 1: Simple test configuration");
    let _device_id = DeviceId::new();
    let _system = EffectRegistry::new(ExecutionMode::Testing);

    println!("Created test system");

    // Example 2: Custom configuration with specific settings
    println!("\nExample 2: Custom configuration");
    let _device_id = DeviceId::new();
    let _system = EffectRegistry::new(ExecutionMode::Testing);

    println!("Created system with custom configuration");

    // Example 3: Production configuration
    println!("\nExample 3: Production-like configuration");
    let _device_id = DeviceId::new();
    let _system = EffectRegistry::new(ExecutionMode::Production);

    println!("Created production system");

    // Example 4: Simulation mode with deterministic seed
    println!("\nExample 4: Simulation mode");
    let _device_id = DeviceId::new();
    let seed = 12345u64;
    let _system = EffectRegistry::new(ExecutionMode::Simulation { seed });

    println!("Created simulation system with seed: {}", seed);

    // Example 5: Standard configurations
    println!("\nExample 5: Standard configurations");
    let _device_id = DeviceId::new();
    let _system = EffectRegistry::new(ExecutionMode::Testing);

    println!("Created system using standard testing configuration");

    // Example 6: Multiple environments
    println!("\nExample 6: Multiple environments");
    let _device_id = DeviceId::new();

    let _test_system = EffectRegistry::new(ExecutionMode::Testing);
    let _prod_system = EffectRegistry::new(ExecutionMode::Production);
    let _sim_system = EffectRegistry::new(ExecutionMode::Simulation { seed: 42 });

    println!("Created systems for different environments");

    println!("\nAll examples completed successfully!");
    Ok(())
}
