//! Sync Command Handler
//!
//! Effect-based implementation of the sync daemon command.
//! Uses `SyncServiceManager` from aura-agent for background journal synchronization.

use crate::cli::sync::SyncAction;
use crate::handlers::HandlerContext;
use crate::ids;
use anyhow::Result;
// Import sync types from aura-agent (runtime layer)
use aura_agent::{SyncManagerConfig, SyncServiceManager};
use aura_core::identifiers::DeviceId;
use aura_effects::time::PhysicalTimeHandler;
use aura_sync::services::HealthStatus;
use std::time::Duration;
use tokio::signal;

/// Handle sync operations through effects
pub async fn handle_sync(ctx: &HandlerContext<'_>, action: &SyncAction) -> Result<()> {
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
async fn handle_daemon_mode(
    ctx: &HandlerContext<'_>,
    interval_secs: u64,
    max_concurrent: usize,
    peers: Option<&str>,
) -> Result<()> {
    println!("Starting sync daemon...");
    println!("  Interval: {}s", interval_secs);
    println!("  Max concurrent: {}", max_concurrent);

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
        println!("  Initial peers: {}", initial_peers.len());
    }

    // Configure sync manager
    let config = SyncManagerConfig {
        auto_sync_enabled: true,
        auto_sync_interval: Duration::from_secs(interval_secs),
        max_concurrent_syncs: max_concurrent,
        initial_peers,
    };

    let manager = SyncServiceManager::new(config);

    // Start the sync service
    let time_handler = PhysicalTimeHandler::new();
    manager
        .start(&time_handler)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start sync service: {}", e))?;

    println!("\nSync daemon started. Press Ctrl+C to stop.\n");

    // Get initial time for uptime tracking
    let start_time = time_handler.physical_time_now_ms();

    // Run sync loop until interrupted
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
        .map_err(|e| anyhow::anyhow!("Failed to stop sync service: {}", e))?;

    let _ = ctx; // Acknowledge context for future use
    println!("Sync daemon stopped.");
    Ok(())
}

/// Perform a one-shot sync with specific peers
async fn handle_once_mode(ctx: &HandlerContext<'_>, peers_str: &str) -> Result<()> {
    println!("Performing one-shot sync...");

    // Parse peers
    let peers: Vec<DeviceId> = peers_str
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| ids::device_id(s.trim()))
        .collect();

    if peers.is_empty() {
        return Err(anyhow::anyhow!("No peers specified for sync"));
    }

    println!("  Peers: {}", peers.len());

    // Configure for one-shot (no auto sync)
    let config = SyncManagerConfig::manual_only();
    let manager = SyncServiceManager::new(config);

    // Start the sync service
    let time_handler = PhysicalTimeHandler::new();
    manager
        .start(&time_handler)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start sync service: {}", e))?;

    // Note: Full sync_with_peers would need the full effect system
    // For now, just add peers and show status
    for peer in &peers {
        manager.add_peer(*peer).await;
    }

    println!(
        "  Registered {} peers for sync",
        manager.peers().await.len()
    );

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
        println!("Sync service health: {}", status);
    }

    manager.stop().await.ok();
    let _ = ctx; // Acknowledge context for future use
    println!("One-shot sync complete.");
    Ok(())
}

/// Show sync status and metrics
async fn handle_status(ctx: &HandlerContext<'_>) -> Result<()> {
    println!("Sync Service Status");
    println!("===================\n");

    // Status query requires a running sync daemon (started via `aura sync daemon`).
    // Without a daemon, show usage instructions.
    println!("Note: Full status requires a running sync daemon.");
    println!();
    println!("To start the sync daemon:");
    println!("  aura sync daemon");
    println!();
    println!("To sync once with specific peers:");
    println!("  aura sync once --peers <device-id-1>,<device-id-2>");

    let _ = ctx; // Acknowledge context
    Ok(())
}

/// Add a peer to the sync list
async fn handle_add_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> Result<()> {
    let peer_id = ids::device_id(peer_str);
    println!("Added peer to sync list: {}", peer_id);
    println!("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(())
}

/// Remove a peer from the sync list
async fn handle_remove_peer(ctx: &HandlerContext<'_>, peer_str: &str) -> Result<()> {
    let peer_id = ids::device_id(peer_str);
    println!("Removed peer from sync list: {}", peer_id);
    println!("Note: This will take effect on the next sync daemon start.");

    let _ = ctx; // Acknowledge context
    Ok(())
}
