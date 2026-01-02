//! Categorized application errors
//!
//! Provides structured error types that enable:
//! - Categorized error handling (network vs auth vs sync vs user action)
//! - Appropriate toast severity routing
//! - Recovery hints for user-actionable errors

use std::fmt;

// Re-export ToastLevel from views/notifications (single source of truth)
pub use crate::views::notifications::ToastLevel;

// ============================================================================
// Error Categories (Terminal-compatible)
// ============================================================================

/// High-level error categories for frontend error handling.
///
/// These categories provide a consistent way to classify errors across
/// frontends (TUI, CLI, mobile) for appropriate UI treatment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCategory {
    /// User input validation errors (correctable by user)
    Input,
    /// Configuration errors (correctable by modifying settings)
    Config,
    /// Authorization/capability errors (may require elevated privileges)
    Capability,
    /// Resource not found errors (transient or permanent)
    NotFound,
    /// Network connectivity errors (often transient)
    Network,
    /// Feature unavailable (development limitation)
    NotImplemented,
    /// General operation failures (catch-all)
    Operation,
}

impl ErrorCategory {
    /// Check if this error category is user-correctable.
    ///
    /// User-correctable errors are those where the user can take action
    /// to resolve the issue (e.g., fix input, change settings).
    #[must_use]
    pub fn is_user_correctable(&self) -> bool {
        matches!(self, Self::Input | Self::Config)
    }

    /// Check if this error category is likely transient.
    ///
    /// Transient errors may resolve on retry.
    #[must_use]
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Network | Self::NotFound)
    }

    /// Get the appropriate toast severity for this category.
    #[must_use]
    pub fn toast_severity(&self) -> ToastLevel {
        match self {
            Self::Input => ToastLevel::Info,
            Self::Config => ToastLevel::Warning,
            Self::Capability => ToastLevel::Error,
            Self::NotFound => ToastLevel::Warning,
            Self::Network => ToastLevel::Warning,
            Self::NotImplemented => ToastLevel::Info,
            Self::Operation => ToastLevel::Error,
        }
    }

    /// Get a short label for this category.
    #[must_use]
    pub fn label(&self) -> &'static str {
        match self {
            Self::Input => "Input",
            Self::Config => "Config",
            Self::Capability => "Permission",
            Self::NotFound => "Not Found",
            Self::Network => "Network",
            Self::NotImplemented => "Unimplemented",
            Self::Operation => "Operation",
        }
    }

    /// Get a hint for the user on how to resolve this category of error.
    #[must_use]
    pub fn resolution_hint(&self) -> &'static str {
        match self {
            Self::Input => "Check your input and try again",
            Self::Config => "Review your configuration settings",
            Self::Capability => "This action requires additional permissions",
            Self::NotFound => "The requested resource could not be found",
            Self::Network => "Check your network connection and retry",
            Self::NotImplemented => "This feature is not yet available",
            Self::Operation => "An unexpected error occurred",
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label())
    }
}

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
    pub fn toast_level(&self) -> ToastLevel {
        match self {
            Self::Network { recoverable, .. } => {
                if *recoverable {
                    ToastLevel::Warning
                } else {
                    ToastLevel::Error
                }
            }
            Self::Auth { .. } => ToastLevel::Error,
            Self::Sync { .. } => ToastLevel::Warning,
            Self::UserAction { .. } => ToastLevel::Info,
            Self::Internal { .. } => ToastLevel::Error,
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
        assert_eq!(err.toast_level(), ToastLevel::Warning);
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
        assert_eq!(err.toast_level(), ToastLevel::Error);
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
        assert_eq!(err.toast_level(), ToastLevel::Info);
    }

    #[test]
    fn test_internal_error() {
        let err = AppError::internal("reducer", "unexpected state transition");
        assert_eq!(err.to_string(), "reducer: unexpected state transition");
        assert_eq!(err.code(), "INTERNAL");
        assert!(!err.is_recoverable());
        assert_eq!(err.toast_level(), ToastLevel::Error);
    }

    // ========================================================================
    // ErrorCategory Tests
    // ========================================================================

    #[test]
    fn test_error_category_user_correctable() {
        assert!(ErrorCategory::Input.is_user_correctable());
        assert!(ErrorCategory::Config.is_user_correctable());
        assert!(!ErrorCategory::Capability.is_user_correctable());
        assert!(!ErrorCategory::NotFound.is_user_correctable());
        assert!(!ErrorCategory::Network.is_user_correctable());
        assert!(!ErrorCategory::NotImplemented.is_user_correctable());
        assert!(!ErrorCategory::Operation.is_user_correctable());
    }

    #[test]
    fn test_error_category_transient() {
        assert!(!ErrorCategory::Input.is_transient());
        assert!(!ErrorCategory::Config.is_transient());
        assert!(!ErrorCategory::Capability.is_transient());
        assert!(ErrorCategory::NotFound.is_transient());
        assert!(ErrorCategory::Network.is_transient());
        assert!(!ErrorCategory::NotImplemented.is_transient());
        assert!(!ErrorCategory::Operation.is_transient());
    }

    #[test]
    fn test_error_category_toast_severity() {
        assert_eq!(ErrorCategory::Input.toast_severity(), ToastLevel::Info);
        assert_eq!(
            ErrorCategory::Config.toast_severity(),
            ToastLevel::Warning
        );
        assert_eq!(
            ErrorCategory::Capability.toast_severity(),
            ToastLevel::Error
        );
        assert_eq!(
            ErrorCategory::NotFound.toast_severity(),
            ToastLevel::Warning
        );
        assert_eq!(
            ErrorCategory::Network.toast_severity(),
            ToastLevel::Warning
        );
        assert_eq!(
            ErrorCategory::NotImplemented.toast_severity(),
            ToastLevel::Info
        );
        assert_eq!(
            ErrorCategory::Operation.toast_severity(),
            ToastLevel::Error
        );
    }

    #[test]
    fn test_error_category_labels() {
        assert_eq!(ErrorCategory::Input.label(), "Input");
        assert_eq!(ErrorCategory::Config.label(), "Config");
        assert_eq!(ErrorCategory::Capability.label(), "Permission");
        assert_eq!(ErrorCategory::NotFound.label(), "Not Found");
        assert_eq!(ErrorCategory::Network.label(), "Network");
        assert_eq!(ErrorCategory::NotImplemented.label(), "Unimplemented");
        assert_eq!(ErrorCategory::Operation.label(), "Operation");
    }

    #[test]
    fn test_error_category_display() {
        assert_eq!(format!("{}", ErrorCategory::Input), "Input");
        assert_eq!(format!("{}", ErrorCategory::Network), "Network");
    }

    #[test]
    fn test_error_category_resolution_hints() {
        // Just verify they're non-empty
        for category in [
            ErrorCategory::Input,
            ErrorCategory::Config,
            ErrorCategory::Capability,
            ErrorCategory::NotFound,
            ErrorCategory::Network,
            ErrorCategory::NotImplemented,
            ErrorCategory::Operation,
        ] {
            assert!(!category.resolution_hint().is_empty());
        }
    }
}
