//! Comprehensive Error Handling for SSB + Storage Integration
//!
//! Implements production-grade error handling with:
//! - Clear error taxonomy (Authentication, Authorization, Network, Corruption, Resource)
//! - Actionable error context
//! - Recovery strategies per error type
//! - Structured logging integration
//!
//! Reference: work/ssb_storage.md Phase 5.1

use thiserror::Error;

/// Error taxonomy for production error handling
#[derive(Error, Debug)]
pub enum IntegrationError {
    // Authentication Errors
    /// Device authentication failed
    #[error("Authentication failed: {reason}")]
    AuthenticationFailed {
        reason: String,
        device_id: Option<Vec<u8>>,
        recovery: RecoveryStrategy,
    },

    /// Invalid device certificate
    #[error("Invalid device certificate: {reason}")]
    InvalidCertificate {
        reason: String,
        device_id: Vec<u8>,
        recovery: RecoveryStrategy,
    },

    /// Signature verification failed
    #[error("Signature verification failed: {context}")]
    SignatureVerificationFailed {
        context: String,
        recovery: RecoveryStrategy,
    },

    // Authorization Errors
    /// Insufficient permissions for operation
    #[error("Insufficient permissions: required {required}, have {actual}")]
    InsufficientPermissions {
        required: String,
        actual: String,
        operation: String,
        recovery: RecoveryStrategy,
    },

    /// Capability token expired
    #[error("Capability expired at {expired_at}, current time {current_time}")]
    CapabilityExpired {
        expired_at: u64,
        current_time: u64,
        recovery: RecoveryStrategy,
    },

    /// Capability revoked
    #[error("Capability revoked: {capability_id}")]
    CapabilityRevoked {
        capability_id: String,
        revoked_at: u64,
        recovery: RecoveryStrategy,
    },

    // Network Errors
    /// Network connection failed
    #[error("Network connection failed: {reason}")]
    NetworkConnectionFailed {
        reason: String,
        peer_id: Option<Vec<u8>>,
        retry_count: u32,
        recovery: RecoveryStrategy,
    },

    /// Network timeout
    #[error("Network timeout after {timeout_ms}ms: {operation}")]
    NetworkTimeout {
        operation: String,
        timeout_ms: u64,
        retry_count: u32,
        recovery: RecoveryStrategy,
    },

    /// Peer unavailable
    #[error("Peer unavailable: {peer_id}")]
    PeerUnavailable {
        peer_id: String,
        last_seen: Option<u64>,
        recovery: RecoveryStrategy,
    },

    // Corruption Errors
    /// Data corruption detected
    #[error("Data corruption detected: {details}")]
    DataCorruption {
        details: String,
        chunk_id: Option<String>,
        manifest_cid: Option<String>,
        recovery: RecoveryStrategy,
    },

    /// Invalid envelope structure
    #[error("Invalid envelope: {reason}")]
    InvalidEnvelope {
        reason: String,
        envelope_cid: Option<String>,
        recovery: RecoveryStrategy,
    },

    /// Merkle proof verification failed
    #[error("Merkle proof verification failed: {reason}")]
    MerkleVerificationFailed {
        reason: String,
        recovery: RecoveryStrategy,
    },

    // Resource Errors
    /// Storage quota exceeded
    #[error("Storage quota exceeded: used {used}, limit {limit}")]
    QuotaExceeded {
        used: u64,
        limit: u64,
        device_id: Vec<u8>,
        recovery: RecoveryStrategy,
    },

    /// Insufficient storage capacity
    #[error("Insufficient storage capacity: need {needed}, available {available}")]
    InsufficientCapacity {
        needed: u64,
        available: u64,
        recovery: RecoveryStrategy,
    },

    /// Resource not found
    #[error("Resource not found: {resource_type} {resource_id}")]
    ResourceNotFound {
        resource_type: String,
        resource_id: String,
        recovery: RecoveryStrategy,
    },

    // Protocol Errors
    /// Invalid protocol state
    #[error("Invalid protocol state: expected {expected}, got {actual}")]
    InvalidProtocolState {
        expected: String,
        actual: String,
        protocol: String,
        recovery: RecoveryStrategy,
    },

    /// Key version mismatch
    #[error("Key version mismatch: expected {expected}, got {actual}")]
    KeyVersionMismatch {
        expected: u32,
        actual: u32,
        key_type: String,
        recovery: RecoveryStrategy,
    },

    /// Replay attack detected
    #[error("Replay attack detected: counter {counter}, last seen {last_seen}")]
    ReplayAttackDetected {
        counter: u64,
        last_seen: u64,
        recovery: RecoveryStrategy,
    },

