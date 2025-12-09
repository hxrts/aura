//! # TUI Effects
//!
//! Connects the TUI to Aura's unified effect system via AppCore.
//!
//! ## Architecture
//!
//! All TUI operations flow through two dispatch paths:
//!
//! 1. **Intent Commands** (journaled): SendMessage, CreateChannel, etc.
//!    → Mapped via `command_to_intent()` → Dispatched through `AppCore.dispatch(Intent)`
//!
//! 2. **Operational Commands** (non-journaled): Ping, ForceSync, ListPeers, etc.
//!    → Handled by `OperationalHandler` → Updates signals directly
//!
//! This module provides:
//! - `EffectCommand`: Commands that can be dispatched
//! - `AuraEvent`: Events for demo mode inter-agent communication
//! - `CommandDispatcher`: Maps IRC commands to effect commands
//! - `OperationalHandler`: Handles non-journaled operational commands
//! - `command_to_intent`: Maps EffectCommand to Intent for AppCore dispatch

mod command_parser;
mod dispatcher;
mod intent_mapper;
mod operational;

// Re-export types from submodules
pub use command_parser::{
    AuraEvent, CommandAuthorizationLevel, EffectCommand, EventFilter, EventSubscription,
};
pub use dispatcher::{CommandDispatcher, DispatchError};
pub use intent_mapper::{command_to_intent, is_intent_command, CommandContext};
pub use operational::{OpError, OpResponse, OpResult, OperationalHandler};
