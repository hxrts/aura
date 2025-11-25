//! Init Command Handler
//!
//! Effect-based implementation of the init command.

use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::time::TimeStamp;
use aura_protocol::effect_traits::StorageEffects;
use std::path::Path;

/// Handle initialization through effects
pub async fn handle_init(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    num_devices: u32,
    threshold: u32,
    output: &Path,
) -> Result<()> {
    // Log initialization start
    println!(
        "Initializing {}-of-{} threshold account",
        threshold, num_devices
    );

    println!("Output directory: {}", output.display());

    // Validate parameters through effects
    if threshold > num_devices {
        eprintln!("Threshold cannot be greater than number of devices");
        return Err(anyhow::anyhow!(
            "Invalid parameters: threshold ({}) > num_devices ({})",
            threshold,
            num_devices
        ));
    }

    if threshold == 0 {
        eprintln!("Threshold must be greater than 0");
        return Err(anyhow::anyhow!("Invalid threshold: 0"));
    }

    // Create directory structure through storage effects
    let configs_dir = output.join("configs");
    create_directory_through_effects(ctx, effects, output).await?;
    create_directory_through_effects(ctx, effects, &configs_dir).await?;

    // Create placeholder effect API through storage effects
    let effect_api_path = output.join("effect_api.cbor");
    let effect_api_data =
        create_placeholder_effect_api(ctx, effects, threshold, num_devices).await?;

    effects
        .store(&effect_api_path.display().to_string(), effect_api_data)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create effect_api: {}", e))?;

    // Create device config files through storage effects
    for i in 1..=num_devices {
        let config_content = create_device_config(i, threshold, num_devices);
        let config_path = configs_dir.join(format!("device_{}.toml", i));

        // Create device config via StorageEffects
        let config_key = format!("device_config:{}", config_path.display());
        effects
            .store(&config_key, config_content.into_bytes())
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create device config {} via storage effects: {}",
                    i,
                    e
                )
            })?;

        println!("Created device_{}.toml", i);
    }

    // Success message
    println!("Account initialized successfully!");

    Ok(())
}

/// Create directory marker through storage effects
async fn create_directory_through_effects(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    path: &Path,
) -> Result<()> {
    // Use unified time system to get physical time
    let physical_time = effects
        .physical_time()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get physical time: {}", e))?;

    let timestamp = TimeStamp::PhysicalClock(physical_time);

    // Create directory marker via StorageEffects with proper TimeStamp serialization
    let dir_marker_key = format!("directory_marker:{}", path.display());
    let timestamp_bytes = serde_json::to_vec(&timestamp)
        .map_err(|e| anyhow::anyhow!("Failed to serialize timestamp: {}", e))?;

    effects
        .store(&dir_marker_key, timestamp_bytes)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to create directory marker via storage effects: {}",
                e
            )
        })?;

    Ok(())
}

/// Create placeholder effect API data
async fn create_placeholder_effect_api(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    threshold: u32,
    num_devices: u32,
) -> Result<Vec<u8>> {
    // Use unified time system to get physical time
    let physical_time = effects
        .physical_time()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get physical time: {}", e))?;
    let timestamp = physical_time.ts_ms / 1000; // Convert to seconds

    // Create a simple CBOR-like structure
    let effect_api_data = format!(
        "placeholder_effect_api:threshold={},devices={},created={}",
        threshold, num_devices, timestamp
    );

    println!("Created placeholder effect API");

    Ok(effect_api_data.into_bytes())
}

/// Create device configuration content
fn create_device_config(device_num: u32, threshold: u32, total_devices: u32) -> String {
    format!(
        r#"# Device {} configuration
device_id = "device_{}"
threshold = {}
total_devices = {}

[logging]
level = "info"
structured = false

[network]
default_port = {}
timeout = 30
max_retries = 3
"#,
        device_num,
        device_num,
        threshold,
        total_devices,
        58835 + device_num - 1 // Different port for each device
    )
}
