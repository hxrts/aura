//! Console-Compatible Event Types
//!
//! Defines event types that are compatible with the dev console's
//! trace format for seamless integration.

use DeviceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Console event for dev console integration
///
/// These events are designed to be compatible with the console-types
/// crate for WebSocket transmission to the browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ConsoleEvent {
    /// Protocol execution event
    Protocol {
        event_id: u64,
        timestamp: u64,
        session_id: Uuid,
        device_id: DeviceId,
        event_type: String,
        details: Option<serde_json::Value>,
    },
    /// State change event
    StateChange {
        event_id: u64,
        timestamp: u64,
        device_id: DeviceId,
        state_type: String,
        old_state: Option<serde_json::Value>,
        new_state: serde_json::Value,
    },
    /// Network communication event
    Network {
        event_id: u64,
        timestamp: u64,
        from_device: DeviceId,
        to_device: DeviceId,
        message_type: String,
        size_bytes: usize,
    },
    /// Error event
    Error {
        event_id: u64,
        timestamp: u64,
        device_id: DeviceId,
        error_type: String,
        error_message: String,
        context: Option<serde_json::Value>,
    },
}

impl ConsoleEvent {
    /// Get the event ID
    pub fn event_id(&self) -> u64 {
        match self {
            ConsoleEvent::Protocol { event_id, .. } => *event_id,
            ConsoleEvent::StateChange { event_id, .. } => *event_id,
            ConsoleEvent::Network { event_id, .. } => *event_id,
            ConsoleEvent::Error { event_id, .. } => *event_id,
        }
    }

    /// Get the timestamp
    pub fn timestamp(&self) -> u64 {
        match self {
            ConsoleEvent::Protocol { timestamp, .. } => *timestamp,
            ConsoleEvent::StateChange { timestamp, .. } => *timestamp,
            ConsoleEvent::Network { timestamp, .. } => *timestamp,
            ConsoleEvent::Error { timestamp, .. } => *timestamp,
        }
    }

    /// Get the primary device ID associated with this event
    pub fn device_id(&self) -> DeviceId {
        match self {
            ConsoleEvent::Protocol { device_id, .. } => *device_id,
            ConsoleEvent::StateChange { device_id, .. } => *device_id,
            ConsoleEvent::Network { from_device, .. } => *from_device,
            ConsoleEvent::Error { device_id, .. } => *device_id,
        }
    }

    /// Get the event type as a string
    pub fn event_type(&self) -> &str {
        match self {
            ConsoleEvent::Protocol { event_type, .. } => event_type,
            ConsoleEvent::StateChange { state_type, .. } => state_type,
            ConsoleEvent::Network { message_type, .. } => message_type,
            ConsoleEvent::Error { error_type, .. } => error_type,
        }
    }
}

/// Protocol-specific event for detailed protocol tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolEvent {
    pub session_id: Uuid,
    pub device_id: DeviceId,
    pub phase: String,
    pub action: String,
    pub success: bool,
    pub details: serde_json::Value,
}

impl ProtocolEvent {
    /// Create a new protocol event
    pub fn new(
        session_id: Uuid,
        device_id: DeviceId,
        phase: impl Into<String>,
        action: impl Into<String>,
        success: bool,
        details: serde_json::Value,
    ) -> Self {
        Self {
            session_id,
            device_id,
            phase: phase.into(),
            action: action.into(),
            success,
            details,
        }
    }

    /// Create a DKD protocol event
    pub fn dkd(
        session_id: Uuid,
        device_id: DeviceId,
        phase: &str,
        action: &str,
        success: bool,
    ) -> Self {
        Self::new(
            session_id,
            device_id,
            format!("DKD::{}", phase),
            action,
            success,
            serde_json::json!({
                "protocol": "DKD",
                "phase": phase,
                "action": action
            }),
        )
    }

    /// Create a recovery protocol event
    pub fn recovery(
        session_id: Uuid,
        device_id: DeviceId,
        phase: &str,
        action: &str,
        success: bool,
    ) -> Self {
        Self::new(
            session_id,
            device_id,
            format!("Recovery::{}", phase),
            action,
            success,
            serde_json::json!({
                "protocol": "Recovery",
                "phase": phase,
                "action": action
            }),
        )
    }

    /// Create a resharing protocol event
    pub fn resharing(
        session_id: Uuid,
        device_id: DeviceId,
        phase: &str,
        action: &str,
        success: bool,
    ) -> Self {
        Self::new(
            session_id,
            device_id,
            format!("Resharing::{}", phase),
            action,
            success,
            serde_json::json!({
                "protocol": "Resharing",
                "phase": phase,
                "action": action
            }),
        )
    }
}
