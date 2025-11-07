//! Session-Type-Based CRDT Choreographies
//!
//! This module implements Aura's session-type approach to CRDT conflict resolution
//! as described in docs/402_crdt_types.md. It provides choreographic protocols using
//! the rumpsteak-aura DSL for expressing CRDT communication patterns.
//!
//! ## Architecture
//!
//! - **Protocols**: Choreographic protocols using `choreography!` macro
//! - **Composition**: Execution utilities bridging session types with effect handlers
//! - **Foundation Integration**: Uses types and handlers from foundation layers
//!
//! ## Usage Pattern
//!
//! ```rust
//! use aura_choreography::semilattice::execute_cv_sync;
//! use aura_protocol::effects::semilattice::CvHandler;
//!
//! let mut handler = CvHandler::<JournalMap>::new();
//! execute_cv_sync(adapter, replicas, my_role, &mut handler).await?;
//! ```

pub mod protocols;
pub mod composition;
pub mod meet_protocols;

// Re-export foundation types
pub use aura_types::semilattice::{
    StateMsg, DeltaMsg, OpWithCtx, MsgKind, Digest, Missing,
    JoinSemilattice, Bottom, CvState, CausalOp, CmApply, Dedup, Delta, DeltaProduce,
    MeetStateMsg, ConstraintMsg, ConsistencyProof, ConstraintScope,
    MeetSemiLattice, Top, MvState,
};

// Re-export effect handlers  
pub use aura_protocol::effects::semilattice::{
    CvHandler, CmHandler, DeltaHandler, HandlerFactory,
    DeliveryEffect, TopicId, GossipStrategy,
};

// Re-export meet-based effect handlers
pub use aura_protocol::effects::semilattice::{
    MvHandler, ConstraintEvent, ConstraintResult, MultiConstraintHandler,
};

// Re-export choreographic protocols and execution
pub use protocols::*;
pub use composition::*;
pub use meet_protocols::*;
