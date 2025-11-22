//! Relational domain types for cross-authority coordination
//!
//! This module contains the core domain types for managing relationships
//! and coordination between authorities in the Aura system. These types
//! are pure data structures without protocol logic.

pub mod consensus;
pub mod fact;
pub mod guardian;
pub mod recovery;

// Re-export all public types for convenience
pub use consensus::{ConsensusProof, ConsensusStatus};
pub use fact::{GenericBinding, RelationalFact};
pub use guardian::{GuardianBinding, GuardianBindingBuilder, GuardianParameters};
pub use recovery::{RecoveryGrant, RecoveryOp, RecoverySeverity};