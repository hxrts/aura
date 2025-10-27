//! Aura Simulation Client WASM Module
//!
//! WebSocket client for browser-to-simulation-server communication.
//! Provides event streaming, command interface, and efficient buffering
//! for the Aura Dev Console frontend.

use wasm_bindgen::prelude::*;
use wasm_core::{
    console_log, initialize_wasm, ClientMode, SimulationHandler, UnifiedWebSocketClient,
};

mod event_buffer;
mod simple_client;

pub use event_buffer::EventBuffer;
pub use simple_client::SimpleSimulationClient;

// Initialize WASM using foundation
#[wasm_bindgen(start)]
pub fn main() {
    initialize_wasm();
}

/// Enhanced simulation client using unified foundation
#[wasm_bindgen]
pub struct SimulationClient {
    websocket: UnifiedWebSocketClient,
    handler: SimulationHandler,
}

#[wasm_bindgen]
impl SimulationClient {
    /// Create new simulation client
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Result<SimulationClient, wasm_bindgen::JsValue> {
        let websocket = UnifiedWebSocketClient::new("simulation", url).map_err(|e| e.into())?;
        let handler = SimulationHandler::new();

        Ok(SimulationClient { websocket, handler })
    }

    /// Connect to simulation server
    pub fn connect(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        console_log!("Connecting simulation client using unified foundation");
        self.websocket.connect().map_err(|e| e.into())
    }

    /// Send command to simulation server
    pub fn send(&self, message: &str) -> Result<(), wasm_bindgen::JsValue> {
        self.websocket.send(message).map_err(|e| e.into())
    }

    /// Close connection
    pub fn close(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        self.websocket.close().map_err(|e| e.into())
    }

    /// Check connection status
    pub fn is_connected(&self) -> bool {
        self.websocket.is_connected()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_simulation_client_creation() {
        console_log!("Testing simulation client with unified foundation");
        let client = SimulationClient::new("ws://localhost:8080");
        assert!(client.is_ok());
    }
}
