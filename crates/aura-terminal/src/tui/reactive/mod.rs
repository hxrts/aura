//! # TUI Reactive Module
//!
//! Reactive data bindings for the TUI. This module provides:
//! - View state types for TUI-specific presentation
//! - Integration with aura-app queries via ReactiveEffects
//!
//! ## Architecture Note
//!
//! Query types are defined in `aura_app::queries` and implement `aura_core::Query`.
//! View types (Channel, Message, Guardian, etc.) come from `aura_app::views`.
//! FRP primitives (Dynamic<T>) and journal integration are in `aura-agent::reactive`.
//!
//! The reactive pipeline is:
//! - AppCore signals emit updates when facts change
//! - TUI components subscribe via `use_future` and `ReactiveEffects::subscribe`
//! - Views render from aura_app view types
//!
//! ## Integration Points
//!
//! - `aura_app::queries::*` - Portable Datalog queries
//! - `aura_app::views::*` - Canonical view types (Channel, Message, Guardian, etc.)
//! - `aura_core::effects::reactive::ReactiveEffects` - Signal subscription
//! - `crate::tui::context::IoContext` - Effect dispatch

// TUI-specific reactive modules
pub mod journal_bridge; // Re-exports from aura_agent::reactive (see module docs)
pub mod views; // View state with TUI-specific presentation types

// Re-export shared FRP primitives from aura-agent (runtime layer)
pub use aura_agent::reactive::Dynamic;
pub use aura_agent::reactive::ReactiveScheduler;

// Re-export view types from aura-app for convenience
pub use aura_app::views::chat::{Channel, ChannelType, Message};
pub use aura_app::views::contacts::Contact;
pub use aura_app::views::invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType,
};
pub use aura_app::views::recovery::{
    Guardian, GuardianStatus, RecoveryApproval as GuardianApproval,
    RecoveryProcessStatus as RecoveryStatus, RecoveryState,
};

// Re-export query types from aura-app for convenience
pub use aura_app::queries::{
    ChannelsQuery, ContactsQuery, GuardiansQuery, InvitationsQuery, MessagesQuery, RecoveryQuery,
};

// Re-export TUI-specific view types
pub use views::{
    BlockAdjacency, BlockInfo, MySuggestion, NeighborhoodBlock, Resident, ResidentRole,
    StorageInfo, SuggestionPolicy, ThresholdConfig, TraversalDepth, TraversalPosition, ViewState,
};
