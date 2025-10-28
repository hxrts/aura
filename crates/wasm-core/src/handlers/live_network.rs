//! Live network client handler

use crate::error::{WasmError, WasmResult};
use crate::websocket::{ClientHandler, MessageEnvelope};
use crate::{console_error, console_log};

/// Handler for live network node communication
pub struct LiveNetworkHandler {
    // State specific to live network client
    pub connected: bool,
    pub node_id: Option<String>,
}

impl LiveNetworkHandler {
    pub fn new() -> Self {
        Self {
            connected: false,
            node_id: None,
        }
    }
}

impl Default for LiveNetworkHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientHandler for LiveNetworkHandler {
    fn handle_message(&mut self, data: &str) -> WasmResult<()> {
        console_log!("[LiveNetwork] Received message: {}", data);

        let envelope = MessageEnvelope::from_json(data)?;

        match envelope.message_type.as_str() {
            "trace_event" => {
                // Handle live trace events
                console_log!("[LiveNetwork] Processing trace event");
                Ok(())
            }
            "node_status" => {
                // Handle node status updates
                console_log!("[LiveNetwork] Processing node status");
                Ok(())
            }
            "command_response" => {
                // Handle command responses
                console_log!("[LiveNetwork] Processing command response");
                Ok(())
            }
            "error" => {
                console_error!("[LiveNetwork] Node error: {:?}", envelope.payload);
                Err(WasmError::Protocol("Node reported error".to_string()))
            }
            _ => {
                console_log!(
                    "[LiveNetwork] Unknown message type: {}",
                    envelope.message_type
                );
                Ok(())
            }
        }
    }

    fn handle_connected(&mut self) -> WasmResult<()> {
        console_log!("[LiveNetwork] Connected to live network node");
        self.connected = true;
        Ok(())
    }

    fn handle_disconnected(&mut self, code: u16, reason: &str) -> WasmResult<()> {
        console_log!("[LiveNetwork] Disconnected: {} - {}", code, reason);
        self.connected = false;
        Ok(())
    }

    fn handle_error(&mut self, error: &str) -> WasmResult<()> {
        console_error!("[LiveNetwork] WebSocket error: {}", error);
        self.connected = false;
        Ok(())
    }
}
