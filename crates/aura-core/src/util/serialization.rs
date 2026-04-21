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

use crate::crypto::hash;
use ciborium::{
    de::from_reader as cbor_from_reader,
    ser::into_writer as cbor_into_writer,
    value::{CanonicalValue, Value as CborValue},
};
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

fn canonicalize_value(value: CborValue) -> Result<CborValue> {
    match value {
        CborValue::Integer(_)
        | CborValue::Bytes(_)
        | CborValue::Float(_)
        | CborValue::Text(_)
        | CborValue::Bool(_)
        | CborValue::Null => Ok(value),
        CborValue::Tag(tag, _) => Err(SerializationError::InvalidFormat(format!(
            "Unsupported DAG-CBOR tag in Aura serialization: {tag}"
        ))),
        CborValue::Array(values) => values
            .into_iter()
            .map(canonicalize_value)
            .collect::<Result<Vec<_>>>()
            .map(CborValue::Array),
        CborValue::Map(entries) => canonicalize_map(entries),
        _ => Err(SerializationError::InvalidFormat(
            "Unsupported CBOR value variant in Aura serialization".to_string(),
        )),
    }
}

fn canonicalize_map(entries: Vec<(CborValue, CborValue)>) -> Result<CborValue> {
    let mut canonical_entries = entries
        .into_iter()
        .map(|(key, value)| Ok((canonicalize_value(key)?, canonicalize_value(value)?)))
        .collect::<Result<Vec<_>>>()?;

    canonical_entries.sort_by(|(left, _), (right, _)| {
        CanonicalValue::from(left.clone()).cmp(&CanonicalValue::from(right.clone()))
    });

    for pair in canonical_entries.windows(2) {
        let left = CanonicalValue::from(pair[0].0.clone());
        let right = CanonicalValue::from(pair[1].0.clone());
        if left == right {
            return Err(SerializationError::InvalidFormat(
                "DAG-CBOR map contains duplicate canonical keys".to_string(),
            ));
        }
    }

    Ok(CborValue::Map(canonical_entries))
}

/// Serialize any serde-compatible type to DAG-CBOR bytes
pub fn to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let value = CborValue::serialized(value).map_err(|e| {
        SerializationError::InvalidFormat(format!("Failed to serialize to DAG-CBOR value: {e}"))
    })?;
    let value = canonicalize_value(value)?;
    let mut bytes = Vec::new();
    cbor_into_writer(&value, &mut bytes).map_err(|e| {
        SerializationError::DagCbor(format!("Failed to encode DAG-CBOR bytes: {e}"))
    })?;
    Ok(bytes)
}

