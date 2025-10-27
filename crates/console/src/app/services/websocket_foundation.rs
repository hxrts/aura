//! WebSocket Client Service using wasm-core
//!
//! Refactored WebSocket service that uses the unified wasm-core
//! infrastructure for consistent WASM client behavior.

use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use wasm_core::{use_reactive_websocket, ConnectionState, MessageEnvelope};

/// Console command types that can be sent to the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConsoleCommand {
    QueryState {
        device_id: String,
    },
    GetEvents {
        since_event_id: Option<u64>,
        limit: Option<usize>,
    },
    GetNetworkTopology,
    GetDevices,
    GetStats,
    SetRecording {
        enabled: bool,
    },
    ClearEvents,
    Subscribe {
        event_types: Vec<String>,
    },
    Unsubscribe,
}

/// Response from the instrumentation server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
pub enum ConsoleResponse {
    Success {
        request_id: Option<String>,
        data: serde_json::Value,
    },
    Error {
        request_id: Option<String>,
        error: String,
        details: Option<serde_json::Value>,
    },
    Event {
        event: serde_json::Value,
    },
    Status {
        message: String,
        timestamp: u64,
    },
}

/// Hook for using WebSocket service in Leptos components
pub fn use_websocket_foundation(
    url: String,
) -> (
    ReadSignal<ConnectionState>,
    ReadSignal<VecDeque<MessageEnvelope>>,
    ReadSignal<VecDeque<serde_json::Value>>,
    impl Fn(ConsoleCommand) + Clone,
    impl Fn() + Clone,
    impl Fn() + Clone,
) {
    // Use the reactive WebSocket hook from wasm-core
    let (connection_state, messages, responses, send_message, connect, disconnect) =
        use_reactive_websocket("simulation", url);

    // Convert ConsoleCommand to MessageEnvelope and send
    let send_command = {
        let send_message = send_message.clone();
        move |command: ConsoleCommand| {
            let payload = serde_json::to_value(&command).unwrap_or_default();
            let envelope = MessageEnvelope::new("console_command", payload);
            send_message(&envelope);
        }
    };

    (
        connection_state,
        messages,
        responses,
        send_command,
        connect,
        disconnect,
    )
}

/// Connection status component using wasm-core types
#[component]
pub fn ConnectionStatus(connection_state: ReadSignal<ConnectionState>) -> impl IntoView {
    view! {
        <div class="connection-status">
            {move || {
                match connection_state.get() {
                    ConnectionState::Connected => view! {
                        <span class="status-indicator connected">
                            "Connected"
                        </span>
                    }.into_any(),
                    ConnectionState::Connecting => view! {
                        <span class="status-indicator connecting">
                            "Connecting..."
                        </span>
                    }.into_any(),
                    ConnectionState::Reconnecting => view! {
                        <span class="status-indicator reconnecting">
                            "Reconnecting..."
                        </span>
                    }.into_any(),
                    ConnectionState::Disconnected => view! {
                        <span class="status-indicator disconnected">
                            "Disconnected"
                        </span>
                    }.into_any(),
                    ConnectionState::Error(msg) => view! {
                        <span class="status-indicator error" title={msg}>
                            "Error"
                        </span>
                    }.into_any(),
                }
            }}
        </div>
    }
}
