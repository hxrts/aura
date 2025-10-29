//! JSON serialization utilities

use super::error::{Result, SerializationError};
use serde::{Deserialize, Serialize};

/// Serialize a value to a JSON string (compact format)
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::json;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Data { name: String }
///
/// let data = Data { name: "test".to_string() };
/// let json = json::to_json_string(&data)?;
/// assert_eq!(json, r#"{"name":"test"}"#);
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn to_json_string<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(SerializationError::from)
}

/// Serialize a value to a pretty-printed JSON string
///
/// Produces human-readable output with indentation and line breaks.
///
/// # Examples
///
/// ```ignore
/// use aura_serialization::json;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Data { name: String }
///
/// let data = Data { name: "test".to_string() };
/// let json = json::to_json_pretty(&data)?;
/// # Ok::<(), aura_serialization::SerializationError>(())
/// ```
pub fn to_json_pretty<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(SerializationError::from)
}

/// Deserialize a value from a JSON string
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::json;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct Data { name: String }
///
/// let json = r#"{"name":"test"}"#;
/// let data: Data = json::from_json_str(json)?;
/// assert_eq!(data.name, "test");
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn from_json_str<'a, T: Deserialize<'a>>(json: &'a str) -> Result<T> {
    serde_json::from_str(json).map_err(SerializationError::from)
}

/// Deserialize a value from a JSON byte slice
pub fn from_json_slice<'a, T: Deserialize<'a>>(json: &'a [u8]) -> Result<T> {
    serde_json::from_slice(json).map_err(SerializationError::from)
}

/// Serialize to JSON and return as bytes
pub fn to_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(SerializationError::from)
}

/// Serialize to pretty-printed JSON and return as bytes
pub fn to_json_bytes_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(value).map_err(SerializationError::from)
}

/// Deserialize a value from JSON bytes
pub fn from_json_bytes<'a, T: Deserialize<'a>>(json: &'a [u8]) -> Result<T> {
    serde_json::from_slice(json).map_err(SerializationError::from)
}

/// Get the size of a value when serialized to JSON (compact format)
///
/// Useful for estimating message sizes or storage requirements.
pub fn json_size<T: Serialize>(value: &T) -> Result<usize> {
    to_json_bytes(value).map(|b| b.len())
}

/// Get the size of a value when serialized to pretty-printed JSON
pub fn json_pretty_size<T: Serialize>(value: &T) -> Result<usize> {
    to_json_bytes_pretty(value).map(|b| b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        name: String,
        value: i32,
    }

    #[test]
    fn test_to_json_string() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        let json = to_json_string(&data)?;
        assert!(json.contains("test"));
        assert!(json.contains("42"));
        Ok(())
    }

    #[test]
    fn test_from_json_str() -> Result<()> {
        let json = r#"{"name":"test","value":42}"#;
        let data: TestData = from_json_str(json)?;
        assert_eq!(data.name, "test");
        assert_eq!(data.value, 42);
        Ok(())
    }

    #[test]
    fn test_roundtrip() -> Result<()> {
        let original = TestData {
            name: "test".to_string(),
            value: 42,
        };
        let json = to_json_string(&original)?;
        let deserialized: TestData = from_json_str(&json)?;
        assert_eq!(original, deserialized);
        Ok(())
    }

    #[test]
    fn test_pretty_formatting() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        let compact = to_json_string(&data)?;
        let pretty = to_json_pretty(&data)?;

        // Pretty should have newlines and indentation
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));

        // Both should deserialize to the same value
        let from_compact: TestData = from_json_str(&compact)?;
        let from_pretty: TestData = from_json_str(&pretty)?;
        assert_eq!(from_compact, from_pretty);

        Ok(())
    }

    #[test]
    fn test_bytes_operations() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        let bytes = to_json_bytes(&data)?;
        let deserialized: TestData = from_json_bytes(&bytes)?;
        assert_eq!(data, deserialized);
        Ok(())
    }

    #[test]
    fn test_size_calculation() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };
        let compact_size = json_size(&data)?;
        let pretty_size = json_pretty_size(&data)?;

        // Pretty printed should be larger
        assert!(pretty_size > compact_size);
        Ok(())
    }

    #[test]
    fn test_invalid_json() {
        let invalid_json = r#"{"name":"test",}"#; // Trailing comma
        let result: Result<TestData> = from_json_str(invalid_json);
        assert!(result.is_err());
    }
}
