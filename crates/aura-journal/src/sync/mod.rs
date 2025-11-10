//! Anti-Entropy CRDT Synchronization
//!
//! This module implements anti-entropy protocols for CRDT synchronization
//! using the existing OpLog OR-set infrastructure. It provides efficient
//! state comparison and incremental synchronization for multi-device coordination.
//!
//! ## Key Components:
//!
//! - `OpLogSynchronizer`: Core synchronization service
//! - `SyncProtocol`: Anti-entropy protocol implementation
//! - `PeerSync`: Per-peer synchronization state management
//! - `SyncScheduler`: Periodic and triggered synchronization

pub mod peer_sync;
pub mod protocol;
pub mod scheduler;
pub mod synchronizer;

pub use peer_sync::{PeerSyncError, PeerSyncManager, PeerSyncState};
pub use protocol::{ProtocolError, SyncMessage, SyncProtocol, SyncState};
pub use scheduler::{SchedulerConfig, SchedulerError, SyncScheduler};
pub use synchronizer::{OpLogSynchronizer, SyncConfiguration, SyncError, SyncResult};

/// Re-export key types for convenience
pub use super::semilattice::op_log::{OpLog, OpLogSummary};
pub use aura_core::{AttestedOp, DeviceId, Hash32};
// Temporary stubs for transport types until aura-transport integration is available
/// Information about a peer device in the network
#[derive(Debug, Clone, PartialEq)]
pub struct PeerInfo {
    /// The device ID of the peer
    pub device_id: DeviceId,
    /// Timestamp of when the peer was last seen
    pub last_seen: u64,
    /// Quality of the connection to this peer (0.0-1.0)
    pub connection_quality: f64,
    /// Performance metrics for the peer connection
    pub metrics: PeerMetrics,
}

/// Performance metrics for a peer connection
#[derive(Debug, Clone, PartialEq)]
pub struct PeerMetrics {
    /// Latency to the peer in milliseconds
    pub latency_ms: u64,
    /// Bandwidth to the peer in bits per second
    pub bandwidth_bps: u64,
    /// Overall reliability score (0.0-1.0)
    pub reliability: f64,
    /// Numeric reliability score
    pub reliability_score: u32,
    /// Average latency measured in milliseconds
    pub average_latency_ms: u64,
}

/// Criteria for selecting peers for synchronization
#[derive(Debug, Clone)]
pub enum SelectionCriteria {
    /// Select peer with highest reliability score
    HighestReliability,
    /// Select peer with lowest latency
    LowestLatency,
    /// Select peer at random
    Random,
}

impl SelectionCriteria {
    /// Check if a peer matches the selection criteria
    pub fn matches(&self, _peer: &PeerInfo, _current_time: u64) -> bool {
        // TODO fix - For now, all peers match all criteria
        // In production, implement proper filtering based on criteria
        true
    }
}
