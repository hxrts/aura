//! # View State Module
//!
//! This module contains the view state types that represent the current
//! application state. These types are FFI-safe and can be:
//!
//! - Serialized for debugging
//! - Passed to UniFFI for mobile
//! - Used with futures-signals for reactive updates

mod state;

pub mod account;
pub mod chat;
pub mod contacts;
pub mod display;
pub mod home;
pub mod invitations;
pub mod neighborhood;
pub mod notifications;
pub mod operations;
pub mod recovery;
pub mod wizards;

pub use state::ViewState;

// Re-export state types for convenience
pub use account::{AccountBackup, AccountConfig, BACKUP_PREFIX, BACKUP_VERSION};
pub use chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus};
pub use contacts::{Contact, ContactsState, MySuggestion, SuggestionPolicy};
pub use home::{BanRecord, HomeState, HomesState, KickRecord, MuteRecord, Resident, ResidentRole};
pub use invitations::{
    Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
};
pub use neighborhood::{AdjacencyType, NeighborHome, NeighborhoodState, TraversalPosition};
pub use recovery::{
    classify_threshold_security, format_recovery_status, security_level_hint, CeremonyProgress,
    Guardian, GuardianBinding, GuardianStatus, RecoveryApproval, RecoveryProcess,
    RecoveryProcessStatus, RecoveryState, SecurityLevel,
};
pub use notifications::{
    duration_ticks, modal_can_user_dismiss, ms_to_ticks, should_auto_dismiss,
    should_interrupt_modal, ticks_to_ms, will_auto_dismiss, ModalPriority, ToastLevel,
    DEFAULT_TOAST_DURATION_MS, DEFAULT_TOAST_TICKS, MAX_PENDING_MODALS, MAX_PENDING_TOASTS,
    NO_AUTO_DISMISS, TOAST_TICK_RATE_MS,
};
pub use display::{
    format_network_status, format_network_status_with_severity, format_relative_time,
    format_relative_time_from, format_relative_time_ms, format_timestamp, format_timestamp_full,
    network_status_severity, selection_indicator, StatusSeverity, MS_PER_HOUR, MS_PER_MINUTE,
    MS_PER_SECOND, SECONDS_PER_DAY, SECONDS_PER_HOUR, SECONDS_PER_MINUTE, SELECTED_INDICATOR,
    UNSELECTED_INDICATOR,
};
pub use operations::{
    ChannelModeUpdated, ContextChanged, DeviceEnrollmentStarted, DeviceRemovalStarted,
    ExportedInvitation, ImportedInvitation, MfaPolicyUpdated, NicknameUpdated, OperationError,
};
pub use wizards::{
    format_wizard_progress, wizard_progress_percent, AccountSetupStep, CreateChannelStep,
    RecoverySetupStep,
};
