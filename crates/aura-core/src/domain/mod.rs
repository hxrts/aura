//! Domain-Specific Logic Types
//!
//! Core types that implement domain-specific semantics: consensus prestates,
//! journal CRDTs, content addressing, temporal database operations, and
//! consistency metadata.
//!
//! **Layer 1**: Type definitions and interfaces. Implementations live in domain crates.
//!
//! # Consistency Metadata
//!
//! The consistency subsystem provides types for tracking the agreement, propagation,
//! and acknowledgment status of facts:
//!
//! - [`Agreement`]: A1/A2/A3 finalization levels (Provisional, SoftSafe, Finalized)
//! - [`Propagation`]: Anti-entropy sync status (Local, Syncing, Complete, Failed)
//! - [`Acknowledgment`]: Per-peer delivery confirmation
//! - [`Consistency`]: Unified metadata combining all dimensions
//! - [`OperationCategory`]: Category A/B/C classification
//!
//! Category-specific status types:
//!
//! - [`OptimisticStatus`]: For Category A operations (immediate effect)
//! - [`DeferredStatus`]: For Category B operations (requires approval)
//! - [`CeremonyStatus`]: For Category C operations (blocking ceremony)

pub mod acknowledgment;
pub mod agreement;
pub mod consensus;
pub mod consistency;
pub mod content;
pub mod journal;
pub mod propagation;
pub mod status;
pub mod temporal;

// Re-export all public types for convenience

// Consensus
pub use consensus::{Prestate, PrestateBuilder};

// Content addressing
pub use content::{ChunkId, ContentId, ContentSize, Hash32};

// Journal
pub use journal::{
    ActorId, AuthLevel, Cap, Fact, FactKey, FactOpId, FactTimestamp, FactValue, Journal,
};

// Temporal database
pub use temporal::{
    AnchorProof, ContentFinalityOverride, FactContent, FactOp, FactReceipt, Finality,
    FinalityError, RetractReason, ScopeFinalityConfig, ScopeId, ScopeParseError, ScopeSegment,
    TemporalPoint, TemporalQuery, Transaction, TransactionId, TransactionMetadata,
    TransactionReceipt,
};

// Consistency metadata (A1/A2/A3 taxonomy)
pub use acknowledgment::{AckRecord, Acknowledgment};
pub use agreement::{Agreement, ConvergenceCert};
pub use consistency::{Consistency, ConsistencyMap, OperationCategory, ProposalId};
pub use propagation::Propagation;

// Category-specific status types
pub use status::{
    ApprovalDecision, ApprovalProgress, ApprovalRecord, ApprovalThreshold, CeremonyResponse,
    CeremonyState, CeremonyStatus, DeferredStatus, OptimisticStatus, ParticipantResponse,
    ProposalState, SupersessionReason,
};
