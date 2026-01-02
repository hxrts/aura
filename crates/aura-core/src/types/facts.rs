//! Canonical encoding for domain facts.
//!
//! This module provides type-safe fact encoding/decoding with proper error handling.
//! Layer 2 domain crates should use the fallible `try_encode`/`try_decode` APIs.

use crate::util::serialization::SerializationError;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;

pub const MAX_FACT_PAYLOAD_BYTES: usize = 65_536;

/// Strongly-typed fact type identifier.
///
/// Replaces raw `&'static str` type IDs with a validated newtype.
/// Type IDs follow the format `domain/version` (e.g., "wot/v1", "verify/v1").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FactTypeId(Cow<'static, str>);

impl FactTypeId {
    /// Create a new fact type ID.
    ///
    /// Type IDs should follow the format `domain/version`.
    #[must_use]
    pub const fn new(id: &'static str) -> Self {
        Self(Cow::Borrowed(id))
    }

    /// Get the type ID as a string slice.
    pub fn as_str(&self) -> &str {
        self.0.as_ref()
    }
}

impl fmt::Display for FactTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<&str> for FactTypeId {
    fn from(s: &str) -> Self {
        Self(Cow::Owned(s.to_string()))
    }
}

impl From<String> for FactTypeId {
    fn from(s: String) -> Self {
        Self(Cow::Owned(s))
    }
}

impl AsRef<str> for FactTypeId {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

/// Error type for fact encoding/decoding operations.
#[derive(Debug, thiserror::Error)]
pub enum FactError {
    /// Serialization failed
    #[error("serialization failed: {0}")]
    Serialization(#[from] SerializationError),

    /// Type ID mismatch
    #[error("type ID mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        /// Expected type ID
        expected: String,
        /// Actual type ID found
        actual: String,
    },

    /// Schema version mismatch
    #[error("schema version mismatch: expected {expected}, got {actual}")]
    VersionMismatch {
        /// Expected schema version
        expected: u16,
        /// Actual schema version found
        actual: u16,
    },

    /// Invalid envelope structure
    #[error("invalid envelope: {0}")]
    InvalidEnvelope(String),

    /// Payload exceeds size limit
    #[error("payload too large: {size} bytes (max {max})")]
    PayloadTooLarge { size: u64, max: u64 },
}

/// Encoding used inside a fact envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactEncoding {
    /// DAG-CBOR encoding (canonical, deterministic).
    DagCbor,
    /// JSON encoding (primarily for debugging).
    Json,
}

/// Canonical envelope for domain fact payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactEnvelope {
    /// Domain fact type identifier (e.g., "chat", "invitation").
    pub type_id: FactTypeId,
    /// Schema version for the encoded payload.
    pub schema_version: u16,
    /// Payload encoding format.
    pub encoding: FactEncoding,
    /// Encoded payload bytes.
    pub payload: Vec<u8>,
}

/// Delta produced by applying domain facts during reduction.
pub trait FactDelta: Default + Clone {
    /// Merge another delta into this one.
    fn merge(&mut self, other: &Self);
}

/// Reducer that maps domain facts to typed deltas.
pub trait FactDeltaReducer<F, D: FactDelta> {
    /// Apply a single fact and return its delta.
    fn apply(&self, fact: &F) -> D;

    /// Apply a fact into an existing delta.
    fn apply_into(&self, fact: &F, delta: &mut D) {
        let update = self.apply(fact);
        delta.merge(&update);
    }

    /// Reduce a batch of facts into a single delta.
    fn reduce_batch(&self, facts: &[F]) -> D {
        let mut delta = D::default();
        for fact in facts {
            self.apply_into(fact, &mut delta);
        }
        delta
    }
}

/// Encode a domain fact payload with a canonical envelope.
pub fn encode_domain_fact<T: Serialize>(
    type_id: &str,
    schema_version: u16,
    value: &T,
) -> Result<Vec<u8>, FactError> {
    let payload = crate::util::serialization::to_vec(value)?;
    if payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return Err(FactError::PayloadTooLarge {
            size: payload.len() as u64,
            max: MAX_FACT_PAYLOAD_BYTES as u64,
        });
    }
    let envelope = FactEnvelope {
        type_id: FactTypeId::from(type_id),
        schema_version,
        encoding: FactEncoding::DagCbor,
        payload,
    };
    Ok(crate::util::serialization::to_vec(&envelope)?)
}

