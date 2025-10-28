//! Bincode serialization utilities
//!
//! Bincode is used for:
//! - Deterministic hashing of data structures
//! - Fast, compact binary serialization
//! - Key share storage
//! - Capability token serialization

use super::error::{Result, SerializationError};
use serde::{Deserialize, Serialize};

/// Serialize a value to bincode bytes (deterministic)
///
/// Bincode provides fast, compact binary serialization suitable for:
/// - Cryptographic operations requiring deterministic output
/// - High-performance serialization
/// - Secure storage of key material
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::bincode;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct KeyShare { data: Vec<u8> }
///
/// let share = KeyShare { data: vec![1, 2, 3] };
/// let bincode_bytes = bincode::to_bincode_bytes(&share)?;
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn to_bincode_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    bincode::serialize(value).map_err(|e| SerializationError::bincode(e.to_string()))
}

/// Deserialize a value from bincode bytes
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::bincode;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct KeyShare { data: Vec<u8> }
///
/// let share = KeyShare { data: vec![1, 2, 3] };
/// let bincode_bytes = bincode::to_bincode_bytes(&share)?;
/// let deserialized: KeyShare = bincode::from_bincode_bytes(&bincode_bytes)?;
/// assert_eq!(share, deserialized);
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn from_bincode_bytes<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T> {
    bincode::deserialize(data).map_err(|e| SerializationError::bincode(e.to_string()))
}

/// Serialize a value to bincode with the standard configuration
///
/// This is an alias for `to_bincode_bytes` for explicit configuration.
pub fn to_bincode_standard<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_bincode_bytes(value)
}

/// Deserialize a value from bincode with standard configuration
pub fn from_bincode_standard<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T> {
    from_bincode_bytes(data)
}

/// Get the size of a value when serialized to bincode
///
/// Useful for estimating storage or transmission sizes.
pub fn bincode_size<T: Serialize>(value: &T) -> Result<usize> {
    to_bincode_bytes(value).map(|b| b.len())
}

/// Serialize a value to bincode and return both the bytes and length
pub fn to_bincode_with_len<T: Serialize>(value: &T) -> Result<(Vec<u8>, usize)> {
    let bytes = to_bincode_bytes(value)?;
    let len = bytes.len();
    Ok((bytes, len))
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
    fn test_to_bincode_bytes() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };
        let bincode_bytes = to_bincode_bytes(&data)?;
        assert!(!bincode_bytes.is_empty());
        Ok(())
    }

    #[test]
    fn test_from_bincode_bytes() -> Result<()> {
        let original = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };
        let bincode_bytes = to_bincode_bytes(&original)?;
        let deserialized: TestData = from_bincode_bytes(&bincode_bytes)?;
        assert_eq!(original, deserialized);
        Ok(())
    }

    #[test]
    fn test_bincode_deterministic() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        // Multiple serializations should produce identical bytes
        let bytes1 = to_bincode_bytes(&data)?;
        let bytes2 = to_bincode_bytes(&data)?;
        let bytes3 = to_bincode_bytes(&data)?;

        assert_eq!(bytes1, bytes2);
        assert_eq!(bytes2, bytes3);
        Ok(())
    }

    #[test]
    fn test_bincode_size() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let size = bincode_size(&data)?;
        let actual_bytes = to_bincode_bytes(&data)?;

        assert_eq!(size, actual_bytes.len());
        Ok(())
    }

    #[test]
    fn test_bincode_with_len() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        let (bytes, len) = to_bincode_with_len(&data)?;
        assert_eq!(len, bytes.len());

        let deserialized: TestData = from_bincode_bytes(&bytes)?;
        assert_eq!(data, deserialized);
        Ok(())
    }

    #[test]
    fn test_bincode_compact() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
            data: vec![1, 2, 3, 4, 5],
        };

        // Bincode should be more compact than JSON in most cases
        let bincode_bytes = to_bincode_bytes(&data)?;
        let json_bytes = serde_json::to_vec(&data).expect("JSON serialization");

        assert!(bincode_bytes.len() < json_bytes.len());
        Ok(())
    }

    #[test]
    fn test_invalid_bincode_data() {
        let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF]; // Invalid bincode
        let result: Result<TestData> = from_bincode_bytes(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_bincode_handles_binary_data() -> Result<()> {
        let binary_data = vec![0u8, 1, 2, 255, 254, 253];
        let bincode_bytes = to_bincode_bytes(&binary_data)?;
        let deserialized: Vec<u8> = from_bincode_bytes(&bincode_bytes)?;

        assert_eq!(binary_data, deserialized);
        Ok(())
    }
}
