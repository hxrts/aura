/// WebSocket Service for Console Backend Connections
///
/// Provides WebSocket connections to simulation server and live network instrumentation
use app_console_types::{
    ClientMessage, ConsoleCommand, ConsoleEvent, ConsoleResponse, DeviceInfo, ServerMessage,
};
use leptos::prelude::*;
use serde_json;
use std::collections::VecDeque;
use wasm_bindgen::prelude::*;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

use super::mock_data::{EdgeType, NetworkEdge, NetworkNode, NodeStatus, NodeType};

/// WebSocket connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

/// WebSocket service for backend connections
#[derive(Clone)]
pub struct WebSocketService {
    connection_state: RwSignal<ConnectionState>,
    messages: RwSignal<VecDeque<ServerMessage>>,
    network_topology: RwSignal<Option<(Vec<NetworkNode>, Vec<NetworkEdge>)>>,
    url: RwSignal<Option<String>>,
}

impl WebSocketService {
    pub fn new() -> Self {
        Self {
            connection_state: RwSignal::new(ConnectionState::Disconnected),
            messages: RwSignal::new(VecDeque::new()),
            network_topology: RwSignal::new(None),
            url: RwSignal::new(None),
        }
    }

    /// Connect to WebSocket endpoint
    pub fn connect(&self, url: &str) {
        let url_str = url.to_string();
        log::info!("Connecting to WebSocket at: {}", url_str);

        self.connection_state.set(ConnectionState::Connecting);
        self.url.set(Some(url_str.clone()));

        // Create WebSocket connection
        match WebSocket::new(&url_str) {
            Ok(ws) => {
                // Setup message handler
                let messages = self.messages;
                let network_topology = self.network_topology;
                let connection_state = self.connection_state;

                let onmessage_callback = Closure::wrap(Box::new(move |e: MessageEvent| {
                    if let Ok(txt) = e.data().dyn_into::<js_sys::JsString>() {
                        let msg_str = String::from(txt);
                        log::info!("Received WebSocket message: {}", msg_str);

                        // Parse server message
                        match serde_json::from_str::<ServerMessage>(&msg_str) {
                            Ok(server_msg) => {
                                match &server_msg {
                                    ServerMessage::Response { response, .. } => {
                                        match response {
                                            ConsoleResponse::NetworkTopology { topology } => {
                                                log::info!(
                                                    "Received network topology with {} nodes",
                                                    topology.nodes.len()
                                                );
                                                let (nodes, edges) =
                                                    convert_topology_to_network_data(topology);
                                                network_topology.set(Some((nodes, edges)));
                                            }
                                            ConsoleResponse::Devices { devices } => {
                                                // Fallback: convert device list to topology if no direct topology available
                                                log::info!("Received devices list, converting to topology: {} devices", devices.len());
                                                let (nodes, edges) =
                                                    convert_devices_to_network_data(devices);
                                                network_topology.set(Some((nodes, edges)));
                                            }
                                            ConsoleResponse::Error { message } => {
                                                log::warn!("Server error: {}", message);
                                            }
                                            _ => {}
                                        }
                                    }
                                    ServerMessage::Event(event) => {
                                        if let ConsoleEvent::NetworkTopologyChanged { topology } =
                                            event
                                        {
                                            log::info!(
                                                "Network topology updated with {} nodes",
                                                topology.nodes.len()
                                            );
                                            let (nodes, edges) =
                                                convert_topology_to_network_data(topology);
                                            network_topology.set(Some((nodes, edges)));
                                        }
                                    }
                                }

                                // Add to message queue
                                messages.update(|msgs| {
                                    msgs.push_back(server_msg);
                                    // Keep only last 100 messages
                                    if msgs.len() > 100 {
                                        msgs.pop_front();
                                    }
                                });
                            }
                            Err(e) => {
                                log::error!("Failed to parse WebSocket message: {}", e);
                            }
                        }
                    }
                })
                    as Box<dyn FnMut(MessageEvent)>);

                ws.set_onmessage(Some(onmessage_callback.as_ref().unchecked_ref()));
                onmessage_callback.forget();

                // Setup open handler
                let connection_state_open = connection_state;
                let onopen_callback = Closure::wrap(Box::new(move |_| {
                    log::info!("WebSocket connected successfully");
                    connection_state_open.set(ConnectionState::Connected);

                    // Subscribe to network topology events and request initial data
                    let _subscribe_msg = ClientMessage::Subscribe {
                        event_types: vec![
                            "NetworkTopologyChanged".to_string(),
                            "TraceEvent".to_string(),
                        ],
                    };

                    // Try GetTopology first, then fallback to listing devices
                    let _topology_cmd = ClientMessage::Command {
                        id: "initial_topology".to_string(),
                        command: ConsoleCommand::GetTopology,
                    };

                    let _devices_cmd = ClientMessage::Command {
                        id: "initial_devices".to_string(),
                        command: ConsoleCommand::QueryState {
                            device_id: "list_all".to_string(),
                        },
                    };

                    // Note: We'll send commands after connection is established
                    log::info!("WebSocket connection established, ready for commands");
                }) as Box<dyn FnMut(JsValue)>);

                ws.set_onopen(Some(onopen_callback.as_ref().unchecked_ref()));
                onopen_callback.forget();

                // Setup error handler
                let connection_state_error = connection_state;
                let onerror_callback = Closure::wrap(Box::new(move |e: ErrorEvent| {
                    let error_msg = format!("WebSocket error: {:?}", e);
                    log::error!("{}", error_msg);
                    connection_state_error.set(ConnectionState::Error(error_msg));
                })
                    as Box<dyn FnMut(ErrorEvent)>);

                ws.set_onerror(Some(onerror_callback.as_ref().unchecked_ref()));
                onerror_callback.forget();

                // Setup close handler
                let connection_state_close = connection_state;
                let onclose_callback = Closure::wrap(Box::new(move |e: CloseEvent| {
                    log::info!("WebSocket closed: code={}, reason={}", e.code(), e.reason());
                    connection_state_close.set(ConnectionState::Disconnected);
                })
                    as Box<dyn FnMut(CloseEvent)>);

                ws.set_onclose(Some(onclose_callback.as_ref().unchecked_ref()));
                onclose_callback.forget();
            }
            Err(e) => {
                let error_msg = format!("Failed to create WebSocket: {:?}", e);
                log::error!("{}", error_msg);
                self.connection_state.set(ConnectionState::Error(error_msg));
            }
        }
    }

