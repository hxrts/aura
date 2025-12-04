//! Status Command Handler
//!
//! Effect-based implementation of the status command.

use crate::handlers::HandlerContext;
use anyhow::Result;
use aura_core::effects::StorageEffects;
use std::path::Path;

/// Handle status display through effects
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_status(ctx: &HandlerContext<'_>, config_path: &Path) -> Result<()> {
    let effects = ctx.effects();

    println!("Account status for config: {}", config_path.display());

    let config_key = config_path.display().to_string();

    // Check if config exists through storage effects
    let config_exists = effects.exists(&config_key).await.unwrap_or(false);

    if !config_exists {
        eprintln!("Config file not found: {}", config_path.display());
        return Err(anyhow::anyhow!(
            "Config file not found: {}",
            config_path.display()
        ));
    }

    // Read and parse config through storage effects
    match read_config_through_effects(ctx, &config_key).await {
        Ok(config) => {
            display_status_info(ctx, &config).await;
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to read config: {}", e);

            // Show basic status anyway
            display_default_status(ctx).await;
            Ok(())
        }
    }
}

/// Read configuration through storage effects
async fn read_config_through_effects(
    ctx: &HandlerContext<'_>,
    config_key: &str,
) -> Result<DeviceConfig> {
    let effects = ctx.effects();

    let config_bytes = effects
        .retrieve(config_key)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("Config {} not found in storage", config_key))?;

    let config_str = String::from_utf8(config_bytes)
        .map_err(|e| anyhow::anyhow!("Config is not valid UTF-8: {}", e))?;

    // Parse TOML configuration
    let config: DeviceConfig = toml::from_str(&config_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    println!("Configuration loaded successfully");

    Ok(config)
}

/// Display status information through console effects
async fn display_status_info(_ctx: &HandlerContext<'_>, config: &DeviceConfig) {
    println!("=== Account Status ===");
    println!("Device ID: {}", config.device_id);
    println!("Status: Active");
    println!("Total Devices: {}", config.total_devices);
    println!("Threshold: {}", config.threshold);

    if let Some(network) = &config.network {
        println!("Default Port: {}", network.default_port);
    }

    println!("=== End Status ===");
}

/// Display default status when config can't be read
async fn display_default_status(_ctx: &HandlerContext<'_>) {
    println!("=== Account Status (Default) ===");
    println!("Status: Unknown (config unreadable)");
    println!("Devices: Unknown");
    println!("Threshold: Unknown");
    println!("=== End Status ===");
}

/// Device configuration structure for parsing
#[derive(Debug, serde::Deserialize)]
struct DeviceConfig {
    device_id: String,
    threshold: u32,
    total_devices: u32,
    network: Option<NetworkConfig>,
}

/// Network configuration parsed from config file
#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    default_port: u16,
    /// Parsed but not yet used - reserved for future network timeout configuration
    #[allow(dead_code)]
    timeout: u64,
    /// Parsed but not yet used - reserved for future retry configuration
    #[allow(dead_code)]
    max_retries: u32,
}
