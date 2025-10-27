//! Main simulation client interface

use anyhow::{anyhow, Result};
use aura_console_types::{
    ClientMessage, ConsoleCommand, ConsoleEvent, ConsoleResponse, ServerMessage,
};
use futures::channel::mpsc;
use futures::StreamExt;
use serde_wasm_bindgen::{from_value, to_value};
use std::collections::HashMap;
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::{console_error, console_log, console_warn, log};
use crate::event_buffer::EventBuffer;
use crate::websocket::WebSocketConnection;

/// JavaScript callback function for handling events
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(event: any) => void")]
    pub type EventCallback;

    #[wasm_bindgen(method, structural, js_name = "call")]
    fn call1(this: &EventCallback, arg1: &JsValue);
}

/// JavaScript callback function for handling responses
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(response: any) => void")]
    pub type ResponseCallback;

    #[wasm_bindgen(method, structural, js_name = "call")]
    fn call1(this: &ResponseCallback, arg1: &JsValue);
}

/// JavaScript callback function for handling connection status
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "(connected: boolean) => void")]
    pub type ConnectionCallback;

    #[wasm_bindgen(method, structural, js_name = "call")]
    fn call1(this: &ConnectionCallback, arg1: bool);
}

/// Main simulation client for browser communication
#[wasm_bindgen]
pub struct SimulationClient {
    websocket: Option<WebSocketConnection>,
    event_buffer: EventBuffer,
    pending_commands: HashMap<String, ResponseCallback>,
    event_callback: Option<EventCallback>,
    connection_callback: Option<ConnectionCallback>,
    server_url: String,
    current_branch_id: Option<Uuid>,
}

#[wasm_bindgen]
impl SimulationClient {
    /// Create a new simulation client
    #[wasm_bindgen(constructor)]
    pub fn new(server_url: String) -> SimulationClient {
        console_log!("Creating new SimulationClient for {}", server_url);
        
        SimulationClient {
            websocket: None,
            event_buffer: EventBuffer::new(),
            pending_commands: HashMap::new(),
            event_callback: None,
            connection_callback: None,
            server_url,
            current_branch_id: None,
        }
    }

    /// Connect to the simulation server
    #[wasm_bindgen]
    pub async fn connect(&mut self) -> Result<(), JsValue> {
        console_log!("Connecting to simulation server at {}", self.server_url);

        let ws_url = if self.server_url.starts_with("ws://") || self.server_url.starts_with("wss://") {
            format!("{}/ws", self.server_url)
        } else {
            format!("ws://{}/ws", self.server_url)
        };

        let websocket = WebSocketConnection::new(&ws_url).await
            .map_err(|e| JsValue::from_str(&format!("Failed to connect: {}", e)))?;

        // Set up message handling
        let (tx, mut rx) = mpsc::unbounded();
        websocket.set_message_handler(tx);

        // Note: In a full implementation, we would set up proper message handling
        // For now, we'll handle this differently to avoid callback cloning issues

        spawn_local(async move {
            while let Some(message) = rx.next().await {
                if let Err(e) = Self::handle_server_message(
                    message,
                    &event_callback,
                    &mut pending_commands,
                    &mut event_buffer,
                ).await {
                    console_error!("Error handling server message: {}", e);
                }
            }

            // Connection closed
            if let Some(callback) = &connection_callback {
                callback.call1(false);
            }
        });

        self.websocket = Some(websocket);

        // Notify connection success
        if let Some(callback) = &self.connection_callback {
            callback.call1(true);
        }

        console_log!("Successfully connected to simulation server");
        Ok(())
    }

    /// Disconnect from the simulation server
    #[wasm_bindgen]
    pub fn disconnect(&mut self) {
        if let Some(websocket) = self.websocket.take() {
            websocket.close();
            console_log!("Disconnected from simulation server");
        }

        if let Some(callback) = &self.connection_callback {
            callback.call1(false);
        }
    }

    /// Send a command to the simulation server
    #[wasm_bindgen]
    pub async fn send_command(
        &mut self,
        command: JsValue,
        callback: ResponseCallback,
    ) -> Result<(), JsValue> {
        let websocket = self.websocket.as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected to server"))?;

        // Parse command from JavaScript
        let console_command: ConsoleCommand = from_value(command)
            .map_err(|e| JsValue::from_str(&format!("Invalid command: {}", e)))?;

        // Generate unique command ID
        let command_id = Uuid::new_v4().to_string();

        // Store callback for response
        self.pending_commands.insert(command_id.clone(), callback);

        // Create client message
        let client_message = ClientMessage::Command {
            id: command_id.clone(),
            command: console_command,
        };

        // Send message
        let message_json = serde_json::to_string(&client_message)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;

        websocket.send_text(&message_json).await
            .map_err(|e| JsValue::from_str(&format!("Send error: {}", e)))?;

        console_log!("Sent command {} to server", command_id);
        Ok(())
    }

    /// Subscribe to event types
    #[wasm_bindgen]
    pub async fn subscribe(&self, event_types: Vec<String>) -> Result<(), JsValue> {
        let websocket = self.websocket.as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected to server"))?;

        let client_message = ClientMessage::Subscribe { event_types };

        let message_json = serde_json::to_string(&client_message)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;

        websocket.send_text(&message_json).await
            .map_err(|e| JsValue::from_str(&format!("Send error: {}", e)))?;

        console_log!("Subscribed to events");
        Ok(())
    }

