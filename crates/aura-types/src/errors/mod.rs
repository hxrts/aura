//! Unified error handling for the Aura platform
#![allow(missing_docs)] // Allow missing docs for repetitive error struct fields
#![allow(clippy::result_large_err)] // ErrorContext provides valuable debugging info
//!
//! This crate provides a centralized, structured error hierarchy for all Aura components.
//! It consolidates the previously fragmented error types across multiple crates into
//! a single, coherent system with proper error classification and handling.
//!
//! ## Design Principles
//!
//! 1. **Hierarchical Structure**: Errors are organized by domain (Protocol, Crypto, etc.)
//! 2. **Rich Context**: Each error includes relevant context and troubleshooting hints
//! 3. **Severity Classification**: Errors are classified by severity and impact
//! 4. **Structured Debugging**: Machine-readable error codes for automated handling
//! 5. **Zero Duplication**: Single source of truth for all error types
//!
//! ## Error Hierarchy
//!
//! ```text
//! AuraError
//! ├── Protocol: Distributed protocol execution errors
//! ├── Crypto: Cryptographic operation failures
//! ├── Infrastructure: Transport, storage, network errors
//! ├── Agent: High-level agent operation errors
//! ├── Data: State management and serialization errors
//! ├── Capability: Authorization and access control errors
//! ├── Session: Session type and state machine errors
//! └── System: Runtime and resource errors
//! ```
//!
//! ## Usage Examples
//!
//! ```rust
//! use aura_types::errors::{AuraError, Result, ErrorCode, ErrorSeverity};
//!
//! // Simple error creation
//! fn example_operation() -> Result<()> {
//!     Err(AuraError::dkd_failed("DKD timeout after 30s"))
//! }
//!
//! // Error with rich context
//! fn example_with_context() -> Result<()> {
//!     Err(AuraError::frost_failed("FROST signing failed"))
//! }
//!
//! // Error classification
//! fn handle_error(error: &AuraError) {
//!     match error.severity() {
//!         ErrorSeverity::Critical => { /* immediate action required */ }
//!         ErrorSeverity::High => { /* escalate to ops */ }
//!         ErrorSeverity::Medium => { /* log and monitor */ }
//!         ErrorSeverity::Low => { /* debug info only */ }
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt;
use thiserror::Error;

// Re-export commonly used types
pub use time::OffsetDateTime;
pub use uuid::Uuid;

// Error constructor macros to reduce boilerplate
pub mod macros;

// =============================================================================
// Error Severity and Classification
// =============================================================================

/// Error severity classification for operational monitoring
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ErrorSeverity {
    /// Debug information, no operational impact
    Low,
    /// Affects single operation, user can retry
    Medium,
    /// Affects multiple operations or requires intervention
    High,
    /// System-wide impact, immediate action required
    Critical,
}

/// Machine-readable error codes for automated handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ErrorCode {
    // Protocol Error Codes (1000-1999)
    /// Distributed Key Derivation protocol timed out
    ProtocolDkdTimeout = 1001,
    /// FROST threshold signature protocol failed
    ProtocolFrostSignFailed = 1002,
    /// Session epoch mismatch between participants
    ProtocolEpochMismatch = 1003,
    /// Continuous Group Key Agreement protocol failed
    ProtocolCgkaFailed = 1004,
    /// Account bootstrap protocol failed
    ProtocolBootstrapFailed = 1005,
    /// Account recovery protocol failed
    ProtocolRecoveryFailed = 1006,
    /// Key resharing protocol failed
    ProtocolResharingFailed = 1007,
    /// Protocol session timed out
    ProtocolSessionTimeout = 1008,
    /// Protocol coordination failed
    ProtocolCoordinationFailed = 1009,
    /// Protocol execution failed
    ProtocolExecutionFailed = 1016,
    /// Invalid protocol instruction
    ProtocolInvalidInstruction = 1017,

    // Crypto Error Codes (2000-2999)
    /// FROST threshold signature operation timed out
    CryptoFrostSignTimeout = 2001,
    /// Cryptographic key derivation failed
    CryptoKeyDerivationFailed = 2002,
    /// Digital signature verification failed
    CryptoInvalidSignature = 2003,
    /// Authentication credential is invalid
    CryptoInvalidCredential = 2004,
    /// Data encryption operation failed
    CryptoEncryptionFailed = 2005,
    /// Data decryption operation failed
    CryptoDecryptionFailed = 2006,
    /// Cryptographic hash computation failed
    CryptoHashingFailed = 2007,
    /// Secure random number generation failed
    CryptoRandomGenerationFailed = 2008,

    // Infrastructure Error Codes (3000-3999)
    /// Network transport connection establishment failed
    InfraTransportConnectionFailed = 3001,
    /// Network transport operation timed out
    InfraTransportTimeout = 3002,
    /// Storage system read operation failed
    InfraStorageReadFailed = 3003,
    /// Storage system write operation failed
    InfraStorageWriteFailed = 3004,
    /// Network destination is unreachable
    InfraNetworkUnreachable = 3005,
    /// Network partition detected
    InfraNetworkPartition = 3006,
    /// Storage quota limit exceeded
    InfraStorageQuotaExceeded = 3007,
    /// Transport layer authentication failed
    InfraTransportAuthenticationFailed = 3008,
    /// Invalid presence ticket
    InfraInvalidTicket = 3009,
    /// Connection handshake failed
    InfraHandshakeFailed = 3010,
    /// Message delivery failed
    InfraDeliveryFailed = 3011,
    /// Broadcast operation failed
    InfraBroadcastFailed = 3012,

    // Agent Error Codes (4000-4999)
    /// Agent is in invalid state for requested operation
    AgentInvalidState = 4001,
    /// Requested operation is not allowed in current context
    AgentOperationNotAllowed = 4002,
    /// Specified device not found in account
    AgentDeviceNotFound = 4003,
    /// Account not found or not accessible
    AgentAccountNotFound = 4004,
    /// Insufficient permissions for requested operation
    AgentInsufficientPermissions = 4005,
    /// Account bootstrap required before operation
    AgentBootstrapRequired = 4006,
    /// Agent already initialized
    AgentAlreadyInitialized = 4007,

    // Data Error Codes (5000-5999)
    /// Data serialization to binary format failed
    DataSerializationFailed = 5001,
    /// Data deserialization from binary format failed
    DataDeserializationFailed = 5002,
    /// Journal/ledger operation failed
    DataLedgerOperationFailed = 5003,
    /// Invalid context provided for operation
    DataInvalidContext = 5004,
    /// Data corruption detected during verification
    DataCorruptionDetected = 5005,
    /// Data version mismatch between components
    DataVersionMismatch = 5006,
    /// Merkle proof verification failed
    DataMerkleProofInvalid = 5007,

    // Session Error Codes (6000-6999)
    /// Invalid session state transition attempted
    SessionInvalidTransition = 6001,
    /// Session type mismatch in protocol
    SessionTypeMismatch = 6002,
    /// Protocol violation detected in session
    SessionProtocolViolation = 6003,
    /// Session operation timed out
    SessionTimeout = 6004,
    /// Session was aborted by participant
    SessionAborted = 6005,
    /// Session recovery after failure unsuccessful
    SessionRecoveryFailed = 6006,

    // System Error Codes (7000-7999)
    /// System time access or manipulation error
    SystemTimeError = 7001,
    /// System resources (memory, disk, etc.) exhausted
    SystemResourceExhausted = 7002,
    /// Requested feature not yet implemented
    SystemNotImplemented = 7003,
    /// System configuration is invalid or missing
    SystemConfigurationError = 7004,
    /// System permission denied for operation
    SystemPermissionDenied = 7005,

    // Generic codes
    /// Unknown or unclassified error
    GenericUnknown = 9999,
}

