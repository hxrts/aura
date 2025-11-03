//! Trace data structures for recording simulation and live network events.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Complete simulation trace including metadata, timeline, and topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationTrace {
    /// Metadata about the trace.
    pub metadata: TraceMetadata,
    /// Ordered timeline of events.
    pub timeline: Vec<TraceEvent>,
    /// Saved checkpoints within the trace.
    pub checkpoints: Vec<CheckpointRef>,
    /// Information about each participant.
    pub participants: HashMap<String, ParticipantInfo>,
    /// Network topology at the time of the trace.
    pub network_topology: super::network::NetworkTopology,
}

/// Metadata about the trace (scenario name, seed, violations, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetadata {
    /// Name of the scenario that produced this trace.
    pub scenario_name: String,
    /// Random seed used in the simulation.
    pub seed: u64,
    /// Total number of simulation ticks.
    pub total_ticks: u64,
    /// Properties that were checked during the trace.
    pub properties_checked: Vec<String>,
    /// Any property violations discovered.
    pub violations: Vec<PropertyViolation>,
}

/// A single event in the simulation timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    /// Simulation tick at which the event occurred.
    pub tick: u64,
    /// Unique event identifier.
    pub event_id: u64,
    /// The type of event.
    pub event_type: EventType,
    /// Participant that generated the event.
    pub participant: String,
    /// Causality information about the event.
    pub causality: CausalityInfo,
}

/// The type of event that occurred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventType {
    /// Protocol state machine transitioned.
    ProtocolStateTransition {
        /// Protocol name.
        protocol: String,
        /// Previous state.
        from_state: String,
        /// New state.
        to_state: String,
        /// Optional witness data proving the transition.
        #[serde(skip_serializing_if = "Option::is_none")]
        witness_data: Option<Vec<u8>>,
    },
    /// Message was sent.
    MessageSent {
        /// Unique message envelope ID.
        envelope_id: String,
        /// Recipients.
        to: Vec<String>,
        /// Type of message.
        message_type: String,
        /// Message size in bytes.
        size_bytes: usize,
    },
    /// Message was received.
    MessageReceived {
        /// Unique message envelope ID.
        envelope_id: String,
        /// Sender.
        from: String,
        /// Type of message.
        message_type: String,
    },
    /// Message was dropped.
    MessageDropped {
        /// Unique message envelope ID.
        envelope_id: String,
        /// Reason the message was dropped.
        reason: DropReason,
    },
    /// Effect was executed.
    EffectExecuted {
        /// Type of effect.
        effect_type: String,
        /// Effect data.
        effect_data: Vec<u8>,
    },
    /// CRDT merge occurred.
    CrdtMerge {
        /// Source replica.
        from_replica: String,
        /// Heads before merge.
        heads_before: Vec<String>,
        /// Heads after merge.
        heads_after: Vec<String>,
    },
    /// Checkpoint was created.
    CheckpointCreated {
        /// Checkpoint identifier.
        checkpoint_id: String,
        /// Checkpoint label.
        label: String,
    },
    /// Property violation detected.
    PropertyViolation {
        /// Property name.
        property: String,
        /// Details about the violation.
        violation_details: String,
    },
}

/// Reason a message was dropped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// Dropped due to network partition.
    NetworkPartition,
    /// Dropped due to invalid signature.
    InvalidSignature,
    /// Dropped because epoch was expired.
    ExpiredEpoch,
    /// Dropped due to rate limiting.
    RateLimited,
    /// Dropped for other reason.
    Other(String),
}

/// Causality information for an event (happens-before relationships).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalityInfo {
    /// Events that directly caused this event.
    pub parent_events: Vec<u64>,
    /// All events that happened before this event.
    pub happens_before: Vec<u64>,
    /// Events concurrent with this event.
    pub concurrent_with: Vec<u64>,
}

/// Reference to a checkpoint in the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointRef {
    /// Checkpoint identifier.
    pub id: String,
    /// User-provided checkpoint label.
    pub label: String,
    /// Simulation tick of the checkpoint.
    pub tick: u64,
}

/// Participant information for a device in the simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantInfo {
    /// Device identifier.
    pub device_id: String,
    /// Type of participant.
    pub participant_type: ParticipantType,
    /// Current status.
    pub status: ParticipantStatus,
}

/// Type of participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantType {
    /// Honest participant following protocol correctly.
    Honest,
    /// Byzantine participant with faulty behavior.
    Byzantine,
    /// Offline participant not available.
    Offline,
}

/// Current status of a participant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantStatus {
    /// Participant is online and reachable.
    Online,
    /// Participant is offline.
    Offline,
    /// Participant is partitioned from the network.
    Partitioned,
}

/// A property violation detected during simulation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// Simulation tick when violation occurred.
    pub tick: u64,
    /// Property name that was violated.
    pub property: String,
    /// Participant involved in the violation.
    pub participant: String,
    /// Detailed description of the violation.
    pub details: String,
}
