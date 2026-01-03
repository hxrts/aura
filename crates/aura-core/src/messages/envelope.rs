//! Generic message envelope wrapper
//!
//! Provides a unified envelope format for all message types.

use crate::types::identifiers::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};
use std::fmt;

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
    pub sequence: MessageSequence,
    /// Timestamp when message was created
    pub timestamp: MessageTimestamp,
    /// The actual message payload
    pub payload: T,
}

impl<T> WireEnvelope<T> {
    /// Create a new message envelope
    #[must_use]
    pub fn new(
        session_id: Option<SessionId>,
        sender_id: DeviceId,
        sequence: MessageSequence,
        timestamp: MessageTimestamp,
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
    pub fn validate(
        &self,
        previous_sequence: Option<MessageSequence>,
    ) -> std::result::Result<(), EnvelopeValidationError> {
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
        if let Some(previous) = previous_sequence {
            if self.sequence <= previous {
                return Err(EnvelopeValidationError::NonMonotonicSequence {
                    previous,
                    current: self.sequence,
                });
            }
        }

        if self.timestamp.value() == 0 {
            return Err(EnvelopeValidationError::InvalidTimestamp(
                self.timestamp.value(),
            ));
        }

        Ok(())
    }
}

/// Monotonic sequence number for wire envelopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MessageSequence(u64);

impl MessageSequence {
    /// Create a new message sequence.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Access the underlying sequence value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MessageSequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Timestamp for wire envelopes (ms since UNIX epoch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageTimestamp(u64);

impl MessageTimestamp {
    /// Create a new message timestamp.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Access the underlying timestamp value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for MessageTimestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Errors that can occur during envelope validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EnvelopeValidationError {
    #[error("Invalid version: {0}")]
    InvalidVersion(u16),

    #[error("Unsupported version {received}, max supported is {max_supported}")]
    UnsupportedVersion { received: u16, max_supported: u16 },

    #[error("Non-monotonic sequence: previous {previous}, current {current}")]
    NonMonotonicSequence {
        previous: MessageSequence,
        current: MessageSequence,
    },

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_zero_version() {
        let sender = DeviceId::new_from_entropy([1u8; 32]);
        let mut envelope = WireEnvelope::new(
            None,
            sender,
            MessageSequence::new(1),
            MessageTimestamp::new(1),
            (),
        );
        envelope.version = 0;
        assert!(matches!(
            envelope.validate(None),
            Err(EnvelopeValidationError::InvalidVersion(0))
        ));
    }

    #[test]
    fn validate_rejects_non_monotonic_sequence() {
        let sender = DeviceId::new_from_entropy([2u8; 32]);
        let envelope = WireEnvelope::new(
            None,
            sender,
            MessageSequence::new(5),
            MessageTimestamp::new(10),
            (),
        );
        let err = envelope
            .validate(Some(MessageSequence::new(5)))
            .unwrap_err();
        assert!(matches!(
            err,
            EnvelopeValidationError::NonMonotonicSequence { .. }
        ));
    }

    #[test]
    fn validate_rejects_zero_timestamp() {
        let sender = DeviceId::new_from_entropy([3u8; 32]);
        let envelope = WireEnvelope::new(
            None,
            sender,
            MessageSequence::new(1),
            MessageTimestamp::new(0),
            (),
        );
        assert!(matches!(
            envelope.validate(None),
            Err(EnvelopeValidationError::InvalidTimestamp(0))
        ));
    }

    #[test]
    fn validate_accepts_valid_envelope() {
        let sender = DeviceId::new_from_entropy([4u8; 32]);
        let envelope = WireEnvelope::new(
            None,
            sender,
            MessageSequence::new(2),
            MessageTimestamp::new(100),
            "payload",
        );
        assert!(envelope.validate(Some(MessageSequence::new(1))).is_ok());
    }
}
