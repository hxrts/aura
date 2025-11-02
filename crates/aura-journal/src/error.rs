//! Error types for the journal

use thiserror::Error;

/// Journal error types
#[derive(Debug, Error)]
pub enum Error {
    /// Storage operation failed
    #[error("Storage failed: {0}")]
    Storage(String),
    
    /// Coordination/sync failed
    #[error("Coordination failed: {0}")]
    Coordination(String),
    
    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    
    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(aura_types::DeviceId),
    
    /// Guardian not found
    #[error("Guardian not found: {0}")]
    GuardianNotFound(aura_types::GuardianId),
    
    /// Automerge error
    #[error("Automerge error: {0}")]
    Automerge(String),
}

/// Result type for journal operations
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Create a storage error
    pub fn storage_failed(msg: impl Into<String>) -> Self {
        Self::Storage(msg.into())
    }
    
    /// Create a coordination error
    pub fn coordination_failed(msg: impl Into<String>) -> Self {
        Self::Coordination(msg.into())
    }
    
    /// Create an invalid operation error
    pub fn invalid_operation(msg: impl Into<String>) -> Self {
        Self::InvalidOperation(msg.into())
    }
}