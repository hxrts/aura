//! Layer 2: Commitment Tree - CRDT Tree State Machine
//!
//! CRDT-based implementation: **OpLog** (fact-based operation log) → **deterministic reduction** → **TreeState**.
//! Enables threshold key management, device membership, and authority state coordination.
//!
//! **Key Invariants** (per docs/102_journal.md):
//! - **OpLog is authoritative**: Only persisted data; immutable facts (AttestedOp)
//! - **Deterministic reduction**: Same OpLog always produces identical TreeState across all replicas
//! - **Monotonic growth**: OpLog append-only; facts never retracted
//! - **Content-addressed**: Each fact identified by CID (content hash)
//!
//! **Architecture**: Separation of concerns:
//! - `application`: Validate & apply operations to tree state
//! - `reduction`: Deterministic state derivation from OpLog
//! - `compaction`: Garbage collection and snapshotting

/// Commitment tree application and verification
pub mod application;
/// AttestedOp converter for fact-based journal
pub mod attested_ops;
/// Authority-internal tree state
pub mod authority_state;
/// Commitment tree compaction and garbage collection
pub mod compaction;
/// Local device types for authority-internal use
pub mod local_types;
/// Commitment tree operation processing
pub mod operations;
/// Commitment tree state reduction from operations
pub mod reduction;
/// Commitment tree state representation
pub mod state;
/// Shared storage helpers for tree op persistence
pub mod storage;

pub use application::{
    apply_verified, apply_verified_sync, validate_invariants, ApplicationError, ApplicationResult,
};
pub use compaction::{compact, CompactionError};
pub use operations::{
    BatchProcessor, OperationProcessorError, ProcessedOperation, ProcessingStats,
    TreeOperationProcessor, TreeStateQuery,
};
pub use reduction::{reduce, ReductionError};
pub use state::{TreeState, TreeStateError};
