//! Tree choreography protocols for distributed tree mutations
//!
//! This module implements TreeSession choreographies that coordinate tree operations
//! across multiple devices using the Intent Pool pattern for lock-free coordination.
//!
//! ## Architecture
//!
//! - **TreeSession**: Coordinator-free intent execution with CAS-style snapshot validation
//! - **Prepare/ACK/NACK**: Snapshot validation phase using propose_and_acknowledge pattern
//! - **Share Exchange**: Byzantine-safe share collection using broadcast_and_gather
//! - **Tree Mutations**: AddLeaf, RemoveLeaf, RotatePath choreographies
//! - **Recovery**: Guardian-based recovery ceremony with temporary capabilities
//!
//! ## Pattern Composition
//!
//! All TreeSessions compose the fundamental choreographic patterns:
//! - `propose_and_acknowledge` for initialization and CAS validation
//! - `broadcast_and_gather` for share exchange with commit-reveal
//! - `threshold_collect` for M-of-N coordination
//! - `verify_consistent_result` for Byzantine-safe result verification
//!
//! ## References
//!
//! - `work/tree_revision.md §8` - Concurrency and consistency model
//! - `work/tree_revision.md §13` - Explicit identity flows
//! - `docs/401_session_type_algebra.md` - Session type foundations

pub mod add_leaf;
pub mod prepare_ack;
pub mod recovery;
pub mod remove_leaf;
pub mod rotate_path;
pub mod session;

pub use add_leaf::{AddLeafChoreography, AddLeafConfig, PathShare, PathShareBundle};
pub use prepare_ack::{PrepareAckConfig, PrepareAckResult, PreparePhase, PrepareProposal};
pub use recovery::{
    DeviceRekeySession, GuardianRecoverySession, RecoveryConfig, RefreshPolicyChoreography,
};
pub use remove_leaf::{RemoveLeafChoreography, RemoveLeafConfig};
pub use rotate_path::{RotatePathChoreography, RotatePathConfig};
pub use session::{
    rank_intents, IntentRank, TreeSession, TreeSessionConfig, TreeSessionError,
    TreeSessionLifecycle,
};
