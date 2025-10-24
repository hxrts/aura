//! Core types for the simulation engine

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a simulated participant
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ParticipantId(pub Uuid);

impl ParticipantId {
    /// Create a new participant ID using injected effects for randomness
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        ParticipantId(effects.gen_uuid())
    }

    /// Create a deterministic participant ID from a name
    ///
    /// Uses UUIDv5 to generate a consistent ID for the same name.
    pub fn from_name(name: &str) -> Self {
        // Use a deterministic UUID based on the name
        ParticipantId(Uuid::new_v5(&Uuid::NAMESPACE_OID, name.as_bytes()))
    }
}

impl fmt::Display for ParticipantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", &self.0.to_string()[..8])
    }
}

impl Default for ParticipantId {
    fn default() -> Self {
        // Note: Default implementation uses non-deterministic UUID
        // Prefer using new_with_effects() for deterministic behavior in simulations
        #[allow(clippy::disallowed_methods)]
        Self(Uuid::new_v4())
    }
}

/// Logical tick in the simulation timeline
///
/// The simulation advances in discrete ticks. Each tick represents a quantum of logical time.
pub type Tick = u64;

/// Message envelope for transport between participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    /// Unique message ID
    pub message_id: Uuid,
    /// Sender participant
    pub sender: ParticipantId,
    /// Recipients (may be empty for broadcast)
    pub recipients: Vec<ParticipantId>,
    /// Serialized payload
    pub payload: Vec<u8>,
    /// Delivery semantics
    pub delivery: DeliverySemantics,
}

/// Delivery semantics for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeliverySemantics {
    /// Best-effort unicast (may be dropped by network)
    Unicast,
    /// Reliable unicast (retries until delivered)
    ReliableUnicast,
    /// Best-effort broadcast to all participants
    Broadcast,
    /// Multicast to specific set of recipients
    Multicast,
}

/// Side effect produced by a participant
///
/// Effects represent all observable actions a participant can take.
/// The simulation runtime intercepts and processes these effects.
#[derive(Debug, Clone)]
pub enum Effect {
    /// Send a message over the network
    Send(Envelope),

    /// Write an event to the local ledger
    WriteToLocalLedger {
        /// Participant writing the event
        participant: ParticipantId,
        /// Serialized event data to write
        event_data: Vec<u8>,
    },

    /// Request to read from local storage
    ReadFromStorage {
        /// Participant requesting the read
        participant: ParticipantId,
        /// Storage key to read
        key: Vec<u8>,
    },

    /// Request to write to local storage
    WriteToStorage {
        /// Participant requesting the write
        participant: ParticipantId,
        /// Storage key to write to
        key: Vec<u8>,
        /// Value to store
        value: Vec<u8>,
    },

    /// Log a message (for debugging)
    Log {
        /// Participant generating the log
        participant: ParticipantId,
        /// Log level for the message
        level: LogLevel,
        /// Log message content
        message: String,
    },
}

/// Log level for debugging effects
///
/// Standard log levels for simulation debugging and analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Detailed tracing information
    Trace,
    /// Debug information for development
    Debug,
    /// General information messages
    Info,
    /// Warning messages for potential issues
    Warn,
    /// Error messages for failures
    Error,
}

/// Context provided to effect interceptors
///
/// This allows interceptors to make decisions based on the current protocol state.
#[derive(Debug, Clone)]
pub struct EffectContext {
    /// The current logical tick
    pub tick: Tick,
    /// The participant producing the effect
    pub sender: ParticipantId,
    /// Recipients of the effect (for Send effects)
    pub recipients: Vec<ParticipantId>,
    /// Protocol operation being performed (if known)
    pub operation: Option<Operation>,
}

/// High-level protocol operation
///
/// This enum tags effects with their protocol context, enabling interceptors
/// to target specific protocol phases for Byzantine testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// DKD protocol - commitment phase
    DkdCommitment,
    /// DKD protocol - reveal phase
    DkdReveal,
    /// DKD protocol - aggregation phase
    DkdAggregation,

    /// Resharing protocol - sub-share distribution
    ResharingDistribution,
    /// Resharing protocol - share reconstruction
    ResharingReconstruction,
    /// Resharing protocol - verification
    ResharingVerification,

    /// Recovery protocol - initiation
    RecoveryInitiation,
    /// Recovery protocol - guardian approval
    RecoveryApproval,
    /// Recovery protocol - share reconstruction
    RecoveryReconstruction,

    /// Generic operation (unknown context)
    Generic,
}

impl EffectContext {
    /// Check if this context matches a specific operation
    pub fn matches(&self, op: Operation) -> bool {
        self.operation == Some(op)
    }
}
