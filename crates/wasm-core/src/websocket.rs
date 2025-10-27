//! Unified WebSocket client infrastructure

use crate::console_log;
use crate::error::{WasmError, WasmResult};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// Client mode determines message handling behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientMode {
    /// Simulation server communication
    Simulation,
    /// Live network node communication
    LiveNetwork,
    /// Trace analysis engine
    Analysis,
}

impl std::fmt::Display for ClientMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                ClientMode::Simulation => "Simulation",
                ClientMode::LiveNetwork => "LiveNetwork",
                ClientMode::Analysis => "Analysis",
            }
        )
    }
}

/// Trait for mode-specific message handling
pub trait ClientHandler {
    /// Handle incoming WebSocket message
    fn handle_message(&mut self, data: &str) -> WasmResult<()>;

    /// Handle WebSocket connection established
    fn handle_connected(&mut self) -> WasmResult<()>;

    /// Handle WebSocket connection closed
    fn handle_disconnected(&mut self, code: u16, reason: &str) -> WasmResult<()>;

    /// Handle WebSocket error
    fn handle_error(&mut self, error: &str) -> WasmResult<()>;
}

/// Default handler that logs messages
pub struct DefaultHandler {
    mode: ClientMode,
}

impl DefaultHandler {
    pub fn new(mode: ClientMode) -> Self {
        Self { mode }
    }
}

impl ClientHandler for DefaultHandler {
    fn handle_message(&mut self, data: &str) -> WasmResult<()> {
        console_log!("[{}] Received message: {}", self.mode, data);
        Ok(())
    }

    fn handle_connected(&mut self) -> WasmResult<()> {
        console_log!("[{}] Connected", self.mode);
        Ok(())
    }

    fn handle_disconnected(&mut self, code: u16, reason: &str) -> WasmResult<()> {
        console_log!("[{}] Disconnected: {} ({})", self.mode, reason, code);
        Ok(())
    }

    fn handle_error(&mut self, error: &str) -> WasmResult<()> {
        console_log!("[{}] Error: {}", self.mode, error);
        Ok(())
    }
}

/// Unified WebSocket client for all WASM clients
pub struct UnifiedWebSocketClient {
    websocket: Option<WebSocket>,
    mode: ClientMode,
    url: String,
    is_connected: bool,
}

impl UnifiedWebSocketClient {
    /// Create new WebSocket client for specified mode
    pub fn new(mode: ClientMode, url: impl Into<String>) -> Self {
        UnifiedWebSocketClient {
            websocket: None,
            mode,
            url: url.into(),
            is_connected: false,
        }
    }

    /// Connect to WebSocket server and set up event handlers
    pub fn connect(&mut self, handler: Rc<RefCell<dyn ClientHandler>>) -> WasmResult<()> {
        console_log!("[{}] Connecting to {}", self.mode, self.url);

        let websocket = WebSocket::new(&self.url)
            .map_err(|e| WasmError::WebSocket(format!("Failed to create WebSocket: {:?}", e)))?;

        websocket.set_binary_type(web_sys::BinaryType::Arraybuffer);

        // Set up onopen handler
        {
            let handler_clone = handler.clone();
            let onopen: Closure<dyn Fn()> = Closure::new(move || {
                if let Err(e) = handler_clone.borrow_mut().handle_connected() {
                    console_log!("Error in handle_connected: {:?}", e);
                }
            });
            websocket.set_onopen(Some(onopen.as_ref().unchecked_ref()));
            onopen.forget();
        }

        // Set up onmessage handler
        {
            let handler_clone = handler.clone();
            let onmessage: Closure<dyn Fn(MessageEvent)> =
                Closure::new(move |event: MessageEvent| {
                    if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                        let message_str = String::from(text);
                        if let Err(e) = handler_clone.borrow_mut().handle_message(&message_str) {
                            console_log!("Error in handle_message: {:?}", e);
                        }
                    }
                });
            websocket.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
            onmessage.forget();
        }

        // Set up onclose handler
        {
            let handler_clone = handler.clone();
            let onclose: Closure<dyn Fn(CloseEvent)> = Closure::new(move |event: CloseEvent| {
                let code = event.code();
                let reason = event.reason();
                if let Err(e) = handler_clone
                    .borrow_mut()
                    .handle_disconnected(code, &reason)
                {
                    console_log!("Error in handle_disconnected: {:?}", e);
                }
            });
            websocket.set_onclose(Some(onclose.as_ref().unchecked_ref()));
            onclose.forget();
        }

        // Set up onerror handler
        {
            let handler_clone = handler;
            let onerror: Closure<dyn Fn(ErrorEvent)> = Closure::new(move |event: ErrorEvent| {
                let error_msg = event.message();
                if let Err(e) = handler_clone.borrow_mut().handle_error(&error_msg) {
                    console_log!("Error in handle_error: {:?}", e);
                }
            });
            websocket.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            onerror.forget();
        }

