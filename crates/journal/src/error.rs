//! Unified error system for Aura
//!
//! Provides structured error types with rich context for distributed debugging,
//! simulation testing, and production observability.

use crate::{AccountId, DeviceId, ParticipantId, ProtocolType, SessionEpoch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Root error type for all Aura operations
#[derive(Debug)]
pub struct AuraError {
    /// The specific error that occurred
    pub kind: AuraErrorKind,
    /// Rich context about where and when the error occurred
    pub context: ErrorContext,
    /// Unique identifier for error correlation across systems
    pub trace_id: Uuid,
    /// When the error occurred (injected via Effects)
    pub timestamp: u64,
    /// Session context if error occurred during protocol execution
    pub session_context: Option<SessionContext>,
}

/// Comprehensive taxonomy of all possible error conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuraErrorKind {
    /// Authentication failures - identity proof issues
    Authentication {
        device_id: DeviceId,
        reason: AuthFailureReason,
        context: Option<String>,
    },

    /// Authorization failures - permission check failures
    Authorization {
        required_capability: String, // CapabilityScope as string
        granted_capabilities: Vec<String>,
        operation: String,
    },

    /// Network and transport issues
    Network {
        peer_id: Option<DeviceId>,
        operation: NetworkOperation,
        underlying: NetworkErrorKind,
    },

    /// Data corruption or validation failures
    Corruption {
        data_type: String,
        expected_hash: Option<[u8; 32]>,
        actual_hash: Option<[u8; 32]>,
        context: String,
    },

    /// Resource exhaustion
    Resource {
        resource_type: ResourceType,
        limit: u64,
        requested: u64,
    },

    /// Session-type protocol violations
    ProtocolViolation {
        protocol: ProtocolType,
        expected_state: String,
        actual_state: String,
        session_id: Uuid,
    },

    /// Byzantine behavior detection
    Byzantine {
        accused_device: DeviceId,
        evidence: ByzantineEvidence,
        severity: ByzantineSeverity,
    },

    /// Choreography execution failures
    Choreography {
        protocol: ProtocolType,
        phase: String,
        reason: String,
        participants: Vec<DeviceId>,
    },
}

/// Context information for every error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorContext {
    /// The operation that was being performed
    pub operation: String,
    /// Device where the error occurred
    pub device_id: DeviceId,
    /// Account the device belongs to
    pub account_id: AccountId,
    /// Current protocol phase if applicable
    pub protocol_phase: Option<String>,
    /// Participant ID in threshold protocols
    pub participant_id: Option<ParticipantId>,
    /// Current session epoch
    pub session_epoch: Option<SessionEpoch>,
    /// Additional structured context
    pub fields: BTreeMap<String, String>,
}

/// Session context for protocol-related errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    /// Unique session identifier
    pub session_id: Uuid,
    /// Type of protocol being executed
    pub protocol_type: ProtocolType,
    /// Current state in the session
    pub current_state: String,
    /// All participants in the session
    pub participants: Vec<DeviceId>,
    /// When the session started
    pub started_at: u64,
    /// Session-specific metadata
    pub metadata: BTreeMap<String, String>,
}

/// Authentication failure reasons
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthFailureReason {
    /// Invalid signature
    InvalidSignature,
    /// Expired certificate
    ExpiredCertificate,
    /// Unknown device
    UnknownDevice,
    /// Malformed credentials
    MalformedCredentials,
    /// Replay attack detected
    ReplayAttack,
}

/// Network operation types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkOperation {
    /// Establishing connection
    Connect,
    /// Sending message
    Send,
    /// Receiving message
    Receive,
    /// Handshake negotiation
    Handshake,
    /// Channel encryption
    Encryption,
}

/// Network error categories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkErrorKind {
    /// Connection timeout
    Timeout,
    /// Connection refused
    ConnectionRefused,
    /// DNS resolution failure
    DnsFailure,
    /// SSL/TLS error
    TlsError,
    /// Protocol version mismatch
    ProtocolMismatch,
    /// Network unreachable
    NetworkUnreachable,
}

/// Resource types for exhaustion errors
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    /// Memory allocation
    Memory,
    /// File descriptors
    FileDescriptors,
    /// Storage space
    StorageSpace,
    /// Network bandwidth
    NetworkBandwidth,
    /// CPU time
    CpuTime,
    /// Concurrent sessions
    ConcurrentSessions,
}

/// Evidence of Byzantine behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineEvidence {
    /// Invalid threshold signature contribution
    InvalidSignatureShare {
        expected_commitment: [u8; 32],
        actual_commitment: [u8; 32],
    },
    /// Double-spending attempt
    DoubleSpending { first_nonce: u64, second_nonce: u64 },
    /// Equivocation (sending conflicting messages)
    Equivocation {
        message1_hash: [u8; 32],
        message2_hash: [u8; 32],
    },
    /// Invalid state transition
    InvalidTransition {
        from_state: String,
        to_state: String,
        reason: String,
    },
    /// Resource exhaustion attack
    ResourceExhaustion {
        resource: ResourceType,
        rate: u64, // requests per second
    },
}

