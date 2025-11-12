//! Aura Storage Layer - Pure Effect Interface
//!
//! This crate implements the storage layer for Aura as a simple effect interface,
//! aligned with the formal system model defined in docs/001_theoretical_foundations.md.
//!
//! ## Architecture
//!
//! The storage layer provides two core capabilities:
//! 1. **Content-addressed storage**: Store and retrieve chunks by content ID (CID)
//! 2. **Capability-filtered search**: Privacy-preserving search over stored content
//!
//! ## Model Alignment
//!
//! From the formal model (Section C.2):
//! ```ignore
//! Storage is simply an effect family:
//!   store_chunk : (ChunkId, Bytes) -> ()
//!   fetch_chunk : ChunkId -> Bytes?
//! ```
//!
//! Search is defined as capability-filtered queries over join-semilattice indices (Section E.1).
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use aura_store::{StorageEffects, ChunkId};
//!
//! // Store a chunk
//! let chunk_id = ChunkId::from_bytes(b"hello world");
//! effects.store_chunk(chunk_id.clone(), b"hello world".to_vec()).await?;
//!
//! // Retrieve a chunk
//! let data = effects.fetch_chunk(chunk_id).await?;
//! ```

pub mod encrypted;
pub mod search;

// Re-export core types
pub use aura_core::{ChunkId, ContentSize};
pub use encrypted::EncryptedStorageHandler;
pub use search::{
    AccessLevel, CapabilityFilteredQuery, CapabilityFilteredSearchEngine, FilteredSearchResult,
    SearchError, SearchQuery, SearchScope,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Storage operation errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageError {
    /// Chunk not found
    NotFound(ChunkId),
    /// I/O error
    IoError(String),
    /// Serialization error
    SerializationError(String),
    /// Invalid chunk ID
    InvalidChunkId(String),
    /// Storage quota exceeded
    QuotaExceeded,
    /// Permission denied
    PermissionDenied,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::NotFound(id) => write!(f, "Chunk not found: {}", id),
            StorageError::IoError(msg) => write!(f, "I/O error: {}", msg),
            StorageError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            StorageError::InvalidChunkId(msg) => write!(f, "Invalid chunk ID: {}", msg),
            StorageError::QuotaExceeded => write!(f, "Storage quota exceeded"),
            StorageError::PermissionDenied => write!(f, "Permission denied"),
        }
    }
}

impl std::error::Error for StorageError {}

// Use storage effects from aura-core instead of defining our own
// This aligns with work/013.md architecture where aura-store focuses on search,
// while aura-core provides the core storage effects interface
pub use aura_core::effects::StorageEffects;

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    /// Total number of chunks stored
    pub chunk_count: u64,
    /// Total bytes stored
    pub total_bytes: u64,
    /// Storage backend type
    pub backend_type: String,
}

/// Simple in-memory storage implementation (for testing)
#[derive(Debug, Clone)]
pub struct MemoryStorage {
    /// Stored chunks by ID (kept for backward compatibility)
    pub chunks: HashMap<ChunkId, Vec<u8>>,
    /// Key-value storage
    storage: std::sync::Arc<std::sync::RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryStorage {
    /// Create a new in-memory storage
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            storage: std::sync::Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Get number of stored chunks
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Check if storage is empty
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Clear all stored chunks
    pub fn clear(&mut self) {
        self.chunks.clear();
    }

    /// Store a chunk by ID
    pub async fn store_chunk(
        &mut self,
        chunk_id: ChunkId,
        data: Vec<u8>,
    ) -> Result<(), aura_core::effects::StorageError> {
        self.chunks.insert(chunk_id, data);
        Ok(())
    }

    /// Retrieve a chunk by ID
    pub async fn fetch_chunk(
        &self,
        chunk_id: &ChunkId,
    ) -> Result<Option<Vec<u8>>, aura_core::effects::StorageError> {
        Ok(self.chunks.get(chunk_id).cloned())
    }

    /// Delete a chunk by ID
    pub async fn delete_chunk(
        &mut self,
        chunk_id: &ChunkId,
    ) -> Result<(), aura_core::effects::StorageError> {
        self.chunks.remove(chunk_id);
        Ok(())
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

// StorageEffects implementation moved to aura-effects to maintain clean domain layer separation

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_storage_basic() {
        let mut storage = MemoryStorage::new();

        // Store a chunk
        let chunk_id = ChunkId::from_bytes(b"test data");
        storage
            .store_chunk(chunk_id.clone(), b"test data".to_vec())
            .await
            .unwrap();

        // Retrieve the chunk
        let data = storage.fetch_chunk(&chunk_id).await.unwrap();
        assert_eq!(data, Some(b"test data".to_vec()));

        // Basic chunk storage test complete
    }

    #[tokio::test]
    async fn test_memory_storage_not_found() {
        let storage = MemoryStorage::new();
        let chunk_id = ChunkId::from_bytes(b"nonexistent");

        let data = storage.fetch_chunk(&chunk_id).await.unwrap();
        assert_eq!(data, None);
    }

    #[tokio::test]
    async fn test_memory_storage_delete() {
        let mut storage = MemoryStorage::new();

        let chunk_id = ChunkId::from_bytes(b"test data");
        storage
            .store_chunk(chunk_id.clone(), b"test data".to_vec())
            .await
            .unwrap();

        // Delete the chunk
        storage.delete_chunk(&chunk_id).await.unwrap();

        // Verify it's gone
        let data = storage.fetch_chunk(&chunk_id).await.unwrap();
        assert_eq!(data, None);
    }

    #[tokio::test]
    async fn test_chunk_id_content_addressing() {
        let mut storage = MemoryStorage::new();

        // Same content should produce same chunk ID
        let data1 = b"identical content";
        let data2 = b"identical content";

        let chunk_id1 = ChunkId::from_bytes(data1);
        let chunk_id2 = ChunkId::from_bytes(data2);

        assert_eq!(chunk_id1, chunk_id2);

        // Store once
        storage
            .store_chunk(chunk_id1.clone(), data1.to_vec())
            .await
            .unwrap();

        // Should be retrievable by either ID
        let retrieved = storage.fetch_chunk(&chunk_id2).await.unwrap();
        assert_eq!(retrieved, Some(data1.to_vec()));
    }
}