        self.websocket = Some(websocket);
        self.is_connected = true;
        Ok(())
    }

    /// Send message through WebSocket
    pub fn send(&self, message: &str) -> WasmResult<()> {
        match &self.websocket {
            Some(ws) => {
                ws.send_with_str(message).map_err(|e| {
                    WasmError::WebSocket(format!("Failed to send message: {:?}", e))
                })?;
                Ok(())
            }
            None => Err(WasmError::WebSocket("Not connected".to_string())),
        }
    }

    /// Close WebSocket connection
    pub fn close(&mut self) -> WasmResult<()> {
        if let Some(ws) = &self.websocket {
            ws.close()
                .map_err(|e| WasmError::WebSocket(format!("Failed to close WebSocket: {:?}", e)))?;
        }
        self.websocket = None;
        self.is_connected = false;
        Ok(())
    }

    /// Get connection status
    pub fn is_connected_status(&self) -> bool {
        self.is_connected
            && self
                .websocket
                .as_ref()
                .map(|ws| ws.ready_state() == WebSocket::OPEN)
                .unwrap_or(false)
    }

    /// Get client mode
    pub fn mode(&self) -> ClientMode {
        self.mode
    }

    /// Get client URL
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// JavaScript-bindable wrapper for WebSocket client
#[wasm_bindgen]
pub struct WebSocketClientJs {
    client: Rc<RefCell<UnifiedWebSocketClient>>,
    handler: Rc<RefCell<DefaultHandler>>,
}

#[wasm_bindgen]
impl WebSocketClientJs {
    /// Create new WebSocket client wrapper
    #[wasm_bindgen(constructor)]
    pub fn new(mode_str: &str, url: &str) -> WasmResult<WebSocketClientJs> {
        let mode = match mode_str {
            "simulation" => ClientMode::Simulation,
            "live" => ClientMode::LiveNetwork,
            "analysis" => ClientMode::Analysis,
            _ => {
                return Err(WasmError::Protocol(format!(
                    "Unknown client mode: {}",
                    mode_str
                )))
            }
        };

        Ok(WebSocketClientJs {
            client: Rc::new(RefCell::new(UnifiedWebSocketClient::new(mode, url))),
            handler: Rc::new(RefCell::new(DefaultHandler::new(mode))),
        })
    }

    /// Connect to WebSocket server
    pub fn connect(&self) -> WasmResult<()> {
        let mut client = self.client.borrow_mut();
        client.connect(self.handler.clone() as Rc<RefCell<dyn ClientHandler>>)
    }

    /// Send message through WebSocket
    pub fn send(&self, message: &str) -> WasmResult<()> {
        let client = self.client.borrow();
        client.send(message)
    }

    /// Close WebSocket connection
    pub fn close(&self) -> WasmResult<()> {
        let mut client = self.client.borrow_mut();
        client.close()
    }

    /// Get connection status
    pub fn is_connected(&self) -> bool {
        self.client.borrow().is_connected_status()
    }

    /// Get client mode as string
    pub fn mode_name(&self) -> String {
        self.client.borrow().mode().to_string()
    }

    /// Get client URL
    pub fn get_url(&self) -> String {
        self.client.borrow().url().to_string()
    }
}

/// Generic message envelope for all client modes
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageEnvelope {
    pub message_type: String,
    pub payload: serde_json::Value,
    pub timestamp: Option<u64>,
    pub client_id: Option<String>,
}

impl MessageEnvelope {
    pub fn new(message_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            message_type: message_type.into(),
            payload,
            timestamp: None,
            client_id: None,
        }
    }

    pub fn to_json(&self) -> WasmResult<String> {
        serde_json::to_string(self).map_err(WasmError::from)
    }

    pub fn from_json(data: &str) -> WasmResult<Self> {
        serde_json::from_str(data).map_err(WasmError::from)
    }

    pub fn with_timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_mode_display() {
        assert_eq!(ClientMode::Simulation.to_string(), "Simulation");
        assert_eq!(ClientMode::LiveNetwork.to_string(), "LiveNetwork");
        assert_eq!(ClientMode::Analysis.to_string(), "Analysis");
    }

    #[test]
    fn test_message_envelope_creation() {
        let envelope = MessageEnvelope::new("test_message", serde_json::json!({"key": "value"}));

        assert_eq!(envelope.message_type, "test_message");
        assert_eq!(envelope.timestamp, None);
        assert_eq!(envelope.client_id, None);
    }

    #[test]
    fn test_message_envelope_with_metadata() {
        let envelope = MessageEnvelope::new("test_message", serde_json::json!({"key": "value"}))
            .with_timestamp(12345)
            .with_client_id("client_1");

        assert_eq!(envelope.timestamp, Some(12345));
        assert_eq!(envelope.client_id, Some("client_1".to_string()));
    }

    #[test]
    fn test_message_envelope_serialization() {
        let envelope = MessageEnvelope::new("test_message", serde_json::json!({"key": "value"}))
            .with_timestamp(12345)
            .with_client_id("client_1");

        let json = envelope.to_json().expect("should serialize");
        let deserialized = MessageEnvelope::from_json(&json).expect("should deserialize");

        assert_eq!(deserialized.message_type, "test_message");
        assert_eq!(deserialized.timestamp, Some(12345));
        assert_eq!(deserialized.client_id, Some("client_1".to_string()));
    }
}