/// Severity of Byzantine behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ByzantineSeverity {
    /// Suspicious but not conclusive
    Suspicious,
    /// Clear protocol violation
    Violation,
    /// Malicious attack
    Malicious,
}

impl AuraError {
    /// Create a new error with minimal context
    pub fn new(
        kind: AuraErrorKind,
        operation: &str,
        device_id: DeviceId,
        account_id: AccountId,
    ) -> Self {
        Self {
            kind,
            context: ErrorContext {
                operation: operation.to_string(),
                device_id,
                account_id,
                protocol_phase: None,
                participant_id: None,
                session_epoch: None,
                fields: BTreeMap::new(),
            },
            trace_id: Uuid::from_bytes([0; 16]), // Deterministic default, should be set by caller
            timestamp: 0,                        // Will be injected by Effects
            session_context: None,
        }
    }

    /// Add protocol context to the error
    pub fn with_protocol_context(mut self, _protocol: ProtocolType, phase: &str) -> Self {
        self.context.protocol_phase = Some(phase.to_string());
        self
    }

    /// Add session context to the error
    pub fn with_session_context(mut self, session_context: SessionContext) -> Self {
        self.session_context = Some(session_context);
        self
    }

    /// Add a context field
    pub fn with_field(mut self, key: &str, value: &str) -> Self {
        self.context
            .fields
            .insert(key.to_string(), value.to_string());
        self
    }

    /// Check if this error indicates Byzantine behavior
    pub fn is_byzantine(&self) -> bool {
        matches!(self.kind, AuraErrorKind::Byzantine { .. })
    }

    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match &self.kind {
            AuraErrorKind::Network { underlying, .. } => {
                matches!(
                    underlying,
                    NetworkErrorKind::Timeout | NetworkErrorKind::NetworkUnreachable
                )
            }
            AuraErrorKind::Resource { .. } => true,
            AuraErrorKind::Authorization { .. } => true, // Can refresh capabilities
            AuraErrorKind::Authentication { reason, .. } => {
                matches!(reason, AuthFailureReason::ExpiredCertificate)
            }
            AuraErrorKind::Byzantine { .. } => false,
            AuraErrorKind::Corruption { .. } => false,
            AuraErrorKind::ProtocolViolation { .. } => false,
            AuraErrorKind::Choreography { .. } => true, // Can retry choreography
        }
    }
}

impl std::fmt::Display for AuraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {:?}",
            self.context.device_id.0, self.context.operation, self.kind
        )
    }
}

impl std::error::Error for AuraError {}

/// Convenience type alias
pub type Result<T> = std::result::Result<T, AuraError>;

/// Helper functions for creating common errors
impl AuraError {
    /// Create an authentication error
    pub fn authentication(
        device_id: DeviceId,
        account_id: AccountId,
        reason: AuthFailureReason,
        operation: &str,
    ) -> Self {
        Self::new(
            AuraErrorKind::Authentication {
                device_id,
                reason,
                context: None,
            },
            operation,
            device_id,
            account_id,
        )
    }

    /// Create an authorization error
    pub fn authorization(
        device_id: DeviceId,
        account_id: AccountId,
        required: &str,
        granted: Vec<String>,
        operation: &str,
    ) -> Self {
        Self::new(
            AuraErrorKind::Authorization {
                required_capability: required.to_string(),
                granted_capabilities: granted,
                operation: operation.to_string(),
            },
            operation,
            device_id,
            account_id,
        )
    }

    /// Create a network error
    pub fn network(
        device_id: DeviceId,
        account_id: AccountId,
        peer_id: Option<DeviceId>,
        operation: NetworkOperation,
        underlying: NetworkErrorKind,
        operation_name: &str,
    ) -> Self {
        Self::new(
            AuraErrorKind::Network {
                peer_id,
                operation,
                underlying,
            },
            operation_name,
            device_id,
            account_id,
        )
    }

    /// Create a Byzantine behavior error
    pub fn byzantine(
        device_id: DeviceId,
        account_id: AccountId,
        accused: DeviceId,
        evidence: ByzantineEvidence,
        severity: ByzantineSeverity,
        operation: &str,
    ) -> Self {
        Self::new(
            AuraErrorKind::Byzantine {
                accused_device: accused,
                evidence,
                severity,
            },
            operation,
            device_id,
            account_id,
        )
    }

    /// Create a protocol violation error
    pub fn protocol_violation(
        device_id: DeviceId,
        account_id: AccountId,
        protocol: ProtocolType,
        expected_state: &str,
        actual_state: &str,
        session_id: Uuid,
        operation: &str,
    ) -> Self {
        Self::new(
            AuraErrorKind::ProtocolViolation {
                protocol,
                expected_state: expected_state.to_string(),
                actual_state: actual_state.to_string(),
                session_id,
            },
            operation,
            device_id,
            account_id,
        )
    }
}
