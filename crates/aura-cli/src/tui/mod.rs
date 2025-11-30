//! # Aura TUI - Terminal User Interface
//!
//! ## Architecture
//!
//! The TUI is organized into several layers:
//!
//! - **Styles**: Centralized theming and color management
//! - **Input**: Modal input handling (Normal, Editing, Command modes)
//! - **Components**: Reusable, trait-based UI components
//! - **Effects**: Bridge to the Aura effect system
//! - **Screens**: Full-screen views composed of components
//! - **App**: Main application orchestrating screens and state

pub mod app;
pub mod commands;
pub mod components;
pub mod context;
pub mod demo;
pub mod effects;
pub mod flow_budget;
pub mod input;
pub mod reactive;
pub mod screens;
pub mod styles;

// Local store integration for TUI preferences
pub mod local_store;

// Re-export main app type
pub use app::TuiApp;

// Re-export DemoEvent for backward compatibility (deprecated)
#[allow(deprecated)]
pub use app::DemoEvent;

// Re-export component infrastructure
pub use components::{Component, FocusDirection, FocusManager, Focusable};
pub use context::TuiContext;
pub use effects::{AuraEvent, BridgeConfig, EffectBridge, EffectCommand, EventFilter};
pub use input::{Command, DefaultInputHandler, InputAction, InputHandler, InputMode};
pub use styles::{ColorPalette, Styles, ToastLevel};

// Re-export reactive types
pub use reactive::{
    Channel, ChannelType, ChannelsQuery, Guardian, GuardianApproval, GuardianStatus,
    GuardiansQuery, Invitation, InvitationDirection, InvitationStatus, InvitationType,
    InvitationsQuery, Message, MessagesQuery, RecoveryQuery, RecoveryState, RecoveryStatus,
    TuiQuery,
};

// Re-export screen types
pub use screens::{
    ChatScreen, GuardiansScreen, HelpScreen, InvitationFilter, InvitationsScreen, OnboardingStep,
    RecoveryScreen, Screen, ScreenAction, ScreenManager, ScreenType, ThresholdInfo, WelcomeScreen,
};

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

// Re-export demo types
pub use demo::{
    DemoScenario, DemoTipProvider, MockStore, SimulatedBridge, Tip, TipContext, TipProvider,
};