impl ErrorCode {
    /// Get the default severity for this error code
    pub fn default_severity(self) -> ErrorSeverity {
        match self {
            // Critical protocol failures
            Self::ProtocolBootstrapFailed
            | Self::ProtocolRecoveryFailed
            | Self::CryptoKeyDerivationFailed => ErrorSeverity::Critical,

            // High severity errors requiring attention
            Self::ProtocolDkdTimeout
            | Self::ProtocolFrostSignFailed
            | Self::ProtocolEpochMismatch
            | Self::CryptoFrostSignTimeout
            | Self::CryptoInvalidSignature
            | Self::InfraTransportConnectionFailed
            | Self::InfraStorageWriteFailed
            | Self::AgentInvalidState
            | Self::DataCorruptionDetected => ErrorSeverity::High,

            // Medium severity - affects operations but recoverable
            Self::ProtocolCgkaFailed
            | Self::ProtocolSessionTimeout
            | Self::CryptoInvalidCredential
            | Self::InfraTransportTimeout
            | Self::InfraStorageReadFailed
            | Self::AgentDeviceNotFound
            | Self::DataSerializationFailed
            | Self::SessionInvalidTransition => ErrorSeverity::Medium,

            // Low severity - debug or informational
            Self::SystemNotImplemented | Self::DataInvalidContext | Self::SessionTimeout => {
                ErrorSeverity::Low
            }

            // Default to medium for unknown codes
            _ => ErrorSeverity::Medium,
        }
    }
}

// =============================================================================
// Domain-Specific Error Types
// =============================================================================

/// Protocol execution and coordination errors (detailed variant with context fields)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolError {
    /// DKD (Distributed Key Derivation) protocol failed
    DkdFailed {
        /// Human-readable reason
        reason: String,
        /// Protocol phase where failure occurred
        phase: Option<String>,
        /// Participants involved
        participants: Option<Vec<String>>,
        /// Additional error context and metadata
        context: String,
    },

    /// FROST (Flexible Round-Optimized Schnorr Threshold) protocol failed
    FrostFailed {
        /// Human-readable reason
        reason: String,
        /// Round number where failure occurred
        round: Option<u8>,
        /// Threshold configuration
        threshold: Option<u16>,
        /// Additional error context and metadata
        context: String,
    },

    /// Resharing protocol failed
    ResharingFailed {
        /// Human-readable reason
        reason: String,
        /// Previous threshold configuration
        old_threshold: Option<u16>,
        /// New threshold configuration
        new_threshold: Option<u16>,
        /// Additional error context and metadata
        context: String,
    },

    /// Recovery protocol failed
    RecoveryFailed {
        /// Human-readable reason
        reason: String,
        /// Number of guardians involved
        guardian_count: Option<usize>,
        /// Required shares for recovery
        required_shares: Option<usize>,
        /// Additional error context and metadata
        context: String,
    },

    /// Coordination service error
    CoordinationFailed {
        /// Human-readable reason
        reason: String,
        /// Service name
        service: Option<String>,
        /// Operation being performed
        operation: Option<String>,
        /// Additional error context and metadata
        context: String,
    },

    /// Session protocol error
    SessionFailed {
        /// Human-readable reason
        reason: String,
        /// Type of session
        session_type: Option<String>,
        /// Session state
        state: Option<String>,
        /// Additional error context and metadata
        context: String,
    },

    /// Consensus protocol error
    ConsensusFailed {
        /// Human-readable reason
        reason: String,
        /// Protocol type involved
        protocol_type: Option<String>,
        /// Round number
        round: Option<u64>,
        /// Additional error context and metadata
        context: String,
    },

    /// Timeout during protocol execution
    Timeout {
        /// Protocol name
        protocol: String,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
        /// Phase where timeout occurred
        phase: Option<String>,
        /// Additional error context and metadata
        context: String,
    },

    /// Invalid protocol state transition
    InvalidStateTransition {
        /// Previous state
        from_state: String,
        /// Attempted state
        to_state: String,
        /// Reason for invalidity
        reason: String,
        /// Additional error context and metadata
        context: String,
    },

    /// Protocol message validation failed
    MessageValidationFailed {
        /// Message type
        message_type: String,
        /// Validation failure reason
        reason: String,
        /// Message sender
        sender: Option<String>,
        /// Additional error context and metadata
        context: String,
    },

    /// Byzantine behavior detected
    ByzantineBehavior {
        /// Participant showing Byzantine behavior
        participant: String,
        /// Type of behavior
        behavior: String,
        /// Evidence of Byzantine behavior
        evidence: Option<String>,
        /// Additional error context and metadata
        context: String,
    },

    /// Insufficient participants for protocol
    InsufficientParticipants {
        /// Required participant count
        required: usize,
        /// Available participant count
        available: usize,
        /// Protocol name
        protocol: String,
        /// Additional error context and metadata
        context: String,
    },
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DkdFailed { reason, phase, .. } => {
                write!(f, "DKD protocol failed: {}", reason)?;
                if let Some(p) = phase {
                    write!(f, " (phase: {})", p)?;
                }
                Ok(())
            }
            Self::FrostFailed {
                reason,
                round,
                threshold,
                ..
            } => {
                write!(f, "FROST protocol failed: {}", reason)?;
                if let Some(r) = round {
                    write!(f, " (round: {})", r)?;
                }
                if let Some(t) = threshold {
                    write!(f, " (threshold: {})", t)?;
                }
                Ok(())
            }
            Self::ResharingFailed {
                reason,
                old_threshold,
                new_threshold,
                ..
            } => {
                write!(f, "Resharing protocol failed: {}", reason)?;
                if let (Some(old), Some(new)) = (old_threshold, new_threshold) {
                    write!(f, " (threshold: {} -> {})", old, new)?;
                }
                Ok(())
            }
            Self::RecoveryFailed {
                reason,
                guardian_count,
                required_shares,
                ..
            } => {
                write!(f, "Recovery protocol failed: {}", reason)?;
                if let (Some(count), Some(required)) = (guardian_count, required_shares) {
                    write!(f, " (guardians: {}, required: {})", count, required)?;
                }
                Ok(())
            }
            Self::CoordinationFailed {
                reason,
                service,
                operation,
                ..
            } => {
                write!(f, "Coordination failed: {}", reason)?;
                if let Some(s) = service {
                    write!(f, " (service: {})", s)?;
                }
                if let Some(op) = operation {
                    write!(f, " (operation: {})", op)?;
                }
                Ok(())
            }
            Self::SessionFailed {
                reason,
                session_type,
                state,
                ..
            } => {
                write!(f, "Session protocol failed: {}", reason)?;
                if let Some(t) = session_type {
                    write!(f, " (type: {})", t)?;
                }
                if let Some(s) = state {
                    write!(f, " (state: {})", s)?;
                }
                Ok(())
            }
            Self::ConsensusFailed {
                reason,
                protocol_type,
                round,
                ..
            } => {
                write!(f, "Consensus failed: {}", reason)?;
                if let Some(p) = protocol_type {
                    write!(f, " (protocol: {})", p)?;
                }
                if let Some(r) = round {
                    write!(f, " (round: {})", r)?;
                }
                Ok(())
            }
            Self::Timeout {
                protocol,
                timeout_ms,
                phase,
                ..
            } => {
                write!(f, "Protocol {} timed out after {}ms", protocol, timeout_ms)?;
                if let Some(p) = phase {
                    write!(f, " (phase: {})", p)?;
                }
                Ok(())
            }
            Self::InvalidStateTransition {
                from_state,
                to_state,
                reason,
                ..
            } => {
                write!(
                    f,
                    "Invalid state transition from {} to {}: {}",
                    from_state, to_state, reason
                )
            }
            Self::MessageValidationFailed {
                message_type,
                reason,
                sender,
                ..
            } => {
                write!(
                    f,
                    "Message validation failed for {}: {}",
                    message_type, reason
                )?;
                if let Some(s) = sender {
                    write!(f, " (sender: {})", s)?;
                }
                Ok(())
            }
            Self::ByzantineBehavior {
                participant,
                behavior,
                ..
            } => {
                write!(
                    f,
                    "Byzantine behavior detected from {}: {}",
                    participant, behavior
                )
            }
            Self::InsufficientParticipants {
                required,
                available,
                protocol,
                ..
            } => {
                write!(
                    f,
                    "Insufficient participants for {}: {} required, {} available",
                    protocol, required, available
                )
            }
        }
    }
}

