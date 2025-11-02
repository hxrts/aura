//! Node management commands
//!
//! Commands for running and managing Aura nodes with optional dev console integration.

use anyhow::Result;
use clap::Args;
use std::sync::Arc;
use tracing::{error, info};

/// Node command for running the Aura agent
#[derive(Args)]
pub struct NodeCommand {
    /// Enable dev console instrumentation
    #[arg(long)]
    pub dev_console: bool,

    /// Port for dev console WebSocket server
    #[arg(long, default_value = "9003")]
    pub dev_console_port: u16,

    /// Port for the node's main services
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Bind address for services
    #[arg(long, default_value = "0.0.0.0")]
    pub bind: String,

    /// Keep the node running indefinitely
    #[arg(long)]
    pub daemon: bool,
}

/// Handle node command execution
pub async fn handle_node_command(cmd: NodeCommand, config: &crate::config::Config) -> Result<()> {
    info!("Starting Aura node...");

    // Load agent from config
    let _integrated_agent = create_integrated_agent(config).await?;

    // Start instrumentation server if requested
    #[cfg(feature = "dev-console")]
    let _instrumentation_server = if cmd.dev_console {
        let server = Arc::new(aura_agent::InstrumentationServer::new(
            integrated_agent.clone(),
        ));
        let server_handle = server.clone();

        // Start instrumentation server in background
        tokio::spawn(async move {
            if let Err(e) = server_handle.start(cmd.dev_console_port).await {
                error!("Instrumentation server failed: {}", e);
            }
        });

        info!("Dev console enabled on port {}", cmd.dev_console_port);
        info!(
            "Connect browser to: ws://localhost:{}/ws",
            cmd.dev_console_port
        );

        Some(server)
    } else {
        info!("Dev console disabled (use --dev-console to enable)");
        None
    };

    #[cfg(not(feature = "dev-console"))]
    if cmd.dev_console {
        error!("Dev console requested but not compiled in. Rebuild with --features dev-console");
        return Err(anyhow::anyhow!("Dev console feature not available"));
    }

    // Start main node services
    info!("Node services starting on {}:{}", cmd.bind, cmd.port);

    // For now, just run a simple event loop
    // In a full implementation, this would start the full agent with transport, etc.
    if cmd.daemon {
        info!("Running in daemon mode. Press Ctrl+C to stop.");

        // Set up graceful shutdown
        let shutdown = setup_shutdown_handler();

        // Wait for shutdown signal
        shutdown.await;

        info!("Shutdown signal received, stopping node...");
    } else {
        info!("Node started successfully. Use --daemon to keep running.");
    }

    Ok(())
}

/// Create an integrated agent from configuration
#[allow(dead_code)]
async fn create_integrated_agent(
    _config: &crate::config::Config,
) -> Result<Arc<dyn aura_agent::Agent>> {
    // TODO: Implement once agent creation is available
    anyhow::bail!("Agent creation not yet implemented")
}

/// Set up graceful shutdown handling
async fn setup_shutdown_handler() {
    use tokio::signal;

    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
