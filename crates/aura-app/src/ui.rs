//! UI-facing facade for aura-app.
//!
//! This module exposes the narrow surface that frontends should use:
//! - workflows (commands)
//! - signals (read/subscribe)
//! - core types (AppCore, AppConfig)

#![allow(missing_docs)] // UI facade - documentation evolving with API

use async_lock::RwLock;
use std::sync::Arc;

use crate::AppCore;

/// UI wrapper around the shared `AppCore` boundary.
#[derive(Clone)]
pub struct UiAppCore {
    shared: Arc<RwLock<AppCore>>,
}

impl UiAppCore {
    pub fn new(shared: Arc<RwLock<AppCore>>) -> Self {
        Self { shared }
    }

    pub fn shared(&self) -> &Arc<RwLock<AppCore>> {
        &self.shared
    }

    pub fn raw(&self) -> &Arc<RwLock<AppCore>> {
        self.shared()
    }
}

impl From<Arc<RwLock<AppCore>>> for UiAppCore {
    fn from(shared: Arc<RwLock<AppCore>>) -> Self {
        Self::new(shared)
    }
}

pub mod signals {
    pub use crate::signal_defs::{
        register_app_signals, register_app_signals_with_queries, DiscoveredPeer,
        DiscoveredPeerMethod, AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL,
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL_NAME, BUDGET_SIGNAL, CHAT_SIGNAL,
        CONNECTION_STATUS_SIGNAL, CONTACTS_SIGNAL, DISCOVERED_PEERS_SIGNAL, ERROR_SIGNAL,
        HOMES_SIGNAL, INVITATIONS_SIGNAL, NEIGHBORHOOD_SIGNAL, NETWORK_STATUS_SIGNAL,
        RECOVERY_SIGNAL, SETTINGS_SIGNAL, SYNC_STATUS_SIGNAL, TRANSPORT_PEERS_SIGNAL,
        UNREAD_COUNT_SIGNAL,
    };
    pub use crate::signal_defs::{ConnectionStatus, NetworkStatus, SyncStatus};
    pub use crate::ui_contract::AuthoritativeSemanticFact;
    // Signal name constants for emit_signal calls
    pub use crate::signal_defs::{
        BUDGET_SIGNAL_NAME, CHAT_SIGNAL_NAME, CONNECTION_STATUS_SIGNAL_NAME, CONTACTS_SIGNAL_NAME,
        DISCOVERED_PEERS_SIGNAL_NAME, ERROR_SIGNAL_NAME, HOMES_SIGNAL_NAME,
        INVITATIONS_SIGNAL_NAME, NEIGHBORHOOD_SIGNAL_NAME, NETWORK_STATUS_SIGNAL_NAME,
        RECOVERY_SIGNAL_NAME, SETTINGS_SIGNAL_NAME, SYNC_STATUS_SIGNAL_NAME,
        TRANSPORT_PEERS_SIGNAL_NAME, UNREAD_COUNT_SIGNAL_NAME,
    };
}

pub mod contract {
    pub use crate::ui_contract::{
        classify_screen_item_id, classify_semantic_settings_section_item_id,
        classify_settings_section_item_id, compare_ui_snapshots_for_parity,
        contacts_friend_action_controls, list_item_dom_id, list_item_selector,
        nav_control_id_for_screen, screen_item_id,
        semantic_settings_section_item_id, semantic_settings_section_surface_id,
        settings_section_item_id, shared_flow_scenarios, shared_flow_support, shared_list_support,
        shared_modal_support, shared_screen_support, AuthoritativeSemanticFact, ConfirmationState,
        ControlId, FieldId, FlowAvailability, FrontendId, HarnessUiCommand,
        HarnessUiCommandReceipt, ListId, ListItemSnapshot, ListSnapshot, MessageSnapshot, ModalId,
        OperationId, OperationInstanceId, OperationSnapshot, OperationState, ParityException,
        RenderHeartbeat, RuntimeEventId, RuntimeEventKind, RuntimeEventSnapshot, ScreenId,
        SelectionSnapshot, SharedFlowId, SharedFlowScenarioCoverage, SharedFlowSupport,
        SharedListSupport, SharedModalSupport, SharedScreenModuleMap, SharedScreenSupport, ToastId,
        ToastKind, ToastSnapshot, UiParityMismatch, UiReadiness, UiSnapshot, ALL_SHARED_FLOW_IDS,
        SHARED_FLOW_SCENARIO_COVERAGE, SHARED_FLOW_SUPPORT, SHARED_LIST_SUPPORT,
        SHARED_MODAL_SUPPORT, SHARED_SCREEN_MODULE_MAP, SHARED_SCREEN_SUPPORT,
    };
}

pub mod scenarios {
    pub use crate::scenario_contract::{
        ActorId, EnvironmentAction, Expectation, ExtractSource, InputKey, IntentAction,
        ScenarioAction, ScenarioDefinition, ScenarioStep, SemanticBarrierRef,
        SemanticCommandRequest, SemanticCommandResponse, SemanticCommandSupport,
        SemanticCommandValue, SemanticSubmissionHandle, SettingsSection, SubmissionState,
        SubmittedAction, UiAction, UiOperationHandle, VariableAction, SEMANTIC_COMMAND_SUPPORT,
    };
}

