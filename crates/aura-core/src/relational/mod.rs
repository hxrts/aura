//! Layer 1: Relational Domain Types
//!
//! Core types for cross-authority coordination: **GuardianBinding** (threshold recovery),
//! **RecoveryGrant** (recovery delegation), **RelationalFact** (multi-authority agreement),
//! **ConsensusProof** (distributed agreement witness), **RecoveryOp** (recovery procedures).
//!
//! **Pure Data Layer**: No protocol logic. Protocol implementations live in:
//! - aura-recovery (Layer 5): Guardian recovery coordination
//! - aura-relational (Layer 5): Relational context state machines
//! - aura-protocol/consensus (Layer 4): Multi-party agreement
//!
//! **Design Principle**: Relational facts enable facts to reference cross-authority context
//! (per docs/103_relational_contexts.md), enabling distributed accountability.

pub mod consensus;
pub mod fact;
pub mod guardian;
pub mod recovery;

// Re-export all public types for convenience
pub use consensus::{ConsensusProof, ConsensusStatus};
pub use fact::{GenericBinding, RelationalFact};
pub use guardian::{GuardianBinding, GuardianBindingBuilder, GuardianParameters};
pub use recovery::{RecoveryGrant, RecoveryOp, RecoverySeverity};
