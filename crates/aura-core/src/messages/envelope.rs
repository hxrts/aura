//! Generic message envelope wrapper
//!
//! Provides a unified envelope format for all message types.

use crate::types::identifiers::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

use super::constants::WIRE_FORMAT_VERSION;

/// Generic message envelope for wire protocol communication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireEnvelope<T> {
    /// Message format version
    pub version: u16,
    /// Session this message belongs to (optional for some protocols)
    pub session_id: Option<SessionId>,
    /// Device that sent this message
    pub sender_id: DeviceId,
    /// Message sequence number
    pub sequence: u64,
    /// Timestamp when message was created
    pub timestamp: u64,
    /// The actual message payload
    pub payload: T,
}

impl<T> WireEnvelope<T> {
    /// Create a new message envelope
    #[must_use]
    pub fn new(
        session_id: Option<SessionId>,
        sender_id: DeviceId,
        sequence: u64,
        timestamp: u64,
        payload: T,
    ) -> Self {
        Self {
            version: WIRE_FORMAT_VERSION,
            session_id,
            sender_id,
            sequence,
            timestamp,
            payload,
        }
    }

    /// Check if the message version is compatible
    pub fn is_version_compatible(&self, max_supported: u16) -> bool {
        self.version <= max_supported
    }

    /// Validate envelope invariants after deserialization.
    ///
    /// Returns `Ok(())` if the envelope is well-formed, or an error describing
    /// which invariant was violated. Call this after deserializing an envelope
    /// to ensure it meets structural requirements.
    pub fn validate(&self) -> std::result::Result<(), EnvelopeValidationError> {
        // Check version is within supported range
        if self.version == 0 {
            return Err(EnvelopeValidationError::InvalidVersion(self.version));
        }
        if self.version > WIRE_FORMAT_VERSION {
            return Err(EnvelopeValidationError::UnsupportedVersion {
                received: self.version,
                max_supported: WIRE_FORMAT_VERSION,
            });
        }
        Ok(())
    }
}

/// Errors that can occur during envelope validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnvelopeValidationError {
    #[error("Invalid version: {0}")]
    InvalidVersion(u16),

    #[error("Unsupported version {received}, max supported is {max_supported}")]
    UnsupportedVersion { received: u16, max_supported: u16 },
}
