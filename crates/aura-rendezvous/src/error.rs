//! Rendezvous-specific error types
//!
//! This module defines error types specific to the rendezvous layer,
//! though most errors are handled through aura_core::AuraError.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Rendezvous-specific error types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendezvousError {
    /// SBB channel not found
    ChannelNotFound { channel_id: String },
    /// Invalid brand proof
    InvalidBrandProof { reason: String },
    /// Relay node unavailable
    RelayUnavailable { node_id: String, reason: String },
    /// Discovery query failed
    DiscoveryQueryFailed { query_id: String, reason: String },
    /// Insufficient privacy protection
    InsufficientPrivacy {
        required_level: String,
        available_level: String,
    },
    /// Message delivery failed
    DeliveryFailed {
        message_id: String,
        attempts: u8,
        last_error: String,
    },
    /// Relationship context not found
    RelationshipContextNotFound { relationship_id: String },
    /// Rendezvous point access denied
    RendezvousAccessDenied {
        rendezvous_id: String,
        reason: String,
    },
    /// Invalid message envelope
    InvalidMessageEnvelope {
        envelope_id: String,
        validation_error: String,
    },
    /// Anonymization failed
    AnonymizationFailed { operation: String, reason: String },
}

impl fmt::Display for RendezvousError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RendezvousError::ChannelNotFound { channel_id } => {
                write!(f, "SBB channel not found: {}", channel_id)
            }
            RendezvousError::InvalidBrandProof { reason } => {
                write!(f, "Invalid brand proof: {}", reason)
            }
            RendezvousError::RelayUnavailable { node_id, reason } => {
                write!(f, "Relay node {} unavailable: {}", node_id, reason)
            }
            RendezvousError::DiscoveryQueryFailed { query_id, reason } => {
                write!(f, "Discovery query {} failed: {}", query_id, reason)
            }
            RendezvousError::InsufficientPrivacy {
                required_level,
                available_level,
            } => {
                write!(
                    f,
                    "Insufficient privacy: required '{}', available '{}'",
                    required_level, available_level
                )
            }
            RendezvousError::DeliveryFailed {
                message_id,
                attempts,
                last_error,
            } => {
                write!(
                    f,
                    "Message {} delivery failed after {} attempts: {}",
                    message_id, attempts, last_error
                )
            }
            RendezvousError::RelationshipContextNotFound { relationship_id } => {
                write!(f, "Relationship context not found: {}", relationship_id)
            }
            RendezvousError::RendezvousAccessDenied {
                rendezvous_id,
                reason,
            } => {
                write!(
                    f,
                    "Rendezvous point {} access denied: {}",
                    rendezvous_id, reason
                )
            }
            RendezvousError::InvalidMessageEnvelope {
                envelope_id,
                validation_error,
            } => {
                write!(
                    f,
                    "Invalid message envelope {}: {}",
                    envelope_id, validation_error
                )
            }
            RendezvousError::AnonymizationFailed { operation, reason } => {
                write!(f, "Anonymization of '{}' failed: {}", operation, reason)
            }
        }
    }
}

impl std::error::Error for RendezvousError {}

/// Convert rendezvous error to AuraError
impl From<RendezvousError> for aura_core::AuraError {
    fn from(err: RendezvousError) -> Self {
        match err {
            RendezvousError::ChannelNotFound { .. }
            | RendezvousError::RelationshipContextNotFound { .. } => {
                aura_core::AuraError::not_found(err.to_string())
            }
            RendezvousError::RendezvousAccessDenied { .. } => {
                aura_core::AuraError::permission_denied(err.to_string())
            }
            RendezvousError::RelayUnavailable { .. } => {
                aura_core::AuraError::service_unavailable(err.to_string())
            }
            RendezvousError::InvalidBrandProof { .. }
            | RendezvousError::InvalidMessageEnvelope { .. } => {
                aura_core::AuraError::invalid(err.to_string())
            }
            RendezvousError::InsufficientPrivacy { .. } => {
                aura_core::AuraError::constraint_violation(err.to_string())
            }
            RendezvousError::DeliveryFailed { .. }
            | RendezvousError::DiscoveryQueryFailed { .. }
            | RendezvousError::AnonymizationFailed { .. } => {
                aura_core::AuraError::operation_failed(err.to_string())
            }
        }
    }
}

/// Rendezvous operation result type
pub type RendezvousResult<T> = Result<T, RendezvousError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rendezvous_error_display() {
        let error = RendezvousError::ChannelNotFound {
            channel_id: "test-channel-123".into(),
        };

        assert_eq!(error.to_string(), "SBB channel not found: test-channel-123");
    }

    #[test]
    fn test_rendezvous_error_to_aura_error() {
        let rendezvous_err = RendezvousError::RelayUnavailable {
            node_id: "relay-1".into(),
            reason: "node offline".into(),
        };

        let aura_err: aura_core::AuraError = rendezvous_err.into();
        assert!(aura_err.to_string().contains("unavailable"));
    }

    #[test]
    fn test_privacy_violation_error() {
        let error = RendezvousError::InsufficientPrivacy {
            required_level: "full_anonymity".into(),
            available_level: "timing_observable".into(),
        };

        let aura_err: aura_core::AuraError = error.into();
        assert!(aura_err.to_string().to_lowercase().contains("privacy"));
    }

    #[test]
    fn test_delivery_failure_error() {
        let error = RendezvousError::DeliveryFailed {
            message_id: "msg-456".into(),
            attempts: 3,
            last_error: "relay timeout".into(),
        };

        assert!(error.to_string().contains("3 attempts"));
        assert!(error.to_string().contains("relay timeout"));
    }
}