impl StdError for ProtocolError {}

impl ProtocolError {
    /// Get the error code for this error
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::DkdFailed { .. } => ErrorCode::ProtocolDkdTimeout,
            Self::FrostFailed { .. } => ErrorCode::ProtocolFrostSignFailed,
            Self::ResharingFailed { .. } => ErrorCode::ProtocolResharingFailed,
            Self::RecoveryFailed { .. } => ErrorCode::ProtocolRecoveryFailed,
            Self::CoordinationFailed { .. } => ErrorCode::ProtocolCoordinationFailed,
            Self::SessionFailed { .. } => ErrorCode::SessionProtocolViolation,
            Self::ConsensusFailed { .. } => ErrorCode::GenericUnknown,
            Self::Timeout { .. } => ErrorCode::SessionTimeout,
            Self::InvalidStateTransition { .. } => ErrorCode::SessionInvalidTransition,
            Self::MessageValidationFailed { .. } => ErrorCode::GenericUnknown,
            Self::ByzantineBehavior { .. } => ErrorCode::GenericUnknown,
            Self::InsufficientParticipants { .. } => ErrorCode::GenericUnknown,
        }
    }

    /// Get the severity of this error
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::ByzantineBehavior { .. } => ErrorSeverity::Critical,
            Self::DkdFailed { .. } | Self::FrostFailed { .. } => ErrorSeverity::High,
            Self::ResharingFailed { .. } | Self::RecoveryFailed { .. } => ErrorSeverity::High,
            Self::CoordinationFailed { .. } | Self::SessionFailed { .. } => ErrorSeverity::Medium,
            Self::ConsensusFailed { .. } => ErrorSeverity::High,
            Self::Timeout { .. } => ErrorSeverity::Medium,
            Self::InvalidStateTransition { .. } => ErrorSeverity::High,
            Self::MessageValidationFailed { .. } => ErrorSeverity::Medium,
            Self::InsufficientParticipants { .. } => ErrorSeverity::Medium,
        }
    }
}

/// Cryptographic operation errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum CryptoError {
    /// FROST threshold signature operation failed
    #[error("FROST signing failed: {message}")]
    FrostSignFailed { message: String, context: String },

    /// Key derivation operation failed
    #[error("Key derivation failed: {message}")]
    KeyDerivationFailed { message: String, context: String },

    /// Invalid signature verification
    #[error("Invalid signature: {message}")]
    InvalidSignature { message: String, context: String },

    /// Invalid credential or authentication token
    #[error("Invalid credential: {message}")]
    InvalidCredential { message: String, context: String },

    /// Encryption operation failed
    #[error("Encryption failed: {message}")]
    EncryptionFailed { message: String, context: String },

    /// Decryption operation failed
    #[error("Decryption failed: {message}")]
    DecryptionFailed { message: String, context: String },

    /// Hash computation failed
    #[error("Hashing failed: {message}")]
    HashingFailed { message: String, context: String },

    /// Random number generation failed
    #[error("Random generation failed: {message}")]
    RandomGenerationFailed { message: String, context: String },

    /// Generic cryptographic operation failure
    #[error("Cryptographic operation failed: {message}")]
    OperationFailed { message: String, context: String },

    /// Invalid input provided to cryptographic operation
    #[error("Invalid input: {message}")]
    InvalidInput { message: String, context: String },

    /// Invalid output from cryptographic operation
    #[error("Invalid output: {message}")]
    InvalidOutput { message: String, context: String },

    /// Signing operation failed
    #[error("Signing failed: {message}")]
    SigningFailed { message: String, context: String },

    /// Verification operation failed
    #[error("Verification failed: {message}")]
    VerificationFailed { message: String, context: String },

    /// Insufficient security level for operation
    #[error("Insufficient security level: {message}")]
    InsufficientSecurityLevel { message: String, context: String },

    /// Rate limiting applied to operation
    #[error("Rate limited: {message}")]
    RateLimited { message: String, context: String },

    /// Permission denied for operation
    #[error("Permission denied: {message}")]
    PermissionDenied { message: String, context: String },

    /// Hardware security module not available
    #[error("Hardware not available: {message}")]
    HardwareNotAvailable { message: String, context: String },

    /// Hardware attestation failed
    #[error("Attestation failed: {message}")]
    AttestationFailed { message: String, context: String },

    /// Timing anomaly detected
    #[error("Timing anomaly: {message}")]
    TimingAnomaly { message: String, context: String },

    /// Insufficient entropy for operation
    #[error("Insufficient entropy: {message}")]
    InsufficientEntropy { message: String, context: String },

    /// Poor randomness quality detected
    #[error("Poor randomness: {message}")]
    PoorRandomness { message: String, context: String },

    /// Component not initialized
    #[error("Not initialized: {message}")]
    NotInitialized { message: String, context: String },

    /// Invalid operation requested
    #[error("Invalid operation: {message}")]
    InvalidOperation { message: String, context: String },

    /// Resource not found
    #[error("Not found: {message}")]
    NotFound { message: String, context: String },

    /// Unsupported algorithm
    #[error("Unsupported algorithm: {message}")]
    UnsupportedAlgorithm { message: String, context: String },

    /// Internal error
    #[error("Internal error: {message}")]
    InternalError { message: String, context: String },
}

