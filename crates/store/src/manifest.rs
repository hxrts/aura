// Object manifest system with inline metadata

use crate::{encryption::KeyEnvelope, Result, StorageError};
use aura_journal::serialization::to_cbor_bytes;
use aura_journal::Cid;
use serde::{Deserialize, Serialize};

/// Object manifest with inline metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectManifest {
    /// Root CID of the object
    pub root_cid: Cid,
    /// Total size in bytes
    pub size: u64,
    /// Chunking parameters
    pub chunking: ChunkingParams,
    /// Erasure coding (None for MVP)
    pub erasure: Option<ErasureMeta>,

    /// Context ID for derived identity
    pub context_id: Option<[u8; 32]>,
    /// Application metadata (max 4 KiB recommended)
    pub app_metadata: Option<Vec<u8>>,

    /// Encryption key envelope
    pub key_envelope: KeyEnvelope,
    /// Authorization token reference (capability)
    pub auth_token_ref: Option<Cid>,

    /// Replication hint
    pub replication_hint: ReplicationHint,
    /// Version number
    pub version: u64,
    /// Previous manifest CID
    pub prev_manifest: Option<Cid>,
    /// Timestamp
    pub issued_at_ms: u64,
    /// Nonce for uniqueness
    pub nonce: [u8; 32],
}

impl ObjectManifest {
    /// Compute manifest CID
    pub fn compute_cid(&self) -> Result<Cid> {
        let bytes = to_cbor_bytes(self)
            .map_err(|e| StorageError::Storage(format!("Failed to serialize manifest: {}", e)))?;
        Ok(Cid::from_bytes(&bytes))
    }
}

/// Chunking parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkingParams {
    /// Chunk size in bytes (1-4 MiB)
    pub chunk_size: u32,
    /// Number of chunks
    pub num_chunks: u32,
}

impl ChunkingParams {
    pub const DEFAULT_CHUNK_SIZE: u32 = 1 * 1024 * 1024; // 1 MiB
    pub const MAX_CHUNK_SIZE: u32 = 4 * 1024 * 1024; // 4 MiB

    pub fn new(total_size: u64) -> Self {
        let chunk_size = Self::DEFAULT_CHUNK_SIZE;
        let num_chunks = ((total_size + chunk_size as u64 - 1) / chunk_size as u64) as u32;
        ChunkingParams {
            chunk_size,
            num_chunks,
        }
    }
}

/// Erasure coding metadata (future use)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErasureMeta {
    pub data_shards: u32,
    pub parity_shards: u32,
}

/// Replication hint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplicationHint {
    /// Target peer IDs
    pub target_peers: Vec<String>,
    /// Minimum replicas
    pub min_replicas: u32,
}

impl Default for ReplicationHint {
    fn default() -> Self {
        ReplicationHint {
            target_peers: Vec::new(),
            min_replicas: 2,
        }
    }
}

/// Chunk metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub chunk_id: ChunkId,
    pub size: u64,
    pub cid: Cid,
    pub offset: u64,
}

/// Chunk identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub String);

impl ChunkId {
    pub fn new(manifest_cid: &Cid, chunk_index: u32) -> Self {
        ChunkId(format!("{}:{}", manifest_cid.0, chunk_index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunking_params() {
        let params = ChunkingParams::new(5 * 1024 * 1024); // 5 MiB
        assert_eq!(params.chunk_size, 1 * 1024 * 1024);
        assert_eq!(params.num_chunks, 5);
    }

    #[test]
    fn test_manifest_cid() {
        let manifest = ObjectManifest {
            root_cid: Cid("test".to_string()),
            size: 1024,
            chunking: ChunkingParams::new(1024),
            erasure: None,
            context_id: None,
            app_metadata: None,
            key_envelope: KeyEnvelope {
                wrapped_keys: vec![],
            },
            auth_token_ref: None,
            replication_hint: ReplicationHint::default(),
            version: 1,
            prev_manifest: None,
            issued_at_ms: 0,
            nonce: [0u8; 32],
        };

        let cid = manifest.compute_cid().unwrap();
        assert!(!cid.0.is_empty());
    }
}
