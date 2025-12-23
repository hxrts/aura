//! # TUI Effects
//!
//! Connects the TUI to Aura's unified effect system via AppCore.
//!
//! ## Architecture
//!
//! All TUI operations flow through a single runtime-backed dispatch path:
//! **Operational Commands** handled by `OperationalHandler`.
//!
//! This module provides:
//! - `EffectCommand`: Commands that can be dispatched
//! - `AuraEvent`: Events for demo mode inter-agent communication
//! - `CommandDispatcher`: Maps IRC commands to effect commands
//! - `OperationalHandler`: Handles non-journaled operational commands

mod command_parser;
mod dispatcher;
mod operational;

// Re-export types from submodules
pub use command_parser::{
    AuraEvent, CommandAuthorizationLevel, EffectCommand, EventFilter, EventSubscription,
};
pub use dispatcher::{CapabilityPolicy, CommandDispatcher, DispatchError};
pub use operational::{OpError, OpResponse, OpResult, OperationalHandler};
