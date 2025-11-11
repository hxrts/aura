//! Choreographic Protocol Implementations
//!
//! This module contains choreographic protocol implementations using rumpsteak-aura.
//! Choreographies are global protocol specifications that automatically project to
//! local session types for each participant.
//!
//! ## Architecture
//!
//! - **aura_handler_adapter**: Integration between rumpsteak session types and Aura's effect system
//! - **protocols**: Concrete protocol implementations (FROST, snapshot, tree coordination, etc.)
//!
//! ## Usage
//!
//! Choreographies use the `AuraHandlerAdapter` to bridge between session-typed protocols
//! and the Aura effect system, enabling implementation-agnostic choreographic execution.

pub mod aura_handler_adapter;
pub mod protocols;

// Re-export commonly used types
pub use aura_handler_adapter::{AuraHandlerAdapter, SendGuardProfile};
