//! Commitment Tree Core Types
//!
//! This module defines the foundational data types for Aura's commitment tree with
//! threshold signing, following the specification in `docs/123_commitment_tree.md`.
//!
//! # Design Principles
//!
//! - **Pure Foundation Types**: No business logic, only data structures
//! - **Meet-Semilattice Policy**: Policies form a partial order where "more restrictive is smaller"
//! - **Content-Addressed Operations**: All operations are identified by their hash (CID)
//! - **Deterministic Commitments**: Tree commitments are reproducible from structure
//!
//! # Key Types
//!
//! - [`Epoch`]: Monotonically increasing epoch counter for key rotation
//! - [`TreeHash32`]: 32-byte cryptographic hash for commitments
//! - [`Policy`]: Meet-semilattice threshold policy (Any, Threshold, All)
//! - [`LeafNode`]: Device or guardian leaf in the tree
//! - [`BranchNode`]: Internal tree node with policy and commitment
//! - [`TreeOp`]: Parent-bound tree modification operation
//! - [`AttestedOp`]: Tree operation with threshold signature attestation
//!
//! # Reference
//!
//! See [`docs/123_commitment_tree.md`](../../../docs/123_commitment_tree.md) for complete specification.

pub mod commitment;
pub mod policy;
/// Snapshot and cut-based pruning of operation history
pub mod snapshot;
pub mod types;

pub use commitment::*;
pub use policy::Policy;
pub use snapshot::*;
pub use types::*;
