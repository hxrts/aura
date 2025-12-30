//! Content addressing types
//!
//! Unified content identifier system using cryptographic hashing.
//!
//! # Type Hierarchy
//!
//! - `Hash32`: Raw 32-byte Blake3 hash (cryptographic primitive)
//! - `ChunkId`: Storage-layer chunk identifier (with optional sequence)
//! - `ContentId`: High-level content identifier (with optional metadata)

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::crypto::hash;

/// Raw 32-byte cryptographic hash
///
/// This is the foundation for all content addressing. Use higher-level types
/// (ContentId, ChunkId) in application code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    /// Create from 32-byte array
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create a zero hash (for testing/defaults)
    pub fn zero() -> Self {
        Self([0u8; 32])
    }

    /// Hash arbitrary bytes using the system hash algorithm
    pub fn from_bytes(data: &[u8]) -> Self {
        Self(hash::hash(data))
    }

    /// Hash a serializable value using canonical DAG-CBOR encoding
    pub fn from_value<T: serde::Serialize>(value: &T) -> Result<Self, crate::SerializationError> {
        let bytes = crate::util::serialization::to_vec(value)?;
        Ok(Self::from_bytes(&bytes))
    }

    /// Get as bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(s)?;
        if bytes.len() != 32 {
            return Err(hex::FromHexError::InvalidStringLength);
        }
        let mut array = [0u8; 32];
        array.copy_from_slice(&bytes);
        Ok(Self(array))
    }
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for Hash32 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Default for Hash32 {
    fn default() -> Self {
        Self::zero()
    }
}

impl From<[u8; 32]> for Hash32 {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Content identifier for high-level blobs
///
/// Represents a complete piece of content (file, document, encrypted payload, CRDT state).
/// May be chunked for storage into multiple ChunkIds.
///
/// # Use Cases
/// - Journal entries, effect API records
/// - User files and documents
/// - Encrypted payloads
/// - CRDT state snapshots
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContentId {
    /// Hash of the complete content
    pub hash: Hash32,
    /// Optional size of the original content in bytes
    pub size: Option<u64>,
}

impl ContentId {
    /// Create from hash
    pub fn new(hash: Hash32) -> Self {
        Self { hash, size: None }
    }

    /// Create with size metadata
    #[must_use]
    pub fn with_size(hash: Hash32, size: u64) -> Self {
        Self {
            hash,
            size: Some(size),
        }
    }

    /// Create from raw bytes
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            hash: Hash32::from_bytes(data),
            size: Some(data.len() as u64),
        }
    }

    /// Create from serializable value (DAG-CBOR)
    pub fn from_value<T: serde::Serialize>(value: &T) -> Result<Self, crate::SerializationError> {
        let bytes = crate::util::serialization::to_vec(value)?;
        Ok(Self {
            hash: Hash32::from_bytes(&bytes),
            size: Some(bytes.len() as u64),
        })
    }

    /// Get the hash
    pub fn hash(&self) -> &Hash32 {
        &self.hash
    }

    /// Get as hex string (hash only)
    pub fn to_hex(&self) -> String {
        self.hash.to_hex()
    }

    /// Parse from hex string (hash only, no metadata)
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        Ok(Self::new(Hash32::from_hex(s)?))
    }

    /// Get size if available
    pub fn size(&self) -> Option<u64> {
        self.size
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "content:{}", self.hash.to_hex())?;
        if let Some(size) = self.size {
            write!(f, ":{size}")?;
        }
        Ok(())
    }
}

impl From<Hash32> for ContentId {
    fn from(hash: Hash32) -> Self {
        Self::new(hash)
    }
}

impl From<[u8; 32]> for ContentId {
    fn from(bytes: [u8; 32]) -> Self {
        Self::new(Hash32(bytes))
    }
}

/// Chunk identifier for storage-layer blocks
///
/// Represents a fixed-size or variable-size storage chunk.
/// Multiple chunks may comprise a single ContentId.
///
/// # Use Cases
/// - Storage layer operations (aura-store, aura-storage)
/// - Chunked upload/download
/// - Erasure coding blocks
/// - Replication tracking
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkId {
    /// Hash of chunk data
    hash: Hash32,
    /// Optional chunk sequence number (for ordered chunks)
    sequence: Option<u32>,
}

impl ChunkId {
    /// Create from hash
    pub fn new(hash: Hash32) -> Self {
        Self {
            hash,
            sequence: None,
        }
    }

    /// Create with sequence number
    #[must_use]
    pub fn with_sequence(hash: Hash32, sequence: u32) -> Self {
        Self {
            hash,
            sequence: Some(sequence),
        }
    }

    /// Create from chunk data
    pub fn from_bytes(data: &[u8]) -> Self {
        Self::new(Hash32::from_bytes(data))
    }

    /// Create from 32-byte hash
    pub fn from_hash(hash: [u8; 32]) -> Self {
        Self::new(Hash32(hash))
    }

    /// Get the hash
    pub fn hash(&self) -> &Hash32 {
        &self.hash
    }

    /// Get the raw hash bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.hash.as_bytes()
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        self.hash.to_hex()
    }

    /// Parse from hex string
    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        Ok(Self::new(Hash32::from_hex(s)?))
    }

    /// Get sequence number if present
    pub fn sequence(&self) -> Option<u32> {
        self.sequence
    }

    /// Check if chunk is part of a sequence
    pub fn is_sequenced(&self) -> bool {
        self.sequence.is_some()
    }
}

impl fmt::Display for ChunkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "chunk:{}", self.hash.to_hex())?;
        if let Some(seq) = self.sequence {
            write!(f, ":{seq}")?;
        }
        Ok(())
    }
}

impl From<Hash32> for ChunkId {
    fn from(hash: Hash32) -> Self {
        Self::new(hash)
    }
}

impl From<[u8; 32]> for ChunkId {
    fn from(bytes: [u8; 32]) -> Self {
        Self::from_hash(bytes)
    }
}

impl AsRef<[u8]> for ChunkId {
    fn as_ref(&self) -> &[u8] {
        self.hash.as_ref()
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
            format!("{size} B")
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