/// Decode a domain fact payload from a canonical envelope.
pub fn decode_domain_fact<T: DeserializeOwned>(
    expected_type_id: &str,
    expected_schema_version: u16,
    bytes: &[u8],
) -> Option<T> {
    let envelope: FactEnvelope = crate::util::serialization::from_slice(bytes).ok()?;
    if envelope.type_id.as_str() != expected_type_id {
        return None;
    }
    if envelope.schema_version != expected_schema_version {
        return None;
    }
    if envelope.payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return None;
    }
    match envelope.encoding {
        FactEncoding::DagCbor => crate::util::serialization::from_slice(&envelope.payload).ok(),
        FactEncoding::Json => serde_json::from_slice(&envelope.payload).ok(),
    }
}

// ============================================================================
// Result-returning APIs (preferred for Layer 2 facts)
// ============================================================================

/// Encode a domain fact with proper error handling.
///
/// This is the preferred API for Layer 2 domain crates. Returns a `Result`
/// instead of panicking or swallowing errors.
///
/// # Errors
///
/// Returns `FactError::Serialization` if the value or envelope fails to serialize.
pub fn try_encode_fact<T: Serialize>(
    type_id: &FactTypeId,
    schema_version: u16,
    value: &T,
) -> Result<Vec<u8>, FactError> {
    let payload = crate::util::serialization::to_vec(value)?;
    if payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return Err(FactError::PayloadTooLarge {
            size: payload.len() as u64,
            max: MAX_FACT_PAYLOAD_BYTES as u64,
        });
    }
    let envelope = FactEnvelope {
        type_id: type_id.clone(),
        schema_version,
        encoding: FactEncoding::DagCbor,
        payload,
    };
    let bytes = crate::util::serialization::to_vec(&envelope)?;
    Ok(bytes)
}

/// Decode a domain fact with proper error handling.
///
/// This is the preferred API for Layer 2 domain crates. Returns a `Result`
/// with detailed error information instead of `Option`.
///
/// # Errors
///
/// - `FactError::Serialization` if deserialization fails
/// - `FactError::TypeMismatch` if the type ID doesn't match
/// - `FactError::VersionMismatch` if the schema version doesn't match
pub fn try_decode_fact<T: DeserializeOwned>(
    expected_type_id: &FactTypeId,
    expected_schema_version: u16,
    bytes: &[u8],
) -> Result<T, FactError> {
    let envelope: FactEnvelope = crate::util::serialization::from_slice(bytes)?;

    if envelope.type_id.as_str() != expected_type_id.as_str() {
        return Err(FactError::TypeMismatch {
            expected: expected_type_id.to_string(),
            actual: envelope.type_id.to_string(),
        });
    }

    if envelope.schema_version != expected_schema_version {
        return Err(FactError::VersionMismatch {
            expected: expected_schema_version,
            actual: envelope.schema_version,
        });
    }
    if envelope.payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return Err(FactError::PayloadTooLarge {
            size: envelope.payload.len() as u64,
            max: MAX_FACT_PAYLOAD_BYTES as u64,
        });
    }

    let payload = match envelope.encoding {
        FactEncoding::DagCbor => crate::util::serialization::from_slice(&envelope.payload)?,
        FactEncoding::Json => serde_json::from_slice(&envelope.payload)
            .map_err(|e| FactError::InvalidEnvelope(format!("JSON decode failed: {e}")))?,
    };

    Ok(payload)
}

