//! # Callbacks Module
//!
//! Centralized callback type aliases and factory functions for TUI screens.
//!
//! ## Organization
//!
//! - `types`: Base callback type aliases (NoArgCallback, IdCallback, etc.)
//! - `factories`: Factory structs that create domain-specific callbacks
//!
//! ## Usage
//!
//! ```rust,ignore
//! use crate::tui::callbacks::{CallbackRegistry, SendCallback, IdCallback};
//!
//! // Create all callbacks at once
//! let registry = CallbackRegistry::new(ctx, tx, app_core);
//!
//! // Use individual callbacks
//! let on_send = registry.chat.on_send.clone();
//! ```

mod factories;
mod types;

// Re-export types
pub use types::*;

// Re-export factories
pub use factories::{
    AppCallbacks, BlockCallbacks, CallbackRegistry, ChatCallbacks, ContactsCallbacks,
    InvitationsCallbacks, NeighborhoodCallbacks, RecoveryCallbacks, SettingsCallbacks,
};
