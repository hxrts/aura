//! Agent-level operation errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// High-level agent operation errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentError {
    /// Agent initialization failed
    InitializationFailed {
        reason: String,
        component: Option<String>,
        context: ErrorContext,
    },
    
    /// Session management error
    SessionManagementError {
        reason: String,
        session_id: Option<String>,
        operation: Option<String>,
        context: ErrorContext,
    },
    
    /// Device management error
    DeviceManagementError {
        reason: String,
        device_id: Option<String>,
        operation: Option<String>,
        context: ErrorContext,
    },
    
    /// Storage adapter error
    StorageAdapterError {
        reason: String,
        adapter_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Transport adapter error
    TransportAdapterError {
        reason: String,
        adapter_type: Option<String>,
        context: ErrorContext,
    },
    
    /// Agent state error
    StateError {
        reason: String,
        expected_state: Option<String>,
        actual_state: Option<String>,
        context: ErrorContext,
    },
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitializationFailed { reason, component, .. } => {
                write!(f, "Agent initialization failed: {}", reason)?;
                if let Some(c) = component {
                    write!(f, " (component: {})", c)?;
                }
                Ok(())
            }
            Self::SessionManagementError { reason, session_id, operation, .. } => {
                write!(f, "Session management error: {}", reason)?;
                if let Some(id) = session_id {
                    write!(f, " (session: {})", id)?;
                }
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::DeviceManagementError { reason, device_id, operation, .. } => {
                write!(f, "Device management error: {}", reason)?;
                if let Some(id) = device_id {
                    write!(f, " (device: {})", id)?;
                }
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::StorageAdapterError { reason, adapter_type, .. } => {
                write!(f, "Storage adapter error: {}", reason)?;
                if let Some(at) = adapter_type {
                    write!(f, " (adapter: {})", at)?;
                }
                Ok(())
            }
            Self::TransportAdapterError { reason, adapter_type, .. } => {
                write!(f, "Transport adapter error: {}", reason)?;
                if let Some(at) = adapter_type {
                    write!(f, " (adapter: {})", at)?;
                }
                Ok(())
            }
            Self::StateError { reason, expected_state, actual_state, .. } => {
                write!(f, "Agent state error: {}", reason)?;
                if let (Some(exp), Some(act)) = (expected_state, actual_state) {
                    write!(f, " (expected: {}, actual: {})", exp, act)?;
                }
                Ok(())
            }
        }
    }
}

impl AgentError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::InitializationFailed { .. } => ErrorCode::AgentInitializationFailed,
            Self::SessionManagementError { .. } => ErrorCode::SessionManagementError,
            Self::DeviceManagementError { .. } => ErrorCode::DeviceManagementError,
            Self::StorageAdapterError { .. } => ErrorCode::StorageAdapterError,
            Self::TransportAdapterError { .. } => ErrorCode::TransportAdapterError,
            Self::StateError { .. } => ErrorCode::AgentStateError,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::InitializationFailed { .. } => ErrorSeverity::Critical,
            Self::SessionManagementError { .. } => ErrorSeverity::High,
            Self::DeviceManagementError { .. } => ErrorSeverity::High,
            Self::StorageAdapterError { .. } => ErrorSeverity::High,
            Self::TransportAdapterError { .. } => ErrorSeverity::Medium,
            Self::StateError { .. } => ErrorSeverity::High,
        }
    }
}