//! Unified error hierarchy for aura-sync
//!
//! This module provides a comprehensive error taxonomy for all sync operations,
//! following the architectural principle of having zero backwards compatibility.

use aura_core::{AuraError, DeviceId};
use std::time::Duration;

/// Unified result type for all sync operations
pub type SyncResult<T> = Result<T, SyncError>;

/// Comprehensive error hierarchy for sync operations
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    /// Protocol-specific errors during sync execution
    #[error("Protocol error in {protocol}: {message}")]
    Protocol {
        protocol: String,
        message: String,
        peer: Option<DeviceId>,
    },

    /// Network-level communication errors
    #[error("Network error: {message}")]
    Network {
        message: String,
        peer: Option<DeviceId>,
        retryable: bool,
    },

    /// Validation errors for sync data
    #[error("Validation error: {message}")]
    Validation {
        message: String,
        field: Option<String>,
    },

    /// Session management errors
    #[error("Session error: {message}")]
    Session {
        message: String,
        session_id: Option<uuid::Uuid>,
        recoverable: bool,
    },

    /// Configuration and setup errors
    #[error("Configuration error: {message}")]
    Config {
        message: String,
        component: String,
    },

    /// Peer discovery and management errors
    #[error("Peer error: {message}")]
    Peer {
        message: String,
        peer: Option<DeviceId>,
        operation: String,
    },

    /// Authorization and capability errors
    #[error("Authorization error: {message}")]
    Authorization {
        message: String,
        required_capability: Option<String>,
        peer: Option<DeviceId>,
    },

    /// Timeout errors with context
    #[error("Timeout after {duration:?}: {operation}")]
    Timeout {
        operation: String,
        duration: Duration,
        peer: Option<DeviceId>,
    },

    /// Resource exhaustion errors
    #[error("Resource exhausted: {resource} - {message}")]
    ResourceExhausted {
        resource: String,
        message: String,
        limit: Option<u64>,
    },

    /// Integration errors with other Aura crates
    #[error("Aura core error: {0}")]
    Core(#[from] AuraError),

    /// Serialization/deserialization errors
    #[error("Serialization error: {message}")]
    Serialization {
        message: String,
        data_type: String,
    },

    /// State consistency errors
    #[error("Consistency error: {message}")]
    Consistency {
        message: String,
        operation: String,
        expected_state: Option<String>,
        actual_state: Option<String>,
    },
}

impl SyncError {
    /// Create a protocol error
    pub fn protocol(protocol: &str, message: &str) -> Self {
        Self::Protocol {
            protocol: protocol.to_string(),
            message: message.to_string(),
            peer: None,
        }
    }

    /// Create a protocol error with peer context
    pub fn protocol_with_peer(protocol: &str, message: &str, peer: DeviceId) -> Self {
        Self::Protocol {
            protocol: protocol.to_string(),
            message: message.to_string(),
            peer: Some(peer),
        }
    }

    /// Create a network error
    pub fn network(message: &str) -> Self {
        Self::Network {
            message: message.to_string(),
            peer: None,
            retryable: true,
        }
    }

    /// Create a non-retryable network error
    pub fn network_permanent(message: &str, peer: Option<DeviceId>) -> Self {
        Self::Network {
            message: message.to_string(),
            peer,
            retryable: false,
        }
    }

    /// Create a validation error
    pub fn validation(message: &str) -> Self {
        Self::Validation {
            message: message.to_string(),
            field: None,
        }
    }

    /// Create a validation error for a specific field
    pub fn validation_field(message: &str, field: &str) -> Self {
        Self::Validation {
            message: message.to_string(),
            field: Some(field.to_string()),
        }
    }

    /// Create a session error
    pub fn session(message: &str) -> Self {
        Self::Session {
            message: message.to_string(),
            session_id: None,
            recoverable: true,
        }
    }

    /// Create an unrecoverable session error
    pub fn session_fatal(message: &str, session_id: uuid::Uuid) -> Self {
        Self::Session {
            message: message.to_string(),
            session_id: Some(session_id),
            recoverable: false,
        }
    }

    /// Create a configuration error
    pub fn config(component: &str, message: &str) -> Self {
        Self::Config {
            message: message.to_string(),
            component: component.to_string(),
        }
    }

    /// Create a peer error
    pub fn peer(operation: &str, message: &str) -> Self {
        Self::Peer {
            message: message.to_string(),
            peer: None,
            operation: operation.to_string(),
        }
    }

    /// Create a peer error with peer context
    pub fn peer_with_device(operation: &str, message: &str, peer: DeviceId) -> Self {
        Self::Peer {
            message: message.to_string(),
            peer: Some(peer),
            operation: operation.to_string(),
        }
    }

    /// Create an authorization error
    pub fn authorization(message: &str) -> Self {
        Self::Authorization {
            message: message.to_string(),
            required_capability: None,
            peer: None,
        }
    }

    /// Create an authorization error with capability context
    pub fn authorization_capability(message: &str, capability: &str, peer: DeviceId) -> Self {
        Self::Authorization {
            message: message.to_string(),
            required_capability: Some(capability.to_string()),
            peer: Some(peer),
        }
    }

