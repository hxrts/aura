//! Categorized application errors
//!
//! Provides structured error types that enable:
//! - Categorized error handling (network vs auth vs sync vs user action)
//! - Appropriate toast severity routing
//! - Recovery hints for user-actionable errors

use std::fmt;

/// Network error codes
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkErrorCode {
    /// Connection timeout
    Timeout,
    /// Connection refused
    ConnectionRefused,
    /// DNS resolution failed
    DnsFailure,
    /// TLS/SSL error
    TlsError,
    /// Peer disconnected
    Disconnected,
    /// Rate limited
    RateLimited,
    /// Generic network error
    Other,
}

impl fmt::Display for NetworkErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Timeout => write!(f, "timeout"),
            Self::ConnectionRefused => write!(f, "connection refused"),
            Self::DnsFailure => write!(f, "DNS resolution failed"),
            Self::TlsError => write!(f, "TLS error"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::RateLimited => write!(f, "rate limited"),
            Self::Other => write!(f, "network error"),
        }
    }
}

/// Authentication/authorization failure reasons
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthFailure {
    /// Invalid credentials
    InvalidCredentials,
    /// Token expired
    TokenExpired,
    /// Insufficient permissions
    InsufficientPermissions,
    /// Account locked
    AccountLocked,
    /// Signature verification failed
    SignatureInvalid,
    /// Capability check failed
    CapabilityDenied,
}

impl fmt::Display for AuthFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCredentials => write!(f, "invalid credentials"),
            Self::TokenExpired => write!(f, "token expired"),
            Self::InsufficientPermissions => write!(f, "insufficient permissions"),
            Self::AccountLocked => write!(f, "account locked"),
            Self::SignatureInvalid => write!(f, "invalid signature"),
            Self::CapabilityDenied => write!(f, "capability denied"),
        }
    }
}

/// Sync protocol stages where failures can occur
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStage {
    /// Initial handshake
    Handshake,
    /// State comparison
    Comparison,
    /// Data transfer
    Transfer,
    /// Conflict resolution
    Resolution,
    /// Final commit
    Commit,
}

impl fmt::Display for SyncStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handshake => write!(f, "handshake"),
            Self::Comparison => write!(f, "comparison"),
            Self::Transfer => write!(f, "transfer"),
            Self::Resolution => write!(f, "resolution"),
            Self::Commit => write!(f, "commit"),
        }
    }
}

/// Toast severity levels for error display
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastSeverity {
    /// Informational - blue styling
    Info,
    /// Warning - yellow styling
    Warning,
    /// Error - red styling
    Error,
}

/// Categorized application errors
#[derive(Clone, Debug)]
pub enum AppError {
    /// Network-related failures
    Network {
        code: NetworkErrorCode,
        message: String,
        recoverable: bool,
    },
    /// Authentication/authorization failures
    Auth {
        reason: AuthFailure,
        context: String,
    },
    /// Sync protocol failures
    Sync { stage: SyncStage, details: String },
    /// User action failures (with recovery hint)
    UserAction { action: String, hint: String },
    /// Internal errors (unexpected conditions)
    Internal { source: String, message: String },
}

impl AppError {
    /// Create a network error
    pub fn network(code: NetworkErrorCode, message: impl Into<String>) -> Self {
        Self::Network {
            code,
            message: message.into(),
            recoverable: true,
        }
    }

    /// Create a fatal network error
    pub fn network_fatal(code: NetworkErrorCode, message: impl Into<String>) -> Self {
        Self::Network {
            code,
            message: message.into(),
            recoverable: false,
        }
    }

    /// Create an auth error
    pub fn auth(reason: AuthFailure, context: impl Into<String>) -> Self {
        Self::Auth {
            reason,
            context: context.into(),
        }
    }

    /// Create a sync error
    pub fn sync(stage: SyncStage, details: impl Into<String>) -> Self {
        Self::Sync {
            stage,
            details: details.into(),
        }
    }

    /// Create a user action error with recovery hint
    pub fn user_action(action: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::UserAction {
            action: action.into(),
            hint: hint.into(),
        }
    }

    /// Create an internal error
    pub fn internal(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Internal {
            source: source.into(),
            message: message.into(),
        }
    }

