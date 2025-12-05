//! Hole Punching Protocol Message Types
//!
//! Core hole punching messages compatible with rumpsteak-aura choreographic DSL.
//! Target: <120 lines (minimal implementation).

use crate::types::endpoint::EndpointAddress;
use aura_core::hash::{hash as core_hash, hasher};
use aura_core::identifiers::AuthorityId;
use aura_core::time::{OrderTime, TimeStamp};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

static HOLE_PUNCH_SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Core hole punching messages for choreographic protocols
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HolePunchMessage {
    /// Request to coordinate hole punching between peers
    CoordinationRequest {
        /// Session ID for this hole punch attempt
        session_id: Uuid,
        /// Authority initiating the hole punch
        initiator: AuthorityId,
        /// Target authority for hole punching
        target: AuthorityId,
        /// Relay server coordinates the process
        relay_server: EndpointAddress,
    },

    /// Punch packet sent directly between peers
    PunchPacket {
        /// Session ID matching coordination request
        session_id: Uuid,
        /// Source authority
        source: AuthorityId,
        /// Target authority
        target: AuthorityId,
        /// Sequence number for this punch attempt
        sequence: u32,
        /// Timestamp when packet was sent
        timestamp: TimeStamp,
    },

    /// Acknowledgment of successful hole punch
    PunchAcknowledgment {
        /// Session ID being acknowledged
        session_id: Uuid,
        /// Authority sending acknowledgment
        acknowledger: AuthorityId,
        /// Successfully reached peer
        reached_peer: AuthorityId,
        /// Local endpoint that succeeded
        local_endpoint: EndpointAddress,
        /// Remote endpoint that was reached
        remote_endpoint: EndpointAddress,
    },

    /// Coordination response from relay
    CoordinationResponse {
        /// Session ID from original request
        session_id: Uuid,
        /// Coordination result
        result: CoordinationResult,
        /// Instructions for peers
        instructions: Vec<HolePunchInstruction>,
    },
}

/// Result of hole punch coordination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinationResult {
    /// Coordination successful, proceed with hole punching
    Success,
    /// Coordination failed - cannot proceed
    Failed {
        /// Reason for coordination failure
        reason: String,
    },
    /// Coordination pending - wait for more information
    Pending,
}

/// Instructions for hole punch execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HolePunchInstruction {
    /// Send punch packets to specific endpoint
    SendPunch {
        /// Target endpoint to send punch packets to
        target_endpoint: EndpointAddress,
        /// Number of punch packets to send
        packet_count: u32,
        /// Interval between packets in milliseconds
        interval_ms: u64,
    },

    /// Listen for incoming punch packets
    ListenForPunch {
        /// Local endpoint to bind for listening
        local_endpoint: EndpointAddress,
        /// Timeout for listening operation in milliseconds
        timeout_ms: u64,
    },

    /// Wait before starting punch sequence
    Wait {
        /// Duration to wait in milliseconds
        duration_ms: u64,
    },

    /// Use specific local endpoint
    BindToEndpoint {
        /// Specific endpoint to bind to
        endpoint: EndpointAddress,
    },
}

impl HolePunchMessage {
    /// Create new coordination request
    pub fn coordination_request(
        initiator: AuthorityId,
        target: AuthorityId,
        relay_server: EndpointAddress,
    ) -> Self {
        Self::coordination_request_with_id(
            Self::generate_session_id(initiator, target),
            initiator,
            target,
            relay_server,
        )
    }

    /// Create coordination request with specific session ID
    pub fn coordination_request_with_id(
        session_id: Uuid,
        initiator: AuthorityId,
        target: AuthorityId,
        relay_server: EndpointAddress,
    ) -> Self {
        Self::CoordinationRequest {
            session_id,
            initiator,
            target,
            relay_server,
        }
    }

    /// Generate deterministic session ID
    fn generate_session_id(initiator: AuthorityId, target: AuthorityId) -> Uuid {
        let counter = HOLE_PUNCH_SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut h = hasher();
        h.update(initiator.0.as_bytes());
        h.update(target.0.as_bytes());
        h.update(&counter.to_le_bytes());
        let digest = h.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(uuid_bytes)
    }

    /// Create punch packet
    pub fn punch_packet(
        session_id: Uuid,
        source: AuthorityId,
        target: AuthorityId,
        sequence: u32,
    ) -> Self {
        Self::punch_packet_at_time(session_id, source, target, sequence, {
            let mut bytes = [0u8; 32];
            bytes[..16].copy_from_slice(session_id.as_bytes());
            let seq_hash = core_hash(&sequence.to_le_bytes());
            bytes[16..].copy_from_slice(&seq_hash[..16]);
            TimeStamp::OrderClock(OrderTime(bytes))
        })
    }

    /// Create punch packet with specific timestamp
    pub fn punch_packet_at_time(
        session_id: Uuid,
        source: AuthorityId,
        target: AuthorityId,
        sequence: u32,
        timestamp: TimeStamp,
    ) -> Self {
        Self::PunchPacket {
            session_id,
            source,
            target,
            sequence,
            timestamp,
        }
    }

    /// Create acknowledgment
    pub fn acknowledgment(
        session_id: Uuid,
        acknowledger: AuthorityId,
        reached_peer: AuthorityId,
        local_endpoint: EndpointAddress,
        remote_endpoint: EndpointAddress,
    ) -> Self {
        Self::PunchAcknowledgment {
            session_id,
            acknowledger,
            reached_peer,
            local_endpoint,
            remote_endpoint,
        }
    }

    /// Create successful coordination response
    pub fn success_response(session_id: Uuid, instructions: Vec<HolePunchInstruction>) -> Self {
        Self::CoordinationResponse {
            session_id,
            result: CoordinationResult::Success,
            instructions,
        }
    }

    /// Create failed coordination response
    pub fn failed_response(session_id: Uuid, reason: String) -> Self {
        Self::CoordinationResponse {
            session_id,
            result: CoordinationResult::Failed { reason },
            instructions: Vec::new(),
        }
    }

    /// Get session ID from any message
    pub fn session_id(&self) -> Uuid {
        match self {
            Self::CoordinationRequest { session_id, .. }
            | Self::PunchPacket { session_id, .. }
            | Self::PunchAcknowledgment { session_id, .. }
            | Self::CoordinationResponse { session_id, .. } => *session_id,
        }
    }

    /// Check if message is a coordination message
    pub fn is_coordination(&self) -> bool {
        matches!(
            self,
            Self::CoordinationRequest { .. } | Self::CoordinationResponse { .. }
        )
    }

    /// Check if message is a punch attempt
    pub fn is_punch_attempt(&self) -> bool {
        matches!(
            self,
            Self::PunchPacket { .. } | Self::PunchAcknowledgment { .. }
        )
    }
}

/// Configuration for hole punching operations
#[derive(Debug, Clone)]
pub struct PunchConfig {
    /// Maximum number of punch attempts
    pub max_attempts: u32,
    /// Timeout for hole punch attempts
    pub punch_timeout: std::time::Duration,
    /// Interval between punch packets
    pub punch_interval: std::time::Duration,
    /// Enable symmetric NAT detection
    pub enable_symmetric_detection: bool,
    /// Relay servers for coordination
    pub relay_servers: Vec<EndpointAddress>,
}

impl Default for PunchConfig {
    fn default() -> Self {
        Self {
            max_attempts: 10,
            punch_timeout: std::time::Duration::from_secs(5),
            punch_interval: std::time::Duration::from_millis(200),
            enable_symmetric_detection: true,
            relay_servers: Vec::new(),
        }
    }
}
