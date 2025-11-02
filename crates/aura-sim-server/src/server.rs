//! Core simulation server implementation
//!
//! Provides WebSocket server infrastructure with branch management, real-time event streaming,
//! and multi-client support for the Aura Dev Console.

use anyhow::Result;
use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use serde_json;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    branch_manager::{BranchId, BranchManager},
    command_handler::CommandHandler,
    websocket::WebSocketHandler,
};

/// Core simulation server state
#[derive(Debug)]
pub struct SimulationServer {
    /// Server bind address
    bind_address: String,
    /// Connected clients with their branch assignments
    clients: Arc<Mutex<HashMap<ClientId, ClientState>>>,
    /// Branch management system
    branch_manager: Arc<Mutex<BranchManager>>,
    /// Command handler for processing REPL commands
    command_handler: Arc<CommandHandler>,
}

/// Unique client identifier
pub type ClientId = Uuid;

/// Per-client connection state
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ClientState {
    /// Unique client identifier
    pub id: ClientId,
    /// Branch this client is currently viewing
    pub current_branch: BranchId,
    /// Whether client is actively streaming events
    pub streaming_events: bool,
    /// Client connection metadata
    pub metadata: ClientMetadata,
}

/// Client connection metadata
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ClientMetadata {
    /// Client user agent or identifier
    pub user_agent: Option<String>,
    /// Connection timestamp
    pub connected_at: std::time::SystemTime,
    /// Last seen activity
    pub last_activity: std::time::SystemTime,
}

/// Shared server state for Axum handlers
#[derive(Clone)]
pub struct ServerState {
    pub clients: Arc<Mutex<HashMap<ClientId, ClientState>>>,
    pub branch_manager: Arc<Mutex<BranchManager>>,
    pub command_handler: Arc<CommandHandler>,
}

#[allow(dead_code)]
impl SimulationServer {
    /// Create a new simulation server
    pub fn new(bind_address: String) -> Self {
        let branch_manager = Arc::new(Mutex::new(BranchManager::new()));
        let command_handler = Arc::new(CommandHandler::new(branch_manager.clone()));

        Self {
            bind_address,
            clients: Arc::new(Mutex::new(HashMap::new())),
            branch_manager,
            command_handler,
        }
    }

    /// Start the WebSocket server
    pub async fn start(self) -> Result<()> {
        let state = ServerState {
            clients: self.clients.clone(),
            branch_manager: self.branch_manager.clone(),
            command_handler: self.command_handler.clone(),
        };

        // Build the router with WebSocket and HTTP endpoints
        let app = Router::new()
            .route("/ws", get(websocket_handler))
            .route("/health", get(health_check))
            .route("/api/status", get(server_status))
            .route("/api/branches", get(list_branches))
            .route("/api/branches/:branch_id", get(get_branch_info))
            .layer(
                ServiceBuilder::new()
                    .layer(TraceLayer::new_for_http())
                    .layer(CorsLayer::permissive()),
            )
            .with_state(state);

        // Parse the bind address
        let addr: SocketAddr = self.bind_address.parse()?;

        info!("Starting simulation server on {}", addr);

        // Start the server
        let listener = TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }

    /// Get server statistics
    pub fn get_stats(&self) -> ServerStats {
        let clients = self.clients.lock().unwrap();
        let branch_manager = self.branch_manager.lock().unwrap();

        ServerStats {
            connected_clients: clients.len(),
            total_branches: branch_manager.get_branch_count(),
            active_simulations: branch_manager.get_active_simulation_count(),
        }
    }
}

/// Server statistics for monitoring
#[derive(Debug, serde::Serialize)]
pub struct ServerStats {
    pub connected_clients: usize,
    pub total_branches: usize,
    pub active_simulations: usize,
}

/// WebSocket connection handler
async fn websocket_handler(ws: WebSocketUpgrade, State(state): State<ServerState>) -> Response {
    ws.on_upgrade(|socket| handle_websocket_connection(socket, state))
}

/// Handle a new WebSocket connection
async fn handle_websocket_connection(socket: WebSocket, state: ServerState) {
    let client_id = Uuid::new_v4();

    info!("New WebSocket connection: {}", client_id);

    // Create initial client state
    let default_branch = {
        let mut branch_manager = state.branch_manager.lock().unwrap();
        branch_manager.get_or_create_default_branch()
    };

    let client_state = ClientState {
        id: client_id,
        current_branch: default_branch,
        streaming_events: false,
        metadata: ClientMetadata {
            user_agent: None,
            connected_at: current_timestamp,
            last_activity: current_timestamp,
        },
    };

    // Register the client
    {
        let mut clients = state.clients.lock().unwrap();
        clients.insert(client_id, client_state.clone());
    }

    // Create WebSocket handler
    let ws_handler = WebSocketHandler::new(client_id, state.clone());

    // Handle the connection
    if let Err(e) = ws_handler.handle_connection(socket).await {
        error!("WebSocket connection error for {}: {}", client_id, e);
    }

    // Clean up client state
    {
        let mut clients = state.clients.lock().unwrap();
        clients.remove(&client_id);
    }

    info!("WebSocket connection closed: {}", client_id);
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

/// Server status endpoint
async fn server_status(State(state): State<ServerState>) -> impl IntoResponse {
    let clients = state.clients.lock().unwrap();
    let branch_manager = state.branch_manager.lock().unwrap();

    let stats = ServerStats {
        connected_clients: clients.len(),
        total_branches: branch_manager.get_branch_count(),
        active_simulations: branch_manager.get_active_simulation_count(),
    };

    (StatusCode::OK, serde_json::to_string(&stats).unwrap())
}

/// List all branches endpoint
async fn list_branches(State(state): State<ServerState>) -> impl IntoResponse {
    let branch_manager = state.branch_manager.lock().unwrap();
    let branches = branch_manager.list_branches();

    (StatusCode::OK, serde_json::to_string(&branches).unwrap())
}

/// Get specific branch info endpoint  
async fn get_branch_info(
    Path(branch_id): Path<String>,
    State(state): State<ServerState>,
) -> impl IntoResponse {
    let branch_manager = state.branch_manager.lock().unwrap();

    if let Ok(branch_uuid) = Uuid::parse_str(&branch_id) {
        if let Some(branch_info) = branch_manager.get_branch_info(branch_uuid) {
            return (StatusCode::OK, serde_json::to_string(&branch_info).unwrap());
        }
    }

    (StatusCode::NOT_FOUND, "Branch not found".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let server = SimulationServer::new("127.0.0.1:9001".to_string());
        let stats = server.get_stats();

        assert_eq!(stats.connected_clients, 0);
        assert_eq!(stats.total_branches, 0);
    }

    #[tokio::test]
    async fn test_server_startup() {
        // Test that we can create a server (actual binding requires available port)
        let server = SimulationServer::new("127.0.0.1:0".to_string());

        // Just verify the server can be created without errors
        assert_eq!(server.bind_address, "127.0.0.1:0");
    }
}
