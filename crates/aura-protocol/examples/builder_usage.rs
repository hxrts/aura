//! Example demonstrating various ways to use the AuraEffectSystemBuilder

use aura_core::{session_epochs::Epoch, AuraResult, DeviceId};
use aura_protocol::effects::{AuraEffectSystemBuilder, StorageConfig};
use aura_protocol::ExecutionMode;

#[tokio::main]
async fn main() -> AuraResult<()> {
    // Example 1: Simple test configuration
    println!("Example 1: Simple test configuration");
    let device_id = DeviceId::new();
    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .build_sync()?;

    println!("Created test system for device: {:?}", system.device_id());

    // Example 2: Custom configuration with specific settings
    println!("\nExample 2: Custom configuration");
    let device_id = DeviceId::new();
    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .with_default_flow_limit(5000)
        .with_initial_epoch(Epoch::from(10))
        .build_sync()?;

    println!("Created system with custom flow limit and epoch");

    // Example 3: Production configuration
    println!("\nExample 3: Production-like configuration");
    let device_id = DeviceId::new();
    let storage_config = StorageConfig {
        base_path: std::env::temp_dir().join("aura_example"),
        master_key: [0u8; 32], // In production, use secure key management
        enable_compression: true,
        max_file_size: 50 * 1024 * 1024, // 50MB
    };

    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Production)
        .with_storage_config(storage_config)
        .build_sync()?;

    println!("Created production-like system");

    // Example 4: Simulation mode with deterministic seed
    println!("\nExample 4: Simulation mode");
    let device_id = DeviceId::new();
    let seed = 12345u64;
    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Simulation { seed })
        .build_sync()?;

    println!("Created simulation system with seed: {}", seed);

    // Example 5: Async initialization
    println!("\nExample 5: Async initialization");
    let device_id = DeviceId::new();
    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_execution_mode(ExecutionMode::Testing)
        .build()
        .await?;

    println!("Created system using async initialization");

    // Example 6: Using with custom handlers (commented out as it requires handler implementation)
    /*
    use aura_effects::crypto::MockCryptoHandler;
    use aura_protocol::effects::{EffectType, handler_adapters::CryptoHandlerAdapter};

    let custom_crypto = MockCryptoHandler::new(99999);
    let system = AuraEffectSystemBuilder::new()
        .with_device_id(device_id)
        .with_handler(
            EffectType::Crypto,
            Box::new(CryptoHandlerAdapter::new(custom_crypto, ExecutionMode::Testing)),
        )
        .build_sync()?;
    */

    println!("\nAll examples completed successfully!");
    Ok(())
}
