//! Unified serialization utilities for all supported formats
//!
//! This module provides a consistent API for serializing and deserializing data
//! across multiple formats: JSON, CBOR, Bincode, TOML, and Postcard.
//!
//! # Format Selection Guide
//!
//! Choose the right format for your use case:
//!
//! - **JSON**: Human-readable, widely compatible, text-based
//!   - Best for: APIs, configuration, human-readable output
//!   - Use: `json::to_json_string()`, `json::from_json_str()`
//!
//! - **CBOR**: Compact binary, deterministic, standard format
//!   - Best for: Protocol messages, canonical encoding, interchange format
//!   - Use: `cbor::to_cbor_bytes()`, `cbor::from_cbor_bytes()`
//!
//! - **Bincode**: Fast binary, compact, deterministic
//!   - Best for: Hashing, state snapshots, performance-critical code
//!   - Use: `bincode::to_bincode_bytes()`, `bincode::from_bincode_bytes()`
//!
//! - **TOML**: Human-readable, configuration-friendly
//!   - Best for: Configuration files, scenario definitions
//!   - Use: `toml::to_toml_string()`, `toml::from_toml_str()`
//!
//! - **Postcard**: Ultra-compact binary (optional feature)
//!   - Best for: Trace compression, minimal message size
//!   - Use: `postcard::to_postcard_bytes()`, `postcard::from_postcard_bytes()`
//!
//! # Error Handling
//!
//! All serialization operations return `Result<T>` which is aliased to
//! `std::result::Result<T, SerializationError>`. Use the `SerializationError` type
//! to handle serialization failures:
//!
//! ```ignore
//! use aura_types::serialization::{json, SerializationError};
//!
//! match json::to_json_string(&data) {
//!     Ok(json) => println!("{}", json),
//!     Err(e) if e.is_retryable() => println!("Retryable error: {}", e),
//!     Err(e) => eprintln!("Fatal error: {}", e),
//! }
//! ```

pub mod bincode;
pub mod cbor;
pub mod error;
pub mod json;
pub mod toml;

#[cfg(feature = "postcard-support")]
pub mod postcard;

// Re-export error types and result type
pub use error::{Result, SerializationError};

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
    fn test_json_roundtrip() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let json = json::to_json_string(&data)?;
        let deserialized: TestData = json::from_json_str(&json)?;
        assert_eq!(data, deserialized);
        Ok(())
    }

    #[test]
    fn test_cbor_roundtrip() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let bytes = cbor::to_cbor_bytes(&data)?;
        let deserialized: TestData = cbor::from_cbor_bytes(&bytes)?;
        assert_eq!(data, deserialized);
        Ok(())
    }

    #[test]
    fn test_bincode_roundtrip() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let bytes = bincode::to_bincode_bytes(&data)?;
        let deserialized: TestData = bincode::from_bincode_bytes(&bytes)?;
        assert_eq!(data, deserialized);
        Ok(())
    }

    #[test]
    fn test_toml_roundtrip() -> Result<()> {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let toml = toml::to_toml_string(&data)?;
        let deserialized: TestData = toml::from_toml_str(&toml)?;
        assert_eq!(data, deserialized);
        Ok(())
    }
}
