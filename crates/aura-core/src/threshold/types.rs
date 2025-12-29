//! Shared lifecycle types for threshold coordination and upgrades.

use crate::{AuthorityId, ContextId, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Coordinator convergence certificate for soft-safe operations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ConvergenceCert {
    /// Relational context containing this operation.
    pub context: ContextId,
    /// Operation identifier.
    pub op_id: Hash32,
    /// Prestate hash bound into the operation.
    pub prestate_hash: Hash32,
    /// Monotonic coordinator epoch (fencing token).
    pub coord_epoch: u64,
    /// Optional acknowledger set for quorum-based convergence.
    pub ack_set: Option<BTreeSet<AuthorityId>>,
    /// Time/sequence window used to declare convergence.
    pub window: u64,
}

/// Explicit reversion fact for soft-safe operations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReversionFact {
    /// Relational context containing this operation.
    pub context: ContextId,
    /// Reverted operation identifier.
    pub op_id: Hash32,
    /// Winning operation identifier.
    pub winner_op_id: Hash32,
    /// Coordinator epoch in which reversion was observed.
    pub coord_epoch: u64,
}

/// Rotation/upgrade marker for lifecycle transitions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RotateFact {
    /// Relational context containing this transition.
    pub context: ContextId,
    /// Previous lifecycle state hash.
    pub from_state: Hash32,
    /// Next lifecycle state hash.
    pub to_state: Hash32,
    /// Prestate hash bound into the transition.
    pub prestate_hash: Hash32,
    /// Opaque reason for the rotation.
    pub reason: String,
}
