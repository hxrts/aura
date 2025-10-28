//! Error construction macros for reducing boilerplate
//!
//! This module provides macros that automatically generate error constructor
//! methods, significantly reducing the amount of boilerplate code required
//! for error handling across the Aura codebase.

use crate::{AuraError, ErrorCode, ErrorContext};

/// Generate error constructor methods automatically
///
/// This macro creates convenient constructor methods for each error variant,
/// eliminating the need to manually implement dozens of constructor functions.
///
/// # Example
///
/// ```text
/// // Instead of manually writing:
/// impl AuraError {
///     pub fn dkd_failed(message: impl Into<String>) -> Self {
///         Self::Protocol(ProtocolError::DkdFailed {
///             message: message.into(),
///             context: ErrorContext::new().with_code(ErrorCode::ProtocolDkdTimeout),
///         })
///     }
/// }
///
/// // The macro would generate all constructors automatically
/// error_constructors! {
///     Protocol {
///         dkd_failed => (DkdFailed, ProtocolDkdTimeout),
///         frost_failed => (FrostFailed, ProtocolFrostSignFailed),
///         epoch_mismatch => (EpochMismatch, ProtocolEpochMismatch),
///         // ... many more
///     }
/// }
/// ```
#[macro_export]
macro_rules! error_constructors {
    (
        $error_category:ident {
            $(
                $method_name:ident => ($variant:ident, $error_code:ident)
            ),* $(,)?
        }
    ) => {
        impl AuraError {
            $(
                #[doc = concat!("Create a ", stringify!($variant), " error")]
                pub fn $method_name(message: impl Into<String>) -> Self {
                    match stringify!($error_category) {
                        "Protocol" => {
                            Self::Protocol($crate::ProtocolError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Crypto" => {
                            Self::Crypto($crate::CryptoError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Infrastructure" => {
                            Self::Infrastructure($crate::InfrastructureError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Agent" => {
                            Self::Agent($crate::AgentError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Data" => {
                            Self::Data($crate::DataError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Capability" => {
                            Self::Capability($crate::CapabilityError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "Session" => {
                            Self::Session($crate::SessionError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        "System" => {
                            Self::System($crate::SystemError::$variant {
                                message: message.into(),
                                context: $crate::ErrorContext::new()
                                    .with_code($crate::ErrorCode::$error_code),
                            })
                        }
                        _ => unreachable!("Unknown error category: {}", stringify!($error_category)),
                    }
                }
            )*
        }
    };
}

/// Generate error classification methods
///
/// This macro creates methods to classify errors by domain, severity, and
/// other characteristics without requiring manual implementation.
#[macro_export]
macro_rules! error_classification {
    (
        $(
            $method_name:ident => $return_type:ty {
                $(
                    $error_pattern:pat => $result:expr
                ),* $(,)?
            }
        ),* $(,)?
    ) => {
        impl AuraError {
            $(
                #[doc = concat!("Check if error ", stringify!($method_name))]
                pub fn $method_name(&self) -> $return_type {
                    match self {
                        $(
                            $error_pattern => $result,
                        )*
                        _ => Default::default(),
                    }
                }
            )*
        }
    };
}

/// Generate error context helpers
///
/// This macro creates helper methods for adding common context to errors.
#[macro_export]
macro_rules! error_context_helpers {
    (
        $(
            $helper_name:ident($param_type:ty) => $context_key:literal
        ),* $(,)?
    ) => {
        impl AuraError {
            $(
                #[doc = concat!("Add ", stringify!($helper_name), " context to the error")]
                pub fn $helper_name(self, value: $param_type) -> Self {
                    self.with_context($context_key, value.to_string())
                }
            )*
        }
    };
}

/// Simple helper functions for common error patterns
///
/// Since the macro approach is overly complex, let's implement
/// the most common error constructors directly.
impl AuraError {
    // Additional convenience constructors beyond those in lib.rs

    /// Create a timeout error
    pub fn timeout_error(message: impl Into<String>) -> Self {
        Self::protocol_timeout(message)
    }

    /// Create a connection error  
    pub fn connection_error(message: impl Into<String>) -> Self {
        Self::transport_connection_failed(message)
    }

    /// Create a quota error
    pub fn quota_error(message: impl Into<String>) -> Self {
        Self::Infrastructure(crate::InfrastructureError::StorageQuotaExceeded {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraStorageQuotaExceeded),
        })
    }

    /// Create a hash mismatch error
    pub fn hash_mismatch_error(message: impl Into<String>) -> Self {
        Self::data_corruption_detected(message)
    }

    /// Create a witness verification error
    pub fn witness_error(message: impl Into<String>) -> Self {
        Self::Session(crate::SessionError::ProtocolViolation {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionProtocolViolation),
        })
    }

    /// Create a session error
    pub fn session_error(message: impl Into<String>) -> Self {
        Self::Session(crate::SessionError::ProtocolViolation {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionProtocolViolation),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convenience_constructors() {
        let timeout_error = AuraError::timeout_error("Operation timed out");
        assert!(matches!(timeout_error, AuraError::Protocol(_)));

        let connection_error = AuraError::connection_error("Cannot connect");
        assert!(matches!(connection_error, AuraError::Infrastructure(_)));

        let quota_error = AuraError::quota_error("Storage full");
        assert!(matches!(quota_error, AuraError::Infrastructure(_)));

        let witness_error = AuraError::witness_error("Invalid witness");
        assert!(matches!(witness_error, AuraError::Session(_)));
    }

    #[test]
    fn test_error_context() {
        let error = AuraError::timeout_error("DKD timeout")
            .with_context("participant", "alice")
            .with_context("round", "2");

        let context = error.context();
        assert_eq!(
            context.context.get("participant"),
            Some(&"alice".to_string())
        );
        assert_eq!(context.context.get("round"), Some(&"2".to_string()));
    }
}
