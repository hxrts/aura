//! Node Command Handler
//!
//! Effect-based implementation of the node command.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::config::load_config_utf8;
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::effects::{PhysicalTimeEffects, TimeEffects};
use std::path::Path;

/// Handle node operations through effects
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_node(
    ctx: &HandlerContext<'_>,
    port: u16,
    daemon: bool,
    config_path: &Path,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println(format!("Starting node on port {port} (daemon: {daemon})"));
    output.kv("Config", config_path.display().to_string());

    // Load and validate configuration
    let _config = load_node_config(ctx, config_path, &mut output).await?;

    if daemon {
        // Simulate daemon mode through effects
        run_daemon_mode(ctx, port, &mut output).await?;
    } else {
        // Run interactive mode through effects
        run_interactive_mode(ctx, port, &mut output).await?;
    }

    Ok(output)
}

/// Load node configuration through storage effects
async fn load_node_config(
    ctx: &HandlerContext<'_>,
    config_path: &Path,
    output: &mut CliOutput,
) -> TerminalResult<NodeConfig> {
    let key = config_path.display().to_string();
    let config_str = load_config_utf8(ctx, &key).await?;

    let config: NodeConfig =
        toml::from_str(&config_str).map_err(|e| TerminalError::Config(e.to_string()))?;

    output.println("Node configuration loaded");

    Ok(config)
}

/// Run node in daemon mode through effects
async fn run_daemon_mode(
    ctx: &HandlerContext<'_>,
    port: u16,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    output.println("Initializing daemon mode...");

    // Simulate daemon initialization
    let start_time = ctx.effects().current_epoch().await;
    output.kv("Node started at epoch", start_time.to_string());

    // Simulate some startup delay and health checks
    simulate_startup_delay(ctx, output).await?;

    output.println(format!("Node daemon started successfully on port {port}"));

    // Run a short, effect-driven heartbeat loop to verify the node can make progress
    for idx in 0..3 {
        ctx.effects()
            .sleep_ms(200)
            .await
            .map_err(|e| TerminalError::Operation(format!("daemon heartbeat sleep failed: {e}")))?;
        let epoch = ctx.effects().current_epoch().await;
        output.println(format!("Daemon heartbeat {} at epoch {}", idx + 1, epoch));
    }

    Ok(())
}

/// Run node in interactive mode through effects
async fn run_interactive_mode(
    ctx: &HandlerContext<'_>,
    port: u16,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    output.println(format!(
        "Node started in interactive mode on port {port}. Press Ctrl+C to stop."
    ));

    let start_time = ctx.effects().current_epoch().await;
    output.kv("Started at epoch", start_time.to_string());

    // Simulate interactive mode - in real implementation would handle signals
    simulate_interactive_session(ctx, output).await?;

    output.println("Node stopped");

    Ok(())
}

/// Simulate startup delay using time effects
async fn simulate_startup_delay(
    ctx: &HandlerContext<'_>,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    let delay_start = ctx.effects().current_epoch().await;

    // Simulate 1 second startup time
    let mut elapsed = 0u64;
    while elapsed < 1000 {
        let current = ctx.effects().current_epoch().await;
        elapsed = current.saturating_sub(delay_start);

        // Yield control using effect-driven sleep
        ctx.effects()
            .sleep_ms(25)
            .await
            .map_err(|e| TerminalError::Operation(format!("startup sleep failed: {e}")))?;
    }

    output.println("Startup complete");

    Ok(())
}

/// Simulate interactive session
async fn simulate_interactive_session(
    ctx: &HandlerContext<'_>,
    output: &mut CliOutput,
) -> TerminalResult<()> {
    for i in 1..=3 {
        let current = ctx.effects().current_epoch().await;
        output.println(format!("Interactive tick {i} at epoch {current}"));

        // Simulate some work
        ctx.effects()
            .sleep_ms(50)
            .await
            .map_err(|e| TerminalError::Operation(format!("interactive sleep failed: {e}")))?;
    }

    output.println("Interactive session ended (simulated)");

    Ok(())
}

/// Node configuration structure
#[derive(Debug, serde::Deserialize)]
struct NodeConfig {
    _device_id: String,
    _threshold: u32,
    _total_devices: u32,
    _logging: Option<LoggingConfig>,
    _network: Option<NetworkConfig>,
}

#[derive(Debug, serde::Deserialize)]
struct LoggingConfig {
    _level: String,
    _structured: bool,
}

#[derive(Debug, serde::Deserialize)]
struct NetworkConfig {
    _default_port: u16,
    _timeout: u64,
    _max_retries: u32,
}
