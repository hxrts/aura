//! Core types for transport layer

use serde::{Deserialize, Serialize};

// Import shared types from aura-types
pub use aura_types::ChunkId;

// Re-export types from infrastructure modules
pub use crate::infrastructure::presence::PresenceTicket;

/// Replica proof for proof-of-storage verification
///
/// Contains the replica tag identifying a storage replica and a signature
/// proving that the replica holds the requested chunk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProof {
    /// Unique identifier for this storage replica
    pub replica_tag: uuid::Uuid,
    /// Cryptographic signature proving possession of the chunk
    pub signature: Vec<u8>,
}
