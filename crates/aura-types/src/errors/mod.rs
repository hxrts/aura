//! Unified error handling for the Aura platform
#![allow(missing_docs)] // Allow missing docs for repetitive error struct fields
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
//!     Err(AuraError::frost_failed("FROST signing failed")
//!         .with_context("participant", "alice")
//!         .with_context("round", "2"))
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
    /// DKD lifecycle protocol error
    ProtocolDkdLifecycle = 1010,
    /// Counter lifecycle protocol error
    ProtocolCounterLifecycle = 1011,
    /// Recovery lifecycle protocol error
    ProtocolRecoveryLifecycle = 1012,
    /// Resharing lifecycle protocol error
    ProtocolResharingLifecycle = 1013,
    /// Locking lifecycle protocol error
    ProtocolLockingLifecycle = 1014,
    /// Group lifecycle protocol error
    ProtocolGroupLifecycle = 1015,
    /// Protocol execution failed
    ProtocolExecutionFailed = 1016,
    /// Invalid protocol instruction
    ProtocolInvalidInstruction = 1017,
    /// BeeKEM operation failed
    ProtocolBeeKemError = 1018,
    /// Invalid group operation
    ProtocolInvalidGroupOperation = 1019,
    /// Missing required parameter
    ProtocolMissingParameter = 1020,

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

/// Protocol execution and coordination errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolError {
    /// Distributed Key Derivation protocol failure
    #[error("DKD protocol failed: {message}")]
    DkdFailed {
        /// Human-readable error message
        message: String,
        /// Additional error context and metadata
        context: ErrorContext,
    },

    /// FROST threshold signature protocol failure
    #[error("FROST protocol failed: {message}")]
    FrostFailed {
        message: String,
        context: ErrorContext,
    },

    /// Session epoch mismatch between participants
    #[error("Session epoch mismatch: {message}")]
    EpochMismatch {
        message: String,
        context: ErrorContext,
    },

    /// Continuous Group Key Agreement protocol failure
    #[error("CGKA protocol failed: {message}")]
    CgkaFailed {
        message: String,
        context: ErrorContext,
    },

    /// Account bootstrap or initialization failure
    #[error("Bootstrap protocol failed: {message}")]
    BootstrapFailed {
        message: String,
        context: ErrorContext,
    },

    /// Recovery protocol failure
    #[error("Recovery protocol failed: {message}")]
    RecoveryFailed {
        message: String,
        context: ErrorContext,
    },

    /// Resharing protocol failure
    #[error("Resharing protocol failed: {message}")]
    ResharingFailed {
        message: String,
        context: ErrorContext,
    },

    /// Protocol session timeout
    #[error("Protocol session timeout: {message}")]
    SessionTimeout {
        message: String,
        context: ErrorContext,
    },

    /// Protocol coordination failure
    #[error("Protocol coordination failed: {message}")]
    CoordinationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Generic protocol orchestration error
    #[error("Protocol orchestration error: {message}")]
    Orchestrator {
        message: String,
        context: ErrorContext,
    },

    /// DKD lifecycle protocol error
    #[error("DKD lifecycle error: {message}")]
    DkdLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Counter lifecycle protocol error
    #[error("Counter lifecycle error: {message}")]
    CounterLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Recovery lifecycle protocol error
    #[error("Recovery lifecycle error: {message}")]
    RecoveryLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Resharing lifecycle protocol error
    #[error("Resharing lifecycle error: {message}")]
    ResharingLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Locking lifecycle protocol error
    #[error("Locking lifecycle error: {message}")]
    LockingLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Group lifecycle protocol error
    #[error("Group lifecycle error: {message}")]
    GroupLifecycle {
        message: String,
        context: ErrorContext,
    },

    /// Protocol execution error
    #[error("Protocol execution error: {message}")]
    ExecutionFailed {
        message: String,
        context: ErrorContext,
    },

    /// Invalid protocol instruction
    #[error("Invalid protocol instruction: {message}")]
    InvalidInstruction {
        message: String,
        context: ErrorContext,
    },

    /// BeeKEM operation failed
    #[error("BeeKEM operation failed: {message}")]
    BeeKemError {
        message: String,
        context: ErrorContext,
    },

    /// Invalid group operation
    #[error("Invalid group operation: {message}")]
    InvalidGroupOperation {
        message: String,
        context: ErrorContext,
    },

    /// Missing required protocol parameter
    #[error("Missing required parameter: {message}")]
    MissingParameter {
        message: String,
        context: ErrorContext,
    },
}

