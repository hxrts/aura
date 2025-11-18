//! Ratchet Tree - New Implementation
//!
//! This module implements the ratchet tree specification from:
//! - docs/123_ratchet_tree.md
//! - docs/123_tree_sync.md
//!
//! ## Architecture (from spec)
//!
//! ```text
//! OpLog (CRDT OR-set of AttestedOp) ─────┐
//!                                         │
//!                                         ↓ reduce()
//!                                    TreeState
//!                                    (derived, materialized on-demand)
//!                                    - epoch: u64
//!                                    - root_commitment: Hash32
//!                                    - nodes: BTreeMap<NodeIndex, Node>
//!                                    - leaves: BTreeMap<LeafId, LeafNode>
//! ```
//!
//! ## Key Principles:
//!
//! 1. **Journal stores only AttestedOp** - no shares, no transcripts
//! 2. **TreeState is derived** - computed on-demand via reduction, never stored
//! 3. **OpLog is OR-set CRDT** - join-based append-only log
//! 4. **Deterministic reduction** - DAG with topological sort and H(op) tie-breaker
//!
//! ## CRITICAL INVARIANTS:
//!
//! - TreeState is **NEVER** stored in the journal
//! - OpLog is the **ONLY** persisted tree data
//! - Reduction is **DETERMINISTIC** across all replicas
//! - Same OpLog always produces same TreeState

/// Ratchet tree application and verification
pub mod application;
/// Ratchet tree compaction and garbage collection
pub mod compaction;
/// Ratchet tree operation processing
pub mod operations;
/// Ratchet tree state reduction from operations
pub mod reduction;
/// Ratchet tree state representation
pub mod state;
/// Tree types (re-exported from aura-core during transition)
pub mod tree_types;

pub use application::{
    apply_verified, apply_verified_sync, validate_invariants, ApplicationError, ApplicationResult,
};
pub use compaction::{compact, CompactionError}; // TODO: Fix verify_join_preserving, verify_retraction exports
pub use operations::{
    BatchProcessor, OperationProcessorError, ProcessedOperation, ProcessingStats,
    TreeOperationProcessor, TreeStateQuery,
};
pub use reduction::{reduce, ReductionError};
pub use state::{TreeState, TreeStateError};
pub use tree_types::*;
