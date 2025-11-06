//! Ratchet Tree Module
//!
//! This module implements a TreeKEM-inspired left-balanced binary tree (LBBT) for
//! identity membership management. The tree structure defines authentication (who you are)
//! separate from authorization (what you can do).
//!
//! ## Architecture
//!
//! - **Leaves**: Devices and Guardians with fixed semantics
//! - **Branches**: Carry policies (All, Any, Threshold{m,n})
//! - **Deterministic Indexing**: TreeKEM-style stable node indices
//! - **Commitments**: Blake3 hashes binding structure and content
//! - **Epochs**: Monotonically increasing version counter
//!
//! ## Key Concepts
//!
//! - **LBBT Property**: Tree maintains left-balanced structure across all mutations
//! - **Policy Inheritance**: Leaves inherit policy from ancestor branch path
//! - **Forward Secrecy**: Path rotation invalidates old secrets
//! - **Structural Integrity**: Commitments verify tree topology

pub mod commitment;
pub mod indexing;
pub mod node;
pub mod operations;
pub mod state;

// Re-export key types for convenience
pub use commitment::{Commitment, CommitmentTag};
pub use indexing::{LeafIndex, NodeIndex};
pub use node::{BranchNode, LeafId, LeafNode, LeafRole, Policy};
pub use operations::{AffectedPath, TreeOperation};
pub use state::RatchetTree;