/// Infrastructure and external system errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum InfrastructureError {
    /// Network transport layer error
    #[error("Transport error: {message}")]
    Transport { message: String, context: String },

    /// Storage layer operation failure
    #[error("Storage error: {message}")]
    Storage { message: String, context: String },

    /// Configuration error
    #[error("Configuration error: {message}")]
    ConfigError { message: String, context: String },

    /// Network communication error
    #[error("Network error: {message}")]
    Network { message: String, context: String },

    /// Transport connection failure
    #[error("Transport connection failed: {message}")]
    TransportConnectionFailed { message: String, context: String },

    /// Transport operation timeout
    #[error("Transport timeout: {message}")]
    TransportTimeout { message: String, context: String },

    /// Storage read operation failed
    #[error("Storage read failed: {message}")]
    StorageReadFailed { message: String, context: String },

    /// Storage write operation failed
    #[error("Storage write failed: {message}")]
    StorageWriteFailed { message: String, context: String },

    /// Network unreachable
    #[error("Network unreachable: {message}")]
    NetworkUnreachable { message: String, context: String },

    /// Network partition detected
    #[error("Network partition: {message}")]
    NetworkPartition { message: String, context: String },

    /// Storage quota exceeded
    #[error("Storage quota exceeded: {message}")]
    StorageQuotaExceeded { message: String, context: String },

    /// Transport authentication failed
    #[error("Transport authentication failed: {message}")]
    TransportAuthenticationFailed { message: String, context: String },

    /// Invalid presence ticket
    #[error("Invalid presence ticket: {message}")]
    InvalidTicket { message: String, context: String },

    /// Connection handshake failed
    #[error("Connection handshake failed: {message}")]
    HandshakeFailed { message: String, context: String },

    /// Message delivery failed
    #[error("Message delivery failed: {message}")]
    DeliveryFailed { message: String, context: String },

    /// Broadcast operation failed
    #[error("Broadcast operation failed: {message}")]
    BroadcastFailed { message: String, context: String },
}

/// Agent operation and state management errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum AgentError {
    /// Agent in invalid state for operation
    #[error("Agent invalid state: {message}")]
    InvalidState { message: String, context: String },

    /// Operation not allowed in current state
    #[error("Operation not allowed: {message}")]
    OperationNotAllowed { message: String, context: String },

    /// Requested device not found in account
    #[error("Device not found: {message}")]
    DeviceNotFound { message: String, context: String },

    /// Account not found or not accessible
    #[error("Account not found: {message}")]
    AccountNotFound { message: String, context: String },

    /// Insufficient permissions for operation
    #[error("Insufficient permissions: {message}")]
    InsufficientPermissions { message: String, context: String },

    /// Bootstrap required before operation
    #[error("Bootstrap required: {message}")]
    BootstrapRequired { message: String, context: String },

    /// Agent already initialized
    #[error("Already initialized: {message}")]
    AlreadyInitialized { message: String, context: String },
}

/// Data handling and state management errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum DataError {
    /// Data serialization failure
    #[error("Serialization failed: {message}")]
    SerializationFailed { message: String, context: String },

    /// Data deserialization failure
    #[error("Deserialization failed: {message}")]
    DeserializationFailed { message: String, context: String },

    /// Ledger operation failure
    #[error("Ledger operation failed: {message}")]
    LedgerOperationFailed { message: String, context: String },

    /// Invalid context provided for operation
    #[error("Invalid context: {message}")]
    InvalidContext { message: String, context: String },

    /// Data corruption detected
    #[error("Data corruption detected: {message}")]
    CorruptionDetected { message: String, context: String },

    /// Data version mismatch
    #[error("Version mismatch: {message}")]
    VersionMismatch { message: String, context: String },

    /// Invalid Merkle proof
    #[error("Invalid Merkle proof: {message}")]
    InvalidMerkleProof { message: String, context: String },

    /// Generic ledger error
    #[error("Ledger error: {message}")]
    Ledger { message: String, context: String },
}

/// Capability system and authorization errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityError {
    /// Operation requires capability not possessed by agent
    #[error("Insufficient capability: {message}")]
    Insufficient { message: String, context: String },

    /// General capability system error
    #[error("Capability system error: {message}")]
    SystemError { message: String, context: String },

    /// Capability grant failed
    #[error("Capability grant failed: {message}")]
    GrantFailed { message: String, context: String },

    /// Capability revocation failed
    #[error("Capability revocation failed: {message}")]
    RevocationFailed { message: String, context: String },

    /// Capability verification failed
    #[error("Capability verification failed: {message}")]
    VerificationFailed { message: String, context: String },

    // Variants from store/capability_manager.rs
    /// No capabilities found for device
    #[error("No capabilities found: {message}")]
    NoCapabilities { message: String, context: String },

    /// All capabilities have expired
    #[error("Capabilities expired: {message}")]
    ExpiredCapabilities { message: String, context: String },

    /// Insufficient permissions for operation
    #[error("Insufficient permissions: {message}")]
    InsufficientPermissions { message: String, context: String },

    /// Invalid delegation chain
    #[error("Invalid delegation: {message}")]
    InvalidDelegation { message: String, context: String },

    /// Parent capability has been revoked
    #[error("Parent capability revoked: {message}")]
    ParentRevoked { message: String, context: String },

    // Variants from store/access_control/capability.rs
    /// Capability token not found
    #[error("Token not found: {message}")]
    TokenNotFound { message: String, context: String },

    /// Capability token is expired
    #[error("Token expired: {message}")]
    TokenExpired { message: String, context: String },

    /// Capability token is revoked
    #[error("Token revoked: {message}")]
    TokenRevoked { message: String, context: String },

    /// Permission denied for operation
    #[error("Permission denied: {message}")]
    PermissionDenied { message: String, context: String },

    /// Invalid signature on capability
    #[error("Invalid signature: {message}")]
    InvalidSignature { message: String, context: String },

    /// Capability validation failed
    #[error("Validation failed: {message}")]
    ValidationFailed { message: String, context: String },

    // Variants from journal/capability/mod.rs
    /// Invalid capability chain
    #[error("Invalid capability chain: {message}")]
    InvalidChain { message: String, context: String },

    /// Authority not found
    #[error("Authority not found: {message}")]
    AuthorityNotFound { message: String, context: String },

    /// Revocation not authorized
    #[error("Revocation not authorized: {message}")]
    RevocationNotAuthorized { message: String, context: String },

    /// Capability expired at specific timestamp
    #[error("Capability expired at {timestamp}: {message}")]
    CapabilityExpired {
        message: String,
        timestamp: u64,
        context: String,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {message}")]
    CryptographicError { message: String, context: String },

    /// Authorization failed
    #[error("Authorization error: {message}")]
    AuthorizationError { message: String, context: String },

    /// Serialization failed
    #[error("Serialization error: {message}")]
    SerializationError { message: String, context: String },
}

