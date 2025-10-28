/// Data Source Service
///
/// Provides unified abstraction for switching between mock data, simulator data, and live network data.
/// This service encapsulates the different data sources and provides a consistent interface to the frontend.
use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::mock_data::{
    get_mock_branches, get_mock_network_data, get_mock_state_data, Branch, NetworkEdge,
    NetworkNode, StateData,
};
use super::websocket::{ConnectionState, WebSocketService};
use crate::app::components::timeline::TimelineEvent;

/// Available data sources for the console
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DataSource {
    /// Mock data for testing and development
    Mock,
    /// Live simulation data from the simulator
    Simulator,
    /// Real network interactions with live nodes
    Real,
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Mock => write!(f, "Mock"),
            DataSource::Simulator => write!(f, "Simulated"),
            DataSource::Real => write!(f, "Live"),
        }
    }
}

/// Data service trait for abstracting different data sources
pub trait DataService {
    /// Get available branches
    fn get_branches(&self) -> Vec<Branch>;

    /// Get current state data
    fn get_state_data(&self) -> StateData;

    /// Get network topology data
    fn get_network_data(&self) -> (Vec<NetworkNode>, Vec<NetworkEdge>);

    /// Get timeline events
    fn get_timeline_events(&self) -> Vec<TimelineEvent>;

    /// Execute a command (returns response text)
    fn execute_command(&self, command: &str) -> String;

    /// Get connection status for this data source
    fn get_connection_status(&self) -> ConnectionState;
}

/// Mock data service implementation
#[derive(Clone)]
pub struct MockDataService;

impl DataService for MockDataService {
    fn get_branches(&self) -> Vec<Branch> {
        get_mock_branches()
    }

    fn get_state_data(&self) -> StateData {
        get_mock_state_data()
    }

    fn get_network_data(&self) -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
        get_mock_network_data()
    }

    fn get_timeline_events(&self) -> Vec<TimelineEvent> {
        vec![
            TimelineEvent {
                id: "1".to_string(),
                timestamp: 0.0,
                event_type: "NodeStart".to_string(),
                description: "Node Alice started".to_string(),
                node_id: Some("alice".to_string()),
                metadata: None,
            },
            TimelineEvent {
                id: "2".to_string(),
                timestamp: 1.5,
                event_type: "KeyGen".to_string(),
                description: "Threshold key generation initiated".to_string(),
                node_id: None,
                metadata: None,
            },
            TimelineEvent {
                id: "3".to_string(),
                timestamp: 3.2,
                event_type: "NodeStart".to_string(),
                description: "Node Bob started".to_string(),
                node_id: Some("bob".to_string()),
                metadata: None,
            },
        ]
    }

    fn execute_command(&self, command: &str) -> String {
        match command.trim() {
            "help" => "Available commands: status, step, reset, branches, state".to_string(),
            "status" => "Mock Status: Simulation running, 3 nodes online".to_string(),
            "step" => "Mock: Advanced simulation by 1 tick".to_string(),
            "reset" => "Mock: Simulation reset to initial state".to_string(),
            "branches" => "Mock: main, experiment-1, byzantine-test".to_string(),
            "state" => serde_json::to_string_pretty(&get_mock_state_data().state)
                .unwrap_or_else(|_| "Error serializing state".to_string()),
            _ => format!("Mock: Unknown command '{}'", command),
        }
    }

    fn get_connection_status(&self) -> ConnectionState {
        ConnectionState::Connected // Mock is always "connected"
    }
}

/// Simulator data service implementation
#[derive(Clone)]
pub struct SimulatorDataService {
    websocket: WebSocketService,
}

impl SimulatorDataService {
    pub fn new() -> Self {
        let websocket = WebSocketService::new();
        // Connect to simulation server
        websocket.connect("ws://localhost:9001/ws");

        Self { websocket }
    }
}

impl DataService for SimulatorDataService {
    fn get_branches(&self) -> Vec<Branch> {
        // TODO: Fetch actual branches from simulator
        vec![Branch {
            id: "main".to_string(),
            name: "main".to_string(),
            is_current: true,
            parent: None,
            fork_tick: None,
            scenario: Some("dkd-simulation.toml".to_string()),
            last_modified: "Running".to_string(),
            commit_count: 0,
        }]
    }

    fn get_state_data(&self) -> StateData {
        // TODO: Fetch actual state from simulator
        let sim_state = serde_json::json!({
            "simulation_tick": 42,
            "participants": ["alice", "bob", "charlie"],
            "active_protocols": ["dkd", "frost"],
            "world_state": {
                "tick": 42,
                "time": 1234567890,
                "participant_count": 3,
                "active_sessions": 2,
                "in_flight_messages": 5
            }
        });

        StateData {
            node_id: "simulator".to_string(),
            timestamp: 1234567890,
            state: sim_state,
        }
    }

