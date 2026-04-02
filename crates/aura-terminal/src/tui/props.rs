//! # Props Extraction Module
//!
//! Pure functions for extracting screen props from TuiState.
//!
//! This module provides a testable layer between the state machine and view components.
//! By extracting props through dedicated functions, we can:
//! 1. Test that all state fields are correctly mapped to props
//! 2. Catch bugs where state changes aren't reflected in the UI
//! 3. Ensure consistency between state machine and view layer
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌──────────────┐     ┌─────────────┐     ┌─────────────┐
//! │  TuiState   │ --> │ extract_*()  │ --> │ ScreenProps │ --> │   Screen    │
//! └─────────────┘     └──────────────┘     └─────────────┘     └─────────────┘
//!                         ✓ TEST
//! ```

use crate::tui::navigation::TwoPanelFocus;
use crate::tui::screens::ChatFocus as ScreenChatFocus;
use crate::tui::state::{
    ChatFocus, ContactsListFocus, CreateInvitationField, DetailFocus, GuardianCeremonyResponse,
    GuardianSetupStep, NeighborhoodMode, QueuedModal, TuiState,
};
use crate::tui::types::{AccessLevel, Contact, Device};
use aura_core::threshold::AgreementMode;
use tracing::warn;

// ============================================================================
// Chat Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for ChatScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatCreateModalViewProps {
    pub visible: bool,
    pub name: String,
    pub topic: String,
    pub active_field: usize,
    pub member_count: usize,
    pub step: crate::tui::state::CreateChannelStep,
    pub contacts: Vec<(String, String)>,
    pub selected_indices: Vec<usize>,
    pub focused_index: usize,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub status: String,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatTopicModalViewProps {
    pub visible: bool,
    pub name: String,
    pub value: String,
    pub active_field: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatInfoModalViewProps {
    pub visible: bool,
    pub channel_name: String,
    pub topic: String,
    pub participants: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatModalProps {
    pub create: ChatCreateModalViewProps,
    pub topic: ChatTopicModalViewProps,
    pub info: ChatInfoModalViewProps,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatViewProps {
    pub focus: ScreenChatFocus,
    pub selected_channel: usize,
    pub message_scroll: usize,
    pub insert_mode: bool,
    pub input_buffer: String,
    pub modals: ChatModalProps,
}

/// Extract ChatScreen view props from TuiState
///
/// This function extracts all view-related state needed by ChatScreen.
/// Domain data (channels, messages, etc.) is passed separately.
pub fn extract_chat_view_props(state: &TuiState) -> ChatViewProps {
    let focus = match state.chat.focus {
        ChatFocus::Channels => ScreenChatFocus::Channels,
        ChatFocus::Messages => ScreenChatFocus::Messages,
        ChatFocus::Input => ScreenChatFocus::Input,
    };

    // Extract modal state from queue (all modals now use queue system)
    let (
        create_visible,
        create_name,
        create_topic,
        create_field,
        create_member_count,
        create_step,
        create_contacts,
        create_selected,
        create_focused,
        create_threshold_k,
        create_threshold_n,
        create_status,
        create_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::ChatCreate(s)) => (
            true,
            s.name.clone(),
            s.topic.clone(),
            s.active_field,
            s.selected_indices.len(),
            s.step.clone(),
            s.contacts
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect(),
            s.selected_indices.clone(),
            s.focused_index,
            s.threshold_k,
            s.total_participants(),
            s.status.clone().unwrap_or_default(),
            s.error.clone().unwrap_or_default(),
        ),
        _ => (
            false,
            String::new(),
            String::new(),
            0,
            0,
            crate::tui::state::CreateChannelStep::default(),
            vec![],
            vec![],
            0,
            1,
            1,
            String::new(),
            String::new(),
        ),
    };

    let (topic_visible, topic_name, topic_value, topic_active_field) = match state
        .modal_queue
        .current()
    {
        Some(QueuedModal::ChatTopic(s)) => (true, s.name.clone(), s.value.clone(), s.active_field),
        _ => (false, String::new(), String::new(), 0),
    };

    let (info_visible, info_channel_name, info_topic, info_participants) =
        match state.modal_queue.current() {
            Some(QueuedModal::ChatInfo(s)) => (
                true,
                s.channel_name.clone(),
                s.topic.clone(),
                s.participants.clone(),
            ),
            _ => (false, String::new(), String::new(), Vec::new()),
        };

    ChatViewProps {
        focus,
        selected_channel: state.chat.selected_channel,
        message_scroll: state.chat.message_scroll,
        insert_mode: state.chat.insert_mode,
        input_buffer: state.chat.input_buffer.clone(),
        modals: ChatModalProps {
            create: ChatCreateModalViewProps {
                visible: create_visible,
                name: create_name,
                topic: create_topic,
                active_field: create_field,
                member_count: create_member_count,
                step: create_step,
                contacts: create_contacts,
                selected_indices: create_selected,
                focused_index: create_focused,
                threshold_k: create_threshold_k,
                threshold_n: create_threshold_n,
                status: create_status,
                error: create_error,
            },
            topic: ChatTopicModalViewProps {
                visible: topic_visible,
                name: topic_name,
                value: topic_value,
                active_field: topic_active_field,
            },
            info: ChatInfoModalViewProps {
                visible: info_visible,
                channel_name: info_channel_name,
                topic: info_topic,
                participants: info_participants,
            },
        },
    }
}

// ============================================================================
// Contacts Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for ContactsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsNicknameModalViewProps {
    pub visible: bool,
    pub contact_id: String,
    pub value: String,
    pub nickname_suggestion: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsImportModalViewProps {
    pub visible: bool,
    pub code: String,
    pub importing: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsCreateModalViewProps {
    pub visible: bool,
    pub receiver_id: String,
    pub receiver_name: String,
    pub type_index: usize,
    pub message: String,
    pub ttl_hours: u64,
    pub focused_field: CreateInvitationField,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsCodeModalViewProps {
    pub visible: bool,
    pub invitation_id: String,
    pub code: String,
    pub loading: bool,
    pub copied: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuardianWorkflowModalViewProps {
    pub visible: bool,
    pub step: GuardianSetupStep,
    pub contacts: Vec<GuardianCandidateViewProps>,
    pub selected_indices: Vec<usize>,
    pub focused_index: usize,
    pub threshold_k: u8,
    pub threshold_n: u8,
    pub ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>,
    pub agreement_mode: AgreementMode,
    pub reversion_risk: bool,
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsModalProps {
    pub nickname: ContactsNicknameModalViewProps,
    pub import_invitation: ContactsImportModalViewProps,
    pub create_invitation: ContactsCreateModalViewProps,
    pub code_display: ContactsCodeModalViewProps,
    pub guardian_setup: GuardianWorkflowModalViewProps,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsViewProps {
    pub focus: TwoPanelFocus,
    pub list_focus: ContactsListFocus,
    pub selected_index: usize,
    pub filter: String,
    pub lan_selected_index: usize,
    pub lan_peer_count: usize,
    pub modals: ContactsModalProps,
    // Demo mode shortcuts
    #[cfg(feature = "development")]
    pub demo_mode: bool,
    #[cfg(feature = "development")]
    pub demo_alice_code: String,
    #[cfg(feature = "development")]
    pub demo_carol_code: String,
}

/// View props for a guardian candidate
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GuardianCandidateViewProps {
    pub id: String,
    pub name: String,
    pub is_current_guardian: bool,
}

/// Extract ContactsScreen view props from TuiState
pub fn extract_contacts_view_props(state: &TuiState) -> ContactsViewProps {
    let focus = state.contacts.focus;
    let list_focus = state.contacts.list_focus;

    // Extract modal state from queue (all modals now use queue system)
    let (nickname_visible, nickname_contact_id, nickname_value, nickname_suggested) =
        match state.modal_queue.current() {
            Some(QueuedModal::ContactsNickname(s)) => (
                true,
                s.contact_id.clone(),
                s.value.clone(),
                s.nickname_suggestion.clone(),
            ),
            _ => (false, String::new(), String::new(), None),
        };

    let (import_visible, import_code, import_importing) = match state.modal_queue.current() {
        Some(QueuedModal::ContactsImport(s)) => (true, s.code.clone(), s.importing),
        _ => (false, String::new(), false),
    };

    let (
        create_visible,
        create_receiver_id,
        create_receiver_name,
        create_type_index,
        create_message,
        create_ttl,
        create_focused_field,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::ContactsCreate(s)) => (
            true,
            s.receiver_id.clone(),
            s.receiver_name.clone(),
            s.type_index,
            s.message.clone(),
            s.ttl_hours,
            s.focused_field,
        ),
        _ => (
            false,
            String::new(),
            String::new(),
            0,
            String::new(),
            24,
            CreateInvitationField::Type,
        ),
    };

    let (code_visible, code_invitation_id, code_code, code_loading, code_copied) =
        match state.modal_queue.current() {
            Some(QueuedModal::ContactsCode(s)) => (
                true,
                s.invitation_id.clone(),
                s.code.clone(),
                s.loading,
                s.copied,
            ),
            _ => (false, String::new(), String::new(), false, false),
        };

    // Guardian setup modal from queue
    let (
        guardian_visible,
        guardian_step,
        guardian_contacts,
        guardian_selected,
        guardian_focused,
        guardian_k,
        guardian_n,
        guardian_responses,
        guardian_agreement_mode,
        guardian_reversion_risk,
        guardian_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::GuardianSetup(s)) => (
            true,
            s.step(),
            s.contacts()
                .map(|contacts| {
                    contacts
                        .iter()
                        .map(|c| GuardianCandidateViewProps {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            is_current_guardian: c.is_current_guardian,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            s.selected_indices().map(|v| v.to_vec()).unwrap_or_default(),
            s.focused_index(),
            s.threshold_k(),
            s.threshold_n(),
            s.ceremony_responses_vec(),
            s.ceremony_agreement_mode(),
            s.ceremony_reversion_risk(),
            s.error().unwrap_or_default().to_string(),
        ),
        _ => (
            false,
            GuardianSetupStep::default(),
            vec![],
            vec![],
            0,
            2,
            3,
            vec![],
            AgreementMode::ConsensusFinalized,
            false,
            String::new(),
        ),
    };

    ContactsViewProps {
        focus,
        list_focus,
        selected_index: state.contacts.selected_index,
        filter: state.contacts.filter.clone(),
        lan_selected_index: state.contacts.lan_selected_index,
        lan_peer_count: state.contacts.lan_peer_count,
        modals: ContactsModalProps {
            nickname: ContactsNicknameModalViewProps {
                visible: nickname_visible,
                contact_id: nickname_contact_id,
                value: nickname_value,
                nickname_suggestion: nickname_suggested,
            },
            import_invitation: ContactsImportModalViewProps {
                visible: import_visible,
                code: import_code,
                importing: import_importing,
            },
            create_invitation: ContactsCreateModalViewProps {
                visible: create_visible,
                receiver_id: create_receiver_id,
                receiver_name: create_receiver_name,
                type_index: create_type_index,
                message: create_message,
                ttl_hours: create_ttl,
                focused_field: create_focused_field,
            },
            code_display: ContactsCodeModalViewProps {
                visible: code_visible,
                invitation_id: code_invitation_id,
                code: code_code,
                loading: code_loading,
                copied: code_copied,
            },
            guardian_setup: GuardianWorkflowModalViewProps {
                visible: guardian_visible,
                step: guardian_step,
                contacts: guardian_contacts,
                selected_indices: guardian_selected,
                focused_index: guardian_focused,
                threshold_k: guardian_k,
                threshold_n: guardian_n,
                ceremony_responses: guardian_responses,
                agreement_mode: guardian_agreement_mode,
                reversion_risk: guardian_reversion_risk,
                error: guardian_error,
            },
        },
        // Demo mode (development feature only)
        #[cfg(feature = "development")]
        demo_mode: !state.contacts.demo_alice_code.is_empty(),
        #[cfg(feature = "development")]
        demo_alice_code: state.contacts.demo_alice_code.clone(),
        #[cfg(feature = "development")]
        demo_carol_code: state.contacts.demo_carol_code.clone(),
    }
}

// ============================================================================
// Notifications Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for NotificationsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotificationsViewProps {
    pub focus: crate::tui::navigation::TwoPanelFocus,
    pub selected_index: usize,
}

/// Extract NotificationsScreen view props from TuiState
pub fn extract_notifications_view_props(state: &TuiState) -> NotificationsViewProps {
    NotificationsViewProps {
        focus: state.notifications.focus,
        selected_index: state.notifications.selected_index,
    }
}

// ============================================================================
// Settings Screen Props Extraction
// ============================================================================

use crate::tui::types::{AuthorityInfo, MfaPolicy, SettingsSection};

/// View state extracted from TuiState for SettingsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthorityPickerModalViewProps {
    pub visible: bool,
    pub authorities: Vec<(String, String)>,
    pub selected_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsNicknameModalViewProps {
    pub visible: bool,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AddDeviceModalViewProps {
    pub visible: bool,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceImportModalViewProps {
    pub visible: bool,
    pub code: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceEnrollmentModalViewProps {
    pub visible: bool,
    pub ceremony_id: String,
    pub nickname_suggestion: String,
    pub code: String,
    pub accepted_count: u16,
    pub total_count: u16,
    pub threshold: u16,
    pub is_complete: bool,
    pub has_failed: bool,
    pub error_message: String,
    pub copied: bool,
    pub agreement_mode: AgreementMode,
    pub reversion_risk: bool,
    pub is_demo_mode: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceSelectModalViewProps {
    pub visible: bool,
    pub devices: Vec<Device>,
    pub selected_index: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfirmRemoveDeviceModalViewProps {
    pub visible: bool,
    pub device_id: String,
    pub display_name: String,
    pub confirm_focused: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsModalProps {
    pub authority_picker: AuthorityPickerModalViewProps,
    pub nickname_suggestion: SettingsNicknameModalViewProps,
    pub add_device: AddDeviceModalViewProps,
    pub device_import: DeviceImportModalViewProps,
    pub device_enrollment: DeviceEnrollmentModalViewProps,
    pub device_select: DeviceSelectModalViewProps,
    pub confirm_remove: ConfirmRemoveDeviceModalViewProps,
    pub mfa_setup: GuardianWorkflowModalViewProps,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsViewProps {
    pub focus: TwoPanelFocus,
    pub section: SettingsSection,
    pub selected_index: usize,
    pub mfa_policy: MfaPolicy,
    pub authorities: Vec<AuthorityInfo>,
    pub current_authority_index: usize,
    pub modals: SettingsModalProps,
}

/// Extract SettingsScreen view props from TuiState
pub fn extract_settings_view_props(state: &TuiState) -> SettingsViewProps {
    // Extract modal state from queue (all modals now use queue system)
    let (nickname_suggestion_visible, nickname_suggestion_value) = match state.modal_queue.current()
    {
        Some(QueuedModal::SettingsNicknameSuggestion(s)) => (true, s.value.clone()),
        _ => (false, String::new()),
    };

    // Authority picker modal
    let (authority_picker_visible, authority_picker_contacts, authority_picker_selected) =
        match state.modal_queue.current() {
            Some(QueuedModal::AuthorityPicker(s)) => (
                true,
                s.contacts
                    .iter()
                    .map(|(id, name)| (id.to_string(), name.clone()))
                    .collect(),
                s.selected_index,
            ),
            _ => (false, vec![], 0),
        };

    let (add_device_visible, add_device_name) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsAddDevice(s)) => (true, s.name.clone()),
        _ => (false, String::new()),
    };

    let (device_import_visible, device_import_code) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsDeviceImport(s)) => (true, s.code.clone()),
        _ => (false, String::new()),
    };

    let (
        enrollment_visible,
        enrollment_ceremony_id,
        enrollment_nickname_suggestion,
        enrollment_code,
        enrollment_accepted,
        enrollment_total,
        enrollment_threshold,
        enrollment_is_complete,
        enrollment_has_failed,
        enrollment_error_message,
        enrollment_copied,
        enrollment_agreement_mode,
        enrollment_reversion_risk,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsDeviceEnrollment(s)) => (
            true,
            s.ceremony.ceremony_id.clone().unwrap_or_else(|| {
                warn!("Device enrollment modal missing ceremony id");
                String::new()
            }),
            s.nickname_suggestion.clone(),
            s.enrollment_code.clone(),
            s.ceremony.accepted_count,
            s.ceremony.total_count,
            s.ceremony.threshold,
            s.ceremony.is_complete,
            s.ceremony.has_failed,
            s.ceremony.error_message.clone().unwrap_or_default(),
            s.copied,
            s.ceremony.agreement_mode,
            s.ceremony.reversion_risk,
        ),
        _ => (
            false,
            String::new(),
            String::new(),
            String::new(),
            0,
            0,
            0,
            false,
            false,
            String::new(),
            false,
            AgreementMode::ConsensusFinalized,
            false,
        ),
    };

    let (device_select_visible, device_select_devices, device_select_selected_index) = match state
        .modal_queue
        .current()
    {
        Some(QueuedModal::SettingsDeviceSelect(s)) => (true, s.devices.clone(), s.selected_index),
        _ => (false, vec![], 0),
    };

    let (
        confirm_remove_visible,
        confirm_remove_device_id,
        confirm_remove_display_name,
        confirm_remove_focused,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsRemoveDevice(s)) => (
            true,
            s.device_id.to_string(),
            s.display_name.clone(),
            s.confirm_focused,
        ),
        _ => (false, String::new(), String::new(), false),
    };

    let (
        mfa_visible,
        mfa_step,
        mfa_contacts,
        mfa_selected,
        mfa_focused,
        mfa_k,
        mfa_n,
        mfa_responses,
        mfa_agreement_mode,
        mfa_reversion_risk,
        mfa_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::MfaSetup(s)) => (
            true,
            s.step(),
            s.contacts()
                .map(|contacts| {
                    contacts
                        .iter()
                        .map(|c| GuardianCandidateViewProps {
                            id: c.id.clone(),
                            name: c.name.clone(),
                            is_current_guardian: c.is_current_guardian,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            s.selected_indices().map(|v| v.to_vec()).unwrap_or_default(),
            s.focused_index(),
            s.threshold_k(),
            s.threshold_n(),
            s.ceremony_responses_vec(),
            s.ceremony_agreement_mode(),
            s.ceremony_reversion_risk(),
            s.error().unwrap_or_default().to_string(),
        ),
        _ => (
            false,
            GuardianSetupStep::default(),
            vec![],
            vec![],
            0,
            2,
            3,
            vec![],
            AgreementMode::ConsensusFinalized,
            false,
            String::new(),
        ),
    };

    SettingsViewProps {
        focus: state.settings.focus,
        section: state.settings.section,
        selected_index: state.settings.selected_index,
        mfa_policy: state.settings.mfa_policy,
        // Authority context is app-global, in TuiState root
        authorities: state.authorities.clone(),
        current_authority_index: state.current_authority_index,
        modals: SettingsModalProps {
            authority_picker: AuthorityPickerModalViewProps {
                visible: authority_picker_visible,
                authorities: authority_picker_contacts,
                selected_index: authority_picker_selected,
            },
            nickname_suggestion: SettingsNicknameModalViewProps {
                visible: nickname_suggestion_visible,
                value: nickname_suggestion_value,
            },
            add_device: AddDeviceModalViewProps {
                visible: add_device_visible,
                name: add_device_name,
            },
            device_import: DeviceImportModalViewProps {
                visible: device_import_visible,
                code: device_import_code,
            },
            device_enrollment: DeviceEnrollmentModalViewProps {
                visible: enrollment_visible,
                ceremony_id: enrollment_ceremony_id,
                nickname_suggestion: enrollment_nickname_suggestion,
                code: enrollment_code,
                accepted_count: enrollment_accepted,
                total_count: enrollment_total,
                threshold: enrollment_threshold,
                is_complete: enrollment_is_complete,
                has_failed: enrollment_has_failed,
                error_message: enrollment_error_message,
                copied: enrollment_copied,
                agreement_mode: enrollment_agreement_mode,
                reversion_risk: enrollment_reversion_risk,
                is_demo_mode: !state.settings.demo_mobile_device_id.is_empty(),
            },
            device_select: DeviceSelectModalViewProps {
                visible: device_select_visible,
                devices: device_select_devices,
                selected_index: device_select_selected_index,
            },
            confirm_remove: ConfirmRemoveDeviceModalViewProps {
                visible: confirm_remove_visible,
                device_id: confirm_remove_device_id,
                display_name: confirm_remove_display_name,
                confirm_focused: confirm_remove_focused,
            },
            mfa_setup: GuardianWorkflowModalViewProps {
                visible: mfa_visible,
                step: mfa_step,
                contacts: mfa_contacts,
                selected_indices: mfa_selected,
                focused_index: mfa_focused,
                threshold_k: mfa_k,
                threshold_n: mfa_n,
                ceremony_responses: mfa_responses,
                agreement_mode: mfa_agreement_mode,
                reversion_risk: mfa_reversion_risk,
                error: mfa_error,
            },
        },
    }
}

// ============================================================================
// Neighborhood Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for NeighborhoodScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HomeCreateModalViewProps {
    pub visible: bool,
    pub name: String,
    pub description: String,
    pub active_field: usize,
    pub error: Option<String>,
    pub creating: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModeratorAssignmentModalViewProps {
    pub visible: bool,
    pub contacts: Vec<Contact>,
    pub selected_index: usize,
    pub assign: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AccessOverrideModalViewProps {
    pub visible: bool,
    pub contacts: Vec<Contact>,
    pub selected_index: usize,
    pub level: AccessLevel,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CapabilityConfigModalViewProps {
    pub visible: bool,
    pub full_caps: String,
    pub partial_caps: String,
    pub limited_caps: String,
    pub active_field: usize,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeighborhoodModalProps {
    pub home_create: HomeCreateModalViewProps,
    pub moderator_assignment: ModeratorAssignmentModalViewProps,
    pub access_override: AccessOverrideModalViewProps,
    pub capability_config: CapabilityConfigModalViewProps,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeighborhoodViewProps {
    pub mode: NeighborhoodMode,
    pub detail_focus: DetailFocus,
    pub selected_index: usize,
    pub grid_row: usize,
    pub grid_col: usize,
    pub enter_depth: AccessLevel,
    pub selected_neighborhood: usize,
    pub neighborhood_count: usize,
    pub selected_home: usize,
    pub home_count: usize,
    pub entered_home_id: Option<String>,
    pub selected_channel: usize,
    pub channel_count: usize,
    pub selected_member: usize,
    pub member_count: usize,
    pub input_buffer: String,
    pub message_scroll: usize,
    pub message_count: usize,
    pub moderator_actions_enabled: bool,
    pub modals: NeighborhoodModalProps,
}

/// Extract NeighborhoodScreen view props from TuiState
pub fn extract_neighborhood_view_props(state: &TuiState) -> NeighborhoodViewProps {
    // Home create modal (from queue)
    let (
        home_create_visible,
        home_create_name,
        home_create_description,
        home_create_active_field,
        home_create_error,
        home_create_creating,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::NeighborhoodHomeCreate(s)) => (
            true,
            s.name.clone(),
            s.description.clone(),
            s.active_field,
            s.error.clone(),
            s.creating,
        ),
        _ => (false, String::new(), String::new(), 0, None, false),
    };

    let (
        moderator_modal_visible,
        moderator_modal_contacts,
        moderator_modal_selected_index,
        moderator_modal_assign,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::NeighborhoodModeratorAssignment(s)) => {
            (true, s.contacts.clone(), s.selected_index, s.assign)
        }
        _ => (false, Vec::new(), 0, true),
    };

    let (
        access_override_modal_visible,
        access_override_modal_contacts,
        access_override_modal_selected_index,
        access_override_modal_level,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::NeighborhoodAccessOverride(s)) => {
            (true, s.contacts.clone(), s.selected_index, s.access_level)
        }
        _ => (false, Vec::new(), 0, AccessLevel::Limited),
    };

    let (
        capability_config_modal_visible,
        capability_config_modal_full_caps,
        capability_config_modal_partial_caps,
        capability_config_modal_limited_caps,
        capability_config_modal_active_field,
        capability_config_modal_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::NeighborhoodCapabilityConfig(s)) => (
            true,
            s.full_caps.clone(),
            s.partial_caps.clone(),
            s.limited_caps.clone(),
            s.active_field,
            s.error.clone(),
        ),
        _ => (false, String::new(), String::new(), String::new(), 0, None),
    };

    NeighborhoodViewProps {
        mode: state.neighborhood.mode,
        detail_focus: state.neighborhood.detail_focus,
        selected_index: state.neighborhood.grid.current(),
        grid_row: state.neighborhood.grid.row(),
        grid_col: state.neighborhood.grid.col(),
        enter_depth: state.neighborhood.enter_depth,
        selected_neighborhood: state.neighborhood.selected_neighborhood,
        neighborhood_count: state.neighborhood.neighborhood_count,
        selected_home: state.neighborhood.selected_home,
        home_count: state.neighborhood.home_count,
        entered_home_id: state.neighborhood.entered_home_id.clone(),
        selected_channel: state.neighborhood.selected_channel,
        channel_count: state.neighborhood.channel_count,
        selected_member: state.neighborhood.selected_member,
        member_count: state.neighborhood.member_count,
        input_buffer: state.neighborhood.input_buffer.clone(),
        message_scroll: state.neighborhood.message_scroll,
        message_count: state.neighborhood.message_count,
        moderator_actions_enabled: state.neighborhood.moderator_actions_enabled,
        modals: NeighborhoodModalProps {
            home_create: HomeCreateModalViewProps {
                visible: home_create_visible,
                name: home_create_name,
                description: home_create_description,
                active_field: home_create_active_field,
                error: home_create_error,
                creating: home_create_creating,
            },
            moderator_assignment: ModeratorAssignmentModalViewProps {
                visible: moderator_modal_visible,
                contacts: moderator_modal_contacts,
                selected_index: moderator_modal_selected_index,
                assign: moderator_modal_assign,
            },
            access_override: AccessOverrideModalViewProps {
                visible: access_override_modal_visible,
                contacts: access_override_modal_contacts,
                selected_index: access_override_modal_selected_index,
                level: access_override_modal_level,
            },
            capability_config: CapabilityConfigModalViewProps {
                visible: capability_config_modal_visible,
                full_caps: capability_config_modal_full_caps,
                partial_caps: capability_config_modal_partial_caps,
                limited_caps: capability_config_modal_limited_caps,
                active_field: capability_config_modal_active_field,
                error: capability_config_modal_error,
            },
        },
    }
}

// ============================================================================
// Help Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for HelpScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HelpViewProps {
    pub scroll: usize,
    pub filter: String,
}

/// Extract HelpScreen view props from TuiState
pub fn extract_help_view_props(state: &TuiState) -> HelpViewProps {
    HelpViewProps {
        scroll: state.help.scroll,
        filter: state.help.filter.clone(),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)] // Tests construct state incrementally
mod tests {
    use super::*;

    #[test]
    fn test_chat_view_props_extraction() {
        use crate::tui::state::CreateChannelModalState;

        let mut state = TuiState::new();
        state.chat.insert_mode = true;
        state.chat.focus = ChatFocus::Input;
        state.chat.input_buffer = "test message".to_string();
        state.chat.selected_channel = 5;
        state.chat.message_scroll = 10;
        // Use queue for modal visibility - create modal with name set
        let mut create_modal = CreateChannelModalState::new();
        create_modal.name = "channel-name".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::ChatCreate(create_modal));

        let props = extract_chat_view_props(&state);

        assert!(props.insert_mode, "insert_mode must be extracted");
        assert_eq!(props.focus, ScreenChatFocus::Input);
        assert_eq!(props.input_buffer, "test message");
        assert_eq!(props.selected_channel, 5);
        assert_eq!(props.message_scroll, 10);
        assert!(props.modals.create.visible);
        assert_eq!(props.modals.create.name, "channel-name");
        // info_modal is not active because create_modal is (only one modal at a time)
        assert!(!props.modals.info.visible);
    }

    #[test]
    fn test_chat_info_modal_props_extraction() {
        use crate::tui::state::ChannelInfoModalState;

        let mut state = TuiState::new();
        // Use queue for modal visibility - use factory constructor
        let info_modal = ChannelInfoModalState::for_channel("ch-123", "info-channel", None);
        state.modal_queue.enqueue(QueuedModal::ChatInfo(info_modal));

        let props = extract_chat_view_props(&state);

        assert!(props.modals.info.visible);
        assert_eq!(props.modals.info.channel_name, "info-channel");
    }

    #[test]
    fn test_contacts_view_props_extraction() {
        use crate::tui::state::NicknameModalState;

        let mut state = TuiState::new();
        state.contacts.selected_index = 7;
        state.contacts.filter = "search".to_string();
        // Use queue for modal visibility - use factory constructor
        let mut nickname_modal = NicknameModalState::for_contact("contact-123", "");
        nickname_modal.value = "new-name".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::ContactsNickname(nickname_modal));

        let props = extract_contacts_view_props(&state);

        assert_eq!(props.selected_index, 7);
        assert_eq!(props.filter, "search");
        assert!(props.modals.nickname.visible);
        assert_eq!(props.modals.nickname.contact_id, "contact-123");
        assert_eq!(props.modals.nickname.value, "new-name");
    }

    #[test]
    fn test_notifications_view_props_extraction() {
        let mut state = TuiState::new();
        state.notifications.selected_index = 2;
        state.notifications.focus = TwoPanelFocus::Detail;

        let props = extract_notifications_view_props(&state);

        assert_eq!(props.selected_index, 2);
        assert_eq!(props.focus, TwoPanelFocus::Detail);
    }

    #[test]
    fn test_settings_view_props_extraction() {
        use crate::tui::state::NicknameSuggestionModalState;

        let mut state = TuiState::new();
        state.settings.section = SettingsSection::Devices;
        state.settings.selected_index = 1;
        state.settings.mfa_policy = MfaPolicy::AlwaysRequired;
        // Use queue for modal visibility (only one modal at a time)
        let nickname_suggestion_modal = NicknameSuggestionModalState::with_name("new-nick");
        state
            .modal_queue
            .enqueue(QueuedModal::SettingsNicknameSuggestion(
                nickname_suggestion_modal,
            ));

        let props = extract_settings_view_props(&state);

        assert_eq!(props.section, SettingsSection::Devices);
        assert_eq!(props.selected_index, 1);
        assert_eq!(props.mfa_policy, MfaPolicy::AlwaysRequired);
        assert!(props.modals.nickname_suggestion.visible);
        assert_eq!(props.modals.nickname_suggestion.value, "new-nick");
        // add_device_modal is not active because nickname_suggestion_modal is (only one modal at a time)
        assert!(!props.modals.add_device.visible);
    }

    #[test]
    fn test_settings_add_device_modal_props_extraction() {
        use crate::tui::state::AddDeviceModalState;

        let mut state = TuiState::new();
        // Use queue for modal visibility
        let mut add_device_modal = AddDeviceModalState::new();
        add_device_modal.name = "my-device".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::SettingsAddDevice(add_device_modal));

        let props = extract_settings_view_props(&state);

        assert!(props.modals.add_device.visible);
        assert_eq!(props.modals.add_device.name, "my-device");
    }

    #[test]
    fn test_neighborhood_view_props_extraction() {
        let mut state = TuiState::new();
        // Set up a 4-column grid with 12 items
        state.neighborhood.grid.set_cols(4);
        state.neighborhood.grid.set_count(12);
        // Select index 9 (row 2, col 1 in a 4-column grid)
        state.neighborhood.grid.select(9);

        let props = extract_neighborhood_view_props(&state);

        assert_eq!(props.selected_index, 9);
        assert_eq!(props.grid_row, 2); // 9 / 4 = 2
        assert_eq!(props.grid_col, 1); // 9 % 4 = 1
    }

    #[test]
    fn test_help_view_props_extraction() {
        let mut state = TuiState::new();
        state.help.scroll = 15;
        state.help.filter = "key".to_string();

        let props = extract_help_view_props(&state);

        assert_eq!(props.scroll, 15);
        assert_eq!(props.filter, "key");
    }
}
