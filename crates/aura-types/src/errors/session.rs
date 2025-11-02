//! Session type and state machine errors

use super::{ErrorCode, ErrorContext, ErrorSeverity};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Session type and state machine errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionError {
    /// Invalid session type
    InvalidSessionType {
        expected: String,
        actual: String,
        context: ErrorContext,
    },
    
    /// Session state mismatch
    StateMismatch {
        expected_state: String,
        actual_state: String,
        transition: Option<String>,
        context: ErrorContext,
    },
    
    /// Protocol violation
    ProtocolViolation {
        protocol: String,
        violation: String,
        phase: Option<String>,
        context: ErrorContext,
    },
    
    /// Choreography error
    ChoreographyError {
        reason: String,
        role: Option<String>,
        step: Option<String>,
        context: ErrorContext,
    },
    
    /// Role assignment error
    RoleAssignmentError {
        reason: String,
        role: String,
        participant: Option<String>,
        context: ErrorContext,
    },
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSessionType { expected, actual, .. } => {
                write!(f, "Invalid session type: expected {}, got {}", expected, actual)
            }
            Self::StateMismatch { expected_state, actual_state, transition, .. } => {
                write!(f, "Session state mismatch: expected {}, got {}", expected_state, actual_state)?;
                if let Some(t) = transition {
                    write!(f, " (during transition: {})", t)?;
                }
                Ok(())
            }
            Self::ProtocolViolation { protocol, violation, phase, .. } => {
                write!(f, "Protocol {} violation: {}", protocol, violation)?;
                if let Some(p) = phase {
                    write!(f, " (phase: {})", p)?;
                }
                Ok(())
            }
            Self::ChoreographyError { reason, role, step, .. } => {
                write!(f, "Choreography error: {}", reason)?;
                if let Some(r) = role {
                    write!(f, " (role: {})", r)?;
                }
                if let Some(s) = step {
                    write!(f, " (step: {})", s)?;
                }
                Ok(())
            }
            Self::RoleAssignmentError { reason, role, participant, .. } => {
                write!(f, "Role assignment error for {}: {}", role, reason)?;
                if let Some(p) = participant {
                    write!(f, " (participant: {})", p)?;
                }
                Ok(())
            }
        }
    }
}

impl SessionError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::InvalidSessionType { .. } => ErrorCode::InvalidSessionType,
            Self::StateMismatch { .. } => ErrorCode::SessionStateMismatch,
            Self::ProtocolViolation { .. } => ErrorCode::ProtocolViolation,
            Self::ChoreographyError { .. } => ErrorCode::ChoreographyError,
            Self::RoleAssignmentError { .. } => ErrorCode::RoleAssignmentError,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::InvalidSessionType { .. } => ErrorSeverity::High,
            Self::StateMismatch { .. } => ErrorSeverity::High,
            Self::ProtocolViolation { .. } => ErrorSeverity::Critical,
            Self::ChoreographyError { .. } => ErrorSeverity::High,
            Self::RoleAssignmentError { .. } => ErrorSeverity::Medium,
        }
    }
}