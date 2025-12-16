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
//! - **state_machine**: Pure state machine model for deterministic testing
//! - **iocraft_adapter**: Bridge between iocraft events and TerminalEffects trait
//!
//! ## Testing Architecture
//!
//! The TUI uses a pure state machine model for deterministic testing:
//!
//! ```text
//! TuiState × TerminalEvent → (TuiState, Vec<TuiCommand>)
//! ```
//!
//! This enables:
//! - **Fast tests**: No PTY setup, no sleeps, pure computation (<1ms per test)
//! - **Determinism**: Same inputs = same outputs, every time
//! - **Debuggability**: Full state visibility at every step
//! - **Formal verification**: Quint spec at `verification/quint/tui_state_machine.qnt`
//!
//! See `tests/tui_deterministic.rs` for examples.

// Core iocraft modules
pub mod components;
pub mod context;
pub mod hooks;
pub mod iocraft_adapter;
pub mod layout;
pub mod props;
pub mod runtime;
pub mod screens;
pub mod state_machine;
pub mod theme;
pub mod types;

// Shared infrastructure
pub mod commands;
pub mod effects;
pub mod flow_budget;
pub mod local_store;
pub mod navigation;
pub mod recovery_session;

// Re-export main types for convenience
pub use components::{
    calculate_scroll, get_help_commands, get_help_commands_for_screen, AccountSetupModal,
    AccountSetupState, Badge, CancelCallback, ChannelInfoModal, ChatCreateModal, ChatCreateState,
    CommandItem, CommandPalette, CompactMessage, ConfirmModal, ContactSelectModal,
    ContactSelectState, CreateChatCallback, DemoHintBar, DemoInviteCodes, DiscoveredPeerInfo,
    DiscoveredPeersPanel, DiscoveredPeersState, Divider, EmptyState, FormField, FormFieldComponent,
    FormModal, FormModalState, GuardianCandidateProps, GuardianSetupModal, Heading, HelpCommand,
    HelpModal, HelpModalState, ImportCallback, InputModal, InvitationCodeModal, InvitationCodeState,
    InvitationCreateModal, InvitationCreateState, InvitationImportModal, InvitationImportState,
    InvitePeerCallback, KeyValue, List, ListEntry, ListItem, ListNavigation, LoadingState,
    MessageBubble, MessageGroupHeader, MessageInput, MessageInputState, NoResults, PaletteCommand,
    Panel, PanelStyle, PeerInvitationStatus, ProgressDots, ScrollDirection, Scrollable, Status,
    StatusBar, StatusDot, StatusIndicator, StyledText, SystemMessage, TextInput, TextInputModal,
    TextInputState, TextStyle, Textarea, TextareaState, ThresholdModal, ThresholdState, Toast,
    ToastContainer, ToastLevel, ToastMessage,
};
pub use context::IoContext;
pub use hooks::{
    is_vec_empty, snapshot_state, snapshot_vec, vec_len, AppCoreContext, BlockSnapshot,
    ChatSnapshot, ContactsSnapshot, GuardiansSnapshot, HasReactiveData, InvitationsSnapshot,
    NeighborhoodSnapshot, ReactiveValue, RecoverySnapshot,
};
pub use screens::{
    run_app_with_context, run_block_screen, run_chat_screen, run_contacts_screen,
    run_neighborhood_screen, run_recovery_screen, run_settings_screen, AddDeviceCallback,
    BlockFocus, BlockInviteCallback, BlockNavCallback, BlockScreen, BlockSendCallback,
    ChannelSelectCallback, ChatFocus, ChatScreen, ContactsScreen, CreateChannelCallback,
    CreateInvitationCallback, ExportInvitationCallback, GoHomeCallback, GrantStewardCallback,
    ImportInvitationCallback, InvitationCallback, InvitationsScreen, IoApp, MfaCallback, NavAction,
    NavigationCallback, NeighborhoodScreen, RecoveryCallback, RecoveryScreen, RemoveDeviceCallback,
    RetryMessageCallback, RevokeStewardCallback, Router, Screen, SendCallback, SetTopicCallback,
    SettingsScreen, StartChatCallback, UpdateNicknameCallback, UpdatePetnameCallback,
    UpdateThresholdCallback,
};
pub use theme::{Spacing, Theme};
pub use types::*;

// Re-export effect types
pub use effects::{AuraEvent, EffectCommand, EventFilter};

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

// Re-export reactive/view types from aura-app and aura-agent
// (formerly in reactive submodule, now inlined for simplicity)

// FRP primitives from aura-agent (runtime layer)
pub use aura_agent::reactive::Dynamic;
pub use aura_agent::reactive::ReactiveScheduler;

// Reactive infrastructure types from aura-agent
pub use aura_agent::reactive::{
    BlockDelta, BlockReduction, ChatReduction, FactSource, FactStreamAdapter, GuardianDelta,
    GuardianReduction, InvitationReduction, RecoveryDelta, RecoveryReduction, SchedulerConfig,
    ViewAdapter,
};

// Domain deltas
pub use aura_chat::ChatDelta;
pub use aura_invitation::InvitationDelta;

// View types from aura-app
pub use aura_app::views::block::{BlockState, Resident, ResidentRole, StorageBudget};
pub use aura_app::views::chat::{Channel, ChannelType, Message};
pub use aura_app::views::contacts::{Contact, MySuggestion, SuggestionPolicy};
pub use aura_app::views::invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType,
};
pub use aura_app::views::neighborhood::{AdjacencyType, NeighborBlock, TraversalPosition};
pub use aura_app::views::recovery::{
    Guardian, GuardianStatus, RecoveryApproval as GuardianApproval,
    RecoveryProcessStatus as RecoveryStatus, RecoveryState,
};

// Query types from aura-app
pub use aura_app::queries::{
    ChannelsQuery, ContactsQuery, GuardiansQuery, InvitationsQuery, MessagesQuery, RecoveryQuery,
};

// ThresholdConfig from aura-core
pub use aura_core::threshold::ThresholdConfig;

// Re-export navigation types
pub use navigation::{
    is_nav_key_press, navigate_grid, navigate_list, NavKey, ThreePanelFocus, TwoPanelFocus,
};

// Re-export iocraft adapter types
pub use iocraft_adapter::{convert_iocraft_event, EventBridge, IocraftTerminalAdapter};

// Re-export state machine types
pub use state_machine::{
    transition, AccountSetupModalState, DispatchCommand, ModalState, ModalType, TuiCommand,
    TuiState,
};

// Re-export props extraction functions
pub use props::{
    extract_block_view_props, extract_chat_view_props, extract_contacts_view_props,
    extract_help_view_props, extract_invitations_view_props, extract_neighborhood_view_props,
    extract_recovery_view_props, extract_settings_view_props, BlockViewProps, ChatViewProps,
    ContactsViewProps, HelpViewProps, InvitationsViewProps, NeighborhoodViewProps,
    RecoveryViewProps, SettingsViewProps,
};
