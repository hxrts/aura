//! Storage effects interface
//!
//! Pure trait definitions for storage operations used by protocols.

use async_trait::async_trait;
use std::collections::HashMap;

/// Storage effects for protocol state persistence
#[async_trait]
pub trait StorageEffects: Send + Sync {
    /// Store data with a key
    async fn store(&self, key: &str, value: Vec<u8>) -> Result<(), StorageError>;
    
    /// Retrieve data by key
    async fn retrieve(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;
    
    /// Remove data by key
    async fn remove(&self, key: &str) -> Result<bool, StorageError>;
    
    /// List all keys with optional prefix filter
    async fn list_keys(&self, prefix: Option<&str>) -> Result<Vec<String>, StorageError>;
    
    /// Check if a key exists
    async fn exists(&self, key: &str) -> Result<bool, StorageError>;
    
    /// Store multiple key-value pairs atomically
    async fn store_batch(&self, pairs: HashMap<String, Vec<u8>>) -> Result<(), StorageError>;
    
    /// Retrieve multiple values by keys
    async fn retrieve_batch(&self, keys: &[String]) -> Result<HashMap<String, Vec<u8>>, StorageError>;
    
    /// Clear all data (use with caution)
    async fn clear_all(&self) -> Result<(), StorageError>;
    
    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats, StorageError>;
}

/// Storage-related errors
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("Key not found: {key}")]
    KeyNotFound { key: String },
    
    #[error("Permission denied for key: {key}")]
    PermissionDenied { key: String },
    
    #[error("Storage quota exceeded")]
    QuotaExceeded,
    
    #[error("Storage backend error: {source}")]
    Backend { source: Box<dyn std::error::Error + Send + Sync> },
    
    #[error("Serialization error: {message}")]
    Serialization { message: String },
    
    #[error("Key validation failed: {key}")]
    InvalidKey { key: String },
    
    #[error("Value too large: {size} bytes (max: {max_size})")]
    ValueTooLarge { size: usize, max_size: usize },
}

/// Storage statistics
#[derive(Debug, Clone)]
pub struct StorageStats {
    /// Number of stored keys
    pub key_count: u64,
    /// Total size in bytes
    pub total_size: u64,
    /// Available space in bytes (None if unlimited)
    pub available_space: Option<u64>,
    /// Storage backend type
    pub backend_type: String,
}