/// Session type and state machine errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::result_large_err)] // ErrorContext provides valuable debugging info
pub enum SessionError {
    /// Invalid state transition attempted
    #[error("Invalid state transition: {message}")]
    InvalidTransition { message: String, context: String },

    /// Session type mismatch
    #[error("Session type mismatch: {message}")]
    TypeMismatch { message: String, context: String },

    /// Protocol violation in session
    #[error("Protocol violation: {message}")]
    ProtocolViolation { message: String, context: String },

    /// Session timeout
    #[error("Session timeout: {message}")]
    Timeout { message: String, context: String },

    /// Session aborted
    #[error("Session aborted: {message}")]
    Aborted { message: String, context: String },

    /// Session recovery failed
    #[error("Session recovery failed: {message}")]
    RecoveryFailed { message: String, context: String },

    /// Session rehydration failed
    #[error("Session rehydration failed: {message}")]
    RehydrationFailed { message: String, context: String },
}

/// System and runtime errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum SystemError {
    /// System time access or manipulation error
    #[error("System time error: {message}")]
    TimeError { message: String, context: String },

    /// System resource exhausted
    #[error("Resource exhausted: {message}")]
    ResourceExhausted { message: String, context: String },

    /// Feature not yet implemented
    #[error("Not implemented: {message}")]
    NotImplemented { message: String, context: String },

    /// System configuration error
    #[error("Configuration error: {message}")]
    ConfigurationError { message: String, context: String },

    /// System permission denied
    #[error("Permission denied: {message}")]
    PermissionDenied { message: String, context: String },
}

// =============================================================================
// Unified Error Type
// =============================================================================

/// Rich error context for debugging and monitoring
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorContext {
    /// Machine-readable error code
    pub code: Option<ErrorCode>,
    /// Error severity level
    pub severity: Option<ErrorSeverity>,
    /// When the error occurred
    pub timestamp: Option<OffsetDateTime>,
    /// Additional context key-value pairs
    pub context: HashMap<String, String>,
    /// Call stack or trace information
    pub trace: Option<String>,
    /// Suggested remediation actions
    pub remediation: Option<String>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new() -> Self {
        Self {
            timestamp: Some(OffsetDateTime::now_utc()),
            ..Default::default()
        }
    }

    /// Add a context key-value pair
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Set the error code
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self.severity = Some(code.default_severity());
        self
    }

    /// Set the severity level
    pub fn with_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// Add trace information
    pub fn with_trace(mut self, trace: impl Into<String>) -> Self {
        self.trace = Some(trace.into());
        self
    }

    /// Add remediation suggestion
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}

/// Unified error type for all Aura operations
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum AuraError {
    /// Protocol execution and coordination errors
    #[error("Protocol error: {0}")]
    Protocol(ProtocolError),

    /// Cryptographic operation errors
    #[error("Crypto error: {0}")]
    Crypto(CryptoError),

    /// Infrastructure and external system errors
    #[error("Infrastructure error: {0}")]
    Infrastructure(InfrastructureError),

    /// Agent operation and state management errors
    #[error("Agent error: {0}")]
    Agent(AgentError),

    /// Data handling and state management errors
    #[error("Data error: {0}")]
    Data(DataError),

    /// Capability system and authorization errors
    #[error("Capability error: {0}")]
    Capability(CapabilityError),

    /// Session type and state machine errors
    #[error("Session error: {0}")]
    Session(SessionError),

    /// System and runtime errors
    #[error("System error: {0}")]
    System(SystemError),
}

