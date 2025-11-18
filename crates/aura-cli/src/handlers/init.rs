//! Init Command Handler
//!
//! Effect-based implementation of the init command.

use anyhow::Result;
use aura_protocol::effect_traits::{ConsoleEffects, StorageEffects, TimeEffects};
use aura_protocol::AuraEffectSystem;
use std::path::Path;

/// Handle initialization through effects
pub async fn handle_init(
    effects: &AuraEffectSystem,
    num_devices: u32,
    threshold: u32,
    output: &Path,
) -> Result<()> {
    // Log initialization start
    let _ = effects
        .log_info(&format!(
            "Initializing {}-of-{} threshold account",
            threshold, num_devices
        ))
        .await;

    let _ = effects
        .log_info(&format!("Output directory: {}", output.display()))
        .await;

    // Validate parameters through effects
    if threshold > num_devices {
        let _ = effects
            .log_error("Threshold cannot be greater than number of devices")
            .await;
        return Err(anyhow::anyhow!(
            "Invalid parameters: threshold ({}) > num_devices ({})",
            threshold,
            num_devices
        ));
    }

    if threshold == 0 {
        let _ = effects.log_error("Threshold must be greater than 0").await;
        return Err(anyhow::anyhow!("Invalid threshold: 0"));
    }

    // Create directory structure through storage effects
    let configs_dir = output.join("configs");
    create_directory_through_effects(effects, output).await?;
    create_directory_through_effects(effects, &configs_dir).await?;

    // Create placeholder ledger through storage effects
    let ledger_path = output.join("ledger.cbor");
    let ledger_data = create_placeholder_ledger(effects, threshold, num_devices).await?;

    effects
        .store(&ledger_path.display().to_string(), ledger_data)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create ledger: {}", e))?;

    // Create device config files through storage effects
    for i in 1..=num_devices {
        let config_content = create_device_config(i, threshold, num_devices);
        let config_path = configs_dir.join(format!("device_{}.toml", i));

        effects
            .store(
                &config_path.display().to_string(),
                config_content.as_bytes().to_vec(),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create device config {}: {}", i, e))?;

        let _ = effects
            .log_info(&format!("Created device_{}.toml", i))
            .await;
    }

    // Success message
    let _ = effects.log_info("Account initialized successfully!").await;

    Ok(())
}

/// Create directory marker through storage effects
async fn create_directory_through_effects(effects: &AuraEffectSystem, path: &Path) -> Result<()> {
    let dir_marker_path = path.join(".aura_directory");
    let timestamp = effects.current_timestamp().await;

    effects
        .store(
            &dir_marker_path.display().to_string(),
            timestamp.to_le_bytes().to_vec(),
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {}", path.display(), e))
}

/// Create placeholder ledger data
async fn create_placeholder_ledger(
    effects: &AuraEffectSystem,
    threshold: u32,
    num_devices: u32,
) -> Result<Vec<u8>> {
    let timestamp = effects.current_timestamp().await;

    // Create a simple CBOR-like structure
    let ledger_data = format!(
        "placeholder_ledger:threshold={},devices={},created={}",
        threshold, num_devices, timestamp
    );

    let _ = effects.log_info("Created placeholder ledger").await;

    Ok(ledger_data.into_bytes())
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
