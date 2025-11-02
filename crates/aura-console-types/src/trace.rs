//! Trace data structures for recording simulation and live network events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete simulation trace including metadata, timeline, and topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationTrace {
    pub metadata: TraceMetadata,
    pub timeline: Vec<TraceEvent>,
    pub checkpoints: Vec<CheckpointRef>,
    pub participants: HashMap<String, ParticipantInfo>,
    pub network_topology: super::network::NetworkTopology,
}

/// Metadata about the trace (scenario name, seed, violations, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    pub scenario_name: String,
    pub seed: u64,
    pub total_ticks: u64,
    pub properties_checked: Vec<String>,
    pub violations: Vec<PropertyViolation>,
}

/// A single event in the simulation timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub tick: u64,
    pub event_id: u64,
    pub event_type: EventType,
    pub participant: String,
    pub causality: CausalityInfo,
}

/// The type of event that occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventType {
    ProtocolStateTransition {
        protocol: String,
        from_state: String,
        to_state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        witness_data: Option<Vec<u8>>,
    },
    MessageSent {
        envelope_id: String,
        to: Vec<String>,
        message_type: String,
        size_bytes: usize,
    },
    MessageReceived {
        envelope_id: String,
        from: String,
        message_type: String,
    },
    MessageDropped {
        envelope_id: String,
        reason: DropReason,
    },
    EffectExecuted {
        effect_type: String,
        effect_data: Vec<u8>,
    },
    CrdtMerge {
        from_replica: String,
        heads_before: Vec<String>,
        heads_after: Vec<String>,
    },
    CheckpointCreated {
        checkpoint_id: String,
        label: String,
    },
    PropertyViolation {
        property: String,
        violation_details: String,
    },
}

/// Reason a message was dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    NetworkPartition,
    InvalidSignature,
    ExpiredEpoch,
    RateLimited,
    Other(String),
}

/// Causality information for an event (happens-before relationships).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityInfo {
    pub parent_events: Vec<u64>,
    pub happens_before: Vec<u64>,
    pub concurrent_with: Vec<u64>,
}

/// Reference to a checkpoint in the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRef {
    pub id: String,
    pub label: String,
    pub tick: u64,
}

/// Participant information for a device in the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInfo {
    pub device_id: String,
    pub participant_type: ParticipantType,
    pub status: ParticipantStatus,
}

/// Type of participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantType {
    Honest,
    Byzantine,
    Offline,
}

/// Current status of a participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantStatus {
    Online,
    Offline,
    Partitioned,
}

/// A property violation detected during simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    pub tick: u64,
    pub property: String,
    pub participant: String,
    pub details: String,
}