/// Cryptographic operation errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum CryptoError {
    /// FROST threshold signature operation failed
    #[error("FROST signing failed: {message}")]
    FrostSignFailed {
        message: String,
        context: ErrorContext,
    },

    /// Key derivation operation failed
    #[error("Key derivation failed: {message}")]
    KeyDerivationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Invalid signature verification
    #[error("Invalid signature: {message}")]
    InvalidSignature {
        message: String,
        context: ErrorContext,
    },

    /// Invalid credential or authentication token
    #[error("Invalid credential: {message}")]
    InvalidCredential {
        message: String,
        context: ErrorContext,
    },

    /// Encryption operation failed
    #[error("Encryption failed: {message}")]
    EncryptionFailed {
        message: String,
        context: ErrorContext,
    },

    /// Decryption operation failed
    #[error("Decryption failed: {message}")]
    DecryptionFailed {
        message: String,
        context: ErrorContext,
    },

    /// Hash computation failed
    #[error("Hashing failed: {message}")]
    HashingFailed {
        message: String,
        context: ErrorContext,
    },

    /// Random number generation failed
    #[error("Random generation failed: {message}")]
    RandomGenerationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Generic cryptographic operation failure
    #[error("Cryptographic operation failed: {message}")]
    OperationFailed {
        message: String,
        context: ErrorContext,
    },
}

/// Infrastructure and external system errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum InfrastructureError {
    /// Network transport layer error
    #[error("Transport error: {message}")]
    Transport {
        message: String,
        context: ErrorContext,
    },

    /// Storage layer operation failure
    #[error("Storage error: {message}")]
    Storage {
        message: String,
        context: ErrorContext,
    },

    /// Network communication error
    #[error("Network error: {message}")]
    Network {
        message: String,
        context: ErrorContext,
    },

    /// Transport connection failure
    #[error("Transport connection failed: {message}")]
    TransportConnectionFailed {
        message: String,
        context: ErrorContext,
    },

    /// Transport operation timeout
    #[error("Transport timeout: {message}")]
    TransportTimeout {
        message: String,
        context: ErrorContext,
    },

    /// Storage read operation failed
    #[error("Storage read failed: {message}")]
    StorageReadFailed {
        message: String,
        context: ErrorContext,
    },

    /// Storage write operation failed
    #[error("Storage write failed: {message}")]
    StorageWriteFailed {
        message: String,
        context: ErrorContext,
    },

    /// Network unreachable
    #[error("Network unreachable: {message}")]
    NetworkUnreachable {
        message: String,
        context: ErrorContext,
    },

    /// Network partition detected
    #[error("Network partition: {message}")]
    NetworkPartition {
        message: String,
        context: ErrorContext,
    },

    /// Storage quota exceeded
    #[error("Storage quota exceeded: {message}")]
    StorageQuotaExceeded {
        message: String,
        context: ErrorContext,
    },

    /// Transport authentication failed
    #[error("Transport authentication failed: {message}")]
    TransportAuthenticationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Invalid presence ticket
    #[error("Invalid presence ticket: {message}")]
    InvalidTicket {
        message: String,
        context: ErrorContext,
    },

    /// Connection handshake failed
    #[error("Connection handshake failed: {message}")]
    HandshakeFailed {
        message: String,
        context: ErrorContext,
    },

    /// Message delivery failed
    #[error("Message delivery failed: {message}")]
    DeliveryFailed {
        message: String,
        context: ErrorContext,
    },

    /// Broadcast operation failed
    #[error("Broadcast operation failed: {message}")]
    BroadcastFailed {
        message: String,
        context: ErrorContext,
    },
}

