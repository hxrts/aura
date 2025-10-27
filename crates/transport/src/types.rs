// Core types for transport layer

use serde::{Deserialize, Serialize};

// Import shared types from aura-types
pub use aura_types::ChunkId;

/// Replica proof for proof-of-storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProof {
    pub replica_tag: uuid::Uuid,
    pub signature: Vec<u8>,
}
