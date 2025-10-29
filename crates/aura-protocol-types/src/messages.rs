//! Protocol messages

use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

/// Result of a DKD (Deterministic Key Derivation) session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkdResult {
    pub app_id: String,
    pub context_label: String,
    pub derived_public_key: Vec<u8>,
    pub proof: Vec<u8>,
}

/// Events that can be received from the transport layer
#[derive(Debug, Clone)]
pub enum TransportEvent {
    /// Device connected
    DeviceConnected(DeviceId),
    /// Device disconnected
    DeviceDisconnected(DeviceId),
    /// Message received from a device
    MessageReceived { from: DeviceId, message: Vec<u8> },
    /// Transport error occurred
    TransportError(String),
}
