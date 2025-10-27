//! Network topology data structures.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete network topology snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkTopology {
    pub nodes: HashMap<String, NodeInfo>,
    pub edges: Vec<NetworkEdge>,
    pub partitions: Vec<PartitionInfo>,
}

/// Information about a node in the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub device_id: String,
    pub participant_type: super::trace::ParticipantType,
    pub status: super::trace::ParticipantStatus,
    pub message_count: u64,
}

/// Edge representing communication between nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkEdge {
    pub from: String,
    pub to: String,
    pub message_count: u64,
    pub last_message_tick: Option<u64>,
}

/// Information about a network partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    pub devices: Vec<String>,
    pub created_at_tick: u64,
}

/// Participant information (re-exported from trace module).
pub use super::trace::ParticipantInfo;
