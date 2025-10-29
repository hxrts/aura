//! Capability-based access control
//!
//! This module provides the core capability system that allows fine-grained
//! access control through delegatable tokens.

pub mod chain;
pub mod delegation;
pub mod scope;
pub mod token;

pub use chain::{verify_capability_chain, CapabilityChain};
pub use delegation::{delegate_capability, CapabilityDelegation};
pub use scope::{CapabilityScope, PermissionLevel};
pub use token::CapabilityToken;