    // Configuration Errors
    /// Invalid configuration
    #[error("Invalid configuration: {field} - {reason}")]
    InvalidConfiguration {
        field: String,
        reason: String,
        recovery: RecoveryStrategy,
    },

    // Cryptographic Errors
    /// Encryption operation failed
    #[error("Encryption failed: {reason}")]
    EncryptionFailed {
        reason: String,
        recovery: RecoveryStrategy,
    },

    /// Decryption operation failed
    #[error("Decryption failed: {reason}")]
    DecryptionFailed {
        reason: String,
        recovery: RecoveryStrategy,
    },

    /// Key derivation failed
    #[error("Key derivation failed: {reason}")]
    KeyDerivationFailed {
        reason: String,
        context: String,
        recovery: RecoveryStrategy,
    },
}

/// Recovery strategy for each error type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryStrategy {
    /// Retry the operation with exponential backoff
    RetryWithBackoff {
        max_retries: u32,
        initial_delay_ms: u64,
    },

    /// Retry immediately (for transient failures)
    RetryImmediate { max_retries: u32 },

    /// Request new capability token
    RequestNewCapability { capability_type: String },

    /// Refresh authentication credentials
    RefreshAuthentication,

    /// Fall back to alternative peer
    UseFallbackPeer,

    /// Request data re-replication
    RequestReReplication { chunk_ids: Vec<String> },

    /// Rotate compromised keys
    RotateKeys { key_types: Vec<String> },

    /// Manual intervention required
    ManualIntervention { reason: String },

    /// No recovery possible
    Unrecoverable { reason: String },
}

impl IntegrationError {
    /// Get the recovery strategy for this error
    pub fn recovery_strategy(&self) -> &RecoveryStrategy {
        match self {
            Self::AuthenticationFailed { recovery, .. }
            | Self::InvalidCertificate { recovery, .. }
            | Self::SignatureVerificationFailed { recovery, .. }
            | Self::InsufficientPermissions { recovery, .. }
            | Self::CapabilityExpired { recovery, .. }
            | Self::CapabilityRevoked { recovery, .. }
            | Self::NetworkConnectionFailed { recovery, .. }
            | Self::NetworkTimeout { recovery, .. }
            | Self::PeerUnavailable { recovery, .. }
            | Self::DataCorruption { recovery, .. }
            | Self::InvalidEnvelope { recovery, .. }
            | Self::MerkleVerificationFailed { recovery, .. }
            | Self::QuotaExceeded { recovery, .. }
            | Self::InsufficientCapacity { recovery, .. }
            | Self::ResourceNotFound { recovery, .. }
            | Self::InvalidProtocolState { recovery, .. }
            | Self::KeyVersionMismatch { recovery, .. }
            | Self::ReplayAttackDetected { recovery, .. }
            | Self::InvalidConfiguration { recovery, .. }
            | Self::EncryptionFailed { recovery, .. }
            | Self::DecryptionFailed { recovery, .. }
            | Self::KeyDerivationFailed { recovery, .. } => recovery,
        }
    }

    /// Get request ID for distributed tracing
    pub fn request_id(&self) -> Option<String> {
        // In production, this would extract request ID from context
        None
    }

    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self.recovery_strategy(),
            RecoveryStrategy::RetryWithBackoff { .. } | RecoveryStrategy::RetryImmediate { .. }
        )
    }

    /// Check if error indicates security issue
    pub fn is_security_issue(&self) -> bool {
        matches!(
            self,
            Self::AuthenticationFailed { .. }
                | Self::SignatureVerificationFailed { .. }
                | Self::ReplayAttackDetected { .. }
                | Self::InvalidCertificate { .. }
                | Self::MerkleVerificationFailed { .. }
        )
    }

    /// Get error severity level
    pub fn severity(&self) -> ErrorSeverity {
        match self {
            Self::NetworkTimeout { .. }
            | Self::PeerUnavailable { .. }
            | Self::ResourceNotFound { .. } => ErrorSeverity::Warning,

            Self::NetworkConnectionFailed { .. }
            | Self::CapabilityExpired { .. }
            | Self::InsufficientCapacity { .. }
            | Self::InvalidProtocolState { .. } => ErrorSeverity::Error,

            Self::AuthenticationFailed { .. }
            | Self::DataCorruption { .. }
            | Self::ReplayAttackDetected { .. }
            | Self::SignatureVerificationFailed { .. }
            | Self::InvalidCertificate { .. }
            | Self::MerkleVerificationFailed { .. } => ErrorSeverity::Critical,

            _ => ErrorSeverity::Error,
        }
    }
}

/// Error severity levels for logging and alerting
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ErrorSeverity {
    /// Informational - no action needed
    Info,
    /// Warning - may need attention
    Warning,
    /// Error - requires action
    Error,
    /// Critical - immediate action required
    Critical,
}

