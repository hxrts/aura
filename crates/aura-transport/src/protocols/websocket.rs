//! WebSocket Protocol Message Types
//!
//! Core WebSocket protocol messages for aura-macros choreography! macro.
//! Target: <100 lines (simple implementation).

use aura_core::hash::{hash as core_hash, hasher};
use aura_core::identifiers::AuthorityId;
use aura_core::time::{OrderTime, TimeStamp};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Core WebSocket protocol messages for choreographic usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WebSocketMessage {
    /// Handshake initiation
    HandshakeRequest {
        /// Session ID for this handshake
        session_id: Uuid,
        /// Authority initiating handshake
        initiator: AuthorityId,
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
        /// Authority requesting teardown
        requester: AuthorityId,
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
    pub fn handshake_request(initiator: AuthorityId, capabilities: Vec<String>) -> Self {
        Self::handshake_request_with_id(
            Self::generate_session_id(initiator),
            initiator,
            capabilities,
        )
    }

    /// Create handshake request with specific session ID
    pub fn handshake_request_with_id(
        session_id: Uuid,
        initiator: AuthorityId,
        capabilities: Vec<String>,
    ) -> Self {
        Self::HandshakeRequest {
            session_id,
            initiator,
            protocol_version: "aura-ws-1.0".to_string(),
            capabilities,
        }
    }

    /// Generate deterministic session ID using initiator + monotonic counter
    fn generate_session_id(initiator: AuthorityId) -> Uuid {
        let counter = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut h = hasher();
        h.update(initiator.0.as_bytes());
        h.update(&counter.to_le_bytes());
        let digest = h.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(uuid_bytes)
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

    /// Create data frame with deterministic order-only timestamp
    pub fn data_frame(session_id: Uuid, payload: Vec<u8>, frame_type: FrameType) -> Self {
        let mut order_bytes = [0u8; 32];
        order_bytes[..16].copy_from_slice(session_id.as_bytes());
        let payload_digest = core_hash(&payload);
        order_bytes[16..].copy_from_slice(&payload_digest[..16]);

        Self::data_frame_with_timestamp(
            session_id,
            payload,
            frame_type,
            TimeStamp::OrderClock(OrderTime(order_bytes)),
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
                sequence: 0, // Deterministically assigned by caller if needed
            },
        }
    }

    /// Create teardown request
    pub fn teardown_request(session_id: Uuid, requester: AuthorityId, reason: String) -> Self {
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