pub mod workflows {
    pub use crate::workflows::access;
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
    pub use crate::workflows::moderator;
    pub use crate::workflows::network;
    pub use crate::workflows::privacy;
    pub use crate::workflows::query;
    pub use crate::workflows::recovery_cli;
    pub use crate::workflows::runtime;
    pub use crate::workflows::settings;
    pub use crate::workflows::signals;
    pub use crate::workflows::slash_commands;
    pub use crate::workflows::snapshot;
    pub use crate::workflows::strong_command;
    pub use crate::workflows::sync;
    pub use crate::workflows::system;
    pub use crate::workflows::time;

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
        AppError, AuthFailure, ErrorCategory, NetworkErrorCode, SyncStage, ToastLevel,
    };
    pub use crate::runtime_bridge::{
        BoxedRuntimeBridge, CeremonyKind, InvitationBridgeStatus, InvitationBridgeType,
        InvitationInfo, KeyRotationCeremonyStatus, LanPeerInfo, RendezvousStatus, RuntimeBridge,
        RuntimeStatus, SyncStatus as RuntimeSyncStatus,
    };
    pub use crate::thresholds::{
        default_channel_threshold, default_guardian_threshold, normalize_channel_threshold,
        normalize_guardian_threshold, normalize_recovery_threshold,
    };
    pub use crate::views::invitations::Invitation;
    pub use crate::workflows::authority::{
        authority_key_prefix, authority_storage_key, deserialize_authority, serialize_authority,
        AuthorityRecord,
    };
    pub use crate::workflows::budget::{
        check_can_add_member, check_can_join_neighborhood, check_can_pin, format_budget_compact,
        format_budget_status, BudgetBreakdown, BudgetError, HomeFlowBudget, HOME_TOTAL_SIZE, KB,
        MAX_MEMBERS, MAX_NEIGHBORHOODS, MB, MEMBER_ALLOCATION, NEIGHBORHOOD_ALLOCATION,
    };
    pub use crate::workflows::chat_commands::{
        all_command_help, command_help, commands_in_category, is_command, normalize_channel_name,
        parse_chat_command, parse_duration, ChatCommand, CommandCapability, CommandCategory,
        CommandError, CommandHelp,
    };
    pub use crate::workflows::config::{
        default_port, generate_device_config, DeviceConfigDefaults, ACCOUNT_FILENAME,
        DEFAULT_BASE_PORT, DEFAULT_LOG_LEVEL, DEFAULT_MAX_RETRIES, DEFAULT_NETWORK_TIMEOUT_SECS,
        JOURNAL_FILENAME, MAX_TUI_LOG_BYTES, PENDING_ACCOUNT_BOOTSTRAP_FILENAME,
        TUI_LOG_KEY_PREFIX, TUI_LOG_QUEUE_CAPACITY, WEB_ACCOUNT_CONFIG_STORAGE_SUFFIX,
        WEB_PENDING_ACCOUNT_BOOTSTRAP_STORAGE_SUFFIX, WEB_SELECTED_RUNTIME_IDENTITY_STORAGE_SUFFIX,
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
        account, chat, contacts, display, home, invitations, naming, neighborhood, notifications,
        operations, recovery, wizards,
    };
    // Naming pattern
    pub use crate::views::naming::EffectiveName;
    pub use crate::views::{
        classify_threshold_security, format_recovery_status, security_level_hint, AccountBackup,
        AccountConfig, BanRecord, BootstrapEvent, BootstrapEventKind, BootstrapRuntimeIdentity,
        BootstrapSurface, CeremonyProgress, Channel, ChannelType, ChatState, Contact,
        ContactsState, Guardian, GuardianBinding, GuardianStatus, HomeMember, HomeRole,
        HomeState, HomesState, InvitationDirection, InvitationStatus, InvitationsState,
        KickRecord, Message, MessageDeliveryStatus, MuteRecord, MySuggestion, NeighborHome,
        NeighborhoodState, OneHopLinkType, PendingAccountBootstrap, RecoveryApproval,
        RecoveryProcess, RecoveryProcessStatus, RecoveryState, SecurityLevel, SuggestionPolicy,
        TraversalPosition, BACKUP_PREFIX, BACKUP_VERSION,
    };
    pub use crate::views::contacts::ContactRelationshipState;
    pub use crate::workflows::account::{
        can_submit_account_setup, is_valid_nickname_suggestion, prepare_pending_account_bootstrap,
        validate_nickname_suggestion, NicknameSuggestionError, MAX_NICKNAME_SUGGESTION_LENGTH,
        MIN_NICKNAME_SUGGESTION_LENGTH,
    };
    pub use aura_social::AccessLevel;
    // Toast and modal lifecycle types
    pub use crate::views::notifications::{
        duration_ticks, modal_can_user_dismiss, ms_to_ticks, should_auto_dismiss,
        should_interrupt_modal, ticks_to_ms, will_auto_dismiss, ModalPriority,
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
    pub use crate::ui_contract::{
        ControlId, FieldId, ListId, MessageSnapshot, ModalId, OperationId, ScreenId, ToastId,
        UiSnapshot,
    };
    pub use crate::views::wizards::{
        format_wizard_progress, wizard_progress_percent, AccountSetupStep, CreateChannelStep,
        RecoverySetupStep,
    };
    pub use aura_core::time::TimeStamp;
    pub use aura_core::types::identifiers::{AuthorityId, ContextId};

    // AMP types for channel state inspection
    pub use aura_journal::ChannelEpochState;

    // Recovery types for scenario simulation
    pub use aura_recovery::types::RecoveryRequest;

    // Types for demo seeding (used by aura-terminal's development feature)
    pub use aura_journal::DomainFact;
    pub use aura_relational::ContactFact;
}

pub mod authorization {
    pub use crate::authorization::*;
}

pub mod frontend {
    pub use crate::frontend_primitives::{
        ClipboardPort, FrontendTaskOwner, FrontendTaskRuntime, FrontendUiOperation, MemoryClipboard,
    };
}

pub mod prelude {
    //! UI prelude — import the approved aura-app surface for frontends.

    pub use crate::ui::signals::*;
    pub use crate::ui::types::*;
}
