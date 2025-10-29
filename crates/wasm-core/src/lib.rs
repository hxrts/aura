//! Aura WASM Foundation
//!
//! Unified foundation for all WASM clients, providing shared setup,
//! logging, error handling, and WebSocket communication infrastructure.

pub mod error;
pub mod handlers;
pub mod logging;
pub mod setup;
pub mod websocket;

#[cfg(feature = "leptos")]
pub mod leptos;

pub use error::{WasmError, WasmResult};
pub use handlers::{AnalysisHandler, LiveNetworkHandler, SimulationHandler};
pub use setup::{init_manually, initialize_wasm};
pub use websocket::{
    ClientHandler, ClientMode, MessageEnvelope, UnifiedWebSocketClient, WebSocketClientJs,
};

#[cfg(feature = "leptos")]
pub use leptos::{use_reactive_websocket, ConnectionState, ReactiveWebSocketClient};

// Re-export commonly used types
pub use serde::{Deserialize, Serialize};
pub use wasm_bindgen::JsValue;
