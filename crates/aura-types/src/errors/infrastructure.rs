//! Infrastructure-related errors (transport, storage, network)

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Infrastructure layer errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InfrastructureError {
    /// Transport layer error
    TransportError {
        reason: String,
        transport_type: Option<String>,
        remote_peer: Option<String>,
        context: ErrorContext,
    },
    
    /// Storage operation failed
    StorageFailed {
        reason: String,
        operation: Option<String>,
        path: Option<String>,
        context: ErrorContext,
    },
    
    /// Network connectivity error
    NetworkError {
        reason: String,
        address: Option<String>,
        port: Option<u16>,
        context: ErrorContext,
    },
    
    /// Connection establishment failed
    ConnectionFailed {
        reason: String,
        peer: Option<String>,
        attempts: Option<u32>,
        context: ErrorContext,
    },
    
    /// Message delivery failed
    MessageDeliveryFailed {
        reason: String,
        recipient: Option<String>,
        message_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Resource exhaustion
    ResourceExhausted {
        resource: String,
        limit: Option<u64>,
        current: Option<u64>,
        context: ErrorContext,
    },
    
    /// IO operation failed
    IoError {
        reason: String,
        path: Option<String>,
        operation: Option<String>,
        context: ErrorContext,
    },
    
    /// Configuration error
    ConfigurationError {
        reason: String,
        parameter: Option<String>,
        value: Option<String>,
        context: ErrorContext,
    },
    
    /// Service unavailable
    ServiceUnavailable {
        service: String,
        reason: Option<String>,
        retry_after: Option<u64>,
        context: ErrorContext,
    },
}

impl fmt::Display for InfrastructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TransportError { reason, transport_type, remote_peer, .. } => {
                write!(f, "Transport error: {}", reason)?;
                if let Some(tt) = transport_type {
                    write!(f, " (type: {})", tt)?;
                }
                if let Some(peer) = remote_peer {
                    write!(f, " (peer: {})", peer)?;
                }
                Ok(())
            }
            Self::StorageFailed { reason, operation, path, .. } => {
                write!(f, "Storage failed: {}", reason)?;
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                if let Some(p) = path {
                    write!(f, " (path: {})", p)?;
                }
                Ok(())
            }
            Self::NetworkError { reason, address, port, .. } => {
                write!(f, "Network error: {}", reason)?;
                if let (Some(addr), Some(p)) = (address, port) {
                    write!(f, " ({}:{})", addr, p)?;
                }
                Ok(())
            }
            Self::ConnectionFailed { reason, peer, attempts, .. } => {
                write!(f, "Connection failed: {}", reason)?;
                if let Some(p) = peer {
                    write!(f, " (peer: {})", p)?;
                }
                if let Some(a) = attempts {
                    write!(f, " (attempts: {})", a)?;
                }
                Ok(())
            }
            Self::MessageDeliveryFailed { reason, recipient, message_type, .. } => {
                write!(f, "Message delivery failed: {}", reason)?;
                if let Some(r) = recipient {
                    write!(f, " (recipient: {})", r)?;
                }
                if let Some(mt) = message_type {
                    write!(f, " (type: {})", mt)?;
                }
                Ok(())
            }
            Self::ResourceExhausted { resource, limit, current, .. } => {
                write!(f, "Resource {} exhausted", resource)?;
                if let (Some(l), Some(c)) = (limit, current) {
                    write!(f, " (limit: {}, current: {})", l, c)?;
                }
                Ok(())
            }
            Self::IoError { reason, path, operation, .. } => {
                write!(f, "IO error: {}", reason)?;
                if let Some(p) = path {
                    write!(f, " (path: {})", p)?;
                }
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::ConfigurationError { reason, parameter, value, .. } => {
                write!(f, "Configuration error: {}", reason)?;
                if let Some(p) = parameter {
                    write!(f, " (parameter: {})", p)?;
                }
                if let Some(v) = value {
                    write!(f, " (value: {})", v)?;
                }
                Ok(())
            }
            Self::ServiceUnavailable { service, reason, retry_after, .. } => {
                write!(f, "Service {} unavailable", service)?;
                if let Some(r) = reason {
                    write!(f, ": {}", r)?;
                }
                if let Some(ra) = retry_after {
                    write!(f, " (retry after: {}s)", ra)?;
                }
                Ok(())
            }
        }
    }
}

impl InfrastructureError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::TransportError { .. } => ErrorCode::TransportError,
            Self::StorageFailed { .. } => ErrorCode::StorageFailed,
            Self::NetworkError { .. } => ErrorCode::NetworkError,
            Self::ConnectionFailed { .. } => ErrorCode::ConnectionFailed,
            Self::MessageDeliveryFailed { .. } => ErrorCode::MessageDeliveryFailed,
            Self::ResourceExhausted { .. } => ErrorCode::ResourceExhausted,
            Self::IoError { .. } => ErrorCode::IoError,
            Self::ConfigurationError { .. } => ErrorCode::ConfigurationError,
            Self::ServiceUnavailable { .. } => ErrorCode::ServiceUnavailable,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::TransportError { .. } => ErrorSeverity::Medium,
            Self::StorageFailed { .. } => ErrorSeverity::High,
            Self::NetworkError { .. } => ErrorSeverity::Medium,
            Self::ConnectionFailed { .. } => ErrorSeverity::Medium,
            Self::MessageDeliveryFailed { .. } => ErrorSeverity::Medium,
            Self::ResourceExhausted { .. } => ErrorSeverity::High,
            Self::IoError { .. } => ErrorSeverity::Medium,
            Self::ConfigurationError { .. } => ErrorSeverity::High,
            Self::ServiceUnavailable { .. } => ErrorSeverity::Medium,
        }
    }
}