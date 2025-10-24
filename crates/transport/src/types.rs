// Core types for transport layer

use serde::{Deserialize, Serialize};

/// Chunk identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub String);

/// Replica proof for proof-of-storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicaProof {
    pub replica_tag: uuid::Uuid,
    pub signature: Vec<u8>,
}
