//! UI-facing facade for aura-app.
//!
//! This module exposes the narrow surface that frontends should use:
//! - workflows (commands)
//! - signals (read/subscribe)
//! - core types (AppCore, AppConfig)

use async_lock::RwLock;
use std::sync::Arc;

use crate::AppCore;

/// UI wrapper around `AppCore` to discourage direct access to internals.
#[derive(Clone)]
pub struct UiAppCore {
    inner: Arc<RwLock<AppCore>>,
}

impl UiAppCore {
    pub fn new(inner: Arc<RwLock<AppCore>>) -> Self {
        Self { inner }
    }

    pub fn raw(&self) -> &Arc<RwLock<AppCore>> {
        &self.inner
    }
}

impl From<Arc<RwLock<AppCore>>> for UiAppCore {
    fn from(inner: Arc<RwLock<AppCore>>) -> Self {
        Self::new(inner)
    }
}

pub mod signals {
    pub use crate::signal_defs::{
        register_app_signals, register_app_signals_with_queries, DiscoveredPeerMethod,
        BUDGET_SIGNAL, CHAT_SIGNAL, CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL,
        DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL,
        NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL, RECOVERY_SIGNAL, SETTINGS_SIGNAL,
        SYNC_STATUS_SIGNAL, TRANSPORT_PEERS_SIGNAL, UNREAD_COUNT_SIGNAL,
    };
    pub use crate::signal_defs::{ConnectionStatus, NetworkStatus, SyncStatus};
}

pub mod workflows {
    pub use crate::workflows::account;
    pub use crate::workflows::admin;
    pub use crate::workflows::amp;
    pub use crate::workflows::authority;
    pub use crate::workflows::budget;
    pub use crate::workflows::ceremonies;
    pub use crate::workflows::chat_commands;
    pub use crate::workflows::config;
    pub use crate::workflows::contacts;
    pub use crate::workflows::context;
    pub use crate::workflows::demo_config;
    pub use crate::workflows::ids;
    pub use crate::workflows::invitation;
    pub use crate::workflows::moderation;
    pub use crate::workflows::network;
    pub use crate::workflows::privacy;
    pub use crate::workflows::query;
    pub use crate::workflows::recovery_cli;
    pub use crate::workflows::settings;
    pub use crate::workflows::snapshot;
    pub use crate::workflows::steward;
    pub use crate::workflows::sync;
    pub use crate::workflows::system;

    #[cfg(feature = "signals")]
    pub use crate::workflows::messaging;

    #[cfg(feature = "signals")]
    pub use crate::workflows::recovery;
}

