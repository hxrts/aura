//! Integration tests for WASM foundation WebSocket and client modes

use serde_json::json;
use wasm_bindgen_test::*;
use wasm_core::websocket::{ClientMode, MessageEnvelope, UnifiedWebSocketClient};

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_client_modes() {
    assert_ne!(ClientMode::Simulation, ClientMode::LiveNetwork);
    assert_ne!(ClientMode::LiveNetwork, ClientMode::Analysis);
    assert_ne!(ClientMode::Analysis, ClientMode::Simulation);
}

#[wasm_bindgen_test]
fn test_message_envelope() {
    let envelope = MessageEnvelope::new("test", json!({"key": "value"}));
    let json_str = envelope.to_json().unwrap();
    let parsed = MessageEnvelope::from_json(&json_str).unwrap();

    assert_eq!(envelope.message_type, parsed.message_type);
    assert_eq!(envelope.payload, parsed.payload);
}

#[wasm_bindgen_test]
fn test_unified_websocket_creation() {
    let client = UnifiedWebSocketClient::new("simulation", "ws://localhost:8080").unwrap();
    assert_eq!(client.mode_name(), "Simulation");
    assert!(!client.is_connected());
}
