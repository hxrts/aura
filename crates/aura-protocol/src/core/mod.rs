//! Core protocol abstractions
//!
//! This module contains the core protocol types and abstractions that were
//! previously in the `protocol-core` crate. This includes protocol capabilities,
//! lifecycle management, metadata types, and protocol typestate.

pub mod capabilities;
pub mod lifecycle;
pub mod metadata;
pub mod typestate;

// Re-export the main types for convenience
pub use capabilities::{ProtocolCapabilities, ProtocolEffects};
pub use lifecycle::{
    ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolRehydration, ProtocolStep,
};
pub use metadata::{OperationType, ProtocolDuration, ProtocolMode, ProtocolPriority, ProtocolType};
pub use typestate::{AnyProtocolState, SessionState, SessionStateTransition, StateWitness};
