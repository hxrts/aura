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
pub mod reactive;
pub mod recovery_session;

// Re-export main types for convenience
pub use components::{
    AccountSetupModal, AccountSetupState, CardFooter, CardHeader, CardStyle, SimpleCard,
    ChannelInfoModal, ChatCreateModal, ChatCreateState, CreateChatCallback, CommandItem,
    CommandPalette, PaletteCommand, ContactSelectModal, ContactSelectState, DemoHintBar,
    DemoInviteCodes, DiscoveredPeerInfo, DiscoveredPeersPanel, DiscoveredPeersState,
    InvitePeerCallback, PeerInvitationStatus, EmptyState, LoadingState, NoResults, FormField,
    FormFieldComponent, FormModal, FormModalState, GuardianCandidateProps, GuardianSetupModal,
    HelpModal, HelpModalState, InvitationCodeModal, InvitationCodeState, CancelCallback,
    InvitationCreateModal, InvitationCreateState, ImportCallback, InvitationImportModal,
    InvitationImportState, KeyHintsBar, List, ListEntry, ListItem, ListNavigation,
    CompactMessage, MessageBubble, MessageGroupHeader, SystemMessage, MessageInput,
    MessageInputState, ConfirmModal, InputModal, Panel, PanelStyle, calculate_scroll,
    ScrollDirection, Scrollable, ProgressDots, Status, StatusDot, StatusIndicator, TextInput,
    TextInputModal, TextInputState, Badge, Divider, Heading, KeyValue, StyledText, TextStyle,
    Textarea, TextareaState, ThresholdModal, ThresholdState, StatusBar, Toast, ToastContainer,
    ToastLevel, ToastMessage,
};
pub use context::IoContext;
pub use hooks::{
    is_vec_empty, snapshot_state, snapshot_vec, vec_len, AppCoreContext, BlockSnapshot,
    ChatSnapshot, ContactsSnapshot, GuardiansSnapshot, HasReactiveData, InvitationsSnapshot,
    NeighborhoodSnapshot, ReactiveValue, RecoverySnapshot,
};
pub use screens::{
    run_app_with_context, IoApp, run_block_screen, BlockFocus, BlockInviteCallback,
    BlockNavCallback, BlockScreen, BlockSendCallback, GrantStewardCallback,
    RevokeStewardCallback, run_chat_screen, ChannelSelectCallback, ChatFocus, ChatScreen,
    CreateChannelCallback, RetryMessageCallback, SendCallback, SetTopicCallback,
    run_contacts_screen, ContactsScreen, ImportInvitationCallback, StartChatCallback,
    UpdatePetnameCallback, get_help_commands, get_help_commands_for_screen, HelpCommand,
    CreateInvitationCallback, ExportInvitationCallback, InvitationCallback, InvitationsScreen,
    run_neighborhood_screen, GoHomeCallback, NavigationCallback, NeighborhoodScreen,
    run_recovery_screen, RecoveryCallback, RecoveryScreen, NavAction, Router, Screen,
    run_settings_screen, AddDeviceCallback, MfaCallback, RemoveDeviceCallback, SettingsScreen,
    UpdateNicknameCallback, UpdateThresholdCallback,
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

// Re-export reactive types
pub use reactive::{
    ChannelType, ChannelsQuery, GuardianApproval, GuardianStatus, GuardiansQuery,
    InvitationDirection, InvitationStatus, InvitationType, InvitationsQuery, MessagesQuery,
    RecoveryQuery, RecoveryState,
};

// Re-export navigation types
pub use navigation::{
    is_nav_key_press, navigate_grid, navigate_list, InputThrottle, NavKey, NavThrottle,
    ThreePanelFocus, TwoPanelFocus, INPUT_THROTTLE_MS, NAV_THROTTLE_MS,
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
