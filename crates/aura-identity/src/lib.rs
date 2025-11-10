//! Aura Identity Management
//!
//! This crate provides identity management and tree operation choreographies
//! for the Aura threshold identity platform.
//!
//! # Architecture
//!
//! This crate implements application-level identity management:
//! - `tree_ops/` - Choreographic tree operations (G_tree_op)
//! - `verification/` - Identity verification and validation
//! - `handlers/` - Application-level tree operation handlers
//!
//! # Design Principles
//!
//! - Uses choreographic programming for distributed tree operations
//! - Integrates with the Aura MPST framework for capability guards and journal coupling
//! - Provides clean separation between foundation (aura-core) and application logic

#![warn(missing_docs)]
#![forbid(unsafe_code)]

/// Tree operation choreographies
pub mod tree_ops;

/// Identity verification and validation
pub mod verification;

/// Application-level tree operation handlers
pub mod handlers;

/// Errors for identity operations
// errors module removed - use aura_core::AuraError directly

// Re-export core types
pub use aura_core::{
    tree::{AttestedOp, BranchNode, Epoch, LeafNode, Policy, TreeOp},
    AccountId, Cap, DeviceId, Hash32, Journal,
};

// Re-export MPST types
pub use aura_mpst::{
    AuraRuntime, CapabilityGuard, ExecutionContext, JournalAnnotation, MpstError, MpstResult,
};

// Re-export choreography implementations
pub use tree_ops::{TreeOpChoreography, TreeOpMessage, TreeOpRole};

// Error re-exports removed - use aura_core::AuraError directly
