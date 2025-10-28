//! Session Error Types for Session-Typed Protocols
//!
//! This module defines error types used throughout the session types system
//! for type-safe protocol state management.

use aura_types::AuraError;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during agent session operations
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum AgentSessionError {
    #[error("Concurrent protocol operation not allowed: {0}")]
    ConcurrentProtocol(String),
    
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    
    #[error("Invalid state for operation: {0}")]
    InvalidState(String),
    
    #[error("Session error: {0}")]
    SessionError(String),
}

/// Errors that can occur during context session operations
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum ContextSessionError {
    #[error("Execution failed: {0}")]
    ExecutionFailed(String),
    
    #[error("Invalid state for operation: {0}")]
    InvalidState(String),
    
    #[error("Session error: {0}")]
    SessionError(String),
}

/// Errors that can occur during FROST session operations
#[derive(Debug, Clone, Error, Serialize, Deserialize)]
pub enum FrostSessionError {
    #[error("Invalid state for operation: {0}")]
    InvalidState(String),
    
    #[error("Threshold not met: expected {expected}, got {actual}")]
    ThresholdNotMet { expected: usize, actual: usize },
    
    #[error("Session error: {0}")]
    SessionError(String),
}

// Implement conversions to AuraError for unified error handling
impl From<AgentSessionError> for AuraError {
    fn from(err: AgentSessionError) -> Self {
        AuraError::session_error(&err.to_string())
    }
}

impl From<ContextSessionError> for AuraError {
    fn from(err: ContextSessionError) -> Self {
        AuraError::session_error(&err.to_string())
    }
}

impl From<FrostSessionError> for AuraError {
    fn from(err: FrostSessionError) -> Self {
        AuraError::session_error(&err.to_string())
    }
}