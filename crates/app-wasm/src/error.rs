//! Unified error handling for WASM clients

use thiserror::Error;
use wasm_bindgen::prelude::*;

/// Unified error type for all WASM operations
#[derive(Error, Debug)]
pub enum WasmError {
    /// WebSocket connection or communication error.
    #[error("WebSocket error: {0}")]
    WebSocket(String),

    /// JSON serialization or deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// JavaScript interop error.
    #[error("JavaScript error: {0}")]
    JavaScript(String),

    /// Client message handler error.
    #[error("Client handler error: {0}")]
    Handler(String),

    /// Network communication error.
    #[error("Network error: {0}")]
    Network(String),

    /// Protocol violation or error.
    #[error("Protocol error: {0}")]
    Protocol(String),
}

impl From<JsValue> for WasmError {
    fn from(js_val: JsValue) -> Self {
        let message = js_val
            .as_string()
            .unwrap_or_else(|| "Unknown JavaScript error".to_string());
        WasmError::JavaScript(message)
    }
}

impl From<WasmError> for JsValue {
    fn from(err: WasmError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

/// Result type for WASM operations
pub type WasmResult<T> = Result<T, WasmError>;
