//! Parameter types for effect serialization
//!
//! These types enable serialization/deserialization when calling effects through
//! the type-erased `AuraHandler` interface.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ═══════════════════════════════════════════════════════════════════════════
// CryptoEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for generating random bytes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RandomBytesParams {
    /// Number of random bytes to generate
    pub len: usize,
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
    pub peer_id: Uuid,
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
    pub peer_id: Uuid,
}

/// Parameters for checking peer connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsPeerConnectedParams {
    /// ID of the peer to check
    pub peer_id: Uuid,
}

// ═══════════════════════════════════════════════════════════════════════════
// StorageEffects Parameters
// ═══════════════════════════════════════════════════════════════════════════

/// Parameters for storing a key-value pair
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreParams {
    /// Storage key
    pub key: String,
    /// Value to store
    pub value: Vec<u8>,
}

/// Parameters for retrieving a value from storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrieveParams {
    /// Storage key to retrieve
    pub key: String,
}

/// Parameters for removing a value from storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveParams {
    /// Storage key to remove
    pub key: String,
}

/// Parameters for listing storage keys
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListKeysParams {
    /// Optional prefix filter for keys
    pub prefix: Option<String>,
}

/// Parameters for checking if a key exists in storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExistsParams {
    /// Storage key to check
    pub key: String,
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
    pub protocol_id: Uuid,
    /// Type of protocol (e.g., "DKD", "FROST")
    pub protocol_type: String,
}

/// Parameters for logging a protocol completion event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolCompletedParams {
    /// Unique ID of the protocol instance
    pub protocol_id: Uuid,
    /// Duration of protocol execution in milliseconds
    pub duration_ms: u64,
}

/// Parameters for logging a protocol failure event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolFailedParams {
    /// Unique ID of the protocol instance
    pub protocol_id: Uuid,
    /// Error description
    pub error: String,
}

/// Parameters for logging a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogParams {
    /// Message to log
    pub message: String,
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
