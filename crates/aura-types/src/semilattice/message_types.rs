//! Message types for session-type CRDT protocols
//!
//! These types serve as precise payloads (`T`) in session type communication.
//! They wrap CRDT-specific data with metadata for protocol clarity.

use crate::identifiers::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Message kind for protocol clarity
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MsgKind {
    /// Full state message for CvRDT anti-entropy
    FullState,
    /// Delta message for incremental synchronization
    Delta,
    /// Operation message for CmRDT broadcast
    Op,
    /// Constraint message for meet semi-lattice protocols
    Constraint,
    /// Consistency proof for constraint synchronization
    ConsistencyProof,
}

/// State message for CvRDT anti-entropy protocols
///
/// Carries full CRDT state for synchronization between replicas.
/// Used in session types as `StateMsg<S>` where `S: CvState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMsg<S> {
    /// The CRDT state payload
    pub payload: S,
    /// Message type tag
    pub kind: MsgKind,
}

impl<S> StateMsg<S> {
    /// Create a new state message
    pub fn new(payload: S) -> Self {
        Self {
            payload,
            kind: MsgKind::FullState,
        }
    }
}

/// Delta message for Δ-CRDT gossip protocols
///
/// Carries incremental updates for bandwidth-optimized synchronization.
/// Used in session types as `DeltaMsg<D>` where `D: Delta`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaMsg<D> {
    /// The delta payload
    pub payload: D,
    /// Message type tag
    pub kind: MsgKind,
}

impl<D> DeltaMsg<D> {
    /// Create a new delta message
    pub fn new(payload: D) -> Self {
        Self {
            payload,
            kind: MsgKind::Delta,
        }
    }
}

/// Operation with causal context for CmRDT protocols
///
/// Carries operations with their causal context for proper ordering.
/// Used in session types as `OpWithCtx<Op, Ctx>` where `Op: CausalOp<Ctx=Ctx>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpWithCtx<Op, Ctx> {
    /// The operation
    pub op: Op,
    /// Causal context (vector clock, dependencies, etc.)
    pub ctx: Ctx,
}

impl<Op, Ctx> OpWithCtx<Op, Ctx> {
    /// Create a new operation with context message
    pub fn new(op: Op, ctx: Ctx) -> Self {
        Self { op, ctx }
    }
}

/// Digest of operation IDs for repair protocols
///
/// Used in repair choreographies to exchange information about
/// which operations each replica has seen.
pub type Digest<Id> = Vec<Id>;

/// Missing operations response for repair protocols
///
/// Contains operations that one replica has but another is missing,
/// sent in response to a digest exchange.
pub type Missing<Op> = Vec<Op>;

// === Meet Semi-Lattice Message Types ===

/// Meet-based state synchronization message
///
/// Carries meet semi-lattice state for constraint synchronization between replicas.
/// Used in session types as `MeetStateMsg<S>` where `S: MvState`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeetStateMsg<S> {
    /// The meet semi-lattice state payload
    pub payload: S,
    /// Message type tag
    pub kind: MsgKind,
    /// Monotonic counter ensuring proper ordering
    pub monotonic_counter: u64,
}

impl<S> MeetStateMsg<S> {
    /// Create a new meet state message
    pub fn new(payload: S, counter: u64) -> Self {
        Self {
            payload,
            kind: MsgKind::FullState,
            monotonic_counter: counter,
        }
    }
}

/// Meet-based constraint message for policy intersection
///
/// Carries constraints that will be intersected through meet operations.
/// Used for capability restriction, security policy intersection, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintMsg<C> {
    /// The constraint payload
    pub constraint: C,
    /// Scope of the constraint application
    pub scope: ConstraintScope,
    /// Priority for constraint resolution
    pub priority: u32,
    /// Message type tag
    pub kind: MsgKind,
}

impl<C> ConstraintMsg<C> {
    /// Create a new constraint message
    pub fn new(constraint: C, scope: ConstraintScope, priority: u32) -> Self {
        Self {
            constraint,
            scope,
            priority,
            kind: MsgKind::Constraint,
        }
    }
}

/// Constraint scope for targeted application
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Hash)]
pub enum ConstraintScope {
    /// Global constraint affecting all participants
    Global,
    /// Session-specific constraint
    Session(SessionId),
    /// Device-specific constraint
    Device(DeviceId),
    /// Resource-specific constraint
    Resource(String),
}

/// Consistency proof message for constraint verification
///
/// Used to verify that all participants have computed the same
/// constraint intersection result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyProof {
    /// Hash of the computed constraint intersection
    pub constraint_hash: [u8; 32],
    /// Participant identifier
    pub participant: DeviceId,
    /// Proof generation timestamp
    pub timestamp: u64,
    /// Message type tag
    pub kind: MsgKind,
}

impl ConsistencyProof {
    /// Create a new consistency proof
    pub fn new(constraint_hash: [u8; 32], participant: DeviceId, timestamp: u64) -> Self {
        Self {
            constraint_hash,
            participant,
            timestamp,
            kind: MsgKind::ConsistencyProof,
        }
    }
}
