//! Parameter types for effect serialization
//!
//! These types enable serialization/deserialization when calling effects through
//! the type-erased `AuraHandler` interface.

use serde::{Deserialize, Serialize};

use aura_core::identifiers::{DeviceId, SessionId};

use crate::types::ProtocolType;

// ═══════════════════════════════════════════════════════════════════════════
// CryptoEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for generating random bytes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomBytesParams {
    /// Number of random bytes to generate
    pub len: u32,
}

/// Parameters for generating 32 random bytes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomBytes32Params;

/// Parameters for generating random number in range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomRangeParams {
    /// Start of range (inclusive)
    pub start: u64,
    /// End of range (exclusive)
    pub end: u64,
}

/// Parameters for BLAKE3 hashing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Blake3HashParams {
    /// Data to hash
    pub data: Vec<u8>,
}

/// Parameters for SHA-256 hashing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sha256HashParams {
    /// Data to hash
    pub data: Vec<u8>,
}

// ═══════════════════════════════════════════════════════════════════════════
// NetworkEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for sending message to a specific peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendToPeerParams {
    /// ID of the peer to send to
    pub peer_id: DeviceId,
    /// Message payload
    pub message: Vec<u8>,
}

/// Parameters for broadcasting message to all peers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BroadcastParams {
    /// Message payload to broadcast
    pub message: Vec<u8>,
}

/// Parameters for receiving message from a specific peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReceiveFromParams {
    /// ID of the peer to receive from
    pub peer_id: DeviceId,
}

/// Parameters for checking peer connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsPeerConnectedParams {
    /// ID of the peer to check
    pub peer_id: DeviceId,
}

// ═══════════════════════════════════════════════════════════════════════════
// StorageEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Typed storage key for effect parameters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StorageKey(String);

impl StorageKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for StorageKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for StorageKey {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Parameters for storing a key-value pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreParams {
    /// Storage key
    pub key: StorageKey,
    /// Value to store
    pub value: Vec<u8>,
}

/// Parameters for retrieving a value from storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveParams {
    /// Storage key to retrieve
    pub key: StorageKey,
}

/// Parameters for removing a value from storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveParams {
    /// Storage key to remove
    pub key: StorageKey,
}

/// Parameters for listing storage keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeysParams {
    /// Optional prefix filter for keys
    pub prefix: Option<StorageKey>,
}

/// Parameters for checking if a key exists in storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistsParams {
    /// Storage key to check
    pub key: StorageKey,
}

// ═══════════════════════════════════════════════════════════════════════════
// TimeEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for sleeping for a duration in milliseconds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepMsParams {
    /// Duration to sleep in milliseconds
    pub ms: u64,
}

/// Parameters for sleeping until a specific epoch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepUntilParams {
    /// Target epoch to sleep until
    pub epoch: u64,
}

/// Parameters for delay with a duration in milliseconds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayParams {
    /// Duration to delay in milliseconds
    pub duration_ms: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// ConsoleEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for logging a protocol start event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolStartedParams {
    /// Unique ID of the protocol instance
    pub protocol_id: SessionId,
    /// Type of protocol (e.g., "DKD", "FROST")
    pub protocol_type: ProtocolType,
}

/// Parameters for logging a protocol completion event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCompletedParams {
    /// Unique ID of the protocol instance
    pub protocol_id: SessionId,
    /// Duration of protocol execution in milliseconds
    pub duration_ms: u64,
}

/// Parameters for logging a protocol failure event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolFailedParams {
    /// Unique ID of the protocol instance
    pub protocol_id: SessionId,
    /// Error description
    pub error: String,
}

/// Parameters for logging a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogParams {
    /// Message to log
    pub message: String,
}

/// Console event types for structured logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsoleEvent {
    /// Protocol started event
    ProtocolStarted {
        /// Unique identifier for the protocol instance
        protocol_id: SessionId,
        /// Type of protocol (e.g., "DKD", "FROST")
        protocol_type: ProtocolType,
    },
    /// Protocol completed successfully
    ProtocolCompleted {
        /// Unique identifier for the protocol instance
        protocol_id: SessionId,
        /// Duration of execution in milliseconds
        duration_ms: u64,
    },
    /// Protocol failed with error
    ProtocolFailed {
        /// Unique identifier for the protocol instance
        protocol_id: SessionId,
        /// Error description
        error: String,
    },
    /// Custom event with arbitrary data
    Custom {
        /// Type/category of the custom event
        event_type: String,
        /// Event data as JSON value
        data: serde_json::Value,
    },
}

// ═══════════════════════════════════════════════════════════════════════════
// RandomEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for generating a random u64
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomU64Params;

/// Parameters for generating a random u64 in a range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomRangeU64Params {
    /// Minimum value (inclusive)
    pub min: u64,
    /// Maximum value (exclusive)
    pub max: u64,
}
