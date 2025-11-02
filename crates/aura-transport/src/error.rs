//! Transport errors using unified error system
//!
//! This module provides transport-specific error constructors that wrap
//! the unified Aura error system. All transport errors are instances of
//! `AuraError` with transport-specific error codes and messages.

// Re-export unified error system
pub use aura_types::{AuraError, ErrorCode, ErrorSeverity};
pub use aura_types::{AuraResult as TransportResult, AuraResult};

/// Type alias for transport errors (AuraError)
pub type TransportError = AuraError;

/// Builder providing transport-specific error constructors
///
/// This struct provides static methods for creating transport-specific errors
/// with appropriate error codes and severity levels.
pub struct TransportErrorBuilder;

impl TransportErrorBuilder {
    /// Create a connection failed error
    pub fn connection_failed(reason: impl Into<String>) -> AuraError {
        AuraError::transport_connection_failed(reason)
    }

    /// Create a network timeout error
    pub fn timeout(operation: impl Into<String>) -> AuraError {
        AuraError::transport_timeout(operation)
    }

    /// Create a handshake failed error
    pub fn handshake_failed(reason: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("Handshake failed: {}", reason.into()))
    }

    /// Create a protocol error
    pub fn protocol_error(reason: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("Protocol error: {}", reason.into()))
    }

    /// Create a peer unreachable error
    pub fn peer_unreachable(peer_id: impl Into<String>) -> AuraError {
        AuraError::network_unreachable(format!("Peer unreachable: {}", peer_id.into()))
    }

    /// Create an authentication error
    pub fn auth_failed(reason: impl Into<String>) -> AuraError {
        AuraError::transport_connection_failed(format!("Authentication failed: {}", reason.into()))
    }

    /// Create a presence ticket error
    pub fn presence_ticket_invalid(reason: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("Invalid presence ticket: {}", reason.into()))
    }

    /// Create a transport configuration error
    pub fn transport_config_invalid(
        field: impl Into<String>,
        reason: impl Into<String>,
    ) -> AuraError {
        AuraError::configuration_error(format!(
            "Invalid transport configuration: {} - {}",
            field.into(),
            reason.into()
        ))
    }

    /// Create a general transport error
    pub fn transport(message: impl Into<String>) -> AuraError {
        AuraError::transport_failed(message)
    }

    /// Create an invalid config error
    pub fn invalid_config(message: impl Into<String>) -> AuraError {
        AuraError::configuration_error(message)
    }

    /// Create an invalid presence ticket error
    pub fn invalid_presence_ticket() -> AuraError {
        AuraError::transport_failed("Invalid presence ticket")
    }

    /// Create an insufficient capability error
    pub fn insufficient_capability(message: impl Into<String>) -> AuraError {
        AuraError::insufficient_capability(message)
    }

    /// Create a not authorized error
    pub fn not_authorized(message: impl Into<String>) -> AuraError {
        AuraError::insufficient_permissions(message)
    }

    /// Create an authentication error
    pub fn authentication(message: impl Into<String>) -> AuraError {
        AuraError::transport_connection_failed(format!("Authentication error: {}", message.into()))
    }

    /// Create a connection error
    pub fn connection(message: impl Into<String>) -> AuraError {
        AuraError::transport_connection_failed(message)
    }

    /// Create an authentication failed error
    pub fn authentication_failed(message: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("Authentication failed: {}", message.into()))
    }

    /// Create a configuration error
    pub fn configuration_error(message: impl Into<String>) -> AuraError {
        AuraError::configuration_error(message)
    }

    /// Create an IO error
    pub fn io_error(message: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("IO error: {}", message.into()))
    }

    /// Create an invalid peer ID error
    pub fn invalid_peer_id(message: impl Into<String>) -> AuraError {
        AuraError::transport_failed(format!("Invalid peer ID: {}", message.into()))
    }
}
