//! Common error types and utilities

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;

/// Error severity classification
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
    /// Distributed Key Derivation protocol failed
    DkdProtocolFailed = 1001,
    /// FROST threshold signature protocol failed
    FrostProtocolFailed = 1002,
    /// Resharing protocol failed
    ResharingFailed = 1003,
    /// Recovery protocol failed
    RecoveryFailed = 1004,
    /// Coordination service failed
    CoordinationFailed = 1005,
    /// Session protocol failed
    SessionProtocolFailed = 1006,
    /// Consensus protocol failed
    ConsensusFailed = 1007,
    /// Protocol timeout
    ProtocolTimeout = 1008,
    /// Invalid state transition
    InvalidStateTransition = 1009,
    /// Message validation failed
    MessageValidationFailed = 1010,
    /// Byzantine behavior detected
    ByzantineBehavior = 1011,
    /// Insufficient participants
    InsufficientParticipants = 1012,

    // Crypto Error Codes (2000-2999)
    /// Key generation failed
    KeyGenerationFailed = 2001,
    /// Key derivation failed
    KeyDerivationFailed = 2002,
    /// Signature operation failed
    SignatureFailed = 2003,
    /// Encryption operation failed
    EncryptionFailed = 2004,
    /// Decryption operation failed
    DecryptionFailed = 2005,
    /// Key storage operation failed
    KeyStorageFailed = 2006,
    /// Threshold operation failed
    ThresholdOperationFailed = 2007,
    /// Invalid cryptographic material
    InvalidCryptoMaterial = 2008,

    // Infrastructure Error Codes (3000-3999)
    /// Transport layer error
    TransportError = 3001,
    /// Storage operation failed
    StorageFailed = 3002,
    /// Network connectivity error
    NetworkError = 3003,
    /// Connection establishment failed
    ConnectionFailed = 3004,
    /// Message delivery failed
    MessageDeliveryFailed = 3005,
    /// Resource exhausted
    ResourceExhausted = 3006,
    /// IO operation failed
    IoError = 3007,
    /// Configuration error
    ConfigurationError = 3008,
    /// Service unavailable
    ServiceUnavailable = 3009,

    // Agent Error Codes (4000-4999)
    /// Agent initialization failed
    AgentInitializationFailed = 4001,
    /// Session management error
    SessionManagementError = 4002,
    /// Device management error
    DeviceManagementError = 4003,
    /// Storage adapter error
    StorageAdapterError = 4004,
    /// Transport adapter error
    TransportAdapterError = 4005,
    /// Agent state error
    AgentStateError = 4006,

    // Data Error Codes (5000-5999)
    /// Serialization failed
    SerializationFailed = 5001,
    /// Deserialization failed
    DeserializationFailed = 5002,
    /// State validation failed
    StateValidationFailed = 5003,
    /// Integrity check failed
    IntegrityCheckFailed = 5004,
    /// Migration failed
    MigrationFailed = 5005,
    /// Resource not found
    NotFound = 5006,

    // Capability Error Codes (6000-6999)
    /// Authorization failed
    AuthorizationFailed = 6001,
    /// Authentication failed
    AuthenticationFailed = 6002,
    /// Access denied
    AccessDenied = 6003,
    /// Invalid capability
    InvalidCapability = 6004,
    /// Capability expired
    CapabilityExpired = 6005,
    /// Delegation failed
    DelegationFailed = 6006,
    /// Invalid delegation chain
    InvalidDelegationChain = 6007,
    /// Policy evaluation failed
    PolicyEvaluationFailed = 6008,
    /// Insufficient permissions
    InsufficientPermissions = 6009,
    /// Trust evaluation failed
    TrustEvaluationFailed = 6010,
    /// Invalid subject
    InvalidSubject = 6011,
    /// Invalid resource
    InvalidResource = 6012,
    /// Invalid scope
    InvalidScope = 6013,
    /// Quota exceeded
    QuotaExceeded = 6014,
    /// Rate limit exceeded
    RateLimitExceeded = 6015,
    /// Condition evaluation failed
    ConditionFailed = 6016,
    /// Unrecognized authority
    UnrecognizedAuthority = 6017,

    // Session Error Codes (7000-7999)
    /// Invalid session type
    InvalidSessionType = 7001,
    /// Session state mismatch
    SessionStateMismatch = 7002,
    /// Protocol violation
    ProtocolViolation = 7003,
    /// Choreography error
    ChoreographyError = 7004,
    /// Role assignment error
    RoleAssignmentError = 7005,

    // System Error Codes (8000-8999)
    /// Runtime panic
    RuntimePanic = 8001,
    /// Thread pool exhausted
    ThreadPoolExhausted = 8002,
    /// Memory allocation failed
    AllocationFailed = 8003,
    /// Platform-specific error
    PlatformError = 8004,
    /// External service error
    ExternalServiceError = 8005,
    /// Shutdown in progress
    ShutdownInProgress = 8006,
    /// Feature not implemented
    NotImplemented = 8007,
    /// Internal error
    InternalError = 8008,

    // Generic Error Codes (9000-9999)
    /// Unknown error
    Unknown = 9999,
}

/// Rich error context with debugging information
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

    /// Create error context with specific code
    pub fn with_code(code: ErrorCode) -> Self {
        Self {
            code: Some(code),
            timestamp: Some(OffsetDateTime::now_utc()),
            ..Default::default()
        }
    }

    /// Set the error code
    pub fn set_code(mut self, code: ErrorCode) -> Self {
        self.code = Some(code);
        self
    }

    /// Set the severity
    pub fn set_severity(mut self, severity: ErrorSeverity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// Add context information
    pub fn add_context(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }

    /// Set trace information
    pub fn set_trace(mut self, trace: impl Into<String>) -> Self {
        self.trace = Some(trace.into());
        self
    }

    /// Set remediation suggestion
    pub fn set_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }
}