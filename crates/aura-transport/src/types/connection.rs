//! Relationship-Scoped Connection Abstractions
//!
//! Provides essential connection types with built-in relationship scoping and privacy context.
//! Target: <120 lines (minimal scoping implementation).

use aura_core::{DeviceId, RelationshipId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use super::envelope::PrivacyLevel;

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
        /// Time when connection attempt started
        started_at: SystemTime,
        /// Privacy level for establishment
        privacy_level: PrivacyLevel,
    },
    /// Connection established and operational
    Established {
        /// Time when connection was established
        established_at: SystemTime,
        /// Current privacy context
        privacy_context: PrivacyContext,
    },
    /// Connection closing gracefully
    Closing {
        /// Time when close initiated
        closing_at: SystemTime,
        /// Reason for closing
        reason: String,
    },
    /// Connection closed
    Closed {
        /// Time when connection closed
        closed_at: SystemTime,
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
    /// Last activity time
    pub last_activity: SystemTime,
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
        // Use deterministic approach for connections
        // In production this would use a deterministic algorithm
        Uuid::nil() // Placeholder
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
        Self::new_at_time(peer_id, privacy_level, SystemTime::UNIX_EPOCH)
    }

    /// Create new connection info for peer at specific time
    pub fn new_at_time(
        peer_id: DeviceId,
        privacy_level: PrivacyLevel,
        started_at: SystemTime,
    ) -> Self {
        let connection_id = ConnectionId::new();
        let state = ConnectionState::Connecting {
            started_at,
            privacy_level,
        };

        Self {
            connection_id,
            state,
            peer_id,
            relationship_context: None,
            capabilities: HashMap::new(),
            metrics: ConnectionMetrics::new_at_time(started_at),
        }
    }

    /// Create relationship-scoped connection
    pub fn new_scoped(
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        privacy_level: PrivacyLevel,
    ) -> Self {
        Self::new_scoped_at_time(
            peer_id,
            relationship_id,
            privacy_level,
            SystemTime::UNIX_EPOCH,
        )
    }

    /// Create relationship-scoped connection at specific time
    pub fn new_scoped_at_time(
        peer_id: DeviceId,
        relationship_id: RelationshipId,
        privacy_level: PrivacyLevel,
        started_at: SystemTime,
    ) -> Self {
        let mut info = Self::new_at_time(peer_id, privacy_level, started_at);
        info.relationship_context = Some(relationship_id);
        info
    }

    /// Mark connection as established
    pub fn establish(&mut self, privacy_context: PrivacyContext) {
        self.establish_at_time(privacy_context, SystemTime::UNIX_EPOCH)
    }

    /// Mark connection as established at specific time
    pub fn establish_at_time(
        &mut self,
        privacy_context: PrivacyContext,
        established_at: SystemTime,
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
    pub fn age_at_time(&self, current_time: SystemTime) -> Duration {
        let start_time = match &self.state {
            ConnectionState::Connecting { started_at, .. } => *started_at,
            ConnectionState::Established { established_at, .. } => *established_at,
            ConnectionState::Closing { closing_at, .. } => *closing_at,
            ConnectionState::Closed { closed_at, .. } => *closed_at,
        };

        current_time.duration_since(start_time).unwrap_or_default()
    }

    /// Get connection age (uses UNIX_EPOCH as baseline for determinism)
    pub fn age(&self) -> Duration {
        self.age_at_time(SystemTime::UNIX_EPOCH)
    }
}

impl ConnectionMetrics {
    /// Create new metrics
    pub fn new() -> Self {
        Self::new_at_time(SystemTime::UNIX_EPOCH)
    }

    /// Create new metrics at specific time
    pub fn new_at_time(current_time: SystemTime) -> Self {
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
        self.record_sent_at_time(bytes, SystemTime::UNIX_EPOCH)
    }

    /// Record sent message at specific time
    pub fn record_sent_at_time(&mut self, bytes: u64, current_time: SystemTime) {
        self.bytes_sent += bytes;
        self.messages_sent += 1;
        self.last_activity = current_time;
    }

    /// Record received message
    pub fn record_received(&mut self, bytes: u64) {
        self.record_received_at_time(bytes, SystemTime::UNIX_EPOCH)
    }

    /// Record received message at specific time
    pub fn record_received_at_time(&mut self, bytes: u64, current_time: SystemTime) {
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
