//! Aura Dev Console Simulation Server
//!
//! WebSocket-based simulation server that provides real-time monitoring and control
//! of distributed protocol simulations. Supports branch management, time travel debugging,
//! and interactive REPL commands for comprehensive protocol testing.

use anyhow::Result;
use tracing::{info, warn};
use tracing_subscriber;

mod branch_manager;
mod command_handler;
mod scenario_export;
mod server;
mod simulation_wrapper;
mod websocket;

use server::SimulationServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("sim_server=debug,tower_http=debug")
        .init();

    info!("Starting Aura Dev Console Simulation Server");

    // Create and start the simulation server
    let server = SimulationServer::new("127.0.0.1:9001".to_string());

    match server.start().await {
        Ok(_) => {
            info!("Simulation server started successfully");
            Ok(())
        }
        Err(e) => {
            warn!("Failed to start simulation server: {}", e);
            Err(e)
        }
    }
}
