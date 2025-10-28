//! Protocol core library providing unified abstractions for Aura choreographies.
//!
//! This crate consolidates the legacy `session-types` and `aura-interfaces`
//! crates into a single home for protocol typestate, capability traits, and
//! shared metadata used across orchestration layers.

#![allow(clippy::result_large_err)]

pub mod capabilities;
pub mod lifecycle;
pub mod metadata;
pub mod typestate;

pub use capabilities::{ProtocolCapabilities, ProtocolEffects};
pub use lifecycle::{
    ProtocolDescriptor, ProtocolInput, ProtocolLifecycle, ProtocolRehydration, ProtocolStep,
};
pub use metadata::{OperationType, ProtocolDuration, ProtocolMode, ProtocolPriority, ProtocolType};
pub use typestate::{AnyProtocolState, SessionState, SessionStateTransition, StateWitness};
