//! Message types for session-type CRDT protocols
//!
//! These are ordinary `T` in local session types; handlers add semantics via traits.

use serde::{Deserialize, Serialize};

/// Message kind tags for runtime clarity (optional)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MsgKind {
    /// Full state message
    FullState,
    /// Delta state message
    Delta,
    /// Operation message
    Op,
}

/// State message for CvRDT anti-entropy
pub type StateMsg<S> = (S, MsgKind);

/// Delta message for Δ-CRDT gossip
pub type DeltaMsg<D> = (D, MsgKind);

/// Operation with causal context for CmRDT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpWithCtx<Op, Ctx> {
    /// The operation to apply
    pub op: Op,
    /// Causal context (vector clock, dependencies)
    pub ctx: Ctx,
}

/// Digest of operation IDs for repair
pub type Digest<Id> = Vec<Id>;

/// Missing operations response for repair
pub type Missing<Op> = Vec<Op>;