    /// Get the appropriate toast severity for this error
    pub fn toast_level(&self) -> ToastSeverity {
        match self {
            Self::Network { recoverable, .. } => {
                if *recoverable {
                    ToastSeverity::Warning
                } else {
                    ToastSeverity::Error
                }
            }
            Self::Auth { .. } => ToastSeverity::Error,
            Self::Sync { .. } => ToastSeverity::Warning,
            Self::UserAction { .. } => ToastSeverity::Info,
            Self::Internal { .. } => ToastSeverity::Error,
        }
    }

    /// Check if the error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Network { recoverable, .. } => *recoverable,
            Self::Auth { reason, .. } => matches!(reason, AuthFailure::TokenExpired),
            Self::Sync { .. } => true,
            Self::UserAction { .. } => true,
            Self::Internal { .. } => false,
        }
    }

    /// Get a short error code string
    pub fn code(&self) -> &'static str {
        match self {
            Self::Network { code, .. } => match code {
                NetworkErrorCode::Timeout => "NET_TIMEOUT",
                NetworkErrorCode::ConnectionRefused => "NET_REFUSED",
                NetworkErrorCode::DnsFailure => "NET_DNS",
                NetworkErrorCode::TlsError => "NET_TLS",
                NetworkErrorCode::Disconnected => "NET_DISCONNECTED",
                NetworkErrorCode::RateLimited => "NET_RATE_LIMITED",
                NetworkErrorCode::Other => "NET_ERROR",
            },
            Self::Auth { reason, .. } => match reason {
                AuthFailure::InvalidCredentials => "AUTH_INVALID",
                AuthFailure::TokenExpired => "AUTH_EXPIRED",
                AuthFailure::InsufficientPermissions => "AUTH_PERMISSION",
                AuthFailure::AccountLocked => "AUTH_LOCKED",
                AuthFailure::SignatureInvalid => "AUTH_SIGNATURE",
                AuthFailure::CapabilityDenied => "AUTH_CAPABILITY",
            },
            Self::Sync { stage, .. } => match stage {
                SyncStage::Handshake => "SYNC_HANDSHAKE",
                SyncStage::Comparison => "SYNC_COMPARE",
                SyncStage::Transfer => "SYNC_TRANSFER",
                SyncStage::Resolution => "SYNC_RESOLVE",
                SyncStage::Commit => "SYNC_COMMIT",
            },
            Self::UserAction { .. } => "USER_ACTION",
            Self::Internal { .. } => "INTERNAL",
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network { code, message, .. } => {
                write!(f, "Network error ({}): {}", code, message)
            }
            Self::Auth { reason, context } => {
                write!(f, "Authentication failed ({}): {}", reason, context)
            }
            Self::Sync { stage, details } => {
                write!(f, "Sync failed at {} stage: {}", stage, details)
            }
            Self::UserAction { action, hint } => {
                write!(f, "{} - {}", action, hint)
            }
            Self::Internal { source, message } => {
                write!(f, "{}: {}", source, message)
            }
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_error_display() {
        let err = AppError::network(NetworkErrorCode::Timeout, "connection timed out after 30s");
        assert_eq!(
            err.to_string(),
            "Network error (timeout): connection timed out after 30s"
        );
        assert_eq!(err.code(), "NET_TIMEOUT");
        assert!(err.is_recoverable());
        assert_eq!(err.toast_level(), ToastSeverity::Warning);
    }

    #[test]
    fn test_auth_error_display() {
        let err = AppError::auth(AuthFailure::TokenExpired, "session expired");
        assert_eq!(
            err.to_string(),
            "Authentication failed (token expired): session expired"
        );
        assert_eq!(err.code(), "AUTH_EXPIRED");
        assert!(err.is_recoverable());
        assert_eq!(err.toast_level(), ToastSeverity::Error);
    }

    #[test]
    fn test_user_action_error() {
        let err = AppError::user_action("Message too long", "Limit is 4096 characters");
        assert_eq!(
            err.to_string(),
            "Message too long - Limit is 4096 characters"
        );
        assert_eq!(err.code(), "USER_ACTION");
        assert!(err.is_recoverable());
        assert_eq!(err.toast_level(), ToastSeverity::Info);
    }

    #[test]
    fn test_internal_error() {
        let err = AppError::internal("reducer", "unexpected state transition");
        assert_eq!(err.to_string(), "reducer: unexpected state transition");
        assert_eq!(err.code(), "INTERNAL");
        assert!(!err.is_recoverable());
        assert_eq!(err.toast_level(), ToastSeverity::Error);
    }
}
