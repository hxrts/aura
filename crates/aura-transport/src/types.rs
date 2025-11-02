//! Core types for transport layer

use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// Import shared types from aura-types
pub use aura_types::ChunkId;

/// Transport-specific message envelope
///
/// Note: This is separate from aura-messages::MessageEnvelope to maintain
/// transport layer independence while providing a path for future unification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportEnvelope {
    /// Source device
    pub from: DeviceId,
    /// Destination device
    pub to: DeviceId,
    /// Unique message identifier
    pub message_id: Uuid,
    /// Serialized payload bytes
    pub payload: Vec<u8>,
    /// Message metadata
    pub metadata: MessageMetadata,
}

/// Message metadata for transport envelopes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMetadata {
    /// Unix timestamp when the message was created
    pub timestamp: u64,
    /// Type classification of the message
    pub message_type: String,
    /// Priority level for message delivery
    pub priority: MessagePriority,
}

/// Message priority levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessagePriority {
    /// Low priority - can be delivered with delay
    Low,
    /// Normal priority - standard delivery
    Normal,
    /// High priority - expedited delivery
    High,
    /// Critical priority - immediate delivery required
    Critical,
}

/// Transport type configuration
#[derive(Debug, Clone)]
pub enum TransportType {
    /// In-memory message passing for local testing
    Memory,
    /// TCP socket transport with address and port
    Tcp {
        /// Server address to connect to
        address: String,
        /// Server port number
        port: u16,
    },
    /// HTTPS relay transport through remote server
    HttpsRelay {
        /// Relay server URL
        url: String,
    },
    /// Simulation transport for deterministic testing
    Simulation,
}

/// Transport configuration
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// The type of transport to use
    pub transport_type: TransportType,
    /// Device identifier for this transport endpoint
    pub device_id: DeviceId,
    /// Maximum allowed message size in bytes
    pub max_message_size: usize,
    /// Connection timeout in milliseconds
    pub connection_timeout_ms: u64,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            transport_type: TransportType::Memory,
            device_id: DeviceId::new(),
            max_message_size: 1024 * 1024, // 1MB
            connection_timeout_ms: 30000,  // 30s
        }
    }
}

impl Default for MessagePriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Replica proof for proof-of-storage verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProof {
    /// Unique identifier for this replica
    pub replica_tag: uuid::Uuid,
    /// Cryptographic signature proving storage
    pub signature: Vec<u8>,
}
