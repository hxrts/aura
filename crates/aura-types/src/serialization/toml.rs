//! TOML serialization utilities
//!
//! TOML is used for:
//! - Configuration files
//! - Scenario definitions
//! - Human-readable configuration data

use super::error::{Result, SerializationError};
use serde::{Deserialize, Serialize};

/// Serialize a value to a TOML string
///
/// TOML provides human-readable configuration format suitable for:
/// - Application configuration files
/// - Test scenario definitions
/// - Settings management
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::toml;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize)]
/// struct Config {
///     name: String,
///     value: u32,
/// }
///
/// let config = Config { name: "app".to_string(), value: 42 };
/// let toml_string = toml::to_toml_string(&config)?;
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn to_toml_string<T: Serialize>(value: &T) -> Result<String> {
    ::toml::to_string(value).map_err(SerializationError::from)
}

/// Serialize a value to a pretty-printed TOML string
///
/// Produces human-readable output with proper formatting.
pub fn to_toml_pretty<T: Serialize>(value: &T) -> Result<String> {
    ::toml::to_string_pretty(value).map_err(SerializationError::from)
}

/// Deserialize a value from a TOML string
///
/// # Examples
///
/// ```ignore
/// use aura_types::serialization::toml;
/// use serde::{Serialize, Deserialize};
///
/// #[derive(Serialize, Deserialize, PartialEq, Debug)]
/// struct Config {
///     name: String,
///     value: u32,
/// }
///
/// let toml_str = r#"name = "app"\nvalue = 42"#;
/// let config: Config = toml::from_toml_str(toml_str)?;
/// assert_eq!(config.name, "app");
/// # Ok::<(), aura_types::SerializationError>(())
/// ```
pub fn from_toml_str<T: for<'de> Deserialize<'de>>(toml: &str) -> Result<T> {
    ::toml::from_str(toml).map_err(SerializationError::from)
}

/// Serialize to TOML and return as bytes
pub fn to_toml_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_toml_string(value).map(|s| s.into_bytes())
}

/// Serialize to pretty-printed TOML and return as bytes
pub fn to_toml_bytes_pretty<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    to_toml_pretty(value).map(|s| s.into_bytes())
}

/// Deserialize a value from TOML bytes
pub fn from_toml_bytes<T: for<'de> Deserialize<'de>>(toml: &[u8]) -> Result<T> {
    let toml_str =
        std::str::from_utf8(toml).map_err(|e| SerializationError::Utf8Error(e.to_string()))?;
    from_toml_str(toml_str)
}

/// Get the size of a value when serialized to TOML
pub fn toml_size<T: Serialize>(value: &T) -> Result<usize> {
    to_toml_bytes(value).map(|b| b.len())
}

/// Parse a TOML value directly
pub fn parse_toml_value(toml: &str) -> Result<::toml::Value> {
    ::toml::from_str(toml).map_err(SerializationError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestConfig {
        name: String,
        value: i32,
        enabled: bool,
    }

    #[test]
    fn test_to_toml_string() -> Result<()> {
        let config = TestConfig {
            name: "test".to_string(),
            value: 42,
            enabled: true,
        };
        let toml = to_toml_string(&config)?;
        assert!(toml.contains("test"));
        assert!(toml.contains("42"));
        assert!(toml.contains("true"));
        Ok(())
    }

    #[test]
    fn test_from_toml_str() -> Result<()> {
        let toml = r#"
        name = "test"
        value = 42
        enabled = true
        "#;
        let config: TestConfig = from_toml_str(toml)?;
        assert_eq!(config.name, "test");
        assert_eq!(config.value, 42);
        assert!(config.enabled);
        Ok(())
    }

    #[test]
    fn test_roundtrip() -> Result<()> {
        let original = TestConfig {
            name: "test".to_string(),
            value: 42,
            enabled: true,
        };
        let toml = to_toml_string(&original)?;
        let deserialized: TestConfig = from_toml_str(&toml)?;
        assert_eq!(original, deserialized);
        Ok(())
    }

    #[test]
    fn test_pretty_formatting() -> Result<()> {
        let config = TestConfig {
            name: "test".to_string(),
            value: 42,
            enabled: true,
        };
        let regular = to_toml_string(&config)?;
        let pretty = to_toml_pretty(&config)?;

        // Both should deserialize to the same value
        let from_regular: TestConfig = from_toml_str(&regular)?;
        let from_pretty: TestConfig = from_toml_str(&pretty)?;
        assert_eq!(from_regular, from_pretty);

        Ok(())
    }

    #[test]
    fn test_bytes_operations() -> Result<()> {
        let config = TestConfig {
            name: "test".to_string(),
            value: 42,
            enabled: true,
        };
        let bytes = to_toml_bytes(&config)?;
        let deserialized: TestConfig = from_toml_bytes(&bytes)?;
        assert_eq!(config, deserialized);
        Ok(())
    }

    #[test]
    fn test_size_calculation() -> Result<()> {
        let config = TestConfig {
            name: "test".to_string(),
            value: 42,
            enabled: true,
        };
        let size = toml_size(&config)?;
        let actual_bytes = to_toml_bytes(&config)?;

        assert_eq!(size, actual_bytes.len());
        Ok(())
    }

    #[test]
    fn test_invalid_toml() {
        let invalid_toml = r#"name = "test" = "#; // Invalid TOML syntax
        let result: Result<TestConfig> = from_toml_str(invalid_toml);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_toml_value() -> Result<()> {
        let toml = r#"
        [database]
        server = "192.168.1.1"
        ports = [ 8001, 8001, 8002 ]
        "#;
        let value = parse_toml_value(toml)?;
        assert!(value.get("database").is_some());
        Ok(())
    }
}
