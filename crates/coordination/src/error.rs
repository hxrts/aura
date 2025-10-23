//! Coordination errors
//!
//! Error types for coordination protocols and execution infrastructure.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoordinationError {
    #[error("Invalid participant count: {0}")]
    InvalidParticipantCount(String),
    
    #[error("Threshold too high: {threshold} > {total}")]
    InvalidThreshold { threshold: u16, total: u16 },
    
    #[error("DKG round failed: {0}")]
    DkgFailed(String),
    
    #[error("Signing round failed: {0}")]
    SigningFailed(String),
    
    #[error("Resharing failed: {0}")]
    ResharingFailed(String),
    
    #[error("Missing participant: {0}")]
    MissingParticipant(String),
    
    #[error("Invalid signature")]
    InvalidSignature,
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Cryptographic error: {0}")]
    CryptoError(String),
    
    #[error("Device mismatch: sealed for {expected:?}, attempted unseal by {provided:?}")]
    DeviceMismatch {
        expected: crate::types::DeviceId,
        provided: crate::types::DeviceId,
    },
}

pub type Result<T> = std::result::Result<T, CoordinationError>;