/// Agent operation and state management errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum AgentError {
    /// Agent in invalid state for operation
    #[error("Agent invalid state: {message}")]
    InvalidState {
        message: String,
        context: ErrorContext,
    },

    /// Operation not allowed in current state
    #[error("Operation not allowed: {message}")]
    OperationNotAllowed {
        message: String,
        context: ErrorContext,
    },

    /// Requested device not found in account
    #[error("Device not found: {message}")]
    DeviceNotFound {
        message: String,
        context: ErrorContext,
    },

    /// Account not found or not accessible
    #[error("Account not found: {message}")]
    AccountNotFound {
        message: String,
        context: ErrorContext,
    },

    /// Insufficient permissions for operation
    #[error("Insufficient permissions: {message}")]
    InsufficientPermissions {
        message: String,
        context: ErrorContext,
    },

    /// Bootstrap required before operation
    #[error("Bootstrap required: {message}")]
    BootstrapRequired {
        message: String,
        context: ErrorContext,
    },

    /// Agent already initialized
    #[error("Already initialized: {message}")]
    AlreadyInitialized {
        message: String,
        context: ErrorContext,
    },
}

/// Data handling and state management errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum DataError {
    /// Data serialization failure
    #[error("Serialization failed: {message}")]
    SerializationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Data deserialization failure
    #[error("Deserialization failed: {message}")]
    DeserializationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Ledger operation failure
    #[error("Ledger operation failed: {message}")]
    LedgerOperationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Invalid context provided for operation
    #[error("Invalid context: {message}")]
    InvalidContext {
        message: String,
        context: ErrorContext,
    },

    /// Data corruption detected
    #[error("Data corruption detected: {message}")]
    CorruptionDetected {
        message: String,
        context: ErrorContext,
    },

    /// Data version mismatch
    #[error("Version mismatch: {message}")]
    VersionMismatch {
        message: String,
        context: ErrorContext,
    },

    /// Invalid Merkle proof
    #[error("Invalid Merkle proof: {message}")]
    InvalidMerkleProof {
        message: String,
        context: ErrorContext,
    },

    /// Generic ledger error
    #[error("Ledger error: {message}")]
    Ledger {
        message: String,
        context: ErrorContext,
    },
}

/// Capability system and authorization errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityError {
    /// Operation requires capability not possessed by agent
    #[error("Insufficient capability: {message}")]
    Insufficient {
        message: String,
        context: ErrorContext,
    },

    /// General capability system error
    #[error("Capability system error: {message}")]
    SystemError {
        message: String,
        context: ErrorContext,
    },

    /// Capability grant failed
    #[error("Capability grant failed: {message}")]
    GrantFailed {
        message: String,
        context: ErrorContext,
    },

    /// Capability revocation failed
    #[error("Capability revocation failed: {message}")]
    RevocationFailed {
        message: String,
        context: ErrorContext,
    },

    /// Capability verification failed
    #[error("Capability verification failed: {message}")]
    VerificationFailed {
        message: String,
        context: ErrorContext,
    },

    // Variants from store/capability_manager.rs
    /// No capabilities found for device
    #[error("No capabilities found: {message}")]
    NoCapabilities {
        message: String,
        context: ErrorContext,
    },

    /// All capabilities have expired
    #[error("Capabilities expired: {message}")]
    ExpiredCapabilities {
        message: String,
        context: ErrorContext,
    },

    /// Insufficient permissions for operation
    #[error("Insufficient permissions: {message}")]
    InsufficientPermissions {
        message: String,
        context: ErrorContext,
    },

    /// Invalid delegation chain
    #[error("Invalid delegation: {message}")]
    InvalidDelegation {
        message: String,
        context: ErrorContext,
    },

    /// Parent capability has been revoked
    #[error("Parent capability revoked: {message}")]
    ParentRevoked {
        message: String,
        context: ErrorContext,
    },

    // Variants from store/access_control/capability.rs
    /// Capability token not found
    #[error("Token not found: {message}")]
    TokenNotFound {
        message: String,
        context: ErrorContext,
    },

    /// Capability token is expired
    #[error("Token expired: {message}")]
    TokenExpired {
        message: String,
        context: ErrorContext,
    },

    /// Capability token is revoked
    #[error("Token revoked: {message}")]
    TokenRevoked {
        message: String,
        context: ErrorContext,
    },

    /// Permission denied for operation
    #[error("Permission denied: {message}")]
    PermissionDenied {
        message: String,
        context: ErrorContext,
    },

    /// Invalid signature on capability
    #[error("Invalid signature: {message}")]
    InvalidSignature {
        message: String,
        context: ErrorContext,
    },

    /// Capability validation failed
    #[error("Validation failed: {message}")]
    ValidationFailed {
        message: String,
        context: ErrorContext,
    },

    // Variants from journal/capability/mod.rs
    /// Invalid capability chain
    #[error("Invalid capability chain: {message}")]
    InvalidChain {
        message: String,
        context: ErrorContext,
    },

    /// Authority not found
    #[error("Authority not found: {message}")]
    AuthorityNotFound {
        message: String,
        context: ErrorContext,
    },

    /// Revocation not authorized
    #[error("Revocation not authorized: {message}")]
    RevocationNotAuthorized {
        message: String,
        context: ErrorContext,
    },

    /// Capability expired at specific timestamp
    #[error("Capability expired at {timestamp}: {message}")]
    CapabilityExpired {
        message: String,
        timestamp: u64,
        context: ErrorContext,
    },

    /// Cryptographic operation failed
    #[error("Cryptographic error: {message}")]
    CryptographicError {
        message: String,
        context: ErrorContext,
    },

    /// Authorization failed
    #[error("Authorization error: {message}")]
    AuthorizationError {
        message: String,
        context: ErrorContext,
    },

    /// Serialization failed
    #[error("Serialization error: {message}")]
    SerializationError {
        message: String,
        context: ErrorContext,
    },
}

