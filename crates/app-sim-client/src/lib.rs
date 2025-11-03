//! Aura Simulation Client WASM Module
//!
//! WebSocket client for browser-to-simulation-server communication.
//! Provides event streaming, command interface, and efficient buffering
//! for the Aura Dev Console frontend.

use app_wasm::{
    console_log, initialize_wasm, ClientMode, SimulationHandler, UnifiedWebSocketClient,
};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

mod event_buffer;
mod simple_client;

pub use event_buffer::EventBuffer;
pub use simple_client::SimpleSimulationClient;

/// Initialize WASM using foundation
#[wasm_bindgen(start)]
pub fn main() {
    initialize_wasm();
}

/// Enhanced simulation client using unified foundation
#[wasm_bindgen]
pub struct SimulationClient {
    websocket: UnifiedWebSocketClient,
    #[allow(dead_code)]
    handler: SimulationHandler,
}

#[wasm_bindgen]
impl SimulationClient {
    /// Create new simulation client
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Result<SimulationClient, wasm_bindgen::JsValue> {
        let websocket = UnifiedWebSocketClient::new(ClientMode::Simulation, url);
        let handler = SimulationHandler::new();

        Ok(SimulationClient { websocket, handler })
    }

    /// Connect to simulation server
    pub fn connect(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        console_log!("Connecting simulation client using unified foundation");
        let handler = Rc::new(RefCell::new(SimulationHandler::new()));
        self.websocket
            .connect(handler)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Send command to simulation server
    pub fn send(&self, message: &str) -> Result<(), wasm_bindgen::JsValue> {
        self.websocket
            .send(message)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Close connection
    pub fn close(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        self.websocket
            .close()
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Check connection status
    pub fn is_connected(&self) -> bool {
        self.websocket.is_connected_status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    #[wasm_bindgen_test]
    fn test_simulation_client_creation() {
        app_wasm::console_log!("Testing simulation client with unified foundation");
        let client = SimulationClient::new("ws://localhost:8080");
        assert!(client.is_ok());
    }
}
