//! Layer 2: Journal Algebra (Semilattice) Types
//!
//! Domain-specific CRDT types built on the aura-core semilattice foundation (Layer 1).
//! All types implement JoinSemilattice (⊔) or MeetSemiLattice (⊓) for eventual consistency.
//!
//! **CRDT Types**:
//! - **AccountState (G-Set)**: Device membership, monotonically growing
//! - **EpochLog**: Monotonic epoch counter and key rotation history
//! - **OpLog (OR-Set)**: Operation log as idempotent set of attested operations
//! - **InvitationRegistry**: Pending invitations with TTL constraints
//! - **GuardianRegistry**: Guardian set membership tracking
//! - **Capability Constraints (Meet-Lattice)**: Meet-semilattice (⊓) policies for resource access
//!
//! **Convergence Invariant** (per docs/110_state_reduction.md):
//! All synchronization operations are idempotent and commutative,
//! enabling deterministic reduction across all replicas.

pub use account_state::{AccountState, GuardianRegistry, MaxCounter};
pub use concrete_types::{EpochLog, IntentPool};
pub use invitations::{InvitationRecord, InvitationRecordRegistry, InvitationStatus};
pub use meet_types::{
    ConsensusConstraint, DeviceCapability, ResourceQuota, SecurityPolicy, TimeWindow,
};
pub use op_log::{OpLog, OpLogSummary};
pub use types::{GCounter, GSet, LwwRegister, Replica};

pub mod account_state;
pub mod concrete_types;
pub mod invitations;
pub mod meet_types;
pub mod op_log;
pub mod types;

// Re-export foundation types for convenience
pub use aura_core::semilattice::{
    Bottom, ConsistencyProof, ConstraintMsg, ConstraintScope, CvState, DeltaMsg, JoinSemilattice,
    MeetSemiLattice, MeetStateMsg, MsgKind, MvState, OpWithCtx, StateMsg, Top,
};
