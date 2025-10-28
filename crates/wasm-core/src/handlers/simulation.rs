//! Simulation client handler

use crate::error::{WasmError, WasmResult};
use crate::websocket::{ClientHandler, MessageEnvelope};
use crate::{console_error, console_log};

/// Handler for simulation server communication
pub struct SimulationHandler {
    // State specific to simulation client
    pub connected: bool,
    pub event_buffer_size: usize,
}

impl SimulationHandler {
    pub fn new() -> Self {
        Self {
            connected: false,
            event_buffer_size: 1000,
        }
    }
}

impl Default for SimulationHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientHandler for SimulationHandler {
    fn handle_message(&mut self, data: &str) -> WasmResult<()> {
        console_log!("[Simulation] Received message: {}", data);

        let envelope = MessageEnvelope::from_json(data)?;

        match envelope.message_type.as_str() {
            "simulation_event" => {
                // Handle simulation events
                console_log!("[Simulation] Processing simulation event");
                Ok(())
            }
            "command_response" => {
                // Handle command responses
                console_log!("[Simulation] Processing command response");
                Ok(())
            }
            "error" => {
                console_error!("[Simulation] Server error: {:?}", envelope.payload);
                Err(WasmError::Protocol("Server reported error".to_string()))
            }
            _ => {
                console_log!(
                    "[Simulation] Unknown message type: {}",
                    envelope.message_type
                );
                Ok(())
            }
        }
    }

    fn handle_connected(&mut self) -> WasmResult<()> {
        console_log!("[Simulation] Connected to simulation server");
        self.connected = true;
        Ok(())
    }

    fn handle_disconnected(&mut self, code: u16, reason: &str) -> WasmResult<()> {
        console_log!("[Simulation] Disconnected: {} - {}", code, reason);
        self.connected = false;
        Ok(())
    }

    fn handle_error(&mut self, error: &str) -> WasmResult<()> {
        console_error!("[Simulation] WebSocket error: {}", error);
        self.connected = false;
        Ok(())
    }
}
