//! Structured metadata for tree leaf nodes.
//!
//! # Invariants
//!
//! - Encoded metadata must fit within `MAX_LEAF_META_BYTES` (256 bytes)
//! - All string fields have explicit length limits with units in constant names
//! - `encode()` validates limits before serialization
//! - `decode()` accepts empty bytes and returns default (not an error)
//! - Round-trip: `decode(encode(x)) == x` for all valid inputs
//!
//! # Safety
//!
//! This module is `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

use crate::{tree::types::LeafMetadata, AuraError};
use serde::{Deserialize, Serialize};

/// Maximum bytes for device nickname suggestion in metadata.
pub const NICKNAME_SUGGESTION_BYTES_MAX: usize = 64;

/// Maximum bytes for platform hint in metadata.
pub const PLATFORM_BYTES_MAX: usize = 16;

/// Structured metadata for device leaf nodes.
///
/// Serialized into `LeafMetadata` bytes using compact bincode encoding.
/// Total encoded size must fit within `MAX_LEAF_META_BYTES` (256).
///
/// This stores the **initial** nickname_suggestion at enrollment time.
/// Post-enrollment updates go through `DeviceNamingFact` in the journal.
///
/// # Version History
///
/// - v1: Initial version with nickname_suggestion, platform, enrolled_at_ms
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceLeafMetadata {
    /// Initial nickname suggestion (what the device wants to be called).
    ///
    /// Set during enrollment from the invitation payload.
    /// Limited to `NICKNAME_SUGGESTION_BYTES_MAX` (64) bytes.
    /// `None` means no suggestion was provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname_suggestion: Option<String>,

    /// Platform hint (e.g., "ios", "android", "macos", "windows", "linux", "browser").
    ///
    /// Informational only, not validated against a fixed set.
    /// Limited to `PLATFORM_BYTES_MAX` (16) bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,

    /// Enrollment timestamp (milliseconds since Unix epoch).
    ///
    /// Set by the enrollment ceremony. Used for audit/display, not ordering.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled_at_ms: Option<u64>,

    /// Schema version for forward compatibility.
    ///
    /// Increment when adding fields. Decoders should accept unknown versions
    /// gracefully (fields may be missing or have different types).
    #[serde(default)]
    pub version: u8,
}

impl DeviceLeafMetadata {
    /// Current schema version.
    pub const VERSION_CURRENT: u8 = 1;

