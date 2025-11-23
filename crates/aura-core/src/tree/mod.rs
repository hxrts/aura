//! Layer 1: Commitment Tree Core Types
//!
//! Foundational data types for threshold tree with FROST signing. All types are
//! pure structures with no business logic.
//!
//! **Key Types**:
//! - **Epoch**: Monotonically increasing epoch counter for key rotation (per docs/110_rendezvous.md)
//! - **Policy**: Meet-semilattice (⊓) threshold policy (Any, Threshold, All);
//!   more restrictive is smaller per docs/002_theoretical_model.md
//! - **LeafNode**: Device or guardian leaf in tree
//! - **BranchNode**: Internal node with policy and content-addressed commitment
//! - **TreeOp**: Parent-bound tree modification operation
//! - **AttestedOp**: TreeOp with threshold signature proof (fact-based journal)
//!
//! **Design Principles**:
//! - Content-addressed: Operations identified by content hash (CID)
//! - Deterministic: Reproducible commitments from structure
//! - Policy semilattice: Refinement forms partial order (per docs/104_consensus.md)
//! - Fact-based: AttestedOps are immutable atomic facts (per docs/102_journal.md)

pub mod commitment;
pub mod policy;
/// Snapshot and cut-based pruning of operation history
pub mod snapshot;
pub mod types;

pub use commitment::*;
pub use policy::Policy;
pub use snapshot::*;
pub use types::*;
