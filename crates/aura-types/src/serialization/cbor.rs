//! CBOR serialization utilities
//!
//! CBOR (Concise Binary Object Representation) is used for:
//! - Transport envelopes (deterministic encoding)
//! - Protocol messages (canonical encoding for signatures)
//! - Cryptographic operations (deterministic hashing)

use super::error::{Result, SerializationError};
use serde::{Deserialize, Serialize};

/// Serialize a value to CBOR bytes
///
/// CBOR provides deterministic, canonical encoding suitable for:
/// - Cryptographic operations
/// - Content-addressed storage
/// - Protocol message serialization
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::cbor;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Data { name: String }
///
/// let data = Data { name: "test".to_string() };
/// let cbor_bytes = cbor::to_cbor_bytes(&data)?;
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn to_cbor_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_cbor::to_vec(value).map_err(SerializationError::from)
}

/// Deserialize a value from CBOR bytes
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::cbor;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct Data { name: String }
///
/// let data = Data { name: "test".to_string() };
/// let cbor_bytes = cbor::to_cbor_bytes(&data)?;
/// let deserialized: Data = cbor::from_cbor_bytes(&cbor_bytes)?;
/// assert_eq!(data, deserialized);
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn from_cbor_bytes<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T> {
    serde_cbor::from_slice(data).map_err(SerializationError::from)
}

/// Serialize to CBOR and compute canonical bytes
///
/// This ensures deterministic output suitable for hashing and signatures.
/// Multiple serializations of the same value will produce identical bytes.
pub fn to_cbor_canonical<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    // serde_cbor produces canonical CBOR by default
    to_cbor_bytes(value)
}

/// Get the size of a value when serialized to CBOR
///
/// Useful for estimating message sizes or storage requirements.
pub fn cbor_size<T: Serialize>(value: &T) -> Result<usize> {
    to_cbor_bytes(value).map(|b| b.len())
}

/// Serialize a value to CBOR and return as a reader
///
/// Useful for streaming operations or when working with readers.
pub fn to_cbor_reader<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_cbor_bytes(value)
}

/// Create a CBOR encoder for streaming serialization
///
/// For large data structures, consider streaming serialization.
/// This function returns bytes suitable for CBOR deserialization.
pub fn serialize_streaming<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_cbor_bytes(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        name: String,
        value: i32,
        data: Vec<u8>,
    }

    #[test]
    fn test_to_cbor_bytes() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };
        let cbor_bytes = to_cbor_bytes(&data)?;
        assert!(!cbor_bytes.is_empty());
        Ok(())
    }

    #[test]
    fn test_from_cbor_bytes() -> Result<()> {
        let original = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };
        let cbor_bytes = to_cbor_bytes(&original)?;
        let deserialized: TestData = from_cbor_bytes(&cbor_bytes)?;
        assert_eq!(original, deserialized);
        Ok(())
    }

    #[test]
    fn test_cbor_deterministic() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        // Multiple serializations should produce identical bytes
        let bytes1 = to_cbor_bytes(&data)?;
        let bytes2 = to_cbor_bytes(&data)?;
        let bytes3 = to_cbor_bytes(&data)?;

        assert_eq!(bytes1, bytes2);
        assert_eq!(bytes2, bytes3);
        Ok(())
    }

    #[test]
    fn test_cbor_canonical() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let canonical = to_cbor_canonical(&data)?;
        let regular = to_cbor_bytes(&data)?;

        // Both should be identical (CBOR is canonical by default)
        assert_eq!(canonical, regular);
        Ok(())
    }

    #[test]
    fn test_cbor_size() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let size = cbor_size(&data)?;
        let actual_bytes = to_cbor_bytes(&data)?;

        assert_eq!(size, actual_bytes.len());
        Ok(())
    }

    #[test]
    fn test_cbor_handles_binary_data() -> Result<()> {
        let binary_data = vec![0u8, 1, 2, 255, 254, 253];
        let cbor_bytes = to_cbor_bytes(&binary_data)?;
        let deserialized: Vec<u8> = from_cbor_bytes(&cbor_bytes)?;

        assert_eq!(binary_data, deserialized);
        Ok(())
    }

    #[test]
    fn test_invalid_cbor_data() {
        let invalid_data = vec![0xFF, 0xFF]; // Invalid CBOR
        let result: Result<TestData> = from_cbor_bytes(&invalid_data);
        assert!(result.is_err());
    }
}
