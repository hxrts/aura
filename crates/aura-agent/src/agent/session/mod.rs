//! Session state machine and protocol coordination
//!
//! This module was refactored from a monolithic 2,383-line session.rs file
//! into focused, maintainable submodules:
//!
//! - `states.rs` - Session state types and AgentProtocol struct (~150 lines)
//! - `bootstrap.rs` - Agent bootstrapping and FROST key generation (~300 lines)
//! - `identity.rs` - DKD identity derivation protocol (~250 lines)
//! - `storage_ops.rs` - Encrypted storage with capability controls (~600 lines)
//! - `coordination.rs` - Recovery and resharing protocols (~400 lines)
//! - `trait_impls.rs` - Agent trait implementations by state (~600 lines)
//!
//! ## Benefits of Refactoring
//!
//! - **Maintainability**: Each module has a single, clear responsibility
//! - **Testability**: Isolated functionality is easier to test
//! - **Code Quality**: Smaller files are easier to review and understand
//! - **Reusability**: Common patterns extracted to shared utilities

pub mod bootstrap;
pub mod coordination;
pub mod identity;
pub mod state_impls;
pub mod states;
pub mod storage_ops;
pub mod trait_impls;

// Re-export main types
pub use states::{
    AgentProtocol, BootstrapConfig, Coordinating, Failed, FailureInfo, Idle, ProtocolCompleted,
    ProtocolStatus, SessionState, Uninitialized,
};
