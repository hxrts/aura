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
//!
//! The implementation has been split for maintainability:
//! - `command_parser`: Command and event types, authorization levels
//! - `bridge_config`: Configuration types
//! - `effect_dispatch`: Command execution and authorization logic
//! - `bridge`: Core bridge implementation
//! - `dispatcher`: IRC command parsing

mod bridge;
mod bridge_config;
mod command_parser;
mod dispatcher;
mod effect_dispatch;

use async_trait::async_trait;

/// Bridge abstraction shared by production and simulator backends
#[async_trait]
pub trait Bridge: Send + Sync {
    /// Subscribe to events matching a filter (e.g., chat only)
    fn subscribe(&self, filter: bridge::EventFilter) -> bridge::EventSubscription;

    /// Subscribe to all events emitted by the backend
    fn subscribe_all(&self) -> bridge::EventSubscription;

    /// Emit an event directly to TUI subscribers
    fn emit(&self, event: bridge::AuraEvent);

    /// Dispatch a fire-and-forget effect command to the backend
    async fn dispatch(&self, command: bridge::EffectCommand) -> Result<(), String>;

    /// Dispatch a command and wait for completion/ack
    async fn dispatch_and_wait(&self, command: bridge::EffectCommand) -> Result<(), String>;

    /// Return true if the bridge considers itself connected to a backend
    async fn is_connected(&self) -> bool;

    /// Return the last backend error, if any
    async fn last_error(&self) -> Option<String>;
}

#[async_trait]
impl Bridge for bridge::EffectBridge {
    fn subscribe(&self, filter: bridge::EventFilter) -> bridge::EventSubscription {
        self.subscribe(filter)
    }

    fn subscribe_all(&self) -> bridge::EventSubscription {
        self.subscribe_all()
    }

    fn emit(&self, event: bridge::AuraEvent) {
        self.emit(event)
    }

    async fn dispatch(&self, command: bridge::EffectCommand) -> Result<(), String> {
        self.dispatch(command).await
    }

    async fn dispatch_and_wait(&self, command: bridge::EffectCommand) -> Result<(), String> {
        self.dispatch_and_wait(command).await
    }

    async fn is_connected(&self) -> bool {
        self.is_connected().await
    }

    async fn last_error(&self) -> Option<String> {
        self.last_error().await
    }
}

// Re-export types from submodules
pub use bridge::EffectBridge;
pub use bridge_config::BridgeConfig;
pub use command_parser::{
    AuraEvent, CommandAuthorizationLevel, EffectCommand, EventFilter, EventSubscription,
};
pub use dispatcher::{CommandDispatcher, DispatchError};
pub use effect_dispatch::check_authorization;
