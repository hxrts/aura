//! # Aura TUI - Terminal User Interface
//!
//! Layer 7 (User Interface) - IRC-style terminal interface for Aura.
//!
//! Built with iocraft for declarative, React-like UI components.
//!
//! ## Module Organization
//!
//! - **screens**: Full-screen views (Block, Chat, Recovery, etc.)
//! - **components**: Reusable UI widgets (Modal, Toast, CommandPalette)
//! - **context**: IoContext for effect dispatch and reactive data
//! - **theme**: Centralized color and style constants
//! - **types**: Shared domain types (Channel, Message, etc.)
//! - **hooks**: futures-signals integration for reactive state
//! - **reactive**: Reactive view layer (queries, views, signals)
//! - **effects**: Bridge to Aura effect system
//! - **commands**: IRC command parser

// Core iocraft modules
pub mod components;
pub mod context;
pub mod hooks;
pub mod screens;
pub mod theme;
pub mod types;

// Shared infrastructure
pub mod commands;
pub mod effects;
pub mod flow_budget;
pub mod local_store;
pub mod navigation;
pub mod reactive;
pub mod recovery_session;

// Re-export main types for convenience
pub use components::*;
pub use context::IoContext;
pub use hooks::{
    is_vec_empty, snapshot_state, snapshot_vec, vec_len, AppCoreContext, BlockSnapshot,
    ChatSnapshot, ContactsSnapshot, GuardiansSnapshot, HasReactiveData, InvitationsSnapshot,
    NeighborhoodSnapshot, ReactiveValue, RecoverySnapshot,
};
pub use screens::*;
pub use theme::{Spacing, Theme};
pub use types::*;

// Re-export effect bridge types
pub use effects::{AuraEvent, BridgeConfig, EffectBridge, EffectCommand, EventFilter};

// Re-export commands types
pub use commands::{
    all_command_help, command_help, is_command, parse_command, CommandCapability, CommandCategory,
    CommandHelp, IrcCommand, ParseError,
};

// Re-export flow budget types
pub use flow_budget::{
    example_budget_table, BlockFlowBudget, BudgetBreakdown, BudgetError, FlowBudgetView,
    BLOCK_TOTAL_SIZE, KB, MAX_NEIGHBORHOODS, MAX_RESIDENTS, MB, NEIGHBORHOOD_DONATION,
    RESIDENT_ALLOCATION,
};

// Re-export local store types
pub use local_store::{derive_key_material, TuiLocalStore};

// Re-export reactive types
pub use reactive::{
    ChannelType, ChannelsQuery, GuardianApproval, GuardianStatus, GuardiansQuery,
    InvitationDirection, InvitationStatus, InvitationType, InvitationsQuery, MessagesQuery,
    RecoveryQuery, RecoveryState, TuiQuery,
};

// Re-export navigation types
pub use navigation::{
    is_nav_key_press, navigate_grid, navigate_list, NavKey, NavThrottle, ThreePanelFocus,
    TwoPanelFocus, NAV_THROTTLE_MS,
};