impl AuraError {
    /// Get the error severity
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::Protocol(e) => e.severity(),
            Self::Crypto(_) => ErrorSeverity::High,
            Self::Infrastructure(_) => ErrorSeverity::Medium,
            Self::Agent(_) => ErrorSeverity::High,
            Self::Data(_) => ErrorSeverity::High,
            Self::Capability(_) => ErrorSeverity::High,
            Self::Session(_) => ErrorSeverity::Medium,
            Self::System(_) => ErrorSeverity::High,
        }
    }

    /// Get the error code if present
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            Self::Protocol(e) => Some(e.error_code()),
            _ => None,
        }
    }

    /// Get the error context
    pub fn context(&self) -> &String {
        match self {
            Self::Protocol(e) => match e {
                ProtocolError::DkdFailed { context, .. }
                | ProtocolError::FrostFailed { context, .. }
                | ProtocolError::RecoveryFailed { context, .. }
                | ProtocolError::ResharingFailed { context, .. }
                | ProtocolError::CoordinationFailed { context, .. }
                | ProtocolError::SessionFailed { context, .. }
                | ProtocolError::ConsensusFailed { context, .. }
                | ProtocolError::Timeout { context, .. }
                | ProtocolError::InvalidStateTransition { context, .. }
                | ProtocolError::MessageValidationFailed { context, .. }
                | ProtocolError::ByzantineBehavior { context, .. }
                | ProtocolError::InsufficientParticipants { context, .. } => context,
            },
            Self::Crypto(e) => match e {
                CryptoError::FrostSignFailed { context, .. }
                | CryptoError::KeyDerivationFailed { context, .. }
                | CryptoError::InvalidSignature { context, .. }
                | CryptoError::InvalidCredential { context, .. }
                | CryptoError::EncryptionFailed { context, .. }
                | CryptoError::DecryptionFailed { context, .. }
                | CryptoError::HashingFailed { context, .. }
                | CryptoError::RandomGenerationFailed { context, .. }
                | CryptoError::OperationFailed { context, .. }
                | CryptoError::InvalidInput { context, .. }
                | CryptoError::InvalidOutput { context, .. }
                | CryptoError::SigningFailed { context, .. }
                | CryptoError::VerificationFailed { context, .. }
                | CryptoError::InsufficientSecurityLevel { context, .. }
                | CryptoError::RateLimited { context, .. }
                | CryptoError::PermissionDenied { context, .. }
                | CryptoError::HardwareNotAvailable { context, .. }
                | CryptoError::AttestationFailed { context, .. }
                | CryptoError::TimingAnomaly { context, .. }
                | CryptoError::InsufficientEntropy { context, .. }
                | CryptoError::PoorRandomness { context, .. }
                | CryptoError::NotInitialized { context, .. }
                | CryptoError::InvalidOperation { context, .. }
                | CryptoError::NotFound { context, .. }
                | CryptoError::UnsupportedAlgorithm { context, .. }
                | CryptoError::InternalError { context, .. } => context,
            },
            Self::Infrastructure(e) => match e {
                InfrastructureError::Transport { context, .. }
                | InfrastructureError::Storage { context, .. }
                | InfrastructureError::ConfigError { context, .. }
                | InfrastructureError::Network { context, .. }
                | InfrastructureError::TransportConnectionFailed { context, .. }
                | InfrastructureError::TransportTimeout { context, .. }
                | InfrastructureError::StorageReadFailed { context, .. }
                | InfrastructureError::StorageWriteFailed { context, .. }
                | InfrastructureError::NetworkUnreachable { context, .. }
                | InfrastructureError::NetworkPartition { context, .. }
                | InfrastructureError::StorageQuotaExceeded { context, .. }
                | InfrastructureError::TransportAuthenticationFailed { context, .. }
                | InfrastructureError::InvalidTicket { context, .. }
                | InfrastructureError::HandshakeFailed { context, .. }
                | InfrastructureError::DeliveryFailed { context, .. }
                | InfrastructureError::BroadcastFailed { context, .. } => context,
            },
            Self::Agent(e) => match e {
                AgentError::InvalidState { context, .. }
                | AgentError::OperationNotAllowed { context, .. }
                | AgentError::DeviceNotFound { context, .. }
                | AgentError::AccountNotFound { context, .. }
                | AgentError::InsufficientPermissions { context, .. }
                | AgentError::BootstrapRequired { context, .. }
                | AgentError::AlreadyInitialized { context, .. } => context,
            },
            Self::Data(e) => match e {
                DataError::SerializationFailed { context, .. }
                | DataError::DeserializationFailed { context, .. }
                | DataError::LedgerOperationFailed { context, .. }
                | DataError::InvalidContext { context, .. }
                | DataError::CorruptionDetected { context, .. }
                | DataError::VersionMismatch { context, .. }
                | DataError::InvalidMerkleProof { context, .. }
                | DataError::Ledger { context, .. } => context,
            },
            Self::Capability(e) => match e {
                CapabilityError::Insufficient { context, .. }
                | CapabilityError::SystemError { context, .. }
                | CapabilityError::GrantFailed { context, .. }
                | CapabilityError::RevocationFailed { context, .. }
                | CapabilityError::VerificationFailed { context, .. }
                | CapabilityError::NoCapabilities { context, .. }
                | CapabilityError::ExpiredCapabilities { context, .. }
                | CapabilityError::InsufficientPermissions { context, .. }
                | CapabilityError::InvalidDelegation { context, .. }
                | CapabilityError::ParentRevoked { context, .. }
                | CapabilityError::TokenNotFound { context, .. }
                | CapabilityError::TokenExpired { context, .. }
                | CapabilityError::TokenRevoked { context, .. }
                | CapabilityError::PermissionDenied { context, .. }
                | CapabilityError::InvalidSignature { context, .. }
                | CapabilityError::ValidationFailed { context, .. }
                | CapabilityError::InvalidChain { context, .. }
                | CapabilityError::AuthorityNotFound { context, .. }
                | CapabilityError::RevocationNotAuthorized { context, .. }
                | CapabilityError::CapabilityExpired { context, .. }
                | CapabilityError::CryptographicError { context, .. }
                | CapabilityError::AuthorizationError { context, .. }
                | CapabilityError::SerializationError { context, .. } => context,
            },
            Self::Session(e) => match e {
                SessionError::InvalidTransition { context, .. }
                | SessionError::TypeMismatch { context, .. }
                | SessionError::ProtocolViolation { context, .. }
                | SessionError::Timeout { context, .. }
                | SessionError::Aborted { context, .. }
                | SessionError::RecoveryFailed { context, .. }
                | SessionError::RehydrationFailed { context, .. } => context,
            },
            Self::System(e) => match e {
                SystemError::TimeError { context, .. }
                | SystemError::ResourceExhausted { context, .. }
                | SystemError::NotImplemented { context, .. }
                | SystemError::ConfigurationError { context, .. }
                | SystemError::PermissionDenied { context, .. } => context,
            },
        }
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        match self {
            // Protocol errors that might be retryable
            Self::Protocol(ProtocolError::Timeout { .. })
            | Self::Protocol(ProtocolError::CoordinationFailed { .. }) => true,

            // Infrastructure errors are generally retryable
            Self::Infrastructure(InfrastructureError::TransportTimeout { .. })
            | Self::Infrastructure(InfrastructureError::NetworkUnreachable { .. })
            | Self::Infrastructure(InfrastructureError::TransportConnectionFailed { .. }) => true,

            // System resource errors might be retryable
            Self::System(SystemError::ResourceExhausted { .. }) => true,

            // Most other errors are not retryable
            _ => false,
        }
    }

    // Private helper to get mutable context
    #[allow(dead_code)]
    fn context_mut(&mut self) -> &mut String {
        match self {
            Self::Protocol(e) => match e {
                ProtocolError::DkdFailed { context, .. }
                | ProtocolError::FrostFailed { context, .. }
                | ProtocolError::ResharingFailed { context, .. }
                | ProtocolError::RecoveryFailed { context, .. }
                | ProtocolError::CoordinationFailed { context, .. }
                | ProtocolError::SessionFailed { context, .. }
                | ProtocolError::ConsensusFailed { context, .. }
                | ProtocolError::Timeout { context, .. }
                | ProtocolError::InvalidStateTransition { context, .. }
                | ProtocolError::MessageValidationFailed { context, .. }
                | ProtocolError::ByzantineBehavior { context, .. }
                | ProtocolError::InsufficientParticipants { context, .. } => context,
            },
            Self::Crypto(e) => match e {
                CryptoError::FrostSignFailed { context, .. }
                | CryptoError::KeyDerivationFailed { context, .. }
                | CryptoError::InvalidSignature { context, .. }
                | CryptoError::InvalidCredential { context, .. }
                | CryptoError::EncryptionFailed { context, .. }
                | CryptoError::DecryptionFailed { context, .. }
                | CryptoError::HashingFailed { context, .. }
                | CryptoError::RandomGenerationFailed { context, .. }
                | CryptoError::OperationFailed { context, .. }
                | CryptoError::InvalidInput { context, .. }
                | CryptoError::InvalidOutput { context, .. }
                | CryptoError::SigningFailed { context, .. }
                | CryptoError::VerificationFailed { context, .. }
                | CryptoError::InsufficientSecurityLevel { context, .. }
                | CryptoError::RateLimited { context, .. }
                | CryptoError::PermissionDenied { context, .. }
                | CryptoError::HardwareNotAvailable { context, .. }
                | CryptoError::AttestationFailed { context, .. }
                | CryptoError::TimingAnomaly { context, .. }
                | CryptoError::InsufficientEntropy { context, .. }
                | CryptoError::PoorRandomness { context, .. }
                | CryptoError::NotInitialized { context, .. }
                | CryptoError::InvalidOperation { context, .. }
                | CryptoError::NotFound { context, .. }
                | CryptoError::UnsupportedAlgorithm { context, .. }
                | CryptoError::InternalError { context, .. } => context,
            },
            Self::Infrastructure(e) => match e {
                InfrastructureError::Transport { context, .. }
                | InfrastructureError::Storage { context, .. }
                | InfrastructureError::ConfigError { context, .. }
                | InfrastructureError::Network { context, .. }
                | InfrastructureError::TransportConnectionFailed { context, .. }
                | InfrastructureError::TransportTimeout { context, .. }
                | InfrastructureError::StorageReadFailed { context, .. }
                | InfrastructureError::StorageWriteFailed { context, .. }
                | InfrastructureError::NetworkUnreachable { context, .. }
                | InfrastructureError::NetworkPartition { context, .. }
                | InfrastructureError::StorageQuotaExceeded { context, .. }
                | InfrastructureError::TransportAuthenticationFailed { context, .. }
                | InfrastructureError::InvalidTicket { context, .. }
                | InfrastructureError::HandshakeFailed { context, .. }
                | InfrastructureError::DeliveryFailed { context, .. }
                | InfrastructureError::BroadcastFailed { context, .. } => context,
            },
            Self::Agent(e) => match e {
                AgentError::InvalidState { context, .. }
                | AgentError::OperationNotAllowed { context, .. }
                | AgentError::DeviceNotFound { context, .. }
                | AgentError::AccountNotFound { context, .. }
                | AgentError::InsufficientPermissions { context, .. }
                | AgentError::BootstrapRequired { context, .. }
                | AgentError::AlreadyInitialized { context, .. } => context,
            },
            Self::Data(e) => match e {
                DataError::SerializationFailed { context, .. }
                | DataError::DeserializationFailed { context, .. }
                | DataError::LedgerOperationFailed { context, .. }
                | DataError::InvalidContext { context, .. }
                | DataError::CorruptionDetected { context, .. }
                | DataError::VersionMismatch { context, .. }
                | DataError::InvalidMerkleProof { context, .. }
                | DataError::Ledger { context, .. } => context,
            },
            Self::Capability(e) => match e {
                CapabilityError::Insufficient { context, .. }
                | CapabilityError::SystemError { context, .. }
                | CapabilityError::GrantFailed { context, .. }
                | CapabilityError::RevocationFailed { context, .. }
                | CapabilityError::VerificationFailed { context, .. }
                | CapabilityError::NoCapabilities { context, .. }
                | CapabilityError::ExpiredCapabilities { context, .. }
                | CapabilityError::InsufficientPermissions { context, .. }
                | CapabilityError::InvalidDelegation { context, .. }
                | CapabilityError::ParentRevoked { context, .. }
                | CapabilityError::TokenNotFound { context, .. }
                | CapabilityError::TokenExpired { context, .. }
                | CapabilityError::TokenRevoked { context, .. }
                | CapabilityError::PermissionDenied { context, .. }
                | CapabilityError::InvalidSignature { context, .. }
                | CapabilityError::ValidationFailed { context, .. }
                | CapabilityError::InvalidChain { context, .. }
                | CapabilityError::AuthorityNotFound { context, .. }
                | CapabilityError::RevocationNotAuthorized { context, .. }
                | CapabilityError::CapabilityExpired { context, .. }
                | CapabilityError::CryptographicError { context, .. }
                | CapabilityError::AuthorizationError { context, .. }
                | CapabilityError::SerializationError { context, .. } => context,
            },
            Self::Session(e) => match e {
                SessionError::InvalidTransition { context, .. }
                | SessionError::TypeMismatch { context, .. }
                | SessionError::ProtocolViolation { context, .. }
                | SessionError::Timeout { context, .. }
                | SessionError::Aborted { context, .. }
                | SessionError::RecoveryFailed { context, .. }
                | SessionError::RehydrationFailed { context, .. } => context,
            },
            Self::System(e) => match e {
                SystemError::TimeError { context, .. }
                | SystemError::ResourceExhausted { context, .. }
                | SystemError::NotImplemented { context, .. }
                | SystemError::ConfigurationError { context, .. }
                | SystemError::PermissionDenied { context, .. } => context,
            },
        }
    }
}

