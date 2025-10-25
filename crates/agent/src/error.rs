//! Refined error handling for agent operations
//!
//! This module provides a structured error hierarchy that groups related error types
//! into sub-enums while maintaining specific error information for debugging.

use thiserror::Error;

/// Protocol-related errors
///
/// Errors that occur during distributed protocol execution and coordination.
#[derive(Error, Debug)]
pub enum ProtocolError {
    /// Error in protocol orchestration or coordination
    #[error("Orchestrator error: {0}")]
    Orchestrator(String),

    /// Deterministic Key Derivation protocol failure
    #[error("DKD protocol failed: {0}")]
    DkdFailed(String),

    /// Session epoch mismatch between participants
    #[error("Session epoch mismatch: {0}")]
    EpochMismatch(String),

    /// Continuous Group Key Agreement protocol error
    #[error("CGKA protocol error: {0}")]
    CgkaFailed(String),

    /// Account bootstrap or initialization failure
    #[error("Bootstrap protocol error: {0}")]
    BootstrapFailed(String),
}

/// Data and state management errors
///
/// Errors related to data handling, state management, and entity lookup.
#[derive(Error, Debug)]
pub enum DataError {
    /// Error in ledger operations or state management
    #[error("Ledger error: {0}")]
    Ledger(String),

    /// Data serialization or deserialization failure
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Requested device not found in account
    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    /// Invalid context provided for operation
    #[error("Invalid context: {0}")]
    InvalidContext(String),
}

/// Cryptographic operation errors
///
/// Errors related to cryptographic operations and credential handling.
#[derive(Error, Debug)]
pub enum CryptoError {
    /// Cryptographic operation failure
    #[error("Cryptographic operation failed: {0}")]
    OperationFailed(String),

    /// Invalid credential or signature
    #[error("Invalid credential: {0}")]
    InvalidCredential(String),
}

/// Infrastructure and external system errors
///
/// Errors related to transport, storage, and network operations.
#[derive(Error, Debug)]
pub enum InfrastructureError {
    /// Network transport layer error
    #[error("Transport error: {0}")]
    Transport(String),

    /// Storage layer operation failure
    #[error("Storage error: {0}")]
    Storage(String),

    /// Network communication error
    #[error("Network error: {0}")]
    Network(String),
}

/// Capability system errors
///
/// Errors related to capability-based authorization and access control.
#[derive(Error, Debug)]
pub enum CapabilityError {
    /// Operation requires capability not possessed by agent
    #[error("Insufficient capability: {0}")]
    Insufficient(String),

    /// General capability system error
    #[error("Capability system error: {0}")]
    SystemError(String),
}

/// System and implementation errors
///
/// Errors related to system resources and unimplemented features.
#[derive(Error, Debug)]
pub enum SystemError {
    /// System time access or manipulation error
    #[error("System time error: {0}")]
    TimeError(String),

    /// Feature not yet implemented
    #[error("Feature not implemented: {0}")]
    NotImplemented(String),
}

