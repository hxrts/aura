//! Relationship-Scoped Connection Abstractions
//!
//! Provides essential connection types with built-in relationship scoping and privacy context.
//! Target: <120 lines (minimal scoping implementation).

use aura_core::{
    hash::{hash as core_hash, hasher},
    identifiers::DeviceId,
    time::{OrderTime, PhysicalTime, TimeStamp},
    RelationshipId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

use super::envelope::PrivacyLevel;

static CONNECTION_COUNTER: AtomicU64 = AtomicU64::new(1);

fn order_from_bytes(seed: &[u8]) -> OrderTime {
    OrderTime(core_hash(seed))
}

/// Simple identifier with privacy-preserving properties
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(Uuid);

/// Privacy-preserving connection identification within relationship context
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScopedConnectionId {
    /// Base connection identifier
    pub connection_id: ConnectionId,
    /// Relationship context for this connection
    pub relationship_id: RelationshipId,
    /// Scoped identifier within relationship
    pub scoped_id: Uuid,
}

/// Essential lifecycle states with privacy context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection being established
    Connecting {
        /// Time when connection attempt started (using Aura unified time system)
        started_at: TimeStamp,
        /// Privacy level for establishment
        privacy_level: PrivacyLevel,
    },
    /// Connection established and operational
    Established {
        /// Time when connection was established (using Aura unified time system)
        established_at: TimeStamp,
        /// Current privacy context
        privacy_context: PrivacyContext,
    },
    /// Connection closing gracefully
    Closing {
        /// Time when close initiated (using Aura unified time system)
        closing_at: TimeStamp,
        /// Reason for closing
        reason: String,
    },
    /// Connection closed
    Closed {
        /// Time when connection closed (using Aura unified time system)
        closed_at: TimeStamp,
        /// Final close reason
        reason: String,
    },
}

/// Essential connection metadata with relationship scoping
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    /// Connection identifier
    pub connection_id: ConnectionId,
    /// Current connection state
    pub state: ConnectionState,
    /// Peer device identifier
    pub peer_id: DeviceId,
    /// Relationship context (if scoped)
    pub relationship_context: Option<RelationshipId>,
    /// Connection capabilities
    pub capabilities: HashMap<String, String>,
    /// Connection metrics
    pub metrics: ConnectionMetrics,
}

/// Privacy context for established connections
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyContext {
    /// Current privacy level
    pub privacy_level: PrivacyLevel,
    /// Relationship scope (if any)
    pub relationship_scope: Option<RelationshipId>,
    /// Capability filtering enabled
    pub capability_filtering: bool,
    /// Message blinding enabled
    pub message_blinding: bool,
}

/// Basic connection metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetrics {
    /// Total bytes sent
    pub bytes_sent: u64,
    /// Total bytes received
    pub bytes_received: u64,
    /// Total messages sent
    pub messages_sent: u64,
    /// Total messages received
    pub messages_received: u64,
    /// Last activity time (using Aura unified time system)
    pub last_activity: TimeStamp,
}

impl ConnectionId {
    /// Generate new connection identifier
    pub fn new() -> Self {
        Self::from_uuid(Self::generate_connection_uuid())
    }

    /// Create connection ID from existing UUID
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get inner UUID
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    /// Generate a deterministic connection UUID
    /// This avoids direct UUID generation while providing uniqueness
    fn generate_connection_uuid() -> Uuid {
        let counter = CONNECTION_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut h = hasher();
        h.update(b"aura-connection-id");
        h.update(&counter.to_le_bytes());
        let digest = h.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(uuid_bytes)
    }
}

impl ScopedConnectionId {
    /// Create scoped connection identifier
    pub fn new(connection_id: ConnectionId, relationship_id: RelationshipId) -> Self {
        // Create a deterministic UUID from relationship and connection IDs
        let mut bytes = [0u8; 16];
        let rel_bytes = relationship_id.as_bytes();
        let conn_uuid = connection_id.as_uuid();
        let conn_bytes = conn_uuid.as_bytes();

        // Mix the bytes for deterministic generation
        for i in 0..16 {
            bytes[i] = rel_bytes[i] ^ conn_bytes[i];
        }

        let scoped_id = Uuid::from_bytes(bytes);

        Self {
            connection_id,
            relationship_id,
            scoped_id,
        }
    }

    /// Get base connection ID
    pub fn connection_id(&self) -> ConnectionId {
        self.connection_id
    }

    /// Get relationship context
    pub fn relationship_id(&self) -> RelationshipId {
        self.relationship_id.clone()
    }
}

impl ConnectionInfo {
    /// Create new connection info for peer
    pub fn new(peer_id: DeviceId, privacy_level: PrivacyLevel) -> Self {
        Self::new_with_timestamp(
            peer_id,
            privacy_level,
            TimeStamp::OrderClock(OrderTime(core_hash(peer_id.0.as_bytes()))),
        )
    }

