//! # TUI Effect Bridge
//!
//! Connects the TUI to Aura's effect system, enabling reactive updates
//! from backend state changes and command dispatch to the effect system.
//!
//! This module provides:
//! - `EffectBridge`: Main connection between TUI and effects
//! - `EffectCommand`: Commands that can be dispatched to the backend
//! - `AuraEvent`: Events emitted by the effect system for TUI consumption
//! - `CommandDispatcher`: Maps IRC commands to effect commands

mod bridge;
mod dispatcher;

pub use bridge::{
    AuraEvent, BridgeConfig, EffectBridge, EffectCommand, EventFilter, EventSubscription,
};
pub use dispatcher::{CommandDispatcher, DispatchError};
