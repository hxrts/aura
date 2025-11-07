//! Status Command Handler
//!
//! Effect-based implementation of the status command.

use anyhow::Result;
use aura_protocol::{AuraEffectSystem, ConsoleEffects, StorageEffects};
use std::path::PathBuf;

/// Handle status display through effects
pub async fn handle_status(
    effects: &AuraEffectSystem,
    config_path: &PathBuf
) -> Result<()> {
    effects.log_info(&format!(
        "Account status for config: {}", 
        config_path.display()
    ), &[]);
    
    // Check if config exists through storage effects
    let config_exists = effects.exists(&config_path.display().to_string()).await.unwrap_or(false);
    
    if !config_exists {
        effects.log_error(&format!(
            "Config file not found: {}", 
            config_path.display()
        ), &[]);
        return Err(anyhow::anyhow!("Config file not found: {}", config_path.display()));
    }
    
    // Read and parse config through storage effects
    match read_config_through_effects(effects, config_path).await {
        Ok(config) => {
            display_status_info(effects, &config).await;
            Ok(())
        }
        Err(e) => {
            effects.log_error(&format!(
                "Failed to read config: {}", e
            ), &[]);
            
            // Show basic status anyway
            display_default_status(effects).await;
            Ok(())
        }
    }
}

/// Read configuration through storage effects
async fn read_config_through_effects(
    effects: &AuraEffectSystem,
    config_path: &PathBuf
) -> Result<DeviceConfig> {
    let config_data = effects.retrieve(&config_path.display().to_string()).await
        .map_err(|e| anyhow::anyhow!("Storage read failed: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("Config file not found"))?;
    
    let config_str = String::from_utf8(config_data)
        .map_err(|e| anyhow::anyhow!("Invalid UTF-8 in config: {}", e))?;
    
    // Parse TOML configuration
    let config: DeviceConfig = toml::from_str(&config_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse config: {}", e))?;
    
    effects.log_info("Configuration loaded successfully", &[]);
    
    Ok(config)
}

/// Display status information through console effects
async fn display_status_info(effects: &AuraEffectSystem, config: &DeviceConfig) {
    effects.log_info("=== Account Status ===", &[]);
    effects.log_info(&format!("Device ID: {}", config.device_id), &[]);
    effects.log_info(&format!("Status: Active"), &[]);
    effects.log_info(&format!("Total Devices: {}", config.total_devices), &[]);
    effects.log_info(&format!("Threshold: {}", config.threshold), &[]);
    
    if let Some(network) = &config.network {
        effects.log_info(&format!("Default Port: {}", network.default_port), &[]);
    }
    
    effects.log_info("=== End Status ===", &[]);
}

/// Display default status when config can't be read
async fn display_default_status(effects: &AuraEffectSystem) {
    effects.log_info("=== Account Status (Default) ===", &[]);
    effects.log_info("Status: Unknown (config unreadable)", &[]);
    effects.log_info("Devices: Unknown", &[]);
    effects.log_info("Threshold: Unknown", &[]);
    effects.log_info("=== End Status ===", &[]);
}

/// Device configuration structure for parsing
#[derive(Debug, serde::Deserialize)]
struct DeviceConfig {
    device_id: String,
    threshold: u32,
    total_devices: u32,
    logging: Option<LoggingConfig>,
    network: Option<NetworkConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct LoggingConfig {
    level: String,
    structured: bool,
}

#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    default_port: u16,
    timeout: u64,
    max_retries: u32,
}