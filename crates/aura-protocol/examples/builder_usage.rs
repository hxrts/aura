//! Example demonstrating various ways to use the EffectRegistry

use aura_core::{AuraResult, DeviceId};
use aura_protocol::effects::EffectRegistry;

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Example 1: Simple test configuration
    println!("Example 1: Simple test configuration");
    let device_id = DeviceId::new();
    let system = EffectRegistry::testing()
        .with_device_id(device_id)
        .build()?;

    println!("Created test system for device: {:?}", device_id);

    // Example 2: Custom configuration with specific settings
    println!("\nExample 2: Custom configuration");
    let device_id = DeviceId::new();
    let system = EffectRegistry::testing()
        .with_device_id(device_id)
        .build()?;

    println!("Created system with custom configuration");

    // Example 3: Production configuration
    println!("\nExample 3: Production-like configuration");
    let device_id = DeviceId::new();
    let system = EffectRegistry::production()
        .with_device_id(device_id)
        .with_logging()
        .with_metrics()
        .build()?;

    println!("Created production system");

    // Example 4: Simulation mode with deterministic seed
    println!("\nExample 4: Simulation mode");
    let device_id = DeviceId::new();
    let seed = 12345u64;
    let system = EffectRegistry::simulation(seed)
        .with_device_id(device_id)
        .with_logging()
        .build()?;

    println!("Created simulation system with seed: {}", seed);

    // Example 5: Standard configurations
    println!("\nExample 5: Standard configurations");
    let device_id = DeviceId::new();
    let system = EffectRegistry::testing()
        .with_device_id(device_id)
        .build()?;

    println!("Created system using standard testing configuration");

    // Example 6: Multiple environments
    println!("\nExample 6: Multiple environments");
    let device_id = DeviceId::new();

    let _test_system = EffectRegistry::testing()
        .with_device_id(device_id)
        .build()?;
    let _prod_system = EffectRegistry::production()
        .with_device_id(device_id)
        .build()?;
    let _sim_system = EffectRegistry::simulation(42)
        .with_device_id(device_id)
        .build()?;

    println!("Created systems for different environments");

    println!("\nAll examples completed successfully!");
    Ok(())
}