    /// Disconnect from WebSocket
    #[allow(dead_code)]
    pub fn disconnect(&self) {
        self.connection_state.set(ConnectionState::Disconnected);
        self.url.set(None);
        log::info!("WebSocket disconnected");
    }

    /// Send command to backend (simplified - we'll implement this later if needed)
    #[allow(dead_code)]
    pub fn send_command(&self, command: ConsoleCommand) {
        log::info!(
            "Command requested: {:?} (WebSocket send not yet implemented)",
            command
        );
    }

    // Getters for reactive signals
    pub fn connection_state(&self) -> ReadSignal<ConnectionState> {
        self.connection_state.read_only()
    }

    #[allow(dead_code)]
    pub fn messages(&self) -> ReadSignal<VecDeque<ServerMessage>> {
        self.messages.read_only()
    }

    pub fn network_topology(&self) -> ReadSignal<Option<(Vec<NetworkNode>, Vec<NetworkEdge>)>> {
        self.network_topology.read_only()
    }
}

/// Convert backend topology data to frontend network visualization format
fn convert_topology_to_network_data(
    topology: &app_console_types::NetworkTopology,
) -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Convert nodes
    for (node_id, node_info) in &topology.nodes {
        let node_type = match node_info.participant_type {
            app_console_types::trace::ParticipantType::Honest => NodeType::Honest,
            app_console_types::trace::ParticipantType::Byzantine => NodeType::Byzantine,
            app_console_types::trace::ParticipantType::Offline => NodeType::Observer,
        };

        let status = match node_info.status {
            app_console_types::trace::ParticipantStatus::Online => NodeStatus::Online,
            app_console_types::trace::ParticipantStatus::Offline => NodeStatus::Offline,
            app_console_types::trace::ParticipantStatus::Partitioned => NodeStatus::Error,
        };

        nodes.push(NetworkNode {
            id: node_id.clone(),
            label: node_info.device_id.clone(),
            node_type,
            status,
            position: None, // NodeInfo doesn't have position data
        });
    }

    // Convert edges
    for edge in &topology.edges {
        // All edges from the console-types are message flows with counts
        let edge_type = if edge.message_count > 0 {
            EdgeType::MessageFlow
        } else {
            EdgeType::P2PConnection
        };

        edges.push(NetworkEdge {
            source: edge.from.clone(),
            target: edge.to.clone(),
            edge_type,
            strength: (edge.message_count as f64).clamp(0.1, 1.0), // Normalize to 0.1-1.0 range
        });
    }

    (nodes, edges)
}

/// Fallback: Convert device list to network data when topology is not available
fn convert_devices_to_network_data(devices: &[DeviceInfo]) -> (Vec<NetworkNode>, Vec<NetworkEdge>) {
    let mut nodes = Vec::new();

    // Convert devices to nodes
    for device in devices {
        let node_type = match device.participant_type {
            app_console_types::trace::ParticipantType::Honest => NodeType::Honest,
            app_console_types::trace::ParticipantType::Byzantine => NodeType::Byzantine,
            app_console_types::trace::ParticipantType::Offline => NodeType::Observer,
        };

        let status = match device.status {
            app_console_types::trace::ParticipantStatus::Online => NodeStatus::Online,
            app_console_types::trace::ParticipantStatus::Offline => NodeStatus::Offline,
            app_console_types::trace::ParticipantStatus::Partitioned => NodeStatus::Error,
        };

        nodes.push(NetworkNode {
            id: device.device_id.clone(),
            label: device.device_id.clone(),
            node_type,
            status,
            position: None,
        });
    }

    // Generate basic connectivity - all nodes connected to each other for now
    let mut edges = Vec::new();
    for i in 0..devices.len() {
        for j in (i + 1)..devices.len() {
            edges.push(NetworkEdge {
                source: devices[i].device_id.clone(),
                target: devices[j].device_id.clone(),
                edge_type: EdgeType::P2PConnection,
                strength: 0.5, // Default strength
            });
        }
    }

    (nodes, edges)
}