    fn get_network_data(&self) -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
        // Try to get live network topology from WebSocket
        if let Some((nodes, edges)) = self.websocket.network_topology().get_untracked() {
            log::info!(
                "Using live simulator network topology: {} nodes, {} edges",
                nodes.len(),
                edges.len()
            );
            (nodes, edges)
        } else {
            log::info!("No simulator topology available - server not connected");
            // Return empty data when not connected
            (Vec::new(), Vec::new())
        }
    }

    fn get_timeline_events(&self) -> Vec<TimelineEvent> {
        // TODO: Convert simulator TraceEvents to TimelineEvents
        vec![
            TimelineEvent {
                id: "sim-1".to_string(),
                timestamp: 0.0,
                event_type: "SimulationStart".to_string(),
                description: "Simulation initialized with 3 participants".to_string(),
                node_id: None,
                metadata: None,
            },
            TimelineEvent {
                id: "sim-2".to_string(),
                timestamp: 1.0,
                event_type: "ProtocolStateTransition".to_string(),
                description: "DKD protocol: Init -> KeyGeneration".to_string(),
                node_id: Some("alice".to_string()),
                metadata: None,
            },
        ]
    }

    fn execute_command(&self, command: &str) -> String {
        match command.trim() {
            "help" => "Simulator commands: status, step, reset, load_scenario <file>".to_string(),
            "status" => "Simulator Status: Running at tick 42, 3 participants active".to_string(),
            "step" => {
                // TODO: Send step command to simulator via WebSocket
                "Simulator: Advanced by 1 tick".to_string()
            }
            "reset" => {
                // TODO: Send reset command to simulator
                "Simulator: Reset to initial state".to_string()
            }
            _ if command.starts_with("load_scenario ") => {
                let scenario = command.strip_prefix("load_scenario ").unwrap_or("");
                // TODO: Send load_scenario command to simulator
                format!("Simulator: Loading scenario '{}'", scenario)
            }
            _ => format!("Simulator: Unknown command '{}'", command),
        }
    }

    fn get_connection_status(&self) -> ConnectionState {
        self.websocket.connection_state().get_untracked()
    }
}

/// Real network data service implementation
#[derive(Clone)]
pub struct RealNetworkDataService {
    websocket: WebSocketService,
}

impl RealNetworkDataService {
    pub fn new() -> Self {
        let websocket = WebSocketService::new();
        // Connect to live network instrumentation
        websocket.connect("ws://localhost:9003/ws");

        Self { websocket }
    }
}

impl DataService for RealNetworkDataService {
    fn get_branches(&self) -> Vec<Branch> {
        // TODO: Fetch branches from live network
        vec![Branch {
            id: "live".to_string(),
            name: "live-network".to_string(),
            is_current: true,
            parent: None,
            fork_tick: None,
            scenario: None,
            last_modified: "Live".to_string(),
            commit_count: 0,
        }]
    }

    fn get_state_data(&self) -> StateData {
        // TODO: Fetch actual state from live network
        let live_state = serde_json::json!({
            "network_status": "live",
            "connected_peers": 12,
            "sync_status": "synced",
            "last_block_height": 1337,
            "network_health": "good"
        });

        StateData {
            node_id: "live-node".to_string(),
            timestamp: 1234567890,
            state: live_state,
        }
    }

    fn get_network_data(&self) -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
        // Try to get live network topology from WebSocket
        if let Some((nodes, edges)) = self.websocket.network_topology().get_untracked() {
            log::info!(
                "Using live network topology: {} nodes, {} edges",
                nodes.len(),
                edges.len()
            );
            (nodes, edges)
        } else {
            log::info!("No live network topology available - node not connected");
            // Return empty data when not connected
            (Vec::new(), Vec::new())
        }
    }

    fn get_timeline_events(&self) -> Vec<TimelineEvent> {
        // TODO: Fetch actual events from live network
        vec![TimelineEvent {
            id: "live-1".to_string(),
            timestamp: 0.0,
            event_type: "NetworkConnection".to_string(),
            description: "Connected to live Aura network".to_string(),
            node_id: None,
            metadata: None,
        }]
    }

    fn execute_command(&self, command: &str) -> String {
        match command.trim() {
            "help" => "Live network commands: status, peers, sync, disconnect".to_string(),
            "status" => "Live Network: Connected, 12 peers, synced".to_string(),
            "peers" => "Live Network: 12 connected peers".to_string(),
            "sync" => "Live Network: Sync status - up to date".to_string(),
            "disconnect" => "Live Network: Disconnected from network".to_string(),
            _ => format!("Live Network: Unknown command '{}'", command),
        }
    }

    fn get_connection_status(&self) -> ConnectionState {
        self.websocket.connection_state().get_untracked()
    }
}

/// Unified data source manager
#[derive(Clone)]
#[allow(dead_code)]
pub struct DataSourceManager {
    current_source: RwSignal<DataSource>,
    mock_service: MockDataService,
    simulator_service: SimulatorDataService,
    real_service: RealNetworkDataService,
}

impl DataSourceManager {
    pub fn new() -> Self {
        Self {
            current_source: RwSignal::new(DataSource::Mock),
            mock_service: MockDataService,
            simulator_service: SimulatorDataService::new(),
            real_service: RealNetworkDataService::new(),
        }
    }

    pub fn current_source(&self) -> ReadSignal<DataSource> {
        self.current_source.read_only()
    }

    pub fn set_source(&self, source: DataSource) {
        self.current_source.set(source);
    }

    pub fn get_service(&self) -> Box<dyn DataService> {
        match self.current_source.get_untracked() {
            DataSource::Mock => Box::new(self.mock_service.clone()),
            DataSource::Simulator => Box::new(self.simulator_service.clone()),
            DataSource::Real => Box::new(self.real_service.clone()),
        }
    }
}

/// Hook for using the data source manager
pub fn use_data_source() -> DataSourceManager {
    use_context::<DataSourceManager>().expect("DataSourceManager must be provided in context")
}
