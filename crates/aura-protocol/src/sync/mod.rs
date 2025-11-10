//! Tree Synchronization - Distributed Programs for CRDT Convergence
//!
//! This module implements the synchronization layer for tree operations using
//! CRDT semilattices and anti-entropy protocols. All sync programs are built
//! on join-semilattice composition for guaranteed convergence.
//!
//! ## Architecture
//!
//! ```text
//! OpLog (OR-set) ←─────┐
//! PeerView (G-set) ←───┼─ Anti-Entropy
//! IntentState (LWW) ←──┘    Protocol
//! ```
//!
//! ## Key Components
//!
//! - **PeerView**: Grow-only set of discovered peers
//! - **IntentState**: Typestate lattice for operation proposals
//! - **Anti-Entropy**: Digest-based OpLog reconciliation
//!
//! ## Design Principles
//!
//! - **Join-Semilattice Composition**: All state uses ⊔ (join) operation
//! - **Monotonic Growth**: State only increases, never decreases
//! - **Eventual Consistency**: All replicas converge to same state
//! - **No Rollbacks**: Forward-only state transitions

pub mod intent_state;
pub mod peer_view;

pub use intent_state::IntentState;
pub use peer_view::PeerView;
