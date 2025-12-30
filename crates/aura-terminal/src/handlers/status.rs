//! Status Command Handler
//!
//! Effect-based implementation of the status command.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::config::load_config_utf8;
use crate::handlers::{CliOutput, HandlerContext};
use std::path::Path;

/// Handle status display through effects
///
/// Returns `CliOutput` instead of printing directly, enabling:
/// - Unit testing without capturing stdout
/// - Consistent output formatting
/// - Clear separation of logic from I/O
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_status(
    ctx: &HandlerContext<'_>,
    config_path: &Path,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println(format!(
        "Account status for config: {}",
        config_path.display()
    ));

    let config_key = config_path.display().to_string();

    // Read and parse config through storage effects
    match read_config_through_effects(ctx, &config_key, &mut output).await {
        Ok(config) => {
            format_status_info(&config, &mut output);
            Ok(output)
        }
        Err(e) => {
            output.eprintln(format!("Failed to read config: {e}"));
            format_default_status(&mut output);
            Ok(output)
        }
    }
}

/// Read configuration through storage effects
async fn read_config_through_effects(
    ctx: &HandlerContext<'_>,
    config_key: &str,
    output: &mut CliOutput,
) -> TerminalResult<DeviceConfig> {
    let config_str = load_config_utf8(ctx, config_key).await?;

    let config: DeviceConfig =
        toml::from_str(&config_str).map_err(|e| TerminalError::Config(e.to_string()))?;

    output.println("Configuration loaded successfully");

    Ok(config)
}

/// Format status information into output (pure function)
fn format_status_info(config: &DeviceConfig, output: &mut CliOutput) {
    output.section("Account Status");
    output.kv("Device ID", &config.device_id);
    output.kv("Status", "Active");
    output.kv("Total Devices", config.total_devices.to_string());
    output.kv("Threshold", config.threshold.to_string());

    if let Some(network) = &config.network {
        output.kv("Default Port", network.default_port.to_string());
    }

    output.println("=== End Status ===");
}

/// Format default status when config can't be read (pure function)
fn format_default_status(output: &mut CliOutput) {
    output.section("Account Status (Default)");
    output.kv("Status", "Unknown (config unreadable)");
    output.kv("Devices", "Unknown");
    output.kv("Threshold", "Unknown");
    output.println("=== End Status ===");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_status_info() {
        let config = DeviceConfig {
            device_id: "device-123".into(),
            threshold: 2,
            total_devices: 3,
            network: Some(NetworkConfig {
                default_port: 8080,
                timeout: 30,
                max_retries: 3,
            }),
        };

        let mut output = CliOutput::new();
        format_status_info(&config, &mut output);

        let lines = output.stdout_lines();
        assert!(lines.iter().any(|l| l.contains("Device ID: device-123")));
        assert!(lines.iter().any(|l| l.contains("Threshold: 2")));
        assert!(lines.iter().any(|l| l.contains("Total Devices: 3")));
        assert!(lines.iter().any(|l| l.contains("Default Port: 8080")));
    }

    #[test]
    fn test_format_default_status() {
        let mut output = CliOutput::new();
        format_default_status(&mut output);

        let lines = output.stdout_lines();
        assert!(lines.iter().any(|l| l.contains("Unknown")));
    }
}
