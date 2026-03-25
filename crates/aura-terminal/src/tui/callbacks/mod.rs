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
//! use crate::tui::callbacks::{CallbackRegistry, IdCallback};
//!
//! // Create all callbacks at once
//! let registry = CallbackRegistry::new(ctx, tx);
//!
//! // Use individual callbacks
//! let on_run_slash_command = registry.chat.on_run_slash_command.clone();
//! ```

mod factories;
mod types;

// Re-export types
pub(crate) use types::SlashCommandCallback;
#[doc(hidden)]
pub use types::{AddDeviceCallback, CreateChannelCallback};
pub use types::{
    ApprovalCallback, ChannelSelectCallback, ExportInvitationCallback, GoHomeCallback, IdCallback,
    InvitationCallback, NoArgCallback, RecoveryCallback, RemoveDeviceCallback,
    StringOptStringCallback, ThreeStringCallback, ThresholdCallback, TwoStringCallback,
};
pub(crate) use types::{
    RetryMessageCallback, SetTopicCallback, StartChatCallback, UpdateMfaCallback,
    UpdateNicknameCallback, UpdateNicknameSuggestionCallback, UpdateThresholdCallback,
};

// Re-export factories
pub use factories::{
    AppCallbacks, CallbackRegistry, ChatCallbacks, ContactsCallbacks, InvitationsCallbacks,
    NeighborhoodCallbacks, RecoveryCallbacks, SettingsCallbacks,
};
