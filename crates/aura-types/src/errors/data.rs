//! Data management and serialization errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// State management and serialization errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataError {
    /// Serialization failed
    SerializationFailed {
        reason: String,
        data_type: Option<String>,
        format: Option<String>,
        context: ErrorContext,
    },
    
    /// Deserialization failed
    DeserializationFailed {
        reason: String,
        data_type: Option<String>,
        format: Option<String>,
        context: ErrorContext,
    },
    
    /// State validation failed
    StateValidationFailed {
        reason: String,
        field: Option<String>,
        value: Option<String>,
        context: ErrorContext,
    },
    
    /// Data integrity check failed
    IntegrityCheckFailed {
        reason: String,
        expected: Option<String>,
        actual: Option<String>,
        context: ErrorContext,
    },
    
    /// Schema migration failed
    MigrationFailed {
        reason: String,
        from_version: Option<u32>,
        to_version: Option<u32>,
        context: ErrorContext,
    },
    
    /// Data not found
    NotFound {
        resource_type: String,
        identifier: String,
        context: ErrorContext,
    },
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SerializationFailed { reason, data_type, format, .. } => {
                write!(f, "Serialization failed: {}", reason)?;
                if let Some(dt) = data_type {
                    write!(f, " (type: {})", dt)?;
                }
                if let Some(fmt) = format {
                    write!(f, " (format: {})", fmt)?;
                }
                Ok(())
            }
            Self::DeserializationFailed { reason, data_type, format, .. } => {
                write!(f, "Deserialization failed: {}", reason)?;
                if let Some(dt) = data_type {
                    write!(f, " (type: {})", dt)?;
                }
                if let Some(fmt) = format {
                    write!(f, " (format: {})", fmt)?;
                }
                Ok(())
            }
            Self::StateValidationFailed { reason, field, value, .. } => {
                write!(f, "State validation failed: {}", reason)?;
                if let Some(fld) = field {
                    write!(f, " (field: {})", fld)?;
                }
                if let Some(val) = value {
                    write!(f, " (value: {})", val)?;
                }
                Ok(())
            }
            Self::IntegrityCheckFailed { reason, expected, actual, .. } => {
                write!(f, "Integrity check failed: {}", reason)?;
                if let (Some(exp), Some(act)) = (expected, actual) {
                    write!(f, " (expected: {}, actual: {})", exp, act)?;
                }
                Ok(())
            }
            Self::MigrationFailed { reason, from_version, to_version, .. } => {
                write!(f, "Migration failed: {}", reason)?;
                if let (Some(from), Some(to)) = (from_version, to_version) {
                    write!(f, " (v{} -> v{})", from, to)?;
                }
                Ok(())
            }
            Self::NotFound { resource_type, identifier, .. } => {
                write!(f, "{} not found: {}", resource_type, identifier)
            }
        }
    }
}

impl DataError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::SerializationFailed { .. } => ErrorCode::SerializationFailed,
            Self::DeserializationFailed { .. } => ErrorCode::DeserializationFailed,
            Self::StateValidationFailed { .. } => ErrorCode::StateValidationFailed,
            Self::IntegrityCheckFailed { .. } => ErrorCode::IntegrityCheckFailed,
            Self::MigrationFailed { .. } => ErrorCode::MigrationFailed,
            Self::NotFound { .. } => ErrorCode::NotFound,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::SerializationFailed { .. } => ErrorSeverity::Medium,
            Self::DeserializationFailed { .. } => ErrorSeverity::Medium,
            Self::StateValidationFailed { .. } => ErrorSeverity::High,
            Self::IntegrityCheckFailed { .. } => ErrorSeverity::Critical,
            Self::MigrationFailed { .. } => ErrorSeverity::High,
            Self::NotFound { .. } => ErrorSeverity::Low,
        }
    }
}