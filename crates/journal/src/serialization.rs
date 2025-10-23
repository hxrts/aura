//! Serialization utilities with proper error handling
//!
//! Provides CBOR serialization/deserialization with proper error propagation instead of panics.

use crate::LedgerError;
use serde::{Deserialize, Serialize};

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
pub fn serialize_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>, LedgerError> {
    serde_cbor::to_vec(value)
        .map_err(|e| LedgerError::SerializationFailed(format!(
            "CBOR serialization failed: {e}"
        )))
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
pub fn deserialize_cbor<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, LedgerError> {
    serde_cbor::from_slice(bytes)
        .map_err(|e| LedgerError::SerializationFailed(format!(
            "CBOR deserialization failed: {e}"
        )))
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
        assert!(matches!(result, Err(LedgerError::SerializationFailed(_))));
    }
}

