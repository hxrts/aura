//! Storage effects for key-value operations

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Storage location wrapper (kept for backwards compatibility)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorageLocation {
    path: PathBuf,
}

impl StorageLocation {
    /// Create a new storage location
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Create from path
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::new(path)
    }

    /// Get the path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get path as string
    pub fn as_str(&self) -> &str {
        self.path.to_str().unwrap_or("")
    }
}

/// Storage operation errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// Failed to read data
    #[error("Failed to read: {0}")]
    ReadFailed(String),
    /// Failed to write data
    #[error("Failed to write: {0}")]
    WriteFailed(String),
    /// Failed to delete data
    #[error("Failed to delete: {0}")]
    DeleteFailed(String),
    /// Failed to list keys
    #[error("Failed to list: {0}")]
    ListFailed(String),
    /// Key not found
    #[error("Key not found: {0}")]
    NotFound(String),
    /// Permission denied for storage operation
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

/// Storage statistics
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct StorageStats {
    /// Number of keys stored
    pub key_count: u64,
    /// Total size of stored data in bytes
    pub total_size: u64,
    /// Available space in bytes (if known)
    pub available_space: Option<u64>,
    /// Backend type (e.g., "memory", "filesystem", "distributed")
    pub backend_type: String,
}

/// Storage effects interface for key-value operations
///
/// This trait provides storage operations for the Aura effects system.
/// Implementations in aura-protocol provide:
/// - Production: Filesystem-based persistent storage
/// - Testing: In-memory storage for fast tests
/// - Simulation: Configurable storage with fault injection
#[async_trait]
pub trait StorageEffects: Send + Sync {
    /// Store a value under the given key
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError>;

    /// Retrieve a value by key
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;

    /// Remove a key-value pair
    async fn remove(&self, key: &str) -> Result<bool, StorageError>;

    /// List all keys with optional prefix filter
    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError>;

    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;

    /// Store multiple key-value pairs atomically
    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError>;

    /// Retrieve multiple values by keys
    async fn retrieve_batch(
        &self,
        keys: &[String],
    ) -> Result<HashMap<String, Vec<u8>>, StorageError>;

    /// Clear all stored data
    async fn clear_all(&self) -> Result<(), StorageError>;

    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats, StorageError>;
}
