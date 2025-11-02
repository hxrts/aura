//! Serialization utilities with proper error handling
//!
//! Provides CBOR serialization/deserialization with proper error propagation instead of panics.
//! Includes trait-based abstractions for easy format swapping in the future.

use crate::error::{AuraError, Result as AuraResult};
use serde::{Deserialize, Serialize};

/// Trait for types that can be serialized to bytes
pub trait Serializable: Serialize {
    /// Serialize to CBOR bytes
    fn to_bytes(&self) -> AuraResult<Vec<u8>>
    where
        Self: Sized,
    {
        serialize_cbor(self)
    }
}

/// Trait for types that can be deserialized from bytes
pub trait Deserializable: for<'de> Deserialize<'de> {
    /// Deserialize from CBOR bytes
    fn from_bytes(bytes: &[u8]) -> AuraResult<Self> {
        deserialize_cbor(bytes)
    }
}

/// Blanket implementation for all Serialize + Sized types
impl<T: Serialize + Sized> Serializable for T {}

/// Blanket implementation for all Deserialize types
impl<T: for<'de> Deserialize<'de> + Sized> Deserializable for T {}

/// Serialize a value to CBOR with proper error handling
///
/// # Errors
///
/// Returns error if serialization fails (e.g., value contains non-serializable data).
///
/// # Example
///
/// ```ignore
/// use aura_journal::serialization::serialize_cbor;
///
/// #[derive(serde::Serialize)]
/// struct MyData {
///     value: u64,
/// }
///
/// let data = MyData { value: 42 };
/// let bytes = serialize_cbor(&data)?;
/// ```
pub fn serialize_cbor<T: Serialize>(value: &T) -> AuraResult<Vec<u8>> {
    serde_cbor::to_vec(value)
        .map_err(|e| AuraError::serialization_failed(format!("CBOR serialization failed: {e}")))
}

/// Deserialize a value from CBOR with proper error handling
///
/// # Errors
///
/// Returns error if deserialization fails (e.g., corrupted data, version mismatch).
///
/// # Example
///
/// ```ignore
/// use aura_journal::serialization::deserialize_cbor;
///
/// #[derive(serde::Deserialize)]
/// struct MyData {
///     value: u64,
/// }
///
/// let bytes: &[u8] = &[/* ... */];
/// let data: MyData = deserialize_cbor(bytes)?;
/// ```
pub fn deserialize_cbor<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> AuraResult<T> {
    serde_cbor::from_slice(bytes)
        .map_err(|e| AuraError::deserialization_failed(format!("CBOR deserialization failed: {e}")))
}

/// Short alias for serialize_cbor for convenience
///
/// Serializes a value to CBOR bytes with proper error handling.
///
/// # Example
///
/// ```ignore
/// let data = MyData { value: 42 };
/// let bytes = to_cbor_bytes(&data)?;
/// ```
pub fn to_cbor_bytes<T: Serialize>(value: &T) -> AuraResult<Vec<u8>> {
    serialize_cbor(value)
}

/// Short alias for deserialize_cbor for convenience
///
/// Deserializes a value from CBOR bytes with proper error handling.
///
/// # Example
///
/// ```ignore
/// let data: MyData = from_cbor_bytes(&bytes)?;
/// ```
pub fn from_cbor_bytes<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> AuraResult<T> {
    deserialize_cbor(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        value: u64,
        name: String,
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let original = TestData {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = serialize_cbor(&original).unwrap();
        let deserialized: TestData = deserialize_cbor(&bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_deserialize_invalid_data() {
        let invalid_bytes = &[0xFF, 0xFF, 0xFF];
        let result: Result<TestData, _> = deserialize_cbor(invalid_bytes);
        assert!(result.is_err());
        assert!(result.is_err());
    }

    #[test]
    fn test_to_cbor_bytes_alias() {
        let original = TestData {
            value: 42,
            name: "test".to_string(),
        };

        let bytes = to_cbor_bytes(&original).unwrap();
        let deserialized: TestData = from_cbor_bytes(&bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_trait_based_serialization() {
        let original = TestData {
            value: 42,
            name: "test".to_string(),
        };

        // Use trait methods
        let bytes = original.to_bytes().unwrap();
        let deserialized: TestData = TestData::from_bytes(&bytes).unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_trait_based_deserialization_error() {
        let invalid_bytes = &[0xFF, 0xFF, 0xFF];
        let result: Result<TestData, _> = TestData::from_bytes(invalid_bytes);
        assert!(result.is_err());
    }
}
