//! Strongly typed storage identifiers and size wrappers.

use serde::{Deserialize, Serialize};

/// Identifier for a storage node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Create a new node identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Borrow the underlying string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

/// Count of chunks for a content item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChunkCount(u32);

impl ChunkCount {
    /// Create a new chunk count.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Return the raw count.
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl From<u32> for ChunkCount {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

/// Index of a chunk within content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChunkIndex(u32);

impl ChunkIndex {
    /// Create a new chunk index.
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    /// Return the raw index.
    pub const fn value(self) -> u32 {
        self.0
    }
}

impl From<u32> for ChunkIndex {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

/// Byte size for storage accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ByteSize(u64);

impl ByteSize {
    /// Create a new byte size.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw byte count.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for ByteSize {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl From<u32> for ByteSize {
    fn from(value: u32) -> Self {
        Self::new(value as u64)
    }
}