/// Session type and state machine errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum SessionError {
    /// Invalid state transition attempted
    #[error("Invalid state transition: {message}")]
    InvalidTransition {
        message: String,
        context: ErrorContext,
    },

    /// Session type mismatch
    #[error("Session type mismatch: {message}")]
    TypeMismatch {
        message: String,
        context: ErrorContext,
    },

    /// Protocol violation in session
    #[error("Protocol violation: {message}")]
    ProtocolViolation {
        message: String,
        context: ErrorContext,
    },

    /// Session timeout
    #[error("Session timeout: {message}")]
    Timeout {
        message: String,
        context: ErrorContext,
    },

    /// Session aborted
    #[error("Session aborted: {message}")]
    Aborted {
        message: String,
        context: ErrorContext,
    },

    /// Session recovery failed
    #[error("Session recovery failed: {message}")]
    RecoveryFailed {
        message: String,
        context: ErrorContext,
    },

    /// Session rehydration failed
    #[error("Session rehydration failed: {message}")]
    RehydrationFailed {
        message: String,
        context: ErrorContext,
    },
}

/// System and runtime errors
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum SystemError {
    /// System time access or manipulation error
    #[error("System time error: {message}")]
    TimeError {
        message: String,
        context: ErrorContext,
    },

    /// System resource exhausted
    #[error("Resource exhausted: {message}")]
    ResourceExhausted {
        message: String,
        context: ErrorContext,
    },

    /// Feature not yet implemented
    #[error("Not implemented: {message}")]
    NotImplemented {
        message: String,
        context: ErrorContext,
    },

    /// System configuration error
    #[error("Configuration error: {message}")]
    ConfigurationError {
        message: String,
        context: ErrorContext,
    },

    /// System permission denied
    #[error("Permission denied: {message}")]
    PermissionDenied {
        message: String,
        context: ErrorContext,
    },
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
        let context = self.context();
        context.severity.unwrap_or_else(|| {
            context
                .code
                .map(|c| c.default_severity())
                .unwrap_or(ErrorSeverity::Medium)
        })
    }

    /// Get the error code if present
    pub fn code(&self) -> Option<ErrorCode> {
        self.context().code
    }

    /// Get the error context
    pub fn context(&self) -> &ErrorContext {
        match self {
            Self::Protocol(e) => match e {
                ProtocolError::DkdFailed { context, .. }
                | ProtocolError::FrostFailed { context, .. }
                | ProtocolError::EpochMismatch { context, .. }
                | ProtocolError::CgkaFailed { context, .. }
                | ProtocolError::BootstrapFailed { context, .. }
                | ProtocolError::RecoveryFailed { context, .. }
                | ProtocolError::ResharingFailed { context, .. }
                | ProtocolError::SessionTimeout { context, .. }
                | ProtocolError::CoordinationFailed { context, .. }
                | ProtocolError::Orchestrator { context, .. }
                | ProtocolError::DkdLifecycle { context, .. }
                | ProtocolError::CounterLifecycle { context, .. }
                | ProtocolError::RecoveryLifecycle { context, .. }
                | ProtocolError::ResharingLifecycle { context, .. }
                | ProtocolError::LockingLifecycle { context, .. }
                | ProtocolError::GroupLifecycle { context, .. }
                | ProtocolError::ExecutionFailed { context, .. }
                | ProtocolError::InvalidInstruction { context, .. }
                | ProtocolError::BeeKemError { context, .. }
                | ProtocolError::InvalidGroupOperation { context, .. }
                | ProtocolError::MissingParameter { context, .. } => context,
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
                | CryptoError::OperationFailed { context, .. } => context,
            },
            Self::Infrastructure(e) => match e {
                InfrastructureError::Transport { context, .. }
                | InfrastructureError::Storage { context, .. }
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
            Self::Protocol(ProtocolError::SessionTimeout { .. })
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

    /// Add additional context to the error
    pub fn with_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        let context = self.context_mut();
        context.context.insert(key.into(), value.into());
        self
    }

    /// Add error code to the error
    pub fn with_code(mut self, code: ErrorCode) -> Self {
        let context = self.context_mut();
        context.code = Some(code);
        context.severity = Some(code.default_severity());
        self
    }

    /// Add remediation information
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        let context = self.context_mut();
        context.remediation = Some(remediation.into());
        self
    }

    // Private helper to get mutable context
    fn context_mut(&mut self) -> &mut ErrorContext {
        match self {
            Self::Protocol(e) => match e {
                ProtocolError::DkdFailed { context, .. }
                | ProtocolError::FrostFailed { context, .. }
                | ProtocolError::EpochMismatch { context, .. }
                | ProtocolError::CgkaFailed { context, .. }
                | ProtocolError::BootstrapFailed { context, .. }
                | ProtocolError::RecoveryFailed { context, .. }
                | ProtocolError::ResharingFailed { context, .. }
                | ProtocolError::SessionTimeout { context, .. }
                | ProtocolError::CoordinationFailed { context, .. }
                | ProtocolError::Orchestrator { context, .. }
                | ProtocolError::DkdLifecycle { context, .. }
                | ProtocolError::CounterLifecycle { context, .. }
                | ProtocolError::RecoveryLifecycle { context, .. }
                | ProtocolError::ResharingLifecycle { context, .. }
                | ProtocolError::LockingLifecycle { context, .. }
                | ProtocolError::GroupLifecycle { context, .. }
                | ProtocolError::ExecutionFailed { context, .. }
                | ProtocolError::InvalidInstruction { context, .. }
                | ProtocolError::BeeKemError { context, .. }
                | ProtocolError::InvalidGroupOperation { context, .. }
                | ProtocolError::MissingParameter { context, .. } => context,
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
                | CryptoError::OperationFailed { context, .. } => context,
            },
            Self::Infrastructure(e) => match e {
                InfrastructureError::Transport { context, .. }
                | InfrastructureError::Storage { context, .. }
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
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolDkdTimeout),
        })
    }

    pub fn frost_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::FrostFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolFrostSignFailed),
        })
    }

    pub fn epoch_mismatch(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::EpochMismatch {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolEpochMismatch),
        })
    }

    pub fn cgka_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::CgkaFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolCgkaFailed),
        })
    }

    pub fn bootstrap_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::BootstrapFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolBootstrapFailed),
        })
    }

    pub fn recovery_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::RecoveryFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolRecoveryFailed),
        })
    }

    pub fn resharing_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::ResharingFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolResharingFailed),
        })
    }

    pub fn protocol_timeout(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::SessionTimeout {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolSessionTimeout),
        })
    }

    pub fn coordination_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::CoordinationFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolCoordinationFailed),
        })
    }

    pub fn protocol_orchestrator(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::Orchestrator {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn dkd_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::DkdLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolDkdLifecycle),
        })
    }

    pub fn counter_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::CounterLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolCounterLifecycle),
        })
    }

    pub fn recovery_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::RecoveryLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolRecoveryLifecycle),
        })
    }

    pub fn resharing_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::ResharingLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolResharingLifecycle),
        })
    }

    pub fn locking_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::LockingLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolLockingLifecycle),
        })
    }

    pub fn group_lifecycle_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::GroupLifecycle {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolGroupLifecycle),
        })
    }

    pub fn protocol_execution_failed(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::ExecutionFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolExecutionFailed),
        })
    }

    pub fn protocol_invalid_instruction(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::InvalidInstruction {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolInvalidInstruction),
        })
    }

    pub fn beekm_error(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::BeeKemError {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolBeeKemError),
        })
    }

    pub fn invalid_group_operation(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::InvalidGroupOperation {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolInvalidGroupOperation),
        })
    }

    pub fn missing_parameter(message: impl Into<String>) -> Self {
        Self::Protocol(ProtocolError::MissingParameter {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::ProtocolMissingParameter),
        })
    }

    // Crypto error constructors
    pub fn frost_sign_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::FrostSignFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoFrostSignTimeout),
        })
    }

    pub fn frost_operation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::OperationFailed {
            message: format!("FROST operation failed: {}", message.into()),
            context: ErrorContext::new().with_code(ErrorCode::CryptoEncryptionFailed),
        })
    }

    pub fn key_derivation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::KeyDerivationFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoKeyDerivationFailed),
        })
    }

    pub fn invalid_signature(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidSignature {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoInvalidSignature),
        })
    }

    pub fn invalid_credential(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::InvalidCredential {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoInvalidCredential),
        })
    }

    pub fn encryption_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::EncryptionFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoEncryptionFailed),
        })
    }

    pub fn decryption_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::DecryptionFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::CryptoDecryptionFailed),
        })
    }

    pub fn crypto_operation_failed(message: impl Into<String>) -> Self {
        Self::Crypto(CryptoError::OperationFailed {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    // Infrastructure error constructors
    pub fn transport_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Transport {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn transport_connection_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::TransportConnectionFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraTransportConnectionFailed),
        })
    }

    pub fn transport_timeout(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::TransportTimeout {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraTransportTimeout),
        })
    }

    pub fn storage_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Storage {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn storage_read_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::StorageReadFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraStorageReadFailed),
        })
    }

    pub fn storage_write_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::StorageWriteFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraStorageWriteFailed),
        })
    }

    pub fn network_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::Network {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn network_unreachable(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::NetworkUnreachable {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraNetworkUnreachable),
        })
    }

    pub fn network_partition(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::NetworkPartition {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraNetworkPartition),
        })
    }

    pub fn invalid_ticket(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::InvalidTicket {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraInvalidTicket),
        })
    }

    pub fn handshake_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::HandshakeFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraHandshakeFailed),
        })
    }

    pub fn delivery_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::DeliveryFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraDeliveryFailed),
        })
    }

    pub fn broadcast_failed(message: impl Into<String>) -> Self {
        Self::Infrastructure(InfrastructureError::BroadcastFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::InfraBroadcastFailed),
        })
    }

    // Agent error constructors
    pub fn agent_invalid_state(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InvalidState {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentInvalidState),
        })
    }

    pub fn operation_not_allowed(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::OperationNotAllowed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentOperationNotAllowed),
        })
    }

    pub fn device_not_found(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::DeviceNotFound {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentDeviceNotFound),
        })
    }

    pub fn account_not_found(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::AccountNotFound {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentAccountNotFound),
        })
    }

    pub fn insufficient_permissions(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::InsufficientPermissions {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentInsufficientPermissions),
        })
    }

    pub fn bootstrap_required(message: impl Into<String>) -> Self {
        Self::Agent(AgentError::BootstrapRequired {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::AgentBootstrapRequired),
        })
    }

    // Data error constructors
    pub fn serialization_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::SerializationFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::DataSerializationFailed),
        })
    }

    pub fn deserialization_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::DeserializationFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::DataDeserializationFailed),
        })
    }

    pub fn ledger_operation_failed(message: impl Into<String>) -> Self {
        Self::Data(DataError::LedgerOperationFailed {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::DataLedgerOperationFailed),
        })
    }

    pub fn invalid_context(message: impl Into<String>) -> Self {
        Self::Data(DataError::InvalidContext {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::DataInvalidContext),
        })
    }

    pub fn data_corruption_detected(message: impl Into<String>) -> Self {
        Self::Data(DataError::CorruptionDetected {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::DataCorruptionDetected),
        })
    }

    pub fn ledger_error(message: impl Into<String>) -> Self {
        Self::Data(DataError::Ledger {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    // Capability error constructors
    pub fn insufficient_capability(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::Insufficient {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_system_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::SystemError {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_grant_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::GrantFailed {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_revocation_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::RevocationFailed {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_verification_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::VerificationFailed {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn no_capabilities(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::NoCapabilities {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn expired_capabilities(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ExpiredCapabilities {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_insufficient_permissions(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InsufficientPermissions {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn invalid_delegation(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidDelegation {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn parent_revoked(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ParentRevoked {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn token_not_found(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenNotFound {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn token_expired(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenExpired {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn token_revoked(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::TokenRevoked {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_permission_denied(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::PermissionDenied {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_invalid_signature(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidSignature {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_validation_failed(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::ValidationFailed {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn invalid_chain(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::InvalidChain {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn authority_not_found(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::AuthorityNotFound {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn revocation_not_authorized(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::RevocationNotAuthorized {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_expired(message: impl Into<String>, timestamp: u64) -> Self {
        Self::Capability(CapabilityError::CapabilityExpired {
            message: message.into(),
            timestamp,
            context: ErrorContext::new(),
        })
    }

    pub fn capability_cryptographic_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::CryptographicError {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_authorization_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::AuthorizationError {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    pub fn capability_serialization_error(message: impl Into<String>) -> Self {
        Self::Capability(CapabilityError::SerializationError {
            message: message.into(),
            context: ErrorContext::new(),
        })
    }

    // Session error constructors
    pub fn invalid_transition(message: impl Into<String>) -> Self {
        Self::Session(SessionError::InvalidTransition {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionInvalidTransition),
        })
    }

    pub fn session_type_mismatch(message: impl Into<String>) -> Self {
        Self::Session(SessionError::TypeMismatch {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionTypeMismatch),
        })
    }

    pub fn session_timeout(message: impl Into<String>) -> Self {
        Self::Session(SessionError::Timeout {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionTimeout),
        })
    }

    pub fn session_aborted(message: impl Into<String>) -> Self {
        Self::Session(SessionError::Aborted {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SessionAborted),
        })
    }

    // System error constructors
    pub fn system_time_error(message: impl Into<String>) -> Self {
        Self::System(SystemError::TimeError {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SystemTimeError),
        })
    }

    pub fn resource_exhausted(message: impl Into<String>) -> Self {
        Self::System(SystemError::ResourceExhausted {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SystemResourceExhausted),
        })
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::System(SystemError::NotImplemented {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SystemNotImplemented),
        })
    }

    pub fn configuration_error(message: impl Into<String>) -> Self {
        Self::System(SystemError::ConfigurationError {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SystemConfigurationError),
        })
    }

    pub fn permission_denied(message: impl Into<String>) -> Self {
        Self::System(SystemError::PermissionDenied {
            message: message.into(),
            context: ErrorContext::new().with_code(ErrorCode::SystemPermissionDenied),
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
    fn test_error_context() {
        let error = AuraError::frost_sign_failed("Round 2 timeout")
            .with_context("participant", "alice")
            .with_context("round", "2")
            .with_remediation("Check network connectivity");

        let context = error.context();
        assert_eq!(
            context.context.get("participant"),
            Some(&"alice".to_string())
        );
        assert_eq!(context.context.get("round"), Some(&"2".to_string()));
        assert_eq!(
            context.remediation.as_ref(),
            Some(&"Check network connectivity".to_string())
        );
    }

    #[test]
    fn test_error_severity() {
        let critical_error = AuraError::bootstrap_failed("Account creation failed");
        assert_eq!(critical_error.severity(), ErrorSeverity::Critical);

        let high_error = AuraError::dkd_failed("DKD timeout");
        assert_eq!(high_error.severity(), ErrorSeverity::High);

        let medium_error = AuraError::invalid_credential("Bad token");
        assert_eq!(medium_error.severity(), ErrorSeverity::Medium);
    }

    #[test]
    fn test_retryable_errors() {
        let retryable_error = AuraError::transport_timeout("Connection timeout");
        assert!(retryable_error.is_retryable());

        let non_retryable_error = AuraError::bootstrap_failed("Invalid parameters");
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
