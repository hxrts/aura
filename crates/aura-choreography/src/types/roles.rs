//! Role definitions compatible with rumpsteak-aura `choreography!` macro

use aura_types::DeviceId;
use rumpsteak_aura::Role;
use serde::{Deserialize, Serialize};

/// Basic choreographic role representing a device in the protocol
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum ChoreographicRole {
    /// A participant identified by an index
    Participant(usize),
    /// The coordinator/leader role
    Coordinator,
    /// A specific device (for device-specific protocols)
    Device(DeviceId),
}

impl ChoreographicRole {
    /// Get the device ID for this role (if applicable)
    pub fn device_id(&self) -> Option<DeviceId> {
        match self {
            ChoreographicRole::Device(id) => Some(*id),
            _ => None,
        }
    }

    /// Create a participant role
    pub fn participant(index: usize) -> Self {
        ChoreographicRole::Participant(index)
    }

    /// Create a coordinator role
    pub fn coordinator() -> Self {
        ChoreographicRole::Coordinator
    }

    /// Create a device role
    pub fn device(device_id: DeviceId) -> Self {
        ChoreographicRole::Device(device_id)
    }
}

/// Threshold-specific roles for cryptographic protocols
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum ThresholdRole {
    /// A participant in the threshold protocol
    Participant(usize),
    /// The coordinator managing the threshold protocol
    Coordinator,
}

impl ThresholdRole {
    /// Create a participant role
    pub fn participant(index: usize) -> Self {
        ThresholdRole::Participant(index)
    }

    /// Create a coordinator role
    pub fn coordinator() -> Self {
        ThresholdRole::Coordinator
    }
}

/// FROST-specific roles for threshold signatures
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum FrostRole {
    /// A signer in the FROST protocol
    Signer(usize),
    /// The aggregator collecting signatures
    Aggregator,
}

impl FrostRole {
    /// Create a signer role
    pub fn signer(index: usize) -> Self {
        FrostRole::Signer(index)
    }

    /// Create an aggregator role
    pub fn aggregator() -> Self {
        FrostRole::Aggregator
    }
}

// Implement Role trait for all role types when rumpsteak-aura is available
// Note: Role trait implementation may be provided by derive macros in rumpsteak-aura
