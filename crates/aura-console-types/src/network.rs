//! Network topology data structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete network topology snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTopology {
    /// Nodes in the network, keyed by device ID.
    pub nodes: HashMap<String, NodeInfo>,
    /// Communication edges between nodes.
    pub edges: Vec<NetworkEdge>,
    /// Active network partitions.
    pub partitions: Vec<PartitionInfo>,
}

/// Information about a node in the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Device identifier.
    pub device_id: String,
    /// Type of participant.
    pub participant_type: super::trace::ParticipantType,
    /// Current network status.
    pub status: super::trace::ParticipantStatus,
    /// Number of messages processed.
    pub message_count: u64,
}

/// Edge representing communication between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEdge {
    /// Source device.
    pub from: String,
    /// Destination device.
    pub to: String,
    /// Number of messages transmitted.
    pub message_count: u64,
    /// Last tick when a message was sent on this edge.
    pub last_message_tick: Option<u64>,
}

/// Information about a network partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    /// Devices isolated in the partition.
    pub devices: Vec<String>,
    /// Simulation tick when partition was created.
    pub created_at_tick: u64,
}

/// Participant information (re-exported from trace module).
pub use super::trace::ParticipantInfo;
