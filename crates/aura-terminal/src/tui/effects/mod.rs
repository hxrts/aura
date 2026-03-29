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
//! - `OperationalHandler`: Handles non-journaled operational commands

mod command_parser;
mod operational;

// Re-export types from submodules
pub use command_parser::{
    AuraEvent, CommandAuthorizationLevel, EffectCommand, EventFilter, EventSubscription,
    ThresholdConfig,
};
pub use operational::{OpError, OpFailureCode, OpResponse, OpResult, OperationalHandler};