    /// Unsubscribe from event types
    #[wasm_bindgen]
    pub async fn unsubscribe(&self, event_types: Vec<String>) -> Result<(), JsValue> {
        let websocket = self.websocket.as_ref()
            .ok_or_else(|| JsValue::from_str("Not connected to server"))?;

        let client_message = ClientMessage::Unsubscribe { event_types };

        let message_json = serde_json::to_string(&client_message)
            .map_err(|e| JsValue::from_str(&format!("Serialization error: {}", e)))?;

        websocket.send_text(&message_json).await
            .map_err(|e| JsValue::from_str(&format!("Send error: {}", e)))?;

        console_log!("Unsubscribed from events");
        Ok(())
    }

    /// Set event callback for receiving real-time events
    #[wasm_bindgen]
    pub fn set_event_callback(&mut self, callback: Option<EventCallback>) {
        self.event_callback = callback;
        console_log!("Event callback set");
    }

    /// Set connection status callback
    #[wasm_bindgen]
    pub fn set_connection_callback(&mut self, callback: Option<ConnectionCallback>) {
        self.connection_callback = callback;
        console_log!("Connection callback set");
    }

    /// Get buffered events since a specific event ID
    #[wasm_bindgen]
    pub fn get_events_since(&self, since_event_id: Option<u64>) -> JsValue {
        let events = self.event_buffer.get_events_since(since_event_id);
        to_value(&events).unwrap_or(JsValue::NULL)
    }

    /// Get current connection status
    #[wasm_bindgen]
    pub fn is_connected(&self) -> bool {
        self.websocket.as_ref().map_or(false, |ws| ws.is_connected())
    }

    /// Get current branch ID
    #[wasm_bindgen]
    pub fn current_branch_id(&self) -> Option<String> {
        self.current_branch_id.map(|id| id.to_string())
    }

    /// Clear the event buffer
    #[wasm_bindgen]
    pub fn clear_event_buffer(&mut self) {
        self.event_buffer.clear();
        console_log!("Event buffer cleared");
    }

    /// Get event buffer statistics
    #[wasm_bindgen]
    pub fn get_buffer_stats(&self) -> JsValue {
        let stats = self.event_buffer.get_stats();
        to_value(&stats).unwrap_or(JsValue::NULL)
    }
}

impl SimulationClient {
    /// Handle incoming server messages
    async fn handle_server_message(
        message: String,
        event_callback: &Option<EventCallback>,
        pending_commands: &mut HashMap<String, ResponseCallback>,
        event_buffer: &mut EventBuffer,
    ) -> Result<()> {
        let server_message: ServerMessage = serde_json::from_str(&message)
            .map_err(|e| anyhow!("Failed to parse server message: {}", e))?;

        match server_message {
            ServerMessage::Response { id, response } => {
                // Handle command response
                if let Some(callback) = pending_commands.remove(&id) {
                    let response_js = to_value(&response)
                        .map_err(|e| anyhow!("Failed to serialize response: {}", e))?;
                    callback.call1(&response_js);
                } else {
                    console_warn!("Received response for unknown command: {}", id);
                }
            }
            ServerMessage::Event(event) => {
                // Handle real-time event
                Self::handle_console_event(&event, event_callback, event_buffer)?;
            }
        }

        Ok(())
    }

    /// Handle console events
    fn handle_console_event(
        event: &ConsoleEvent,
        event_callback: &Option<EventCallback>,
        event_buffer: &mut EventBuffer,
    ) -> Result<()> {
        match event {
            ConsoleEvent::TraceEvent { event: trace_event } => {
                // Buffer the trace event
                event_buffer.add_event(trace_event.clone());

                // Notify callback if set
                if let Some(callback) = event_callback {
                    let event_js = to_value(event)
                        .map_err(|e| anyhow!("Failed to serialize event: {}", e))?;
                    callback.call1(&event_js);
                }
            }
            ConsoleEvent::BranchSwitched { new_branch_id, .. } => {
                console_log!("Switched to branch: {}", new_branch_id);

                // Notify callback if set
                if let Some(callback) = event_callback {
                    let event_js = to_value(event)
                        .map_err(|e| anyhow!("Failed to serialize event: {}", e))?;
                    callback.call1(&event_js);
                }
            }
            ConsoleEvent::SubscriptionChanged { subscribed, unsubscribed } => {
                console_log!("Subscription changed: +{:?} -{:?}", subscribed, unsubscribed);
            }
            ConsoleEvent::SimulationStateChanged { branch_id, new_tick, new_time } => {
                console_log!("Simulation state changed: branch={}, tick={}, time={}", 
                            branch_id, new_tick, new_time);

                // Notify callback if set
                if let Some(callback) = event_callback {
                    let event_js = to_value(event)
                        .map_err(|e| anyhow!("Failed to serialize event: {}", e))?;
                    callback.call1(&event_js);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_client_creation() {
        let client = SimulationClient::new("ws://localhost:9001".to_string());
        assert!(!client.is_connected());
        assert_eq!(client.server_url, "ws://localhost:9001");
    }

    #[wasm_bindgen_test]
    fn test_event_buffer() {
        let client = SimulationClient::new("ws://localhost:9001".to_string());
        let stats = client.get_buffer_stats();
        assert!(!stats.is_null());
    }
}