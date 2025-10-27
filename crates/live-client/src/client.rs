//! Live Network Client for connecting to instrumented Aura nodes

use aura_console_types::{ClientMessage, ConsoleCommand, ConsoleResponse, TraceEvent};
use futures::channel::mpsc;
use futures::StreamExt;
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use wasm_core::{console_error, console_log};
// use crate::websocket::LiveWebSocketConnection;  // Will be replaced with unified foundation

/// Live network client for connecting to instrumented Aura nodes
#[wasm_bindgen]
pub struct LiveNetworkClient {
    node_url: String,
    auth_token: Option<String>,
    connection: Option<LiveWebSocketConnection>,
    event_receiver: Option<mpsc::UnboundedReceiver<String>>,
    event_callback: Option<js_sys::Function>,
    connected: bool,
    device_id: Option<String>,
}

#[wasm_bindgen]
impl LiveNetworkClient {
    /// Create a new live network client
    #[wasm_bindgen(constructor)]
    pub fn new(node_url: String, auth_token: Option<String>) -> LiveNetworkClient {
        console_log!("Creating LiveNetworkClient for node: {}", node_url);

        LiveNetworkClient {
            node_url,
            auth_token,
            connection: None,
            event_receiver: None,
            event_callback: None,
            connected: false,
            device_id: None,
        }
    }

    /// Get node URL
    #[wasm_bindgen(getter)]
    pub fn node_url(&self) -> String {
        self.node_url.clone()
    }

    /// Set authentication token
    #[wasm_bindgen]
    pub fn set_auth_token(&mut self, token: String) {
        self.auth_token = Some(token);
    }

    /// Set device ID for identification
    #[wasm_bindgen]
    pub fn set_device_id(&mut self, device_id: String) {
        self.device_id = Some(device_id);
    }

    /// Check if connected to live node
    #[wasm_bindgen]
    pub fn is_connected(&self) -> bool {
        self.connected && self.connection.as_ref().map_or(false, |c| c.is_connected())
    }

    /// Connect to the live node
    #[wasm_bindgen]
    pub async fn connect(&mut self) -> Result<bool, JsValue> {
        console_log!("Connecting to live node at {}", self.node_url);

        // Create WebSocket connection
        let connection = LiveWebSocketConnection::new(&self.node_url)
            .await
            .map_err(|e| JsValue::from_str(&format!("Connection failed: {}", e)))?;

        // Set up event streaming
        let (sender, receiver) = mpsc::unbounded();

        // Store receiver for event processing
        self.event_receiver = Some(receiver);
        self.connection = Some(connection);

        // Set up message handler
        if let Some(ref mut conn) = self.connection {
            conn.set_message_handler(sender);
        }

        // Authenticate if token is provided
        if let Some(ref token) = self.auth_token {
            self.authenticate(token.clone()).await?;
        }

        self.connected = true;
        console_log!("Successfully connected to live node");
        Ok(true)
    }

    /// Authenticate with the live node
    async fn authenticate(&mut self, token: String) -> Result<(), JsValue> {
        if let Some(ref connection) = self.connection {
            connection
                .authenticate(&token)
                .await
                .map_err(|e| JsValue::from_str(&format!("Authentication failed: {}", e)))?;
            console_log!("Authenticated with live node");
        }
        Ok(())
    }

    /// Subscribe to specific event types
    #[wasm_bindgen]
    pub async fn subscribe(&mut self, event_types: Vec<String>) -> Result<(), JsValue> {
        if let Some(ref connection) = self.connection {
            connection
                .subscribe(&event_types)
                .await
                .map_err(|e| JsValue::from_str(&format!("Subscription failed: {}", e)))?;
            console_log!("Subscribed to event types: {:?}", event_types);
        }
        Ok(())
    }

    /// Send command to live node
    #[wasm_bindgen]
    pub async fn send_command(&mut self, command_js: JsValue) -> Result<JsValue, JsValue> {
        let command: ConsoleCommand = from_value(command_js)
            .map_err(|e| JsValue::from_str(&format!("Invalid command: {}", e)))?;

        if let Some(ref connection) = self.connection {
            let message = ClientMessage::Command {
                id: uuid::Uuid::new_v4().to_string(),
                command,
            };
            let message_json = serde_json::to_string(&message)
                .map_err(|e| JsValue::from_str(&format!("Serialization failed: {}", e)))?;

            connection
                .send_command(&message_json)
                .await
                .map_err(|e| JsValue::from_str(&format!("Send failed: {}", e)))?;

            // For now, return a simple acknowledgment
            let response = ConsoleResponse::Status {
                simulation_info: aura_console_types::SimulationInfo {
                    id: uuid::Uuid::new_v4(),
                    current_tick: 0,
                    current_time: js_sys::Date::now() as u64,
                    seed: 42,
                    is_recording: true,
                },
            };
            to_value(&response)
                .map_err(|e| JsValue::from_str(&format!("Response serialization failed: {}", e)))
        } else {
            Err(JsValue::from_str("Not connected"))
        }
    }

