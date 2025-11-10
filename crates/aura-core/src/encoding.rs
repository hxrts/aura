//! Unified encoding utilities for hex and base64
//!
//! This module provides consistent, reusable encoding/decoding helpers for
//! common patterns across the codebase, eliminating duplication of hex/base64
//! conversion logic.
//!
//! # Usage
//!
//! For types with byte representations:
//!
//! ```ignore
//! use aura_core::encoding::{ToHex, FromHex};
//!
//! let bytes = vec![1, 2, 3, 4];
//! let hex_str = bytes.to_hex();
//! let restored = Vec::<u8>::from_hex(&hex_str)?;
//! assert_eq!(bytes, restored);
//! ```

use base64::Engine;
use std::fmt;

/// Trait for types that can be converted to hex strings
pub trait ToHex {
    /// Convert to hex string representation
    fn to_hex(&self) -> String;
}

/// Trait for types that can be created from hex strings
pub trait FromHex: Sized {
    /// Error type for hex decoding
    type Error: fmt::Display;

    /// Create from hex string representation
    fn from_hex(hex_str: &str) -> Result<Self, Self::Error>;
}

/// Trait for types that can be converted to base64 strings
pub trait ToBase64 {
    /// Convert to base64 string representation
    fn to_base64(&self) -> String;
}

/// Trait for types that can be created from base64 strings
pub trait FromBase64: Sized {
    /// Error type for base64 decoding
    type Error: fmt::Display;

    /// Create from base64 string representation
    fn from_base64(b64_str: &str) -> Result<Self, Self::Error>;
}

// ============================================================================
// Standard implementations for common types
// ============================================================================

impl ToHex for Vec<u8> {
    fn to_hex(&self) -> String {
        hex::encode(self)
    }
}

impl FromHex for Vec<u8> {
    type Error = hex::FromHexError;

    fn from_hex(hex_str: &str) -> Result<Self, Self::Error> {
        hex::decode(hex_str)
    }
}

impl ToHex for [u8] {
    fn to_hex(&self) -> String {
        hex::encode(self)
    }
}

impl ToHex for &[u8] {
    fn to_hex(&self) -> String {
        hex::encode(self)
    }
}

// For fixed-size arrays
impl<const N: usize> ToHex for [u8; N] {
    fn to_hex(&self) -> String {
        hex::encode(self)
    }
}

impl ToBase64 for Vec<u8> {
    fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self)
    }
}

impl FromBase64 for Vec<u8> {
    type Error = base64::DecodeError;

    fn from_base64(b64_str: &str) -> Result<Self, Self::Error> {
        base64::engine::general_purpose::STANDARD.decode(b64_str)
    }
}

impl ToBase64 for [u8] {
    fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self)
    }
}

impl ToBase64 for &[u8] {
    fn to_base64(&self) -> String {
        base64::engine::general_purpose::STANDARD.encode(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)] // Test code - panics are expected on failure
    fn test_hex_roundtrip() {
        let bytes = vec![1, 2, 3, 255, 254, 253];
        let hex = bytes.to_hex();
        let restored = Vec::<u8>::from_hex(&hex).expect("should decode");
        assert_eq!(bytes, restored);
    }

    #[test]
    #[allow(clippy::expect_used)] // Test code - panics are expected on failure
    fn test_base64_roundtrip() {
        let bytes = vec![1, 2, 3, 255, 254, 253];
        let b64 = bytes.to_base64();
        let restored = Vec::<u8>::from_base64(&b64).expect("should decode");
        assert_eq!(bytes, restored);
    }

    #[test]
    fn test_hex_slice() {
        let bytes: &[u8] = &[0xAB, 0xCD, 0xEF];
        let hex = bytes.to_hex();
        assert_eq!(hex, "abcdef");
    }

    #[test]
    fn test_hex_array() {
        let bytes: [u8; 3] = [0xAB, 0xCD, 0xEF];
        let hex = bytes.to_hex();
        assert_eq!(hex, "abcdef");
    }
}
