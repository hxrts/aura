//! Postcard serialization utilities (optional)
//!
//! Postcard is used for:
//! - Compact binary serialization
//! - Simulation trace compression
//! - Reduced network overhead in trace protocols

use super::error::{Result, SerializationError};
use serde::{Deserialize, Serialize};

/// Serialize a value to postcard bytes (compact binary)
///
/// Postcard provides extremely compact binary serialization suitable for:
/// - Simulation trace recording
/// - Network transmission of large data
/// - Compressed storage of traces
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::postcard;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct TraceEvent { timestamp: u64, data: Vec<u8> }
///
/// let event = TraceEvent { timestamp: 1000, data: vec![1, 2, 3] };
/// let postcard_bytes = postcard::to_postcard_bytes(&event)?;
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn to_postcard_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    ::postcard::to_allocvec(value).map_err(|e| SerializationError::postcard(e.to_string()))
}

/// Deserialize a value from postcard bytes
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::postcard;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct TraceEvent { timestamp: u64, data: Vec<u8> }
///
/// let event = TraceEvent { timestamp: 1000, data: vec![1, 2, 3] };
/// let postcard_bytes = postcard::to_postcard_bytes(&event)?;
/// let deserialized: TraceEvent = postcard::from_postcard_bytes(&postcard_bytes)?;
/// assert_eq!(event, deserialized);
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn from_postcard_bytes<'a, T: Deserialize<'a>>(data: &'a [u8]) -> Result<T> {
    ::postcard::from_bytes(data).map_err(|e| SerializationError::postcard(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        id: u64,
        data: Vec<u8>,
    }

    #[test]
    fn test_to_postcard_bytes() -> Result<()> {
        let data = TestData {
            id: 1000,
            data: vec![1, 2, 3, 4, 5],
        };
        let postcard_bytes = to_postcard_bytes(&data)?;
        assert!(!postcard_bytes.is_empty());
        Ok(())
    }

    #[test]
    fn test_from_postcard_bytes() -> Result<()> {
        let original = TestData {
            id: 1000,
            data: vec![1, 2, 3, 4, 5],
        };
        let postcard_bytes = to_postcard_bytes(&original)?;
        let deserialized: TestData = from_postcard_bytes(&postcard_bytes)?;
        assert_eq!(original, deserialized);
        Ok(())
    }

    #[test]
    fn test_postcard_deterministic() -> Result<()> {
        let data = TestData {
            id: 1000,
            data: vec![1, 2, 3, 4, 5],
        };

        // Multiple serializations should produce identical bytes
        let bytes1 = to_postcard_bytes(&data)?;
        let bytes2 = to_postcard_bytes(&data)?;
        let bytes3 = to_postcard_bytes(&data)?;

        assert_eq!(bytes1, bytes2);
        assert_eq!(bytes2, bytes3);
        Ok(())
    }

    #[test]
    fn test_postcard_compact() -> Result<()> {
        let data = TestData {
            id: 1000,
            data: vec![1, 2, 3, 4, 5],
        };

        // Postcard should be more compact than bincode/CBOR
        let postcard_bytes = to_postcard_bytes(&data)?;
        #[allow(clippy::expect_used)] // Test code - panics are expected on failure
        let json_bytes = serde_json::to_vec(&data).expect("JSON serialization");

        assert!(postcard_bytes.len() < json_bytes.len());
        Ok(())
    }

    #[test]
    fn test_invalid_postcard_data() {
        let invalid_data = vec![0xFF, 0xFF, 0xFF]; // Invalid postcard
        let result: Result<TestData> = from_postcard_bytes(&invalid_data);
        assert!(result.is_err());
    }
}
