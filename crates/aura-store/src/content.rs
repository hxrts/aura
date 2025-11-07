//! Storage-specific content addressing types
//!
//! This module provides storage-layer types for chunk identification and content sizing.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Unified chunk identifier using content addressing
///
/// This consolidates the different ChunkId definitions across crates
/// and standardizes on `Vec<u8>` for content addressing compatibility.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkId(pub Vec<u8>);

impl ChunkId {
    /// Create a new chunk ID from bytes
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Create from a byte slice
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    /// Create from a string (for backward compatibility)
    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into().into_bytes())
    }

    /// Create from a 32-byte hash (typically Blake3)
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        Self(hash.to_vec())
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Get as string (for backward compatibility)
    pub fn as_string(&self) -> Option<String> {
        String::from_utf8(self.0.clone()).ok()
    }

    /// Get length in bytes
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Convert to hex string for display
    pub fn to_hex(&self) -> String {
        hex::encode(&self.0)
    }

    /// Create from hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        Ok(Self(bytes))
    }

    /// Create chunk ID from content using blake3 hash
    pub fn from_content(content: &[u8]) -> Self {
        let hash = blake3::hash(content);
        Self::from_hash(hash.as_bytes())
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "chunk-{}", self.to_hex())
    }
}

impl From<Vec<u8>> for ChunkId {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for ChunkId {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl From<String> for ChunkId {
    fn from(s: String) -> Self {
        Self::from_string(s)
    }
}

impl From<&str> for ChunkId {
    fn from(s: &str) -> Self {
        Self::from_string(s)
    }
}

impl From<[u8; 32]> for ChunkId {
    fn from(hash: [u8; 32]) -> Self {
        Self::from_hash(&hash)
    }
}

impl From<ChunkId> for Vec<u8> {
    fn from(chunk_id: ChunkId) -> Self {
        chunk_id.0
    }
}

/// Content size in bytes
///
/// Represents the size of content chunks or data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentSize(pub u64);

impl ContentSize {
    /// Create a new content size
    pub fn new(size: u64) -> Self {
        Self(size)
    }

    /// Get the size in bytes
    pub fn bytes(&self) -> u64 {
        self.0
    }

    /// Check if content is empty
    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Format as human-readable size
    pub fn human_readable(&self) -> String {
        let size = self.0 as f64;
        if size < 1024.0 {
            format!("{} B", size)
        } else if size < 1024.0 * 1024.0 {
            format!("{:.1} KB", size / 1024.0)
        } else if size < 1024.0 * 1024.0 * 1024.0 {
            format!("{:.1} MB", size / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", size / (1024.0 * 1024.0 * 1024.0))
        }
    }
}

impl fmt::Display for ContentSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.human_readable())
    }
}

impl From<u64> for ContentSize {
    fn from(size: u64) -> Self {
        Self(size)
    }
}

impl From<usize> for ContentSize {
    fn from(size: usize) -> Self {
        Self(size as u64)
    }
}

impl From<ContentSize> for u64 {
    fn from(size: ContentSize) -> Self {
        size.0
    }
}
