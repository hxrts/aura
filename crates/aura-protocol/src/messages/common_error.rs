//! Message-specific error types using unified error system
//!
//! **CLEANUP**: Replaced custom MessageError enum (28 lines with 7+ variants) with
//! unified AuraError from aura-core. This eliminates redundant error definitions while
//! preserving essential error information through structured messages.

use aura_core::AuraError;

/// Convenience type alias for backward compatibility
pub type MessageError = AuraError;

/// Result type for message operations using unified error system
pub type MessageResult<T> = Result<T, AuraError>;

/// Convenience functions for message-specific error types.
/// These now map to unified AuraError variants.

/// Create an unsupported version error (maps to Invalid).
pub fn unsupported_version_error(found: u16, max_supported: u16) -> AuraError {
    AuraError::invalid(format!(
        "Unsupported wire format version: {}, max supported: {}",
        found, max_supported
    ))
}

/// Create a message serialization error (maps to Serialization).
pub fn message_serialization_error(error: impl Into<String>) -> AuraError {
    AuraError::serialization(format!("Message serialization failed: {}", error.into()))
}

/// Create a message deserialization error (maps to Serialization).
pub fn message_deserialization_error(error: impl Into<String>) -> AuraError {
    AuraError::serialization(format!("Message deserialization failed: {}", error.into()))
}

/// Create an invalid message format error (maps to Invalid).
pub fn invalid_message_format_error(error: impl Into<String>) -> AuraError {
    AuraError::invalid(format!("Invalid message format: {}", error.into()))
}

/// Create an invalid envelope size error (maps to Invalid).
pub fn invalid_envelope_size_error(expected: usize, actual: usize) -> AuraError {
    AuraError::invalid(format!(
        "Invalid envelope size: expected {}, got {}",
        expected, actual
    ))
}

/// Create a CID mismatch error (maps to Invalid).
pub fn cid_mismatch_error(expected: impl Into<String>, computed: impl Into<String>) -> AuraError {
    AuraError::invalid(format!(
        "CID mismatch: expected {}, computed {}",
        expected.into(),
        computed.into()
    ))
}

/// Create a message too large error (maps to Invalid).
pub fn message_too_large_error(size: usize, limit: usize) -> AuraError {
    AuraError::invalid(format!(
        "Message too large: {} bytes exceeds limit of {} bytes",
        size, limit
    ))
}