    /// Create new connection info for peer with specific timestamp
    pub fn new_with_timestamp(
        peer_id: DeviceId,
        privacy_level: PrivacyLevel,
        started_at: TimeStamp,
    ) -> Self {
        let connection_id = ConnectionId::new();
        let state = ConnectionState::Connecting {
            started_at: started_at.clone(),
            privacy_level,
        };

        Self {
            connection_id,
            state,
            peer_id,
            relationship_context: None,
            capabilities: HashMap::new(),
            metrics: ConnectionMetrics::new_with_timestamp(started_at),
        }
    }

    /// Create relationship-scoped connection
    pub fn new_scoped(
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        privacy_level: PrivacyLevel,
    ) -> Self {
        let timestamp = TimeStamp::OrderClock(order_from_bytes(relationship_id.as_bytes()));
        Self::new_scoped_with_timestamp(peer_id, relationship_id, privacy_level, timestamp)
    }

    /// Create relationship-scoped connection with specific timestamp
    pub fn new_scoped_with_timestamp(
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        privacy_level: PrivacyLevel,
        started_at: TimeStamp,
    ) -> Self {
        let mut info = Self::new_with_timestamp(peer_id, privacy_level, started_at);
        info.relationship_context = Some(relationship_id);
        info
    }

    /// Mark connection as established
    pub fn establish(&mut self, privacy_context: PrivacyContext) {
        self.establish_with_timestamp(
            privacy_context,
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
        )
    }

    /// Mark connection as established with specific timestamp
    pub fn establish_with_timestamp(
        &mut self,
        privacy_context: PrivacyContext,
        established_at: TimeStamp,
    ) {
        self.state = ConnectionState::Established {
            established_at,
            privacy_context,
        };
    }

    /// Check if connection is established
    pub fn is_established(&self) -> bool {
        matches!(self.state, ConnectionState::Established { .. })
    }

    /// Get connection age relative to specified current time
    pub fn age_relative_to(&self, current_time: TimeStamp) -> Duration {
        let start_ts = match &self.state {
            ConnectionState::Connecting { started_at, .. } => started_at.clone(),
            ConnectionState::Established { established_at, .. } => established_at.clone(),
            ConnectionState::Closing { closing_at, .. } => closing_at.clone(),
            ConnectionState::Closed { closed_at, .. } => closed_at.clone(),
        };

        fn ts_to_ms(ts: &TimeStamp) -> u128 {
            match ts {
                TimeStamp::PhysicalClock(p) => p.ts_ms as u128,
                TimeStamp::OrderClock(o) => {
                    let mut buf = [0u8; 16];
                    buf.copy_from_slice(&o.0[..16]);
                    u128::from_le_bytes(buf)
                }
                _ => 0,
            }
        }

        let start = ts_to_ms(&start_ts);
        let end = ts_to_ms(&current_time);
        let diff = end.saturating_sub(start);
        Duration::from_millis(diff as u64)
    }

    /// Get connection age (uses epoch as baseline for determinism)
    pub fn age(&self) -> Duration {
        let uuid = self.connection_id.as_uuid();
        let seed = uuid.as_bytes();
        self.age_relative_to(TimeStamp::OrderClock(order_from_bytes(seed)))
    }
}

impl ConnectionMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::new_with_timestamp(TimeStamp::OrderClock(order_from_bytes(&[1u8; 1])))
    }

    /// Create new metrics with specific timestamp
    pub fn new_with_timestamp(current_time: TimeStamp) -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            messages_sent: 0,
            messages_received: 0,
            last_activity: current_time,
        }
    }

    /// Record sent message
    pub fn record_sent(&mut self, bytes: u64) {
        self.record_sent_with_timestamp(
            bytes,
            TimeStamp::OrderClock(order_from_bytes(&bytes.to_le_bytes())),
        )
    }

    /// Record sent message with specific timestamp
    pub fn record_sent_with_timestamp(&mut self, bytes: u64, current_time: TimeStamp) {
        self.bytes_sent += bytes;
        self.messages_sent += 1;
        self.last_activity = current_time;
    }

    /// Record received message
    pub fn record_received(&mut self, bytes: u64) {
        self.record_received_with_timestamp(
            bytes,
            TimeStamp::OrderClock(order_from_bytes(&bytes.to_le_bytes())),
        )
    }

    /// Record received message with specific timestamp
    pub fn record_received_with_timestamp(&mut self, bytes: u64, current_time: TimeStamp) {
        self.bytes_received += bytes;
        self.messages_received += 1;
        self.last_activity = current_time;
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for ConnectionMetrics {
    fn default() -> Self {
        Self::new()
    }
}
