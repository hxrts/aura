//! Protocol definitions and bootstrap
//!
//! This module contains protocol-related definitions:
//! - Event types: All 50+ event definitions for CRDT operations
//! - Bootstrap: Account initialization and genesis ceremony
//!
//! These define the vocabulary of operations available in the system.

pub mod bootstrap;
pub mod events;

// Re-export bootstrap types
pub use bootstrap::{
    AccountInitResult, BootstrapConfig, BootstrapManager, BootstrapResult, KeyShareData,
};

// Re-export event types
pub use events::*;