    /// Set event callback for receiving live events
    #[wasm_bindgen]
    pub fn set_event_callback(&mut self, callback: js_sys::Function) {
        self.event_callback = Some(callback);

        // Start processing events if we have a receiver
        if self.event_receiver.is_some() {
            self.start_event_processing();
        }
    }

    /// Start processing events from the live node
    fn start_event_processing(&mut self) {
        if let (Some(mut receiver), Some(callback)) =
            (self.event_receiver.take(), self.event_callback.clone())
        {
            spawn_local(async move {
                while let Some(message) = receiver.next().await {
                    // Try to parse as TraceEvent
                    match serde_json::from_str::<TraceEvent>(&message) {
                        Ok(event) => {
                            if let Ok(event_js) = to_value(&event) {
                                let _ = callback.call1(&JsValue::NULL, &event_js);
                            }
                        }
                        Err(e) => {
                            console_error!("Failed to parse live event: {}", e);
                        }
                    }
                }
            });
        }
    }

    /// Disconnect from live node
    #[wasm_bindgen]
    pub fn disconnect(&mut self) {
        console_log!("Disconnecting from live node");

        if let Some(connection) = self.connection.take() {
            connection.close();
        }

        self.connected = false;
        self.event_receiver = None;
        self.event_callback = None;
    }

    /// Get connection status info
    #[wasm_bindgen]
    pub fn get_status(&self) -> JsValue {
        let status = serde_json::json!({
            "connected": self.is_connected(),
            "node_url": self.node_url,
            "has_auth_token": self.auth_token.is_some(),
            "device_id": self.device_id,
            "ready_state": self.connection.as_ref().map(|c| c.ready_state()).unwrap_or(3)
        });

        to_value(&status).unwrap_or(JsValue::NULL)
    }

    /// Create a DKD command for live testing
    #[wasm_bindgen]
    pub fn create_dkd_command(&self, participants: Vec<String>, context: String) -> JsValue {
        let command = ConsoleCommand::InitiateDkd {
            participants,
            context,
        };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Create a step command for live node debugging
    #[wasm_bindgen]
    pub fn create_step_command(&self, count: u64) -> JsValue {
        let command = ConsoleCommand::Step { count };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Create a checkpoint command for live state capture
    #[wasm_bindgen]
    pub fn create_checkpoint_command(&self, label: Option<String>) -> JsValue {
        let command = ConsoleCommand::Checkpoint { label };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Create an inject message command for live testing
    #[wasm_bindgen]
    pub fn create_inject_command(&self, to: String, message: String) -> JsValue {
        let command = ConsoleCommand::InjectMessage { to, message };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Get current device ID
    #[wasm_bindgen]
    pub fn device_id(&self) -> Option<String> {
        self.device_id.clone()
    }

    /// Test JSON serialization of commands
    #[wasm_bindgen]
    pub fn test_command_serialization(&self) -> String {
        let command = ConsoleCommand::InitiateDkd {
            participants: vec!["test_device".to_string()],
            context: "test_context".to_string(),
        };
        serde_json::to_string(&command).unwrap_or_else(|e| format!("Error: {}", e))
    }

    /// Ping the live node
    #[wasm_bindgen]
    pub async fn ping(&mut self) -> Result<String, JsValue> {
        if let Some(ref connection) = self.connection {
            let ping_message =
                serde_json::json!({"type": "ping", "timestamp": js_sys::Date::now()});
            connection
                .send_command(&ping_message.to_string())
                .await
                .map_err(|e| JsValue::from_str(&format!("Ping failed: {}", e)))?;
            Ok("Ping sent".to_string())
        } else {
            Err(JsValue::from_str("Not connected"))
        }
    }
}

// Allow automatic cleanup
impl Drop for LiveNetworkClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_live_client_creation() {
        let client = LiveNetworkClient::new(
            "ws://localhost:9002".to_string(),
            Some("test_token".to_string()),
        );
        assert_eq!(client.node_url(), "ws://localhost:9002");
        assert!(!client.is_connected());
    }

    #[wasm_bindgen_test]
    fn test_command_creation() {
        let client = LiveNetworkClient::new("ws://localhost:9002".to_string(), None);

        let dkd_command =
            client.create_dkd_command(vec!["device1".to_string()], "ctx1".to_string());
        assert!(!dkd_command.is_null());

        let step_command = client.create_step_command(5);
        assert!(!step_command.is_null());
    }

    #[wasm_bindgen_test]
    fn test_serialization() {
        let client = LiveNetworkClient::new("ws://localhost:9002".to_string(), None);

        let json = client.test_command_serialization();
        assert!(json.contains("InitiateDkd"));
        assert!(json.contains("test_device"));
        assert!(json.contains("test_context"));
    }

    #[wasm_bindgen_test]
    fn test_status() {
        let client = LiveNetworkClient::new(
            "ws://localhost:9002".to_string(),
            Some("test_token".to_string()),
        );

        let status = client.get_status();
        assert!(!status.is_null());
    }
}
