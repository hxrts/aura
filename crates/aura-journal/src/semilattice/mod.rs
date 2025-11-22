//! Journal CRDT implementations using harmonized architecture
//!
//! This module provides journal-specific CRDTs built on the harmonized
//! foundation from `aura-core`. All types implement the standard CRDT
//! traits and can participate in choreographic synchronization.

pub use account_state::{AccountState, GuardianRegistry, MaxCounter};
pub use concrete_types::{EpochLog, IntentPool};
pub use invitations::{InvitationRecord, InvitationRecordRegistry, InvitationStatus};
pub use meet_types::{
    ConsensusConstraint, DeviceCapability, ResourceQuota, SecurityPolicy, TimeWindow,
};
pub use op_log::{OpLog, OpLogSummary};
pub use relay_capability::{BudgetDecayPolicy, RelayCapability};
pub use types::{GCounter, GSet, LwwRegister, Replica};

pub mod account_state;
pub mod concrete_types;
pub mod invitations;
pub mod meet_types;
pub mod op_log;
pub mod relay_capability;
pub mod types;

// Re-export foundation types for convenience
pub use aura_core::semilattice::{
    Bottom, ConsistencyProof, ConstraintMsg, ConstraintScope, CvState, DeltaMsg, JoinSemilattice,
    MeetSemiLattice, MeetStateMsg, MsgKind, MvState, OpWithCtx, StateMsg, Top,
};
