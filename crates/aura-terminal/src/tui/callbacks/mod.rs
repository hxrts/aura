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
//! let registry = CallbackRegistry::new(ctx, tx, app_core);
//!
//! // Use individual callbacks
//! let on_send = registry.chat.on_send.clone();
//! ```

mod factories;
mod types;

// Re-export types
pub(crate) use types::SendCallback;
#[doc(hidden)]
pub use types::{AddDeviceCallback, CreateChannelCallback};
pub use types::{
    ApprovalCallback, ChannelSelectCallback, CreateHomeCallback, CreateNeighborhoodCallback,
    ExportInvitationCallback, GoHomeCallback, GuardianSelectCallback, IdCallback,
    InvitationCallback, NeighborhoodHomeCallback, NoArgCallback, RecoveryCallback,
    RemoveDeviceCallback, RetryMessageCallback, SetModeratorCallback, SetTopicCallback,
    StartChatCallback, StringOptStringCallback, ThreeStringCallback, ThresholdCallback,
    TwoStringCallback, UpdateNicknameCallback, UpdateNicknameSuggestionCallback,
    UpdateThresholdCallback,
};

// Re-export factories
pub use factories::{
    AppCallbacks, CallbackRegistry, ChatCallbacks, ContactsCallbacks, InvitationsCallbacks,
    NeighborhoodCallbacks, RecoveryCallbacks, SettingsCallbacks,
};
