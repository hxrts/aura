//! Choreographic effect interface
//!
//! Pure trait definitions for choreographic protocol operations.
//!
//! # Effect Classification
//!
//! - **Category**: Protocol Coordination Effect
//! - **Implementation**: `aura-protocol::choreography` (Layer 4)
//! - **Usage**: Multi-party protocol coordination (roles, sessions, message passing)
//!
//! This is a protocol coordination effect for multi-party choreography. Implements
//! role-based message passing, session management, and protocol coordination needed
//! for distributed FROST ceremonies, recovery protocols, and other multi-device
//! operations. Handlers in `aura-protocol` coordinate protocol execution.

use async_trait::async_trait;
use uuid::Uuid;

/// Choreographic effects for distributed protocol coordination
#[async_trait]
pub trait ChoreographicEffects: Send + Sync {
    /// Send raw bytes to a specific role in the choreography
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError>;

    /// Receive raw bytes from a specific role
    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError>;

    /// Broadcast raw bytes to all roles
    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError>;

    /// Get the current role in the choreography
    fn current_role(&self) -> ChoreographicRole;

    /// Get all roles participating in the choreography
    fn all_roles(&self) -> Vec<ChoreographicRole>;

    /// Check if a role is currently active/connected
    async fn is_role_active(&self, role: ChoreographicRole) -> bool;

    /// Start a new choreography session
    async fn start_session(
        &self,
        session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError>;

    /// End the current choreography session
    async fn end_session(&self) -> Result<(), ChoreographyError>;

    /// Emit a choreography event for debugging/visualization
    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError>;

    /// Set a timeout for the next operation
    async fn set_timeout(&self, timeout_ms: u64);

    /// Get performance metrics for the choreography
    async fn get_metrics(&self) -> ChoreographyMetrics;
}

/// Role in a choreographic protocol
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ChoreographicRole {
    /// Device ID for this role
    pub device_id: Uuid,
    /// Role index in the protocol (0-based)
    pub role_index: u16,
}

impl ChoreographicRole {
    /// Create a new choreographic role
    pub fn new(device_id: Uuid, role_index: u16) -> Self {
        Self {
            device_id,
            role_index,
        }
    }
}

/// Choreography-related errors
#[derive(Debug, thiserror::Error)]
pub enum ChoreographyError {
    /// Role not found in the choreography
    #[error("Role not found: {role:?}")]
    RoleNotFound {
        /// The role that was not found
        role: ChoreographicRole,
    },

    /// Communication timeout with a role
    #[error("Communication timeout with role {role:?} after {timeout_ms}ms")]
    CommunicationTimeout {
        /// The role that timed out
        role: ChoreographicRole,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },

    /// Failed to serialize a message
    #[error("Message serialization failed: {reason}")]
    SerializationFailed {
        /// Reason for serialization failure
        reason: String,
    },

    /// Failed to deserialize a message
    #[error("Message deserialization failed: {reason}")]
    DeserializationFailed {
        /// Reason for deserialization failure
        reason: String,
    },

    /// Protocol violation detected
    #[error("Protocol violation: {message}")]
    ProtocolViolation {
        /// Description of the violation
        message: String,
    },

    /// Session has not been started
    #[error("Session not started")]
    SessionNotStarted,

    /// Session already exists
    #[error("Session already exists: {session_id}")]
    SessionAlreadyExists {
        /// ID of the existing session
        session_id: Uuid,
    },

    /// Insufficient participants for the protocol
    #[error("Insufficient participants: got {actual}, need {required}")]
    InsufficientParticipants {
        /// Actual number of participants
        actual: u16,
        /// Required number of participants
        required: u16,
    },

    /// Byzantine behavior detected
    #[error("Byzantine behavior detected from role {role:?}: {evidence}")]
    ByzantineBehavior {
        /// The role exhibiting Byzantine behavior
        role: ChoreographicRole,
        /// Evidence of the Byzantine behavior
        evidence: String,
    },

    /// Transport layer error
    #[error("Transport error: {source}")]
    Transport {
        /// The underlying transport error
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Internal system error
    #[error("Internal error: {message}")]
    InternalError {
        /// Error message
        message: String,
    },
}

/// Choreography events for debugging and visualization
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChoreographyEvent {
    /// Protocol phase started
    PhaseStarted {
        /// Name of the phase
        phase: String,
        /// Roles participating in this phase
        participants: Vec<ChoreographicRole>,
    },
    /// Message sent between roles
    MessageSent {
        /// Sender role
        from: ChoreographicRole,
        /// Recipient role
        to: ChoreographicRole,
        /// Type of message sent
        message_type: String,
    },
    /// Protocol completed successfully
    ProtocolCompleted {
        /// Name of the protocol
        protocol: String,
        /// Total duration in milliseconds
        duration_ms: u64,
    },
    /// Protocol failed
    ProtocolFailed {
        /// Name of the protocol
        protocol: String,
        /// Error message
        error: String,
    },
    /// Timeout occurred
    Timeout {
        /// Operation that timed out
        operation: String,
        /// Timeout duration in milliseconds
        timeout_ms: u64,
    },
    /// Byzantine behavior detected
    ByzantineDetected {
        /// Role exhibiting Byzantine behavior
        role: ChoreographicRole,
        /// Evidence of the behavior
        evidence: String,
    },
}

/// Performance metrics for choreography execution
#[derive(Debug, Clone)]
pub struct ChoreographyMetrics {
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Average message latency in milliseconds
    pub avg_latency_ms: f64,
    /// Number of timeouts
    pub timeout_count: u64,
    /// Number of retries
    pub retry_count: u64,
    /// Total execution time in milliseconds
    pub total_duration_ms: u64,
}
