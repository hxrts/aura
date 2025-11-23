//! Status Command Handler
//!
//! Effect-based implementation of the status command.

use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use std::path::Path;

/// Handle status display through effects
pub async fn handle_status(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    config_path: &Path,
) -> Result<()> {
    println!("Account status for config: {}", config_path.display());

    // Check if config exists through storage effects
    let config_exists = std::path::Path::new(&config_path.display().to_string()).exists();

    if !config_exists {
        eprintln!("Config file not found: {}", config_path.display());
        return Err(anyhow::anyhow!(
            "Config file not found: {}",
            config_path.display()
        ));
    }

    // Read and parse config through storage effects
    match read_config_through_effects(ctx, effects, config_path).await {
        Ok(config) => {
            display_status_info(ctx, effects, &config).await;
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to read config: {}", e);

            // Show basic status anyway
            display_default_status(ctx, effects).await;
            Ok(())
        }
    }
}

/// Read configuration through storage effects
async fn read_config_through_effects(
    _ctx: &EffectContext,
    _effects: &AuraEffectSystem,
    config_path: &Path,
) -> Result<DeviceConfig> {
    let config_str = std::fs::read_to_string(&config_path.display().to_string())
        .map_err(|e| anyhow::anyhow!("Failed to read config: {}", e))?;

    // Parse TOML configuration
    let config: DeviceConfig = toml::from_str(&config_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;

    println!("Configuration loaded successfully");

    Ok(config)
}

/// Display status information through console effects
async fn display_status_info(
    _ctx: &EffectContext,
    _effects: &AuraEffectSystem,
    config: &DeviceConfig,
) {
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
async fn display_default_status(_ctx: &EffectContext, _effects: &AuraEffectSystem) {
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
    _logging: Option<LoggingConfig>,
    network: Option<NetworkConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct LoggingConfig {
    _level: String,
    _structured: bool,
}

#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    default_port: u16,
    _timeout: u64,
    _max_retries: u32,
}
