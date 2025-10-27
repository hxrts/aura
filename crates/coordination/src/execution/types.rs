//! Core types for choreographic protocol architecture
//!
//! This module defines the foundational types for Aura's choreographic protocol system:
//! - Instruction: Instructions yielded by choreographic protocols
//! - Protocol results and error types
//! - Event filters and patterns
//!
//! Reference: work/04_declarative_protocol_evolution.md

use aura_journal::Event;
use aura_types::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

// ========== Protocol Results & Errors ==========

/// Result of a successfully completed protocol
#[derive(Debug, Clone)]
pub enum ProtocolResult {
    /// DKD protocol completed
    DkdComplete {
        session_id: Uuid,
        derived_key: Vec<u8>,
    },

    /// Resharing protocol completed
    ResharingComplete {
        session_id: Uuid,
        new_share: Vec<u8>,
    },

    /// Lock acquired
    LockAcquired { session_id: Uuid },

    /// Lock released
    LockReleased { session_id: Uuid },

    /// Recovery completed
    RecoveryComplete {
        recovery_id: Uuid,
        recovered_share: Vec<u8>,
    },
}

/// Error from a failed protocol
#[derive(Debug, Clone)]
pub struct ProtocolError {
    pub session_id: Uuid,
    pub error_type: ProtocolErrorType,
    pub message: String,
}

impl ProtocolError {
    /// Create a new protocol error with empty session ID
    pub fn new(message: String) -> Self {
        ProtocolError {
            session_id: Uuid::nil(),
            error_type: ProtocolErrorType::InvalidState,
            message,
        }
    }

    /// Create a new protocol error with session ID
    pub fn with_session(session_id: Uuid, error_type: ProtocolErrorType, message: String) -> Self {
        ProtocolError {
            session_id,
            error_type,
            message,
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Protocol error in session {}: {:?} - {}",
            self.session_id, self.error_type, self.message
        )
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Debug, Clone)]
pub enum ProtocolErrorType {
    Timeout,
    ByzantineBehavior,
    InsufficientParticipants,
    VerificationFailed,
    InvalidState,
    UnexpectedEvent,
    RecoveryVetoed,
    InvalidMerkleProof,
    InvalidSignature,
    CryptoError,
    Other,
}

// Error conversions
impl From<aura_crypto::CryptoError> for ProtocolError {
    fn from(err: aura_crypto::CryptoError) -> Self {
        ProtocolError {
            session_id: Uuid::nil(), // Will be set by caller if needed
            error_type: ProtocolErrorType::CryptoError,
            message: format!("Crypto error: {:?}", err),
        }
    }
}

/// Type of protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProtocolType {
    Dkd,
    Resharing,
    Locking,
    Recovery,
    Compaction,
}

// ========== Choreographic Protocol Instructions ==========

/// An instruction yielded by a protocol choreography.
///
/// The ProtocolContext executes this instruction and returns the result,
/// enabling protocols to wait for distributed events, write to the ledger,
/// and coordinate with other participants.
///
/// This enables writing protocols as linear, async choreographies that look
/// like single-threaded code but can wait for distributed events.
#[derive(Debug, Clone)]
pub enum Instruction {
    /// Write an event to the ledger and wait for it to be integrated
    WriteToLedger(Event),

    /// Wait for a single event that matches a filter
    AwaitEvent {
        filter: EventFilter,
        timeout_epochs: Option<u64>,
    },

    /// Wait for a threshold number of events that match a filter
    AwaitThreshold {
        count: usize,
        filter: EventFilter,
        timeout_epochs: Option<u64>,
    },

    /// Get the current state of the ledger
    GetLedgerState,

    /// Get the current Lamport clock value
    GetCurrentEpoch,

    /// Wait for a certain number of epochs to pass
    WaitEpochs(u64),

    /// Run a sub-protocol and wait for its result
    RunSubProtocol {
        protocol_type: ProtocolType,
        config: ProtocolConfig,
    },

    /// Check if an event exists without waiting
    CheckForEvent { filter: EventFilter },

    /// Mark guardian shares for deletion
    MarkGuardianSharesForDeletion {
        session_id: uuid::Uuid,
        ttl_hours: u64,
    },

    /// Check for session collision and determine winner via lottery
    CheckSessionCollision {
        operation_type: aura_journal::OperationType,
        context_id: Vec<u8>, // Unique identifier for the operation context
    },
}

/// Filter for matching ledger events
#[derive(Debug, Clone)]
pub struct EventFilter {
    /// Session ID to match (if any)
    pub session_id: Option<Uuid>,

    /// Event types to match
    pub event_types: Option<Vec<EventTypePattern>>,

    /// Author devices to match
    pub authors: Option<BTreeSet<DeviceId>>,

    /// Custom predicate (cannot be cloned, so we use an enum of known predicates)
    pub predicate: Option<EventPredicate>,
}

#[derive(Debug, Clone)]
pub enum EventTypePattern {
    DkdCommitment,
    DkdReveal,
    DkdFinalize,
    ResharingDistribute,
    ResharingAcknowledge,
    ResharingFinalize,
    LockRequest,
    LockGrant,
    LockRelease,
    InitiateRecovery,
    CollectGuardianApproval,
    SubmitRecoveryShare,
    CompleteRecovery,
    AbortRecovery,
    InitiateResharing,
    DistributeSubShare,
    AcknowledgeSubShare,
    FinalizeResharing,
    // ... etc
}

#[derive(Debug, Clone)]
pub enum EventPredicate {
    /// Author is in set of device IDs
    AuthorIn(BTreeSet<DeviceId>),

    /// Epoch is greater than value
    EpochGreaterThan(u64),

    /// Combination of predicates
    And(Box<EventPredicate>, Box<EventPredicate>),
    Or(Box<EventPredicate>, Box<EventPredicate>),
}

/// Configuration for starting a protocol
#[derive(Debug, Clone)]
pub enum ProtocolConfig {
    Dkd {
        participants: BTreeSet<DeviceId>,
        threshold: u16,
    },
    Resharing {
        new_participants: BTreeSet<DeviceId>,
        new_threshold: u16,
    },
    Locking {
        operation_type: String,
    },
    Recovery {
        guardians: BTreeSet<Uuid>,
        threshold: usize,
    },
}

/// Result returned from executing an instruction
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum InstructionResult {
    /// Event was written to ledger
    EventWritten,

    /// Single event was received
    EventReceived(Event),

    /// Multiple events were received (from AwaitThreshold)
    EventsReceived(Vec<Event>),

    /// Ledger state snapshot
    LedgerState(LedgerStateSnapshot),

    /// Current Lamport clock value
    CurrentEpoch(u64),

    /// Epochs have passed
    EpochsElapsed,

    /// Sub-protocol completed
    SubProtocolComplete(ProtocolResult),

    /// Session collision check result
    SessionStatus {
        existing_sessions: Vec<aura_journal::Session>,
        winner: Option<DeviceId>, // If collision exists, who won the lottery
    },
}

/// Snapshot of ledger state for instruction results
#[derive(Debug, Clone)]
pub struct LedgerStateSnapshot {
    pub account_id: AccountId,
    pub next_nonce: u64,
    pub last_event_hash: Option<[u8; 32]>,
    pub current_epoch: u64,
    pub relationship_counters:
        std::collections::BTreeMap<aura_journal::events::RelationshipId, (u64, u64)>,
}

/// Generate a deterministic test UUID for non-production use
fn generate_test_uuid() -> uuid::Uuid {
    // Use UUID v4 with a fixed seed for deterministic tests
    uuid::Uuid::from_bytes([
        0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd,
        0xef,
    ])
}