    /// Create new metadata with a nickname suggestion.
    ///
    /// Empty strings are normalized to `None`.
    #[must_use]
    pub fn with_nickname_suggestion(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            nickname_suggestion: if name.is_empty() { None } else { Some(name) },
            version: Self::VERSION_CURRENT,
            ..Default::default()
        }
    }

    /// Create new empty metadata.
    #[must_use]
    pub fn new() -> Self {
        Self {
            version: Self::VERSION_CURRENT,
            ..Default::default()
        }
    }

    /// Set the platform hint. Builder pattern.
    ///
    /// Empty strings are normalized to `None`.
    #[must_use]
    pub fn with_platform(mut self, platform: impl Into<String>) -> Self {
        let platform = platform.into();
        self.platform = if platform.is_empty() {
            None
        } else {
            Some(platform)
        };
        self
    }

    /// Set the enrollment timestamp. Builder pattern.
    #[must_use]
    pub fn with_enrolled_at_ms(mut self, ts_ms: u64) -> Self {
        self.enrolled_at_ms = Some(ts_ms);
        self
    }

    /// Encode to `LeafMetadata` bytes.
    ///
    /// # Errors
    ///
    /// Returns `AuraError::Invalid` if:
    /// - `nickname_suggestion` exceeds `NICKNAME_SUGGESTION_BYTES_MAX`
    /// - `platform` exceeds `PLATFORM_BYTES_MAX`
    /// - Encoded size exceeds `MAX_LEAF_META_BYTES`
    pub fn encode(&self) -> Result<LeafMetadata, AuraError> {
        // Validate nickname length
        if let Some(ref name) = self.nickname_suggestion {
            if name.len() > NICKNAME_SUGGESTION_BYTES_MAX {
                return Err(AuraError::invalid(format!(
                    "nickname_suggestion exceeds {} bytes (got {})",
                    NICKNAME_SUGGESTION_BYTES_MAX,
                    name.len()
                )));
            }
        }

        // Validate platform length
        if let Some(ref platform) = self.platform {
            if platform.len() > PLATFORM_BYTES_MAX {
                return Err(AuraError::invalid(format!(
                    "platform exceeds {} bytes (got {})",
                    PLATFORM_BYTES_MAX,
                    platform.len()
                )));
            }
        }

        let bytes = bincode::serialize(self)
            .map_err(|e| AuraError::serialization(format!("DeviceLeafMetadata encode: {e}")))?;

        let meta = LeafMetadata::try_new(bytes)?;

        // Paired assertion: verify round-trip in debug builds
        debug_assert_eq!(
            Self::decode(&meta).ok(),
            Some(self.clone()),
            "DeviceLeafMetadata round-trip failed"
        );

        Ok(meta)
    }

    /// Decode from `LeafMetadata` bytes.
    ///
    /// Empty bytes return `Default::default()` (not an error).
    ///
    /// # Errors
    ///
    /// Returns `AuraError::Serialization` if bytes are non-empty but malformed.
    pub fn decode(meta: &LeafMetadata) -> Result<Self, AuraError> {
        if meta.as_bytes().is_empty() {
            return Ok(Self::default());
        }

        let result: Self = bincode::deserialize(meta.as_bytes())
            .map_err(|e| AuraError::serialization(format!("DeviceLeafMetadata decode: {e}")))?;

        // Paired assertion: verify constraints hold after decode
        debug_assert!(
            result
                .nickname_suggestion
                .as_ref()
                .map_or(true, |s| s.len() <= NICKNAME_SUGGESTION_BYTES_MAX),
            "decoded nickname_suggestion exceeds limit"
        );
        debug_assert!(
            result
                .platform
                .as_ref()
                .map_or(true, |s| s.len() <= PLATFORM_BYTES_MAX),
            "decoded platform exceeds limit"
        );

        Ok(result)
    }

    /// Check if this metadata has any meaningful content.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nickname_suggestion.is_none()
            && self.platform.is_none()
            && self.enrolled_at_ms.is_none()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests use expect for simplicity
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        let original = DeviceLeafMetadata::with_nickname_suggestion("My Laptop")
            .with_platform("macos")
            .with_enrolled_at_ms(1_234_567_890);

        let encoded = original.encode().expect("encode should succeed");
        let decoded = DeviceLeafMetadata::decode(&encoded).expect("decode should succeed");

        assert_eq!(original, decoded);
    }

    #[test]
    fn test_empty_bytes_returns_default() {
        let empty = LeafMetadata::empty();
        let decoded = DeviceLeafMetadata::decode(&empty).expect("decode empty should succeed");
        assert_eq!(decoded, DeviceLeafMetadata::default());
    }

    #[test]
    fn test_rejects_oversized_nickname() {
        let long_name = "x".repeat(NICKNAME_SUGGESTION_BYTES_MAX + 1);
        let meta = DeviceLeafMetadata::with_nickname_suggestion(long_name);
        assert!(meta.encode().is_err());
    }

    #[test]
    fn test_rejects_oversized_platform() {
        let long_platform = "x".repeat(PLATFORM_BYTES_MAX + 1);
        let meta = DeviceLeafMetadata::new().with_platform(long_platform);
        assert!(meta.encode().is_err());
    }

    #[test]
    fn test_normalizes_empty_strings() {
        let meta = DeviceLeafMetadata::with_nickname_suggestion("").with_platform("");
        assert!(meta.nickname_suggestion.is_none());
        assert!(meta.platform.is_none());
    }

    #[test]
    fn test_is_empty() {
        assert!(DeviceLeafMetadata::default().is_empty());
        assert!(!DeviceLeafMetadata::with_nickname_suggestion("Test").is_empty());
        assert!(!DeviceLeafMetadata::new().with_platform("macos").is_empty());
    }

    #[test]
    fn test_max_size_fits_within_leaf_metadata() {
        // Create metadata with maximum allowed content
        let max_name = "x".repeat(NICKNAME_SUGGESTION_BYTES_MAX);
        let max_platform = "y".repeat(PLATFORM_BYTES_MAX);

        let meta = DeviceLeafMetadata::with_nickname_suggestion(max_name)
            .with_platform(max_platform)
            .with_enrolled_at_ms(u64::MAX);

        // Should fit within MAX_LEAF_META_BYTES (256)
        let encoded = meta.encode().expect("max content should fit");
        assert!(encoded.as_bytes().len() <= 256);
    }
}