pub mod types {
    pub use crate::core::{
        AppConfig, AppCore, Intent, IntentError, InvitationType, Screen, StateSnapshot,
    };
    pub use crate::errors::{
        AppError, AuthFailure, ErrorCategory, NetworkErrorCode, SyncStage, ToastSeverity,
    };
    pub use crate::runtime_bridge::{
        BoxedRuntimeBridge, CeremonyKind, InvitationBridgeType, LanPeerInfo, RendezvousStatus,
        RuntimeBridge, RuntimeStatus, SyncStatus as RuntimeSyncStatus,
    };
    pub use crate::thresholds::{
        default_channel_threshold, default_guardian_threshold, normalize_channel_threshold,
        normalize_guardian_threshold, normalize_recovery_threshold,
    };
    pub use crate::workflows::authority::{
        authority_key_prefix, authority_storage_key, deserialize_authority, serialize_authority,
        AuthorityRecord,
    };
    pub use crate::workflows::budget::{
        check_can_add_resident, check_can_join_neighborhood, check_can_pin, format_budget_compact,
        format_budget_status, BudgetBreakdown, BudgetError, HomeFlowBudget, HOME_TOTAL_SIZE, KB,
        MAX_NEIGHBORHOODS, MAX_RESIDENTS, MB, NEIGHBORHOOD_DONATION, RESIDENT_ALLOCATION,
    };
    pub use crate::workflows::chat_commands::{
        all_command_help, command_help, commands_in_category, is_command, normalize_channel_name,
        parse_chat_command, parse_duration, ChatCommand, CommandCapability, CommandCategory,
        CommandError, CommandHelp,
    };
    pub use crate::workflows::config::{
        default_port, generate_device_config, DeviceConfigDefaults, ACCOUNT_FILENAME,
        DEFAULT_BASE_PORT, DEFAULT_LOG_LEVEL, DEFAULT_MAX_RETRIES, DEFAULT_NETWORK_TIMEOUT_SECS,
        JOURNAL_FILENAME, MAX_TUI_LOG_BYTES, TUI_LOG_KEY_PREFIX, TUI_LOG_QUEUE_CAPACITY,
    };
    pub use crate::workflows::invitation::{
        format_invitation_type, format_invitation_type_detailed, format_ttl_display,
        next_ttl_preset, parse_invitation_role, prev_ttl_preset, ttl_hours_to_ms, ttl_preset_index,
        InvitationRoleValue, DEFAULT_INVITATION_TTL_HOURS, INVITATION_TTL_1_DAY,
        INVITATION_TTL_1_HOUR, INVITATION_TTL_1_WEEK, INVITATION_TTL_30_DAYS,
        INVITATION_TTL_PRESETS,
    };
    pub use crate::workflows::system::{
        parse_semantic_version, parse_upgrade_kind, validate_version_string, UpgradeKindValue,
    };
    // Account validation
    pub use crate::views::{
        account, chat, contacts, display, home, invitations, neighborhood, notifications,
        operations, recovery, wizards,
    };
    pub use crate::views::{
        classify_threshold_security, format_recovery_status, security_level_hint, AccountBackup,
        AccountConfig, AdjacencyType, BanRecord, CeremonyProgress, Channel, ChannelType, ChatState,
        Contact, ContactsState, Guardian, GuardianBinding, GuardianStatus, HomeState, HomesState,
        Invitation, InvitationDirection, InvitationStatus, InvitationsState, KickRecord, Message,
        MessageDeliveryStatus, MuteRecord, MySuggestion, NeighborHome, NeighborhoodState,
        RecoveryApproval, RecoveryProcess, RecoveryProcessStatus, RecoveryState, Resident,
        ResidentRole, SecurityLevel, SuggestionPolicy, TraversalPosition, BACKUP_PREFIX,
        BACKUP_VERSION,
    };
    pub use crate::workflows::account::{
        can_submit_account_setup, is_valid_display_name, validate_display_name, DisplayNameError,
        MAX_DISPLAY_NAME_LENGTH, MIN_DISPLAY_NAME_LENGTH,
    };
    // Toast and modal lifecycle types
    pub use crate::views::notifications::{
        duration_ticks, modal_can_user_dismiss, ms_to_ticks, should_auto_dismiss,
        should_interrupt_modal, ticks_to_ms, will_auto_dismiss, ModalPriority, ToastLevel,
        DEFAULT_TOAST_DURATION_MS, DEFAULT_TOAST_TICKS, MAX_PENDING_MODALS, MAX_PENDING_TOASTS,
        NO_AUTO_DISMISS, TOAST_TICK_RATE_MS,
    };
    // Display formatting utilities
    pub use crate::views::display::{
        format_network_status, format_network_status_with_severity, format_relative_time,
        format_relative_time_from, format_relative_time_ms, format_timestamp,
        format_timestamp_full, network_status_severity, selection_indicator, StatusSeverity,
        MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND, SECONDS_PER_DAY, SECONDS_PER_HOUR,
        SECONDS_PER_MINUTE, SELECTED_INDICATOR, UNSELECTED_INDICATOR,
    };
    // Operation result types
    pub use crate::views::operations::{
        ChannelModeUpdated, ContextChanged, DeviceEnrollmentStarted, DeviceRemovalStarted,
        ExportedInvitation, ImportedInvitation, MfaPolicyUpdated, NicknameUpdated, OperationError,
    };
    // Wizard step types
    pub use crate::effects::reactive::{ReactiveHandler, SignalGraph, SignalGraphStats};
    #[cfg(feature = "signals")]
    pub use crate::reactive_state::{ReactiveState, ReactiveVec};
    pub use crate::views::wizards::{
        format_wizard_progress, wizard_progress_percent, AccountSetupStep, CreateChannelStep,
        RecoverySetupStep,
    };
    pub use aura_core::identifiers::{AuthorityId, ContextId};
    pub use aura_core::time::TimeStamp;

    // AMP types for channel state inspection
    pub use aura_journal::ChannelEpochState;
}

pub mod authorization {
    pub use crate::authorization::*;
}

pub mod prelude;
