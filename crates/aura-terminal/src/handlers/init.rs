//! Init Command Handler
//!
//! Effect-based implementation of the init command.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::effects::time::PhysicalTimeEffects;
use aura_core::effects::StorageCoreEffects;
use aura_core::time::TimeStamp;
use std::path::Path;

/// Handle initialization through effects
///
/// Returns `CliOutput` instead of printing directly, enabling:
/// - Unit testing without capturing stdout
/// - Consistent output formatting
/// - Clear separation of logic from I/O
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_init(
    ctx: &HandlerContext<'_>,
    num_devices: u32,
    threshold: u32,
    output_dir: &Path,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    // Log initialization start
    output.println(format!(
        "Initializing {threshold}-of-{num_devices} threshold account"
    ));
    output.kv("Output directory", output_dir.display().to_string());

    // Validate parameters through effects
    if threshold > num_devices {
        output.eprintln("Threshold cannot be greater than number of devices");
        return Err(TerminalError::Input(format!(
            "Invalid parameters: threshold ({threshold}) > num_devices ({num_devices})"
        )));
    }

    if threshold == 0 {
        output.eprintln("Threshold must be greater than 0");
        return Err(TerminalError::Input("Invalid threshold: 0".into()));
    }

    // Create directory structure through storage effects
    let configs_dir = output_dir.join("configs");
    create_directory_through_effects(ctx, output_dir).await?;
    create_directory_through_effects(ctx, &configs_dir).await?;

    // Create effect API metadata through storage effects
    let effect_api_path = output_dir.join("effect_api.cbor");
    let effect_api_data = create_effect_api(ctx, threshold, num_devices, &mut output).await?;

    ctx.effects()
        .store(&effect_api_path.display().to_string(), effect_api_data)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to create effect_api: {e}")))?;

    // Create device config files through storage effects
    for i in 1..=num_devices {
        let config_content = create_device_config(i, threshold, num_devices);
        let config_path = configs_dir.join(format!("device_{i}.toml"));

        // Create device config via StorageEffects
        let config_key = format!("device_config:{}", config_path.display());
        ctx.effects()
            .store(&config_key, config_content.into_bytes())
            .await
            .map_err(|e| {
                TerminalError::Operation(format!(
                    "Failed to create device config {i} via storage effects: {e}"
                ))
            })?;

        output.println(format!("Created device_{i}.toml"));
    }

    // Success message
    output.blank();
    output.println("Account initialized successfully!");

    Ok(output)
}

/// Create directory marker through storage effects
async fn create_directory_through_effects(
    ctx: &HandlerContext<'_>,
    path: &Path,
) -> TerminalResult<()> {
    // Use unified time system to get physical time
    let physical_time = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get physical time: {e}")))?;

    let timestamp = TimeStamp::PhysicalClock(physical_time);

    // Create directory marker via StorageEffects with proper TimeStamp serialization
    let dir_marker_key = format!("directory_marker:{}", path.display());
    let timestamp_bytes = serde_json::to_vec(&timestamp)
        .map_err(|e| TerminalError::Operation(format!("Failed to serialize timestamp: {e}")))?;

    ctx.effects()
        .store(&dir_marker_key, timestamp_bytes)
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to create directory marker via storage effects: {e}"
            ))
        })?;

    Ok(())
}

/// Create effect API data
async fn create_effect_api(
    ctx: &HandlerContext<'_>,
    threshold: u32,
    num_devices: u32,
    output: &mut CliOutput,
) -> TerminalResult<Vec<u8>> {
    // Use unified time system to get physical time
    let physical_time = ctx
        .effects()
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get physical time: {e}")))?;
    let timestamp = physical_time.ts_ms / 1000; // Convert to seconds

    // Create a simple CBOR-like structure
    let effect_api_data =
        format!("effect_api:threshold={threshold},devices={num_devices},created={timestamp}");

    output.println("Created effect API metadata");

    Ok(effect_api_data.into_bytes())
}

/// Create device configuration content (pure function)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_device_config() {
        let config = create_device_config(1, 2, 3);
        assert!(config.contains("device_id = \"device_1\""));
        assert!(config.contains("threshold = 2"));
        assert!(config.contains("total_devices = 3"));
        assert!(config.contains("default_port = 58835"));
    }

    #[test]
    fn test_create_device_config_ports() {
        // Each device should have a different port
        let config1 = create_device_config(1, 2, 3);
        let config2 = create_device_config(2, 2, 3);
        let config3 = create_device_config(3, 2, 3);

        assert!(config1.contains("default_port = 58835"));
        assert!(config2.contains("default_port = 58836"));
        assert!(config3.contains("default_port = 58837"));
    }
}
