//! Domain-Specific Logic Types
//!
//! Core types that implement domain-specific semantics: consensus prestates,
//! journal CRDTs, content addressing, and temporal database operations.
//!
//! **Layer 1**: Type definitions and interfaces. Implementations live in domain crates.

pub mod consensus;
pub mod content;
pub mod journal;
pub mod temporal;

// Re-export all public types for convenience
pub use consensus::{Prestate, PrestateBuilder};
pub use content::{ChunkId, ContentId, ContentSize, Hash32};
pub use journal::{
    ActorId, AuthLevel, Cap, Fact, FactKey, FactOpId, FactTimestamp, FactValue, Journal,
};
pub use temporal::{
    AnchorProof, ContentFinalityOverride, FactContent, FactOp, FactReceipt, Finality,
    FinalityError, RetractReason, ScopeFinalityConfig, ScopeId, ScopeParseError, ScopeSegment,
    TemporalPoint, TemporalQuery, Transaction, TransactionId, TransactionMetadata,
    TransactionReceipt,
};