/// Deserialize DAG-CBOR bytes to any serde-compatible type
pub fn from_slice<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<T> {
    let value: CborValue =
        cbor_from_reader(bytes).map_err(|e| SerializationError::DagCbor(e.to_string()))?;
    let value = canonicalize_value(value)?;
    value
        .deserialized()
        .map_err(|e| SerializationError::DagCbor(e.to_string()))
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
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::{ser::into_writer as cbor_into_writer, value::Value as CborValue};
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;

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

    #[test]
    fn test_hash_map_encoding_is_canonical() {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct MapEnvelope {
            fields: HashMap<String, u64>,
        }

        let mut left = HashMap::new();
        left.insert("beta".to_string(), 2);
        left.insert("alpha".to_string(), 1);

        let mut right = HashMap::new();
        right.insert("alpha".to_string(), 1);
        right.insert("beta".to_string(), 2);

        let left_bytes = to_vec(&MapEnvelope { fields: left }).unwrap();
        let right_bytes = to_vec(&MapEnvelope { fields: right }).unwrap();
        assert_eq!(left_bytes, right_bytes);
    }

    #[test]
    fn test_nested_map_decode_recanonicalizes_noncanonical_input() {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        struct NestedEnvelope {
            fields: HashMap<String, HashMap<String, u64>>,
        }

        let noncanonical = CborValue::Map(vec![(
            CborValue::Text("fields".to_string()),
            CborValue::Map(vec![
                (
                    CborValue::Text("beta".to_string()),
                    CborValue::Map(vec![
                        (
                            CborValue::Text("zeta".to_string()),
                            CborValue::Integer(6.into()),
                        ),
                        (
                            CborValue::Text("alpha".to_string()),
                            CborValue::Integer(1.into()),
                        ),
                    ]),
                ),
                (
                    CborValue::Text("alpha".to_string()),
                    CborValue::Map(vec![
                        (
                            CborValue::Text("delta".to_string()),
                            CborValue::Integer(4.into()),
                        ),
                        (
                            CborValue::Text("beta".to_string()),
                            CborValue::Integer(2.into()),
                        ),
                    ]),
                ),
            ]),
        )]);
        let mut bytes = Vec::new();
        cbor_into_writer(&noncanonical, &mut bytes).unwrap();

        let decoded: NestedEnvelope = from_slice(&bytes).unwrap();

        let mut expected = HashMap::new();
        expected.insert(
            "beta".to_string(),
            HashMap::from([("alpha".to_string(), 1), ("zeta".to_string(), 6)]),
        );
        expected.insert(
            "alpha".to_string(),
            HashMap::from([("beta".to_string(), 2), ("delta".to_string(), 4)]),
        );
        let expected = NestedEnvelope { fields: expected };

        assert_eq!(decoded, expected);
        assert_eq!(to_vec(&decoded).unwrap(), to_vec(&expected).unwrap());
    }

    #[test]
    fn test_duplicate_map_keys_are_rejected() {
        let duplicate_map = CborValue::Map(vec![
            (
                CborValue::Text("dup".to_string()),
                CborValue::Integer(1.into()),
            ),
            (
                CborValue::Text("dup".to_string()),
                CborValue::Integer(2.into()),
            ),
        ]);
        let mut bytes = Vec::new();
        cbor_into_writer(&duplicate_map, &mut bytes).unwrap();

        let err = from_slice::<HashMap<String, u64>>(&bytes).unwrap_err();
        assert!(err.to_string().contains("duplicate canonical keys"));
    }

    #[test]
    fn test_nested_duplicate_map_keys_are_rejected() {
        let duplicate_map = CborValue::Map(vec![(
            CborValue::Text("fields".to_string()),
            CborValue::Map(vec![(
                CborValue::Text("nested".to_string()),
                CborValue::Map(vec![
                    (
                        CborValue::Text("dup".to_string()),
                        CborValue::Integer(1.into()),
                    ),
                    (
                        CborValue::Text("dup".to_string()),
                        CborValue::Integer(2.into()),
                    ),
                ]),
            )]),
        )]);
        let mut bytes = Vec::new();
        cbor_into_writer(&duplicate_map, &mut bytes).unwrap();

        let err = from_slice::<HashMap<String, HashMap<String, HashMap<String, u64>>>>(&bytes)
            .unwrap_err();
        assert!(err.to_string().contains("duplicate canonical keys"));
    }

    #[test]
    fn test_tags_are_rejected() {
        let tagged = CborValue::Tag(42, Box::new(CborValue::Bytes(vec![0, 1, 2])));
        let err = to_vec(&tagged).unwrap_err();
        assert!(err.to_string().contains("Unsupported DAG-CBOR tag"));
    }

    #[test]
    fn test_nested_tags_are_rejected_on_decode() {
        let tagged = CborValue::Map(vec![(
            CborValue::Text("items".to_string()),
            CborValue::Array(vec![
                CborValue::Integer(1.into()),
                CborValue::Tag(42, Box::new(CborValue::Bytes(vec![0, 1, 2]))),
            ]),
        )]);
        let mut bytes = Vec::new();
        cbor_into_writer(&tagged, &mut bytes).unwrap();

        let err = from_slice::<HashMap<String, Vec<u64>>>(&bytes).unwrap_err();
        assert!(err.to_string().contains("Unsupported DAG-CBOR tag"));
    }
}
