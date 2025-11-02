//! System-level runtime errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Runtime and resource errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemError {
    /// Runtime panic or crash
    RuntimePanic {
        message: String,
        location: Option<String>,
        context: ErrorContext,
    },
    
    /// Thread pool exhausted
    ThreadPoolExhausted {
        pool_name: String,
        max_threads: usize,
        context: ErrorContext,
    },
    
    /// Memory allocation failed
    AllocationFailed {
        size_bytes: usize,
        reason: Option<String>,
        context: ErrorContext,
    },
    
    /// Platform-specific error
    PlatformError {
        platform: String,
        error: String,
        code: Option<i32>,
        context: ErrorContext,
    },
    
    /// External service error
    ExternalServiceError {
        service: String,
        error: String,
        retry_possible: bool,
        context: ErrorContext,
    },
    
    /// Shutdown in progress
    ShutdownInProgress {
        reason: String,
        initiated_by: Option<String>,
        context: ErrorContext,
    },
    
    /// Not implemented
    NotImplemented {
        feature: String,
        reason: Option<String>,
        context: ErrorContext,
    },
    
    /// Internal error
    InternalError {
        message: String,
        file: Option<String>,
        line: Option<u32>,
        context: ErrorContext,
    },
}

impl fmt::Display for SystemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimePanic { message, location, .. } => {
                write!(f, "Runtime panic: {}", message)?;
                if let Some(loc) = location {
                    write!(f, " at {}", loc)?;
                }
                Ok(())
            }
            Self::ThreadPoolExhausted { pool_name, max_threads, .. } => {
                write!(f, "Thread pool '{}' exhausted (max: {} threads)", pool_name, max_threads)
            }
            Self::AllocationFailed { size_bytes, reason, .. } => {
                write!(f, "Memory allocation failed for {} bytes", size_bytes)?;
                if let Some(r) = reason {
                    write!(f, ": {}", r)?;
                }
                Ok(())
            }
            Self::PlatformError { platform, error, code, .. } => {
                write!(f, "Platform error on {}: {}", platform, error)?;
                if let Some(c) = code {
                    write!(f, " (code: {})", c)?;
                }
                Ok(())
            }
            Self::ExternalServiceError { service, error, retry_possible, .. } => {
                write!(f, "External service '{}' error: {}", service, error)?;
                if !retry_possible {
                    write!(f, " (not retryable)")?;
                }
                Ok(())
            }
            Self::ShutdownInProgress { reason, initiated_by, .. } => {
                write!(f, "Shutdown in progress: {}", reason)?;
                if let Some(by) = initiated_by {
                    write!(f, " (initiated by: {})", by)?;
                }
                Ok(())
            }
            Self::NotImplemented { feature, reason, .. } => {
                write!(f, "Feature not implemented: {}", feature)?;
                if let Some(r) = reason {
                    write!(f, " ({})", r)?;
                }
                Ok(())
            }
            Self::InternalError { message, file, line, .. } => {
                write!(f, "Internal error: {}", message)?;
                if let (Some(file_name), Some(l)) = (file, line) {
                    write!(f, " at {}:{}", file_name, l)?;
                }
                Ok(())
            }
        }
    }
}

impl SystemError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::RuntimePanic { .. } => ErrorCode::RuntimePanic,
            Self::ThreadPoolExhausted { .. } => ErrorCode::ThreadPoolExhausted,
            Self::AllocationFailed { .. } => ErrorCode::AllocationFailed,
            Self::PlatformError { .. } => ErrorCode::PlatformError,
            Self::ExternalServiceError { .. } => ErrorCode::ExternalServiceError,
            Self::ShutdownInProgress { .. } => ErrorCode::ShutdownInProgress,
            Self::NotImplemented { .. } => ErrorCode::NotImplemented,
            Self::InternalError { .. } => ErrorCode::InternalError,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::RuntimePanic { .. } => ErrorSeverity::Critical,
            Self::ThreadPoolExhausted { .. } => ErrorSeverity::High,
            Self::AllocationFailed { .. } => ErrorSeverity::Critical,
            Self::PlatformError { .. } => ErrorSeverity::High,
            Self::ExternalServiceError { .. } => ErrorSeverity::Medium,
            Self::ShutdownInProgress { .. } => ErrorSeverity::Low,
            Self::NotImplemented { .. } => ErrorSeverity::Medium,
            Self::InternalError { .. } => ErrorSeverity::Critical,
        }
    }
}