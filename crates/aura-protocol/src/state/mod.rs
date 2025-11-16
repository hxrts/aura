//! CRDT State Types for Distributed Coordination
//!
//! This module contains CRDT (Conflict-free Replicated Data Type) state implementations
//! for distributed coordination protocols. All types implement semilattice operations
//! to guarantee eventual consistency across replicas.
//!
//! ## Architecture
//!
//! CRDT state types provide the foundation for distributed coordination:
//!
//! ```text
//! PeerView (G-Set) ─────┐
//!                       ├── Anti-Entropy Protocol
//! IntentState (LWW) ────┘
//! ```
//!
//! ## CRDT Types
//!
//! - **PeerView**: Grow-only set (G-Set) of discovered peers
//!   - Implements join-semilattice: `A ⊔ B` = union of peer sets
//!   - Monotonic: peers can be added but never removed
//!   - Used for peer discovery and membership tracking
//!
//! - **IntentState**: Last-Writer-Wins (LWW) register for operation proposals
//!   - Implements join-semilattice with timestamp ordering
//!   - Resolves conflicts by preferring most recent updates
//!   - Used for coordinated operation scheduling
//!
//! ## Semilattice Properties
//!
//! All CRDT types satisfy the mathematical requirements for convergence:
//!
//! - **Associativity**: `(A ⊔ B) ⊔ C = A ⊔ (B ⊔ C)`
//! - **Commutativity**: `A ⊔ B = B ⊔ A`
//! - **Idempotency**: `A ⊔ A = A`
//!
//! This ensures that merge operations can be applied in any order and
//! all replicas will eventually converge to the same state.

pub mod intent_state;
pub mod peer_view;

pub use intent_state::IntentState;
pub use peer_view::PeerView;
