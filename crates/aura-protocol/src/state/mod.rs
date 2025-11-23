//! Layer 4: CRDT State Types - Semilattice Coordination
//!
//! Distributed state management using CRDT semilattices (⊔, ⊓) for conflict-free replication
//! and eventual consistency (per docs/002_theoretical_model.md, docs/110_state_reduction.md).
//!
//! **Mathematical Foundation** (per docs/002_theoretical_model.md §3):
//! All types satisfy required semilattice properties for convergence:
//! - **Associativity**: `(A ⊔ B) ⊔ C = A ⊔ (B ⊔ C)`
//! - **Commutativity**: `A ⊔ B = B ⊔ A`
//! - **Idempotency**: `A ⊔ A = A`
//!
//! These properties ensure merge operations can be applied in any order;
//! all replicas converge regardless of network delays or message reordering.
//!
//! **State Models**:
//! - **PeerView (G-Set)**: Peer discovery (grow-only set, ⊔ = union)
//! - **IntentState (LWW)**: Operation proposals (last-write-wins with timestamp resolution)
//!
//! **Critical Invariant: Idempotency** (per docs/110_state_reduction.md):
//! Applying same state update twice must have identical effects. Idempotency violation
//! introduces inconsistency across replicas and violates eventual consistency guarantee.
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
