//! CRDT (Conflict-free Replicated Data Type) foundations for Aura
//!
//! This module provides CRDT implementations that integrate with Aura's session type
//! algebra and effect system. CRDTs enable automatic conflict resolution and eventual
//! consistency across distributed replicas.
//!
//! ## Architecture (see docs/402_crdt_types.md)
//!
//! Aura expresses CRDT replication protocols using multi-party session types:
//!
//! - **CvRDT (State-based)**: Exchange full states, merge via join semilattice
//! - **Δ-CRDT (Delta-based)**: Exchange deltas, accumulate and fold into state
//! - **CmRDT (Operation-based)**: Broadcast operations with causal delivery
//!
//! ## CRDT Categories
//!
//! ### State-Based CRDTs (CvRDT)
//! - `GCounter`: Grow-only counter (increment-only)
//! - `GSet`: Grow-only set (add-only)
//! - Session protocol: `CvSync<S> := μX . ! StateMsg<S> . ? StateMsg<S> . X`
//!
//! ### Operation-Based CRDTs (CmRDT)
//! - `LwwRegister`: Last-writer-wins register
//! - Session protocol: `OpBroadcast<Op, Ctx> := μX . (r → * : OpWithCtx<Op, Ctx> . X)`
//!
//! ## Usage Pattern
//!
//! ```rust
//! use crate::crdt::{JoinSemilattice, CvState, GCounter};
//!
//! // State-based CRDT (CvRDT)
//! let mut counter1 = GCounter::new();
//! counter1.increment("replica1".into(), 5);
//!
//! let mut counter2 = GCounter::new();
//! counter2.increment("replica2".into(), 3);
//!
//! // Merge states (join operation)
//! counter1.merge(&counter2);
//! assert_eq!(counter1.value(), 8); // 5 + 3
//! ```
//!
//! ## Integration with Session Types
//!
//! CRDTs are expressed as typed message payloads (`T`) in Aura's session algebra:
//!
//! ```rust,ignore
//! // Global session type for CvRDT anti-entropy
//! CvSync<S> := μX . (A → B : StateMsg<S> . X) ∥ (B → A : StateMsg<S> . X)
//!
//! // Handler enforces semilattice law: on receive s' => state := state.join(&s')
//! ```
//!
//! See docs/402_crdt_types.md for complete specification.

pub mod traits;
pub mod types;

// Re-export core CRDT traits
pub use traits::{
    Bottom,
    CausalOp,
    CmApply,
    CrdtOperation,
    // Legacy traits (being phased out)
    CrdtState,
    CrdtValue,
    CvState,
    Dedup,
    Delta,
    DeltaProduce,
    // Modern CRDT traits (aligned with 402_crdt_types.md)
    JoinSemilattice,
};

// Re-export CRDT types and implementations
pub use types::{
    CrdtError,
    // Concrete CRDT implementations
    GCounter,
    GSet,
    LwwRegister,
    Replica,
};

/// Message type for state-based CRDT synchronization
///
/// Carries full state with optional tag for protocol tracking.
#[derive(Clone, Debug)]
pub struct StateMsg<S> {
    /// The CRDT state payload
    pub payload: S,
    /// Message kind tag
    pub kind: MsgKind,
}

/// Message type for delta-based CRDT synchronization
///
/// Carries incremental delta with optional tag.
#[derive(Clone, Debug)]
pub struct DeltaMsg<D> {
    /// The delta payload
    pub payload: D,
    /// Message kind tag
    pub kind: MsgKind,
}

/// Operation with causal context for CmRDT
///
/// Operations carry their causal dependencies to enable
/// proper ordering during delivery.
#[derive(Clone, Debug)]
pub struct OpWithCtx<Op, Ctx> {
    /// The CRDT operation
    pub op: Op,
    /// Causal context (vector clock or dependency set)
    pub ctx: Ctx,
}

/// Message kind tags for protocol clarity
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MsgKind {
    /// Full state message for CvRDT synchronization
    FullState,
    /// Delta message for incremental Δ-CRDT synchronization
    Delta,
    /// Operation message for CmRDT causal broadcast
    Op,
}

/// Digest of operation IDs for repair protocols
pub type Digest<Id> = Vec<Id>;

/// Missing operations response for repair
pub type Missing<Op> = Vec<Op>;

// Implement JoinSemilattice for common types

impl JoinSemilattice for u64 {
    fn join(&self, other: &Self) -> Self {
        (*self).max(*other)
    }
}

impl Bottom for u64 {
    fn bottom() -> Self {
        0
    }
}

impl CvState for u64 {}

impl<T: JoinSemilattice> JoinSemilattice for Vec<T> {
    fn join(&self, other: &Self) -> Self {
        self.iter()
            .zip(other.iter())
            .map(|(a, b)| a.join(b))
            .collect()
    }
}

impl<T: Bottom> Bottom for Vec<T> {
    fn bottom() -> Self {
        Vec::new()
    }
}

impl<T: CvState> CvState for Vec<T> {}
