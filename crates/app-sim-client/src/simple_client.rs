//! Simplified simulation client for WASM

use app_console_types::{ClientMessage, ConsoleCommand};
use serde_wasm_bindgen::{from_value, to_value};
use wasm_bindgen::prelude::*;

use crate::event_buffer::EventBuffer;
use app_wasm::console_log;

/// Simplified simulation client that avoids callback complexity
#[wasm_bindgen]
pub struct SimpleSimulationClient {
    server_url: String,
    event_buffer: EventBuffer,
    connected: bool,
}

#[wasm_bindgen]
impl SimpleSimulationClient {
    /// Create a new simple simulation client
    #[wasm_bindgen(constructor)]
    pub fn new(server_url: String) -> SimpleSimulationClient {
        console_log!("Creating SimpleSimulationClient for {}", server_url);

        SimpleSimulationClient {
            server_url,
            event_buffer: EventBuffer::new(),
            connected: false,
        }
    }

    /// Get server URL
    #[wasm_bindgen(getter)]
    pub fn server_url(&self) -> String {
        self.server_url.clone()
    }

    /// Check if connected
    #[wasm_bindgen]
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Simulate connection (for testing)
    #[wasm_bindgen]
    pub fn connect(&mut self) -> bool {
        console_log!("Connecting to {}", self.server_url);
        self.connected = true;
        true
    }

    /// Disconnect
    #[wasm_bindgen]
    pub fn disconnect(&mut self) {
        console_log!("Disconnecting");
        self.connected = false;
    }

    /// Create a command JSON (for testing)
    #[wasm_bindgen]
    pub fn create_step_command(&self, count: u64) -> JsValue {
        let command = ConsoleCommand::Step { count };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Validate a command (for testing)
    #[wasm_bindgen]
    pub fn validate_command(&self, command_js: JsValue) -> bool {
        from_value::<ConsoleCommand>(command_js).is_ok()
    }

    /// Get buffer stats
    #[wasm_bindgen]
    pub fn get_buffer_stats(&self) -> JsValue {
        let stats = self.event_buffer.get_stats();
        to_value(&stats).unwrap_or(JsValue::NULL)
    }

    /// Clear event buffer
    #[wasm_bindgen]
    pub fn clear_buffer(&mut self) {
        self.event_buffer.clear();
    }

    /// Get event count
    #[wasm_bindgen]
    pub fn event_count(&self) -> usize {
        self.event_buffer.len()
    }

    /// Create export scenario command
    #[wasm_bindgen]
    pub fn create_export_command(&self, branch_id: String, filename: String) -> JsValue {
        let command = ConsoleCommand::ExportScenario {
            branch_id,
            filename,
        };
        to_value(&command).unwrap_or(JsValue::NULL)
    }

    /// Create subscription message
    #[wasm_bindgen]
    pub fn create_subscribe_message(&self, event_types: Vec<String>) -> JsValue {
        let message = ClientMessage::Subscribe { event_types };
        to_value(&message).unwrap_or(JsValue::NULL)
    }

    /// Test JSON serialization
    #[wasm_bindgen]
    pub fn test_serialization(&self) -> String {
        let command = ConsoleCommand::Step { count: 5 };
        serde_json::to_string(&command).unwrap_or_else(|e| format!("Error: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_simple_client_creation() {
        let client = SimpleSimulationClient::new("ws://localhost:9001".to_string());
        assert_eq!(client.server_url(), "ws://localhost:9001");
        assert!(!client.is_connected());
    }

    #[wasm_bindgen_test]
    fn test_connection() {
        let mut client = SimpleSimulationClient::new("ws://localhost:9001".to_string());
        assert!(client.connect());
        assert!(client.is_connected());

        client.disconnect();
        assert!(!client.is_connected());
    }

    #[wasm_bindgen_test]
    fn test_command_creation() {
        let client = SimpleSimulationClient::new("ws://localhost:9001".to_string());
        let command = client.create_step_command(3);
        assert!(!command.is_null());
        assert!(client.validate_command(command));
    }

    #[wasm_bindgen_test]
    fn test_serialization() {
        let client = SimpleSimulationClient::new("ws://localhost:9001".to_string());
        let json = client.test_serialization();
        assert!(json.contains("Step"));
        assert!(json.contains("\"count\":5"));
    }
}