/// Refined agent operation errors
///
/// Structured error hierarchy that groups related error types while maintaining
/// specific error information for debugging and appropriate error handling.
#[derive(Error, Debug)]
pub enum AgentError {
    /// Protocol execution and coordination errors
    #[error("Protocol error: {0}")]
    Protocol(#[from] ProtocolError),

    /// Data handling and state management errors
    #[error("Data error: {0}")]
    Data(#[from] DataError),

    /// Cryptographic operation errors
    #[error("Crypto error: {0}")]
    Crypto(#[from] CryptoError),

    /// Infrastructure and external system errors
    #[error("Infrastructure error: {0}")]
    Infrastructure(#[from] InfrastructureError),

    /// Capability system errors
    #[error("Capability error: {0}")]
    Capability(#[from] CapabilityError),

    /// System and implementation errors
    #[error("System error: {0}")]
    System(#[from] SystemError),
}

/// Result type alias for agent operations
///
/// Provides a convenient Result<T> that defaults to AgentError for error cases.
pub type Result<T> = std::result::Result<T, AgentError>;

// Convenience constructors for common error patterns
impl AgentError {
    /// Create a protocol orchestrator error
    pub fn orchestrator(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::Orchestrator(msg.into()))
    }

    /// Create a DKD protocol error
    pub fn dkd_failed(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::DkdFailed(msg.into()))
    }

    /// Create an epoch mismatch error
    pub fn epoch_mismatch(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::EpochMismatch(msg.into()))
    }

    /// Create a CGKA protocol error
    pub fn cgka_failed(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::CgkaFailed(msg.into()))
    }

    /// Create a bootstrap error
    pub fn bootstrap_failed(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::BootstrapFailed(msg.into()))
    }

    /// Create a ledger error
    pub fn ledger(msg: impl Into<String>) -> Self {
        AgentError::Data(DataError::Ledger(msg.into()))
    }

    /// Create a serialization error
    pub fn serialization(msg: impl Into<String>) -> Self {
        AgentError::Data(DataError::Serialization(msg.into()))
    }

    /// Create a device not found error
    pub fn device_not_found(msg: impl Into<String>) -> Self {
        AgentError::Data(DataError::DeviceNotFound(msg.into()))
    }

    /// Create an invalid context error
    pub fn invalid_context(msg: impl Into<String>) -> Self {
        AgentError::Data(DataError::InvalidContext(msg.into()))
    }

    /// Create a cryptographic operation error
    pub fn crypto_operation(msg: impl Into<String>) -> Self {
        AgentError::Crypto(CryptoError::OperationFailed(msg.into()))
    }

    /// Create an invalid credential error
    pub fn invalid_credential(msg: impl Into<String>) -> Self {
        AgentError::Crypto(CryptoError::InvalidCredential(msg.into()))
    }

    /// Create a transport error
    pub fn transport(msg: impl Into<String>) -> Self {
        AgentError::Infrastructure(InfrastructureError::Transport(msg.into()))
    }

    /// Create a storage error
    pub fn storage(msg: impl Into<String>) -> Self {
        AgentError::Infrastructure(InfrastructureError::Storage(msg.into()))
    }

    /// Create a network error
    pub fn network(msg: impl Into<String>) -> Self {
        AgentError::Infrastructure(InfrastructureError::Network(msg.into()))
    }

    /// Create an insufficient capability error
    pub fn insufficient_capability(msg: impl Into<String>) -> Self {
        AgentError::Capability(CapabilityError::Insufficient(msg.into()))
    }

    /// Create a capability system error
    pub fn capability_system(msg: impl Into<String>) -> Self {
        AgentError::Capability(CapabilityError::SystemError(msg.into()))
    }

    /// Create a system time error
    pub fn system_time(msg: impl Into<String>) -> Self {
        AgentError::System(SystemError::TimeError(msg.into()))
    }

    /// Create a not implemented error
    pub fn not_implemented(msg: impl Into<String>) -> Self {
        AgentError::System(SystemError::NotImplemented(msg.into()))
    }
    
    /// Create a coordination error
    pub fn coordination(msg: impl Into<String>) -> Self {
        AgentError::Protocol(ProtocolError::Orchestrator(msg.into()))
    }
}

// Compatibility conversions from existing crypto errors
impl From<aura_crypto::CryptoError> for AgentError {
    fn from(error: aura_crypto::CryptoError) -> Self {
        AgentError::Crypto(CryptoError::OperationFailed(format!("Crypto error: {:?}", error)))
    }
}

// Compatibility conversion from ledger errors
impl From<aura_journal::LedgerError> for AgentError {
    fn from(error: aura_journal::LedgerError) -> Self {
        AgentError::Data(DataError::Ledger(format!("Ledger error: {:?}", error)))
    }
}

// Compatibility conversion from String errors
impl From<String> for AgentError {
    fn from(error: String) -> Self {
        AgentError::Data(DataError::Ledger(error))
    }
}

// TODO: Re-enable when coordination is fixed
// impl From<aura_coordination::CoordinationError> for AgentError {
//     fn from(error: aura_coordination::CoordinationError) -> Self {
//         AgentError::Protocol(ProtocolError::DkdFailed(format!("Coordination error: {:?}", error)))
//     }
// }