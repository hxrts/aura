//! Leptos integration for reactive WebSocket clients

use crate::error::{WasmError, WasmResult};
use crate::websocket::{ClientMode, DefaultHandler, MessageEnvelope, UnifiedWebSocketClient};
#[cfg(feature = "leptos")]
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

/// Connection states for reactive UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Not connected.
    Disconnected,
    /// Connection in progress.
    Connecting,
    /// Connected and ready.
    Connected,
    /// Attempting to reconnect.
    Reconnecting,
    /// Connection error.
    Error(String),
}

/// Reactive WebSocket client for Leptos applications.
#[cfg(feature = "leptos")]
#[allow(dead_code)]
pub struct ReactiveWebSocketClient {
    /// The underlying unified client.
    client: Rc<RefCell<UnifiedWebSocketClient>>,
    /// Signal for connection state.
    connection_state: WriteSignal<ConnectionState>,
    /// Signal for incoming messages.
    messages: WriteSignal<VecDeque<MessageEnvelope>>,
    /// Signal for responses.
    responses: WriteSignal<VecDeque<serde_json::Value>>,
    /// Whether to automatically reconnect on failure.
    auto_reconnect: bool,
    /// Current number of reconnection attempts.
    reconnect_attempts: u32,
    /// Maximum number of reconnection attempts.
    max_reconnect_attempts: u32,
}

#[cfg(feature = "leptos")]
impl ReactiveWebSocketClient {
    /// Creates a new reactive WebSocket client.
    pub fn new(
        mode: &str,
        url: String,
        connection_state: WriteSignal<ConnectionState>,
        messages: WriteSignal<VecDeque<MessageEnvelope>>,
        responses: WriteSignal<VecDeque<serde_json::Value>>,
    ) -> WasmResult<Self> {
        let client_mode = match mode {
            "simulation" => ClientMode::Simulation,
            "live" => ClientMode::LiveNetwork,
            "analysis" => ClientMode::Analysis,
            _ => ClientMode::Simulation,
        };
        let client = UnifiedWebSocketClient::new(client_mode, url);

        Ok(Self {
            client: Rc::new(RefCell::new(client)),
            connection_state,
            messages,
            responses,
            auto_reconnect: true,
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        })
    }

    /// Connects with event handlers set up.
    pub fn connect(&mut self) -> WasmResult<()> {
        self.connection_state.set(ConnectionState::Connecting);

        let handler = Rc::new(RefCell::new(DefaultHandler::new(ClientMode::Simulation)));
        let mut client = self.client.borrow_mut();
        client.connect(handler)?;
        self.connection_state.set(ConnectionState::Connected);

        Ok(())
    }

    /// Sends a message.
    pub fn send_message(&self, envelope: &MessageEnvelope) -> WasmResult<()> {
        let json = envelope.to_json()?;
        self.client.borrow().send(&json)
    }

    /// Sends a typed message.
    pub fn send_typed<T: Serialize>(&self, message_type: &str, payload: &T) -> WasmResult<()> {
        let payload_json = serde_json::to_value(payload).map_err(WasmError::from)?;
        let envelope = MessageEnvelope::new(message_type, payload_json);
        self.send_message(&envelope)
    }

    /// Disconnects from the server.
    pub fn disconnect(&mut self) -> WasmResult<()> {
        self.client.borrow_mut().close()?;
        self.connection_state.set(ConnectionState::Disconnected);
        Ok(())
    }

    /// Checks if currently connected.
    pub fn is_connected(&self) -> bool {
        self.client.borrow().is_connected_status()
    }
}

/// Hook for using reactive WebSocket in Leptos components.
#[cfg(feature = "leptos")]
#[allow(clippy::type_complexity)]
pub fn use_reactive_websocket(
    mode: &str,
    url: String,
) -> (
    ReadSignal<ConnectionState>,
    ReadSignal<VecDeque<MessageEnvelope>>,
    ReadSignal<VecDeque<serde_json::Value>>,
    impl Fn(&MessageEnvelope) + Clone,
    impl Fn() + Clone,
    impl Fn() + Clone,
) {
    let (connection_state, set_connection_state) = signal(ConnectionState::Disconnected);
    let (messages, set_messages) = signal(VecDeque::new());
    let (responses, set_responses) = signal(VecDeque::new());

    let client = Rc::new(RefCell::new(
        ReactiveWebSocketClient::new(mode, url, set_connection_state, set_messages, set_responses)
            .expect("Failed to create WebSocket client"),
    ));

    // Connect function
    let connect = {
        let client = client.clone();
        move || {
            let _ = client.borrow_mut().connect();
        }
    };

    // Disconnect function
    let disconnect = {
        let client = client.clone();
        move || {
            let _ = client.borrow_mut().disconnect();
        }
    };

    // Send message function
    let send_message = {
        let client = client.clone();
        move |envelope: &MessageEnvelope| {
            let _ = client.borrow().send_message(envelope);
        }
    };

    // Auto-connect on mount
    Effect::new({
        let connect = connect.clone();
        move |_| {
            connect();
        }
    });

    (
        connection_state,
        messages,
        responses,
        send_message,
        connect,
        disconnect,
    )
}

#[cfg(not(feature = "leptos"))]
pub struct ConnectionState;
