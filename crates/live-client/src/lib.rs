//! Aura Live Network Client WASM Module
//!
//! WebSocket client for browser-to-live-node communication.
//! Provides real-time trace events and command forwarding for live network nodes.

use wasm_bindgen::prelude::*;
use wasm_core::{console_log, initialize_wasm, LiveNetworkHandler, UnifiedWebSocketClient};

mod client;

pub use client::LiveNetworkClient;

// Initialize WASM using foundation
#[wasm_bindgen(start)]
pub fn main() {
    initialize_wasm();
}

/// Enhanced live network client using unified foundation
#[wasm_bindgen]
pub struct LiveClient {
    websocket: UnifiedWebSocketClient,
    handler: LiveNetworkHandler,
}

#[wasm_bindgen]
impl LiveClient {
    /// Create new live network client
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Result<LiveClient, wasm_bindgen::JsValue> {
        let websocket = UnifiedWebSocketClient::new("live", url).map_err(|e| e.into())?;
        let handler = LiveNetworkHandler::new();

        Ok(LiveClient { websocket, handler })
    }

    /// Connect to live network node
    pub fn connect(&mut self) -> Result<(), wasm_bindgen::JsValue> {
        console_log!("Connecting live network client using unified foundation");
        self.websocket.connect().map_err(|e| e.into())
    }

    /// Send command to live network node
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
    fn test_live_client_creation() {
        console_log!("Testing live network client with unified foundation");
        let client = LiveClient::new("ws://localhost:8080");
        assert!(client.is_ok());
    }
}
