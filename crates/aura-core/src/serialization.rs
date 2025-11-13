//! DAG-CBOR serialization for Aura core types
//!
//! This module provides a unified serialization interface using DAG-CBOR as the
//! canonical format for all wire protocols, CRDT state, and cryptographic commitments.
//!
//! DAG-CBOR provides:
//! - Deterministic canonical encoding (required for FROST signatures)
//! - Content-addressable format (IPLD compatibility)
//! - Forward/backward compatibility (semantic versioning support)
//! - Efficient binary encoding

use crate::hash;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Unified error type for serialization operations
#[derive(Debug, thiserror::Error)]
pub enum SerializationError {
    /// DAG-CBOR encoding/decoding error
    #[error("DAG-CBOR error: {0}")]
    DagCbor(String),

    /// Invalid data format
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
}

/// Standard Result type for serialization operations
pub type Result<T> = std::result::Result<T, SerializationError>;

/// Serialize any serde-compatible type to DAG-CBOR bytes
pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_ipld_dagcbor::to_vec(value).map_err(|e| {
        SerializationError::InvalidFormat(format!("Failed to serialize to DAG-CBOR: {}", e))
    })
}

/// Deserialize DAG-CBOR bytes to any serde-compatible type
pub fn from_slice<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    serde_ipld_dagcbor::from_slice(bytes).map_err(|e| SerializationError::DagCbor(e.to_string()))
}

/// Serialize to DAG-CBOR and return the canonical hash
pub fn hash_canonical<T: Serialize>(value: &T) -> Result<[u8; 32]> {
    let bytes = to_vec(value)?;
    Ok(hash::hash(&bytes))
}

/// Version information for semantic versioning support
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticVersion {
    /// Major version number - increment for incompatible changes
    pub major: u16,
    /// Minor version number - increment for backwards-compatible additions
    pub minor: u16,
    /// Patch version number - increment for backwards-compatible bug fixes
    pub patch: u16,
}

impl SemanticVersion {
    /// Create a new semantic version
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Check if this version is compatible with another
    pub fn is_compatible(&self, other: &Self) -> bool {
        // Major version must match for compatibility
        self.major == other.major
    }

    /// Check if this version is newer than another
    pub fn is_newer(&self, other: &Self) -> bool {
        (self.major, self.minor, self.patch) > (other.major, other.minor, other.patch)
    }
}

impl std::fmt::Display for SemanticVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Versioned message envelope for forward/backward compatibility
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedMessage<T> {
    /// Protocol version
    pub version: SemanticVersion,
    /// Message payload
    pub payload: T,
    /// Optional metadata for debugging
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, String>,
}

impl<T> VersionedMessage<T> {
    /// Create a new versioned message
    pub fn new(payload: T, version: SemanticVersion) -> Self {
        Self {
            version,
            payload,
            metadata: HashMap::new(),
        }
    }

    /// Add metadata to the message
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Optional JSON export for debugging (feature-gated)
///
/// Provides JSON serialization utilities for debugging purposes only.
/// These functions are not intended for wire protocol use.
#[cfg(feature = "json-debug")]
pub mod json_debug {
    use super::*;

    /// Serialize to JSON for debugging (not for wire protocol)
    pub fn to_json_pretty<T: Serialize>(value: &T) -> serde_json::Result<String> {
        serde_json::to_string_pretty(value)
    }

    /// Serialize to JSON for debugging (not for wire protocol)
    pub fn to_json<T: Serialize>(value: &T) -> serde_json::Result<String> {
        serde_json::to_string(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestData {
        id: u64,
        name: String,
        tags: Vec<String>,
    }

    #[test]
    fn test_dag_cbor_roundtrip() {
        let data = TestData {
            id: 42,
            name: "test".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let bytes = to_vec(&data).unwrap();
        let decoded: TestData = from_slice(&bytes).unwrap();

        assert_eq!(data, decoded);
    }

    #[test]
    fn test_canonical_hash() {
        let data1 = TestData {
            id: 42,
            name: "test".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let data2 = TestData {
            id: 42,
            name: "test".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let hash1 = hash_canonical(&data1).unwrap();
        let hash2 = hash_canonical(&data2).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_semantic_version() {
        let v1 = SemanticVersion::new(1, 0, 0);
        let v2 = SemanticVersion::new(1, 1, 0);
        let v3 = SemanticVersion::new(2, 0, 0);

        assert!(v1.is_compatible(&v2));
        assert!(!v1.is_compatible(&v3));
        assert!(v2.is_newer(&v1));
        assert!(v3.is_newer(&v2));
    }

    #[test]
    fn test_versioned_message() {
        let data = TestData {
            id: 42,
            name: "test".to_string(),
            tags: vec!["tag1".to_string(), "tag2".to_string()],
        };

        let version = SemanticVersion::new(1, 0, 0);
        let message = VersionedMessage::new(data.clone(), version)
            .with_metadata("source".to_string(), "test".to_string());

        let bytes = to_vec(&message).unwrap();
        let decoded: VersionedMessage<TestData> = from_slice(&bytes).unwrap();

        assert_eq!(decoded.payload, data);
        assert_eq!(decoded.version.major, 1);
        assert_eq!(decoded.metadata.get("source").unwrap(), "test");
    }
}
