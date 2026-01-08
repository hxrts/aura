//! Choreographic effect interface
//!
//! Pure trait definitions for choreographic protocol operations.

use async_trait::async_trait;
use aura_core::DeviceId;
use std::num::NonZeroU32;
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

/// Blanket implementation for Arc<T> where T: ChoreographicEffects
#[async_trait]
impl<T: ChoreographicEffects + ?Sized> ChoreographicEffects for std::sync::Arc<T> {
    async fn send_to_role_bytes(
        &self,
        role: ChoreographicRole,
        message: Vec<u8>,
    ) -> Result<(), ChoreographyError> {
        (**self).send_to_role_bytes(role, message).await
    }

    async fn receive_from_role_bytes(
        &self,
        role: ChoreographicRole,
    ) -> Result<Vec<u8>, ChoreographyError> {
        (**self).receive_from_role_bytes(role).await
    }

    async fn broadcast_bytes(&self, message: Vec<u8>) -> Result<(), ChoreographyError> {
        (**self).broadcast_bytes(message).await
    }

    fn current_role(&self) -> ChoreographicRole {
        (**self).current_role()
    }

    fn all_roles(&self) -> Vec<ChoreographicRole> {
        (**self).all_roles()
    }

    async fn is_role_active(&self, role: ChoreographicRole) -> bool {
        (**self).is_role_active(role).await
    }

    async fn start_session(
        &self,
        session_id: Uuid,
        roles: Vec<ChoreographicRole>,
    ) -> Result<(), ChoreographyError> {
        (**self).start_session(session_id, roles).await
    }

    async fn end_session(&self) -> Result<(), ChoreographyError> {
        (**self).end_session().await
    }

    async fn emit_choreo_event(&self, event: ChoreographyEvent) -> Result<(), ChoreographyError> {
        (**self).emit_choreo_event(event).await
    }

    async fn set_timeout(&self, timeout_ms: u64) {
        (**self).set_timeout(timeout_ms).await;
    }

    async fn get_metrics(&self) -> ChoreographyMetrics {
        (**self).get_metrics().await
    }
}

/// Role in a choreographic protocol
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct RoleIndex(NonZeroU32);

impl RoleIndex {
    /// Create a new role index from a 0-based index.
    pub fn new(index: u32) -> Option<Self> {
        index.checked_add(1).and_then(NonZeroU32::new).map(Self)
    }

    /// Return the 0-based index value.
    pub fn get(self) -> u32 {
        self.0.get().saturating_sub(1)
    }
}

/// Role in a choreographic protocol
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct ChoreographicRole {
    /// Device ID for this role
    pub device_id: DeviceId,
    /// Role index in the protocol (0-based)
    pub role_index: RoleIndex,
}

impl ChoreographicRole {
    /// Create a new choreographic role
    pub fn new(device_id: DeviceId, role_index: RoleIndex) -> Self {
        Self {
            device_id,
            role_index,
        }
    }

    /// Create a new role from a 0-based index.
    pub fn from_index(device_id: DeviceId, role_index: u32) -> Option<Self> {
        RoleIndex::new(role_index).map(|idx| Self::new(device_id, idx))
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
        actual: u32,
        /// Required number of participants
        required: u32,
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

    /// Role family is empty (no instances registered)
    #[error("Role family '{family}' is empty")]
    EmptyRoleFamily {
        /// The name of the empty role family
        family: String,
    },

    /// Role family not found (unknown family name)
    #[error("Role family '{family}' not found")]
    RoleFamilyNotFound {
        /// The name of the unknown role family
        family: String,
    },

    /// Invalid role family range
    #[error("Invalid role family range [{start}, {end}) for family '{family}'")]
    InvalidRoleFamilyRange {
        /// The role family name
        family: String,
        /// Start of the requested range
        start: u32,
        /// End of the requested range
        end: u32,
    },

    /// Authorization failed (guard chain denied)
    #[error("Authorization failed: {reason}")]
    AuthorizationFailed {
        /// Reason for authorization failure
        reason: String,
    },
}

impl From<rumpsteak_aura_choreography::ChoreographyError> for ChoreographyError {
    fn from(e: rumpsteak_aura_choreography::ChoreographyError) -> Self {
        ChoreographyError::InternalError {
            message: e.to_string(),
        }
    }
}

impl aura_core::ProtocolErrorCode for ChoreographyError {
    fn code(&self) -> &'static str {
        match self {
            ChoreographyError::RoleNotFound { .. } => "choreography_role_not_found",
            ChoreographyError::CommunicationTimeout { .. } => "choreography_timeout",
            ChoreographyError::SerializationFailed { .. } => "choreography_serialization",
            ChoreographyError::DeserializationFailed { .. } => "choreography_deserialization",
            ChoreographyError::ProtocolViolation { .. } => "choreography_protocol_violation",
            ChoreographyError::SessionNotStarted => "choreography_session_not_started",
            ChoreographyError::SessionAlreadyExists { .. } => "choreography_session_exists",
            ChoreographyError::InsufficientParticipants { .. } => {
                "choreography_insufficient_participants"
            }
            ChoreographyError::ByzantineBehavior { .. } => "choreography_byzantine",
            ChoreographyError::Transport { .. } => "choreography_transport",
            ChoreographyError::InternalError { .. } => "choreography_internal",
            ChoreographyError::EmptyRoleFamily { .. } => "choreography_empty_role_family",
            ChoreographyError::RoleFamilyNotFound { .. } => "choreography_role_family_not_found",
            ChoreographyError::InvalidRoleFamilyRange { .. } => {
                "choreography_invalid_role_family_range"
            }
            ChoreographyError::AuthorizationFailed { .. } => "choreography_authorization_failed",
        }
    }
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