    /// Create a timeout error
    pub fn timeout(operation: &str, duration: Duration) -> Self {
        Self::Timeout {
            operation: operation.to_string(),
            duration,
            peer: None,
        }
    }

    /// Create a timeout error with peer context
    pub fn timeout_with_peer(operation: &str, duration: Duration, peer: DeviceId) -> Self {
        Self::Timeout {
            operation: operation.to_string(),
            duration,
            peer: Some(peer),
        }
    }

    /// Create a resource exhausted error
    pub fn resource_exhausted(resource: &str, message: &str) -> Self {
        Self::ResourceExhausted {
            resource: resource.to_string(),
            message: message.to_string(),
            limit: None,
        }
    }

    /// Create a resource exhausted error with limit
    pub fn resource_exhausted_with_limit(resource: &str, message: &str, limit: u64) -> Self {
        Self::ResourceExhausted {
            resource: resource.to_string(),
            message: message.to_string(),
            limit: Some(limit),
        }
    }

    /// Create a serialization error
    pub fn serialization(data_type: &str, message: &str) -> Self {
        Self::Serialization {
            message: message.to_string(),
            data_type: data_type.to_string(),
        }
    }

    /// Create a consistency error
    pub fn consistency(operation: &str, message: &str) -> Self {
        Self::Consistency {
            message: message.to_string(),
            operation: operation.to_string(),
            expected_state: None,
            actual_state: None,
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Network { retryable, .. } => *retryable,
            Self::Session { recoverable, .. } => *recoverable,
            Self::Timeout { .. } => true,
            Self::ResourceExhausted { .. } => true,
            Self::Protocol { .. } => false,
            Self::Validation { .. } => false,
            Self::Config { .. } => false,
            Self::Authorization { .. } => false,
            Self::Core(_) => false,
            Self::Serialization { .. } => false,
            Self::Consistency { .. } => false,
            Self::Peer { .. } => true, // Peer issues are often transient
        }
    }

    /// Get error category for metrics and logging
    pub fn category(&self) -> &'static str {
        match self {
            Self::Protocol { .. } => "protocol",
            Self::Network { .. } => "network",
            Self::Validation { .. } => "validation",
            Self::Session { .. } => "session",
            Self::Config { .. } => "config",
            Self::Peer { .. } => "peer",
            Self::Authorization { .. } => "authorization",
            Self::Timeout { .. } => "timeout",
            Self::ResourceExhausted { .. } => "resource",
            Self::Core(_) => "core",
            Self::Serialization { .. } => "serialization",
            Self::Consistency { .. } => "consistency",
        }
    }

    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            Self::Protocol { protocol, message, peer } => {
                if let Some(peer) = peer {
                    format!("Sync protocol '{}' failed with peer {}: {}", protocol, peer, message)
                } else {
                    format!("Sync protocol '{}' failed: {}", protocol, message)
                }
            }
            Self::Network { peer, .. } => {
                if let Some(peer) = peer {
                    format!("Network communication with peer {} failed", peer)
                } else {
                    "Network communication failed".to_string()
                }
            }
            Self::Validation { field, .. } => {
                if let Some(field) = field {
                    format!("Invalid data in field '{}'", field)
                } else {
                    "Invalid sync data".to_string()
                }
            }
            Self::Session { .. } => "Sync session error".to_string(),
            Self::Config { component, .. } => format!("Configuration error in {}", component),
            Self::Peer { operation, peer, .. } => {
                if let Some(peer) = peer {
                    format!("Peer operation '{}' failed for {}", operation, peer)
                } else {
                    format!("Peer operation '{}' failed", operation)
                }
            }
            Self::Authorization { .. } => "Insufficient permissions for sync operation".to_string(),
            Self::Timeout { operation, .. } => format!("Operation '{}' timed out", operation),
            Self::ResourceExhausted { resource, .. } => {
                format!("Resource '{}' exhausted", resource)
            }
            Self::Core(err) => format!("System error: {}", err),
            Self::Serialization { data_type, .. } => {
                format!("Failed to serialize/deserialize {}", data_type)
            }
            Self::Consistency { operation, .. } => {
                format!("Consistency error during '{}'", operation)
            }
        }
    }
}

#[cfg(test)]
    use aura_core::test_utils::test_device_id;
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = SyncError::protocol("anti_entropy", "sync failed");
        assert_eq!(err.category(), "protocol");
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_network_error_retryable() {
        let err = SyncError::network("connection refused");
        assert!(err.is_retryable());

        let err = SyncError::network_permanent("invalid protocol", None);
        assert!(!err.is_retryable());
    }

    #[test]
    fn test_user_messages() {
        let err = SyncError::timeout("journal_sync", Duration::from_secs(30));
        assert!(err.user_message().contains("timed out"));

        let peer = test_device_id(1);
        let err = SyncError::peer_with_device("discovery", "unreachable", peer);
        assert!(err.user_message().contains(&peer.to_string()));
    }
}