/// Error context builder for rich error information
pub struct ErrorContext {
    operation: String,
    device_id: Option<Vec<u8>>,
    peer_id: Option<Vec<u8>>,
    resource_id: Option<String>,
    timestamp: u64,
    metadata: Vec<(String, String)>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(operation: impl Into<String>, timestamp: u64) -> Self {
        Self {
            operation: operation.into(),
            device_id: None,
            peer_id: None,
            resource_id: None,
            timestamp,
            metadata: Vec::new(),
        }
    }

    /// Add device ID to context
    pub fn with_device_id(mut self, device_id: Vec<u8>) -> Self {
        self.device_id = Some(device_id);
        self
    }

    /// Add peer ID to context
    pub fn with_peer_id(mut self, peer_id: Vec<u8>) -> Self {
        self.peer_id = Some(peer_id);
        self
    }

    /// Add resource ID to context
    pub fn with_resource_id(mut self, resource_id: impl Into<String>) -> Self {
        self.resource_id = Some(resource_id.into());
        self
    }

    /// Add custom metadata
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    /// Get operation name
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Get timestamp
    pub fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Get metadata
    pub fn metadata(&self) -> &[(String, String)] {
        &self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authentication_error() {
        let error = IntegrationError::AuthenticationFailed {
            reason: "Invalid signature".to_string(),
            device_id: Some(vec![1, 2, 3]),
            recovery: RecoveryStrategy::RefreshAuthentication,
        };

        assert_eq!(error.severity(), ErrorSeverity::Critical);
        assert!(!error.is_retryable());
        assert!(error.is_security_issue());
    }

    #[test]
    fn test_network_timeout_error() {
        let error = IntegrationError::NetworkTimeout {
            operation: "chunk_upload".to_string(),
            timeout_ms: 5000,
            retry_count: 2,
            recovery: RecoveryStrategy::RetryWithBackoff {
                max_retries: 3,
                initial_delay_ms: 1000,
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Warning);
        assert!(error.is_retryable());
        assert!(!error.is_security_issue());
    }

    #[test]
    fn test_capability_expired_error() {
        let error = IntegrationError::CapabilityExpired {
            expired_at: 1000,
            current_time: 2000,
            recovery: RecoveryStrategy::RequestNewCapability {
                capability_type: "storage".to_string(),
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Error);
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_data_corruption_error() {
        let error = IntegrationError::DataCorruption {
            details: "Checksum mismatch".to_string(),
            chunk_id: Some("chunk123".to_string()),
            manifest_cid: Some("manifest456".to_string()),
            recovery: RecoveryStrategy::RequestReReplication {
                chunk_ids: vec!["chunk123".to_string()],
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Critical);
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_replay_attack_error() {
        let error = IntegrationError::ReplayAttackDetected {
            counter: 100,
            last_seen: 150,
            recovery: RecoveryStrategy::RotateKeys {
                key_types: vec!["relationship".to_string()],
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Critical);
        assert!(error.is_security_issue());
    }

    #[test]
    fn test_error_context() {
        let ctx = ErrorContext::new("upload_chunk", 1000)
            .with_device_id(vec![1, 2, 3])
            .with_peer_id(vec![4, 5, 6])
            .with_resource_id("chunk123")
            .with_metadata("attempt", "3");

        assert_eq!(ctx.operation(), "upload_chunk");
        assert_eq!(ctx.timestamp(), 1000);
        assert_eq!(ctx.metadata().len(), 1);
    }

    #[test]
    fn test_recovery_strategy_equality() {
        let strategy1 = RecoveryStrategy::RetryWithBackoff {
            max_retries: 3,
            initial_delay_ms: 1000,
        };
        let strategy2 = RecoveryStrategy::RetryWithBackoff {
            max_retries: 3,
            initial_delay_ms: 1000,
        };
        assert_eq!(strategy1, strategy2);
    }

    #[test]
    fn test_quota_exceeded_error() {
        let error = IntegrationError::QuotaExceeded {
            used: 1000000,
            limit: 500000,
            device_id: vec![1, 2, 3],
            recovery: RecoveryStrategy::ManualIntervention {
                reason: "Increase quota or delete data".to_string(),
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Error);
    }

    #[test]
    fn test_key_version_mismatch() {
        let error = IntegrationError::KeyVersionMismatch {
            expected: 2,
            actual: 1,
            key_type: "relationship".to_string(),
            recovery: RecoveryStrategy::RotateKeys {
                key_types: vec!["relationship".to_string()],
            },
        };

        assert_eq!(error.severity(), ErrorSeverity::Error);
    }
}
