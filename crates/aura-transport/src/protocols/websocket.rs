//! WebSocket Protocol Message Types
//!
//! Core WebSocket protocol messages for aura-macros choreography! macro.
//! Target: <100 lines (simple implementation).

use aura_core::identifiers::DeviceId;
use aura_core::time::{PhysicalTime, TimeStamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Core WebSocket protocol messages for choreographic usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketMessage {
    /// Handshake initiation
    HandshakeRequest {
        /// Session ID for this handshake
        session_id: Uuid,
        /// Device initiating handshake
        initiator: DeviceId,
        /// Protocol version
        protocol_version: String,
        /// Supported capabilities
        capabilities: Vec<String>,
    },

    /// Handshake response
    HandshakeResponse {
        /// Session ID from request
        session_id: Uuid,
        /// Response result
        result: HandshakeResult,
        /// Accepted capabilities
        accepted_capabilities: Vec<String>,
    },

    /// Session data frame
    DataFrame {
        /// Session ID
        session_id: Uuid,
        /// Frame payload
        payload: Vec<u8>,
        /// Frame metadata
        metadata: FrameMetadata,
    },

    /// Session teardown initiation
    TeardownRequest {
        /// Session ID to teardown
        session_id: Uuid,
        /// Device requesting teardown
        requester: DeviceId,
        /// Reason for teardown
        reason: String,
    },

    /// Teardown acknowledgment
    TeardownResponse {
        /// Session ID being torn down
        session_id: Uuid,
        /// Teardown accepted
        accepted: bool,
    },
}

/// Handshake result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeResult {
    /// Handshake successful
    Success,
    /// Handshake failed
    Failed {
        /// Reason for handshake failure
        reason: String,
    },
}

/// Frame metadata for data messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMetadata {
    /// Frame type
    pub frame_type: FrameType,
    /// Timestamp when frame was created (using Aura unified time system)
    pub timestamp: TimeStamp,
    /// Frame sequence number
    pub sequence: u64,
}

/// WebSocket frame types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FrameType {
    /// Text data frame
    Text,
    /// Binary data frame
    Binary,
    /// Control frame
    Control,
}

impl WebSocketMessage {
    /// Create handshake request
    pub fn handshake_request(initiator: DeviceId, capabilities: Vec<String>) -> Self {
        Self::handshake_request_with_id(Self::generate_session_id(), initiator, capabilities)
    }

    /// Create handshake request with specific session ID
    pub fn handshake_request_with_id(
        session_id: Uuid,
        initiator: DeviceId,
        capabilities: Vec<String>,
    ) -> Self {
        Self::HandshakeRequest {
            session_id,
            initiator,
            protocol_version: "aura-ws-1.0".to_string(),
            capabilities,
        }
    }

    /// Generate deterministic session ID
    fn generate_session_id() -> Uuid {
        // Use deterministic approach for session IDs
        // In production this would use a proper deterministic algorithm
        Uuid::nil() // Placeholder
    }

    /// Create successful handshake response
    pub fn handshake_success(session_id: Uuid, accepted_capabilities: Vec<String>) -> Self {
        Self::HandshakeResponse {
            session_id,
            result: HandshakeResult::Success,
            accepted_capabilities,
        }
    }

    /// Create failed handshake response
    pub fn handshake_failed(session_id: Uuid, reason: String) -> Self {
        Self::HandshakeResponse {
            session_id,
            result: HandshakeResult::Failed { reason },
            accepted_capabilities: Vec::new(),
        }
    }

    /// Create data frame with default timestamp
    ///
    /// Note: In production, timestamp should be provided via PhysicalTimeEffects
    pub fn data_frame(session_id: Uuid, payload: Vec<u8>, frame_type: FrameType) -> Self {
        Self::data_frame_with_timestamp(
            session_id,
            payload,
            frame_type,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
        )
    }

    /// Create data frame with specific timestamp
    pub fn data_frame_with_timestamp(
        session_id: Uuid,
        payload: Vec<u8>,
        frame_type: FrameType,
        timestamp: TimeStamp,
    ) -> Self {
        Self::DataFrame {
            session_id,
            payload,
            metadata: FrameMetadata {
                frame_type,
                timestamp,
                sequence: 0, // Will be set by sender
            },
        }
    }

    /// Create teardown request
    pub fn teardown_request(session_id: Uuid, requester: DeviceId, reason: String) -> Self {
        Self::TeardownRequest {
            session_id,
            requester,
            reason,
        }
    }

    /// Get session ID from any message
    pub fn session_id(&self) -> Uuid {
        match self {
            Self::HandshakeRequest { session_id, .. }
            | Self::HandshakeResponse { session_id, .. }
            | Self::DataFrame { session_id, .. }
            | Self::TeardownRequest { session_id, .. }
            | Self::TeardownResponse { session_id, .. } => *session_id,
        }
    }

    /// Check if this is a handshake message
    pub fn is_handshake(&self) -> bool {
        matches!(
            self,
            Self::HandshakeRequest { .. } | Self::HandshakeResponse { .. }
        )
    }

    /// Check if this is a data message
    pub fn is_data(&self) -> bool {
        matches!(self, Self::DataFrame { .. })
    }
}
