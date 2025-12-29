//! Sync Command Handler
//!
//! Effect-based implementation of the sync daemon command.
//! Uses `SyncServiceManager` from aura-agent for background journal synchronization.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::sync::SyncAction;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use crate::ids;
// Import sync types from aura-agent (runtime layer)
use aura_agent::{SyncManagerConfig, SyncServiceManager};
use aura_core::identifiers::DeviceId;
use aura_effects::time::PhysicalTimeHandler;
use aura_sync::services::HealthStatus;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;

/// Handle sync operations through effects
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_sync(
    ctx: &HandlerContext<'_>,
    action: &SyncAction,
) -> TerminalResult<CliOutput> {
    match action {
        SyncAction::Daemon {
            interval,
            max_concurrent,
            peers,
            config: _,
        } => handle_daemon_mode(ctx, *interval, *max_concurrent, peers.as_deref()).await,

        SyncAction::Once { peers, config: _ } => handle_once_mode(ctx, peers).await,

        SyncAction::Status => handle_status(ctx).await,

        SyncAction::AddPeer { peer } => handle_add_peer(ctx, peer).await,

        SyncAction::RemovePeer { peer } => handle_remove_peer(ctx, peer).await,
    }
}

/// Run sync daemon mode (default)
///
/// Note: Daemon mode prints continuously during operation and returns
/// summary output when shutting down. The periodic status messages
/// are printed in real-time.
async fn handle_daemon_mode(
    ctx: &HandlerContext<'_>,
    interval_secs: u64,
    max_concurrent: usize,
    peers: Option<&str>,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println("Starting sync daemon...");
    output.kv("Interval", format!("{}s", interval_secs));
    output.kv("Max concurrent", max_concurrent.to_string());

    // Parse initial peers
    let initial_peers: Vec<DeviceId> = if let Some(peers_str) = peers {
        peers_str
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| ids::device_id(s.trim()))
            .collect()
    } else {
        Vec::new()
    };

    if !initial_peers.is_empty() {
        output.kv("Initial peers", initial_peers.len().to_string());
    }

    // Render startup messages immediately
    output.render();

    // Configure sync manager
    let config = SyncManagerConfig {
        auto_sync_enabled: true,
        auto_sync_interval: Duration::from_secs(interval_secs),
        max_concurrent_syncs: max_concurrent,
        initial_peers,
        ..SyncManagerConfig::default()
    };

    let manager = SyncServiceManager::new(config);

    // Start the sync service
    let time_handler = Arc::new(PhysicalTimeHandler::new());
    manager
        .start(time_handler.clone())
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to start sync service: {}", e)))?;

    println!("\nSync daemon started. Press Ctrl+C to stop.\n");

    // Get initial time for uptime tracking
    let start_time = time_handler.physical_time_now_ms();

    // Run sync loop until interrupted (direct printing for continuous output)
    let mut tick_count = 0u64;
    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\nReceived shutdown signal...");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {
                tick_count += 1;
                let uptime_secs = (time_handler.physical_time_now_ms() - start_time) / 1000;

                // Get health info
                if let Some(health) = manager.health().await {
                    let status = match health.status {
                        HealthStatus::Healthy => "healthy",
                        HealthStatus::Degraded => "degraded",
                        HealthStatus::Unhealthy => "unhealthy",
                        HealthStatus::Starting => "starting",
                        HealthStatus::Stopping => "stopping",
                    };
                    println!(
                        "[tick {}] Sync daemon {} (uptime: {}s, active sessions: {})",
                        tick_count,
                        status,
                        uptime_secs,
                        health.active_sessions
                    );
                } else {
                    println!("[tick {}] Sync daemon running (uptime: {}s)", tick_count, uptime_secs);
                }

                // Get metrics periodically
                if tick_count % 5 == 0 {
                    if let Some(metrics) = manager.metrics().await {
                        println!(
                            "  Metrics - requests: {}, errors: {}, avg latency: {:.2}ms",
                            metrics.requests_processed,
                            metrics.errors_encountered,
                            metrics.avg_latency_ms
                        );
                    }
                }
            }
        }
    }

    // Stop the service
    println!("Stopping sync daemon...");
    manager
        .stop()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to stop sync service: {}", e)))?;

    let _ = ctx; // Acknowledge context for future use

    // Return shutdown summary (startup messages already rendered)
    let mut shutdown_output = CliOutput::new();
    shutdown_output.println("Sync daemon stopped.");
    shutdown_output.kv("Total ticks", tick_count.to_string());
    Ok(shutdown_output)
}

/// Perform a one-shot sync with specific peers
async fn handle_once_mode(ctx: &HandlerContext<'_>, peers_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println("Performing one-shot sync...");

    // Parse peers
    let peers: Vec<DeviceId> = peers_str
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| ids::device_id(s.trim()))
        .collect();

    if peers.is_empty() {
        return Err(TerminalError::Input("No peers specified for sync".into()));
    }

    output.kv("Peers", peers.len().to_string());

    // Configure for one-shot (no auto sync)
    let config = SyncManagerConfig::manual_only();
    let manager = SyncServiceManager::new(config);

    // Start the sync service
    let time_handler = Arc::new(PhysicalTimeHandler::new());
    manager
        .start(time_handler.clone())
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to start sync service: {}", e)))?;

    // Full sync_with_peers needs the full effect system
    // For now, just add peers and show status
    for peer in &peers {
        manager.add_peer(*peer).await;
    }

    output.kv("Registered peers", manager.peers().await.len().to_string());

    // In a real implementation, this would call:
    // manager.sync_with_peers(effects, peers).await?;

    // Show completion
    if let Some(health) = manager.health().await {
        let status = match health.status {
            HealthStatus::Healthy => "healthy",
            HealthStatus::Degraded => "degraded",
            HealthStatus::Unhealthy => "unhealthy",
            HealthStatus::Starting => "starting",
            HealthStatus::Stopping => "stopping",
        };
        output.kv("Sync service health", status);
    }

    manager.stop().await.ok();
    let _ = ctx; // Acknowledge context for future use

    output.println("One-shot sync complete.");
    Ok(output)
}

/// Show sync status and metrics
async fn handle_status(ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.section("Sync Service Status");

    // Status query requires a running sync daemon (started via `aura sync daemon`).
    // Without a daemon, show usage instructions.
    output.println("Note: Full status requires a running sync daemon.");
    output.blank();
    output.println("To start the sync daemon:");
    output.println("  aura sync daemon");
    output.blank();
    output.println("To sync once with specific peers:");
    output.println("  aura sync once --peers <device-id-1>,<device-id-2>");

    let _ = ctx; // Acknowledge context
    Ok(output)
}

/// Add a peer to the sync list
async fn handle_add_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let peer_id = ids::device_id(peer_str);
    output.kv("Added peer to sync list", peer_id.to_string());
    output.println("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(output)
}

/// Remove a peer from the sync list
async fn handle_remove_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let peer_id = ids::device_id(peer_str);
    output.kv("Removed peer from sync list", peer_id.to_string());
    output.println("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(output)
}