/// Decode a domain fact with version compatibility checking.
///
/// Allows decoding facts where the schema version is compatible (equal or lower)
/// rather than requiring an exact match.
///
/// # Errors
///
/// - `FactError::Serialization` if deserialization fails
/// - `FactError::TypeMismatch` if the type ID doesn't match
/// - `FactError::VersionMismatch` if the schema version is higher than expected
pub fn try_decode_fact_compatible<T: DeserializeOwned>(
    expected_type_id: &FactTypeId,
    max_schema_version: u16,
    bytes: &[u8],
) -> Result<T, FactError> {
    let envelope: FactEnvelope = crate::util::serialization::from_slice(bytes)?;

    if envelope.type_id.as_str() != expected_type_id.as_str() {
        return Err(FactError::TypeMismatch {
            expected: expected_type_id.to_string(),
            actual: envelope.type_id.to_string(),
        });
    }

    if envelope.schema_version > max_schema_version {
        return Err(FactError::VersionMismatch {
            expected: max_schema_version,
            actual: envelope.schema_version,
        });
    }
    if envelope.payload.len() > MAX_FACT_PAYLOAD_BYTES {
        return Err(FactError::PayloadTooLarge {
            size: envelope.payload.len() as u64,
            max: MAX_FACT_PAYLOAD_BYTES as u64,
        });
    }

    let payload = match envelope.encoding {
        FactEncoding::DagCbor => crate::util::serialization::from_slice(&envelope.payload)?,
        FactEncoding::Json => serde_json::from_slice(&envelope.payload)
            .map_err(|e| FactError::InvalidEnvelope(format!("JSON decode failed: {e}")))?,
    };

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestFact {
        value: u32,
        name: String,
    }

    #[test]
    fn test_fact_type_id() {
        let id = FactTypeId::new("test/v1");
        assert_eq!(id.as_str(), "test/v1");
        assert_eq!(id.to_string(), "test/v1");

        let id2: FactTypeId = "test/v2".into();
        assert_eq!(id2.as_str(), "test/v2");
    }

    #[test]
    fn test_try_encode_decode_roundtrip() {
        let type_id = FactTypeId::new("test/v1");
        let fact = TestFact {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = try_encode_fact(&type_id, 1, &fact).unwrap();
        let decoded: TestFact = try_decode_fact(&type_id, 1, &bytes).unwrap();

        assert_eq!(fact, decoded);
    }

    #[test]
    fn test_type_mismatch_error() {
        let type_id = FactTypeId::new("test/v1");
        let wrong_type_id = FactTypeId::new("wrong/v1");
        let fact = TestFact {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = try_encode_fact(&type_id, 1, &fact).unwrap();
        let result: Result<TestFact, _> = try_decode_fact(&wrong_type_id, 1, &bytes);

        assert!(matches!(result, Err(FactError::TypeMismatch { .. })));
    }

    #[test]
    fn test_version_mismatch_error() {
        let type_id = FactTypeId::new("test/v1");
        let fact = TestFact {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = try_encode_fact(&type_id, 2, &fact).unwrap();
        let result: Result<TestFact, _> = try_decode_fact(&type_id, 1, &bytes);

        assert!(matches!(result, Err(FactError::VersionMismatch { .. })));
    }

    #[test]
    fn test_compatible_version_decode() {
        let type_id = FactTypeId::new("test/v1");
        let fact = TestFact {
            value: 42,
            name: "test".to_string(),
        };

        // Encode with version 1
        let bytes = try_encode_fact(&type_id, 1, &fact).unwrap();

        // Decode accepting up to version 2 (compatible)
        let decoded: TestFact = try_decode_fact_compatible(&type_id, 2, &bytes).unwrap();
        assert_eq!(fact, decoded);

        // Encode with version 3
        let bytes_v3 = try_encode_fact(&type_id, 3, &fact).unwrap();

        // Decode accepting only up to version 2 (incompatible)
        let result: Result<TestFact, _> = try_decode_fact_compatible(&type_id, 2, &bytes_v3);
        assert!(matches!(result, Err(FactError::VersionMismatch { .. })));
    }

    #[test]
    fn test_deny_unknown_fields_on_envelope() {
        // Create a valid envelope, then manually add an unknown field
        let type_id = FactTypeId::new("test/v1");
        let fact = TestFact {
            value: 42,
            name: "test".to_string(),
        };

        // First encode normally
        let bytes = try_encode_fact(&type_id, 1, &fact).unwrap();

        // Decode the envelope and verify it works
        let envelope: FactEnvelope = crate::util::serialization::from_slice(&bytes).unwrap();
        assert_eq!(envelope.type_id.as_str(), "test/v1");
        assert_eq!(envelope.schema_version, 1);

        // Create a JSON envelope with unknown field (for testing deny_unknown_fields)
        let json_with_unknown = r#"{"type_id":"test/v1","schema_version":1,"encoding":"DagCbor","payload":[],"unknown_field":"bad"}"#;
        let result: Result<FactEnvelope, _> = serde_json::from_str(json_with_unknown);
        assert!(result.is_err(), "Should reject unknown field 'unknown_field'");
    }

    #[test]
    fn test_encode_rejects_large_payload() {
        #[derive(Serialize)]
        struct BigFact {
            data: Vec<u8>,
        }

        let fact = BigFact {
            data: vec![0u8; MAX_FACT_PAYLOAD_BYTES + 1],
        };

        let err = encode_domain_fact("test/v1", 1, &fact).unwrap_err();
        assert!(matches!(err, FactError::PayloadTooLarge { .. }));
    }

    #[test]
    fn test_decode_rejects_large_payload() {
        let type_id = FactTypeId::new("test/v1");

        let envelope = FactEnvelope {
            type_id: type_id.clone(),
            schema_version: 1,
            encoding: FactEncoding::DagCbor,
            payload: vec![0u8; MAX_FACT_PAYLOAD_BYTES + 1],
        };

        let bytes = crate::util::serialization::to_vec(&envelope).unwrap();
        let result: Result<TestFact, _> = try_decode_fact(&type_id, 1, &bytes);

        assert!(matches!(result, Err(FactError::PayloadTooLarge { .. })));
    }
}