// =============================================================================
// Convenience Constructors
// =============================================================================

impl AuraError {
    // Protocol error constructors
    /// Create a DKD protocol failure error
    pub fn dkd_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::DkdFailed {
            reason: message.into(),
            phase: Some("unknown".to_string()),
            participants: Some(vec![]),
            context: "".to_string(),
        })
    }

    pub fn frost_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::FrostFailed {
            reason: message.into(),
            round: Some(0),
            threshold: Some(0),
            context: "".to_string(),
        })
    }

    pub fn recovery_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::RecoveryFailed {
            reason: message.into(),
            guardian_count: Some(0),
            required_shares: Some(0),
            context: "".to_string(),
        })
    }

    pub fn resharing_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::ResharingFailed {
            reason: message.into(),
            old_threshold: Some(0),
            new_threshold: Some(0),
            context: "".to_string(),
        })
    }

    pub fn coordination_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::CoordinationFailed {
            reason: message.into(),
            service: Some("unknown".to_string()),
            operation: Some("unknown".to_string()),
            context: "".to_string(),
        })
    }

    // Crypto error constructors
    pub fn frost_sign_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::FrostSignFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn frost_operation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::OperationFailed {
            message: format!("FROST operation failed: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn key_derivation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::KeyDerivationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_signature(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidSignature {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_credential(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidCredential {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn encryption_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::EncryptionFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn decryption_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::DecryptionFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn crypto_operation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::OperationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidInput {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_output(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidOutput {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn signing_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::SigningFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn verification_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::VerificationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn insufficient_security_level(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InsufficientSecurityLevel {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::RateLimited {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn hardware_not_available(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::HardwareNotAvailable {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn attestation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::AttestationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn timing_anomaly(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::TimingAnomaly {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn insufficient_entropy(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InsufficientEntropy {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn poor_randomness(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::PoorRandomness {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn not_initialized(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::NotInitialized {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_operation(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidOperation {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::NotFound {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn unsupported_algorithm(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::UnsupportedAlgorithm {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InternalError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Infrastructure error constructors
    pub fn transport_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Transport {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn transport_connection_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::TransportConnectionFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn transport_timeout(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::TransportTimeout {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn storage_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Storage {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn storage_read_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::StorageReadFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn storage_write_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::StorageWriteFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn network_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Network {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn network_unreachable(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::NetworkUnreachable {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn network_partition(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::NetworkPartition {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_ticket(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::InvalidTicket {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn handshake_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::HandshakeFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn delivery_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::DeliveryFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn broadcast_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::BroadcastFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Agent error constructors
    pub fn agent_invalid_state(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InvalidState {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn operation_not_allowed(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::OperationNotAllowed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn device_not_found(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::DeviceNotFound {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn account_not_found(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::AccountNotFound {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn insufficient_permissions(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InsufficientPermissions {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn bootstrap_required(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::BootstrapRequired {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Data error constructors
    pub fn serialization_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::SerializationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn deserialization_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::DeserializationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn ledger_operation_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::LedgerOperationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_context(message: impl Into<String>) -> Self {
        Self::Data(DataError::InvalidContext {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn data_corruption_detected(message: impl Into<String>) -> Self {
        Self::Data(DataError::CorruptionDetected {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn ledger_error(message: impl Into<String>) -> Self {
        Self::Data(DataError::Ledger {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Capability error constructors
    pub fn insufficient_capability(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::Insufficient {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_system_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::SystemError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_grant_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::GrantFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_revocation_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::RevocationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_verification_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::VerificationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn no_capabilities(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::NoCapabilities {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn expired_capabilities(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ExpiredCapabilities {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_insufficient_permissions(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InsufficientPermissions {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_delegation(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidDelegation {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn parent_revoked(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ParentRevoked {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn token_not_found(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenNotFound {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn token_expired(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenExpired {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn token_revoked(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenRevoked {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_permission_denied(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::PermissionDenied {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_invalid_signature(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidSignature {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_validation_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ValidationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn invalid_chain(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidChain {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn authority_not_found(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::AuthorityNotFound {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn revocation_not_authorized(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::RevocationNotAuthorized {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_expired(message: impl Into<String>, timestamp: u64) -> Self {
        Self::Capability(CapabilityError::CapabilityExpired {
            message: message.into(),
            timestamp,
            context: "".to_string(),
        })
    }

    pub fn capability_cryptographic_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::CryptographicError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_authorization_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::AuthorizationError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn capability_serialization_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::SerializationError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Session error constructors
    pub fn invalid_transition(message: impl Into<String>) -> Self {
        Self::Session(SessionError::InvalidTransition {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn session_type_mismatch(message: impl Into<String>) -> Self {
        Self::Session(SessionError::TypeMismatch {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn session_timeout(message: impl Into<String>) -> Self {
        Self::Session(SessionError::Timeout {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn session_aborted(message: impl Into<String>) -> Self {
        Self::Session(SessionError::Aborted {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // System error constructors
    pub fn system_time_error(message: impl Into<String>) -> Self {
        Self::System(SystemError::TimeError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn resource_exhausted(message: impl Into<String>) -> Self {
        Self::System(SystemError::ResourceExhausted {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::System(SystemError::NotImplemented {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn configuration_error(message: impl Into<String>) -> Self {
        Self::System(SystemError::ConfigurationError {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::System(SystemError::PermissionDenied {
            message: message.into(),
            context: "".to_string(),
        })
    }

    // Legacy compatibility constructors for aura-agent
    pub fn serialization_error(message: impl Into<String>) -> Self {
        Self::Data(DataError::SerializationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn deserialization_error(message: impl Into<String>) -> Self {
        Self::Data(DataError::DeserializationFailed {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn data_not_found(message: impl Into<String>) -> Self {
        Self::Data(DataError::LedgerOperationFailed {
            message: format!("Data not found: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn invalid_data(message: impl Into<String>) -> Self {
        Self::Data(DataError::InvalidContext {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn already_initialized(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::AlreadyInitialized {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn session_limit_exceeded(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::OperationNotAllowed {
            message: format!("Session limit exceeded: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn session_required(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InvalidState {
            message: format!("Session required: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn session_expired(message: impl Into<String>) -> Self {
        Self::Session(SessionError::Timeout {
            message: format!("Session expired: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn session_access_denied(message: impl Into<String>) -> Self {
        Self::Session(SessionError::ProtocolViolation {
            message: format!("Session access denied: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn session_not_found(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InvalidState {
            message: format!("Session not found: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn policy_violation(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::PermissionDenied {
            message: format!("Policy violation: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn device_not_registered(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::DeviceNotFound {
            message: format!("Device not registered: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn capability_missing(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::Insufficient {
            message: format!("Capability missing: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn storage_quota_exceeded(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::StorageQuotaExceeded {
            message: message.into(),
            context: "".to_string(),
        })
    }

    pub fn backup_limit_exceeded(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::OperationNotAllowed {
            message: format!("Backup limit exceeded: {}", message.into()),
            context: "".to_string(),
        })
    }

    pub fn backup_rate_limited(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::OperationNotAllowed {
            message: format!("Backup rate limited: {}", message.into()),
            context: "".to_string(),
        })
    }
}

/// Result type alias for Aura operations
pub type Result<T> = std::result::Result<T, AuraError>;

// =============================================================================
// Trait Implementations and Conversions
// =============================================================================

impl From<String> for AuraError {
    fn from(error: String) -> Self {
        Self::ledger_error(error)
    }
}

impl From<&str> for AuraError {
    fn from(error: &str) -> Self {
        Self::ledger_error(error)
    }
}

// Additional convenience implementations for common error types
impl fmt::Display for ErrorSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = AuraError::dkd_failed("Timeout after 30s");
        assert_eq!(error.severity(), ErrorSeverity::High);
        assert_eq!(error.code(), Some(ErrorCode::ProtocolDkdTimeout));
    }

    #[test]
    fn test_error_severity() {
        let critical_error = AuraError::bootstrap_required("Account creation failed");
        assert_eq!(critical_error.severity(), ErrorSeverity::High);

        let high_error = AuraError::dkd_failed("DKD timeout");
        assert_eq!(high_error.severity(), ErrorSeverity::High);

        let medium_error = AuraError::invalid_credential("Bad token");
        assert_eq!(medium_error.severity(), ErrorSeverity::High);
    }

    #[test]
    fn test_retryable_errors() {
        let retryable_error = AuraError::timeout_error("Connection timeout");
        assert!(retryable_error.is_retryable());

        let non_retryable_error = AuraError::bootstrap_required("Invalid parameters");
        assert!(!non_retryable_error.is_retryable());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(
            ErrorCode::CryptoFrostSignTimeout.default_severity(),
            ErrorSeverity::High
        );
        assert_eq!(
            ErrorCode::ProtocolBootstrapFailed.default_severity(),
            ErrorSeverity::Critical
        );
        assert_eq!(
            ErrorCode::SystemNotImplemented.default_severity(),
            ErrorSeverity::Low
        );
    }
}
