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
    ChatFocus, CreateInvitationField, DetailFocus, GuardianCeremonyResponse, GuardianSetupStep,
    NeighborhoodMode, QueuedModal, TuiState,
};
use crate::tui::types::TraversalDepth;
use tracing::warn;

// ============================================================================
// Chat Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for ChatScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChatViewProps {
    pub focus: ScreenChatFocus,
    pub selected_channel: usize,
    pub message_scroll: usize,
    pub insert_mode: bool,
    pub input_buffer: String,
    // Create modal
    pub create_modal_visible: bool,
    pub create_modal_name: String,
    pub create_modal_topic: String,
    pub create_modal_active_field: usize,
    pub create_modal_member_count: usize,
    pub create_modal_step: crate::tui::state::CreateChannelStep,
    pub create_modal_contacts: Vec<(String, String)>,
    pub create_modal_selected_indices: Vec<usize>,
    pub create_modal_focused_index: usize,
    pub create_modal_threshold_k: u8,
    pub create_modal_threshold_n: u8,
    pub create_modal_status: String,
    pub create_modal_error: String,
    // Topic modal
    pub topic_modal_visible: bool,
    pub topic_modal_value: String,
    // Info modal
    pub info_modal_visible: bool,
    pub info_modal_channel_name: String,
    pub info_modal_topic: String,
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
    ) =
        match state.modal_queue.current() {
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

    let (topic_visible, topic_value) = match state.modal_queue.current() {
        Some(QueuedModal::ChatTopic(s)) => (true, s.value.clone()),
        _ => (false, String::new()),
    };

    let (info_visible, info_channel_name, info_topic) = match state.modal_queue.current() {
        Some(QueuedModal::ChatInfo(s)) => (true, s.channel_name.clone(), s.topic.clone()),
        _ => (false, String::new(), String::new()),
    };

    ChatViewProps {
        focus,
        selected_channel: state.chat.selected_channel,
        message_scroll: state.chat.message_scroll,
        insert_mode: state.chat.insert_mode,
        input_buffer: state.chat.input_buffer.clone(),
        // Create modal (from queue)
        create_modal_visible: create_visible,
        create_modal_name: create_name,
        create_modal_topic: create_topic,
        create_modal_active_field: create_field,
        create_modal_member_count: create_member_count,
        create_modal_step: create_step,
        create_modal_contacts: create_contacts,
        create_modal_selected_indices: create_selected,
        create_modal_focused_index: create_focused,
        create_modal_threshold_k: create_threshold_k,
        create_modal_threshold_n: create_threshold_n,
        create_modal_status: create_status,
        create_modal_error: create_error,
        // Topic modal (from queue)
        topic_modal_visible: topic_visible,
        topic_modal_value: topic_value,
        // Info modal (from queue)
        info_modal_visible: info_visible,
        info_modal_channel_name: info_channel_name,
        info_modal_topic: info_topic,
    }
}

// ============================================================================
// Contacts Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for ContactsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContactsViewProps {
    pub focus: TwoPanelFocus,
    pub selected_index: usize,
    pub filter: String,
    // Nickname modal
    pub nickname_modal_visible: bool,
    pub nickname_modal_contact_id: String,
    pub nickname_modal_value: String,
    pub nickname_modal_suggested_name: Option<String>,
    // Import invitation modal (accept invitation code)
    pub import_modal_visible: bool,
    pub import_modal_code: String,
    pub import_modal_importing: bool,
    // Create invitation modal (send invitation)
    pub create_modal_visible: bool,
    pub create_modal_receiver_id: String,
    pub create_modal_receiver_name: String,
    pub create_modal_type_index: usize,
    pub create_modal_message: String,
    pub create_modal_ttl_hours: u64,
    pub create_modal_focused_field: CreateInvitationField,
    // Code display modal (show generated code)
    pub code_modal_visible: bool,
    pub code_modal_invitation_id: String,
    pub code_modal_code: String,
    pub code_modal_loading: bool,
    pub code_modal_copied: bool,
    // Guardian setup modal
    pub guardian_setup_modal_visible: bool,
    pub guardian_setup_modal_step: GuardianSetupStep,
    pub guardian_setup_modal_contacts: Vec<GuardianCandidateViewProps>,
    pub guardian_setup_modal_selected_indices: Vec<usize>,
    pub guardian_setup_modal_focused_index: usize,
    pub guardian_setup_modal_threshold_k: u8,
    pub guardian_setup_modal_threshold_n: u8,
    pub guardian_setup_modal_ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>,
    pub guardian_setup_modal_error: String,
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

    // Extract modal state from queue (all modals now use queue system)
    let (nickname_visible, nickname_contact_id, nickname_value, nickname_suggested) =
        match state.modal_queue.current() {
            Some(QueuedModal::ContactsNickname(s)) => (
                true,
                s.contact_id.clone(),
                s.value.clone(),
                s.suggested_name.clone(),
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
            Some(QueuedModal::ContactsCode(s)) => {
                (true, s.invitation_id.clone(), s.code.clone(), s.loading, s.copied)
            }
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
        guardian_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::GuardianSetup(s)) => (
            true,
            s.step.clone(),
            s.contacts
                .iter()
                .map(|c| GuardianCandidateViewProps {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    is_current_guardian: c.is_current_guardian,
                })
                .collect(),
            s.selected_indices.clone(),
            s.focused_index,
            s.threshold_k,
            s.threshold_n(),
            s.ceremony_responses.clone(),
            s.error.clone().unwrap_or_default(),
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
            String::new(),
        ),
    };

    ContactsViewProps {
        focus,
        selected_index: state.contacts.selected_index,
        filter: state.contacts.filter.clone(),
        // Nickname modal (from queue)
        nickname_modal_visible: nickname_visible,
        nickname_modal_contact_id: nickname_contact_id,
        nickname_modal_value: nickname_value,
        nickname_modal_suggested_name: nickname_suggested,
        // Import modal (from queue)
        import_modal_visible: import_visible,
        import_modal_code: import_code,
        import_modal_importing: import_importing,
        // Create modal (from queue)
        create_modal_visible: create_visible,
        create_modal_receiver_id: create_receiver_id,
        create_modal_receiver_name: create_receiver_name,
        create_modal_type_index: create_type_index,
        create_modal_message: create_message,
        create_modal_ttl_hours: create_ttl,
        create_modal_focused_field: create_focused_field,
        // Code display modal (from queue)
        code_modal_visible: code_visible,
        code_modal_invitation_id: code_invitation_id,
        code_modal_code: code_code,
        code_modal_loading: code_loading,
        code_modal_copied: code_copied,
        // Guardian setup modal (from queue)
        guardian_setup_modal_visible: guardian_visible,
        guardian_setup_modal_step: guardian_step,
        guardian_setup_modal_contacts: guardian_contacts,
        guardian_setup_modal_selected_indices: guardian_selected,
        guardian_setup_modal_focused_index: guardian_focused,
        guardian_setup_modal_threshold_k: guardian_k,
        guardian_setup_modal_threshold_n: guardian_n,
        guardian_setup_modal_ceremony_responses: guardian_responses,
        guardian_setup_modal_error: guardian_error,
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

use crate::tui::types::{AuthorityInfo, AuthoritySubSection, MfaPolicy, SettingsSection};

/// View state extracted from TuiState for SettingsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsViewProps {
    pub focus: TwoPanelFocus,
    pub section: SettingsSection,
    pub selected_index: usize,
    pub mfa_policy: MfaPolicy,
    // Authority panel state
    pub authority_sub_section: AuthoritySubSection,
    pub authorities: Vec<AuthorityInfo>,
    pub current_authority_index: usize,
    // Authority picker modal
    pub authority_picker_modal_visible: bool,
    pub authority_picker_modal_contacts: Vec<(String, String)>,
    pub authority_picker_modal_selected_index: usize,
    // Display name modal (user's own display name)
    pub display_name_modal_visible: bool,
    pub display_name_modal_value: String,
    // Add device modal
    pub add_device_modal_visible: bool,
    pub add_device_modal_name: String,
    // Import device enrollment code modal
    pub device_import_modal_visible: bool,
    pub device_import_modal_code: String,
    // Device enrollment ceremony modal
    pub device_enrollment_modal_visible: bool,
    pub device_enrollment_modal_ceremony_id: String,
    pub device_enrollment_modal_device_name: String,
    pub device_enrollment_modal_code: String,
    pub device_enrollment_modal_accepted_count: u16,
    pub device_enrollment_modal_total_count: u16,
    pub device_enrollment_modal_threshold: u16,
    pub device_enrollment_modal_is_complete: bool,
    pub device_enrollment_modal_has_failed: bool,
    pub device_enrollment_modal_error_message: String,
    pub device_enrollment_modal_copied: bool,
    // Confirm remove modal
    pub confirm_remove_modal_visible: bool,
    pub confirm_remove_modal_device_id: String,
    pub confirm_remove_modal_device_name: String,
    pub confirm_remove_modal_confirm_focused: bool,
    // MFA setup modal (wizard-based)
    pub mfa_setup_modal_visible: bool,
    pub mfa_setup_modal_step: GuardianSetupStep,
    pub mfa_setup_modal_contacts: Vec<GuardianCandidateViewProps>,
    pub mfa_setup_modal_selected_indices: Vec<usize>,
    pub mfa_setup_modal_focused_index: usize,
    pub mfa_setup_modal_threshold_k: u8,
    pub mfa_setup_modal_threshold_n: u8,
    pub mfa_setup_modal_ceremony_responses: Vec<(String, String, GuardianCeremonyResponse)>,
    pub mfa_setup_modal_error: String,
}

/// Extract SettingsScreen view props from TuiState
pub fn extract_settings_view_props(state: &TuiState) -> SettingsViewProps {
    // Extract modal state from queue (all modals now use queue system)
    let (display_name_visible, display_name_value) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsDisplayName(s)) => (true, s.value.clone()),
        _ => (false, String::new()),
    };

    // Authority picker modal
    let (authority_picker_visible, authority_picker_contacts, authority_picker_selected) =
        match state.modal_queue.current() {
            Some(QueuedModal::AuthorityPicker(s)) => (
                true,
                s.contacts.clone(),
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
        enrollment_device_name,
        enrollment_code,
        enrollment_accepted,
        enrollment_total,
        enrollment_threshold,
        enrollment_is_complete,
        enrollment_has_failed,
        enrollment_error_message,
        enrollment_copied,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsDeviceEnrollment(s)) => (
            true,
            s.ceremony.ceremony_id.clone().unwrap_or_else(|| {
                warn!("Device enrollment modal missing ceremony id");
                String::new()
            }),
            s.device_name.clone(),
            s.enrollment_code.clone(),
            s.ceremony.accepted_count,
            s.ceremony.total_count,
            s.ceremony.threshold,
            s.ceremony.is_complete,
            s.ceremony.has_failed,
            s.ceremony.error_message.clone().unwrap_or_default(),
            s.copied,
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
        ),
    };

    let (
        confirm_remove_visible,
        confirm_remove_device_id,
        confirm_remove_device_name,
        confirm_remove_focused,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsRemoveDevice(s)) => (
            true,
            s.device_id.clone(),
            s.device_name.clone(),
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
        mfa_error,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::MfaSetup(s)) => (
            true,
            s.step.clone(),
            s.contacts
                .iter()
                .map(|c| GuardianCandidateViewProps {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    is_current_guardian: c.is_current_guardian,
                })
                .collect(),
            s.selected_indices.clone(),
            s.focused_index,
            s.threshold_k,
            s.threshold_n(),
            s.ceremony_responses.clone(),
            s.error.clone().unwrap_or_default(),
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
            String::new(),
        ),
    };

    SettingsViewProps {
        focus: state.settings.focus,
        section: state.settings.section,
        selected_index: state.settings.selected_index,
        mfa_policy: state.settings.mfa_policy,
        // Authority panel state
        authority_sub_section: state.settings.authority_sub_section,
        authorities: state.settings.authorities.clone(),
        current_authority_index: state.settings.current_authority_index,
        // Authority picker modal (from queue)
        authority_picker_modal_visible: authority_picker_visible,
        authority_picker_modal_contacts: authority_picker_contacts,
        authority_picker_modal_selected_index: authority_picker_selected,
        // Display name modal (from queue)
        display_name_modal_visible: display_name_visible,
        display_name_modal_value: display_name_value,
        // Add device modal (from queue)
        add_device_modal_visible: add_device_visible,
        add_device_modal_name: add_device_name,
        // Import device enrollment code modal (from queue)
        device_import_modal_visible: device_import_visible,
        device_import_modal_code: device_import_code,
        // Device enrollment ceremony modal (from queue)
        device_enrollment_modal_visible: enrollment_visible,
        device_enrollment_modal_ceremony_id: enrollment_ceremony_id,
        device_enrollment_modal_device_name: enrollment_device_name,
        device_enrollment_modal_code: enrollment_code,
        device_enrollment_modal_accepted_count: enrollment_accepted,
        device_enrollment_modal_total_count: enrollment_total,
        device_enrollment_modal_threshold: enrollment_threshold,
        device_enrollment_modal_is_complete: enrollment_is_complete,
        device_enrollment_modal_has_failed: enrollment_has_failed,
        device_enrollment_modal_error_message: enrollment_error_message,
        device_enrollment_modal_copied: enrollment_copied,
        // Confirm remove modal (from queue)
        confirm_remove_modal_visible: confirm_remove_visible,
        confirm_remove_modal_device_id: confirm_remove_device_id,
        confirm_remove_modal_device_name: confirm_remove_device_name,
        confirm_remove_modal_confirm_focused: confirm_remove_focused,
        // MFA setup modal (from queue)
        mfa_setup_modal_visible: mfa_visible,
        mfa_setup_modal_step: mfa_step,
        mfa_setup_modal_contacts: mfa_contacts,
        mfa_setup_modal_selected_indices: mfa_selected,
        mfa_setup_modal_focused_index: mfa_focused,
        mfa_setup_modal_threshold_k: mfa_k,
        mfa_setup_modal_threshold_n: mfa_n,
        mfa_setup_modal_ceremony_responses: mfa_responses,
        mfa_setup_modal_error: mfa_error,
    }
}

// ============================================================================
// Neighborhood Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for NeighborhoodScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeighborhoodViewProps {
    pub mode: NeighborhoodMode,
    pub detail_focus: DetailFocus,
    pub selected_index: usize,
    pub grid_row: usize,
    pub grid_col: usize,
    pub enter_depth: TraversalDepth,
    pub selected_neighborhood: usize,
    pub neighborhood_count: usize,
    pub selected_block: usize,
    pub block_count: usize,
    pub entered_block_id: Option<String>,
    pub selected_channel: usize,
    pub channel_count: usize,
    pub selected_resident: usize,
    pub resident_count: usize,
    pub insert_mode: bool,
    pub input_buffer: String,
    pub message_scroll: usize,
    pub message_count: usize,
    pub steward_actions_enabled: bool,
    // Block create modal
    pub block_create_modal_visible: bool,
    pub block_create_modal_name: String,
    pub block_create_modal_description: String,
    pub block_create_modal_active_field: usize,
    pub block_create_modal_error: Option<String>,
    pub block_create_modal_creating: bool,
}

/// Extract NeighborhoodScreen view props from TuiState
pub fn extract_neighborhood_view_props(state: &TuiState) -> NeighborhoodViewProps {
    // Block create modal (from queue)
    let (
        block_create_visible,
        block_create_name,
        block_create_description,
        block_create_active_field,
        block_create_error,
        block_create_creating,
    ) = match state.modal_queue.current() {
        Some(QueuedModal::NeighborhoodBlockCreate(s)) => (
            true,
            s.name.clone(),
            s.description.clone(),
            s.active_field,
            s.error.clone(),
            s.creating,
        ),
        _ => (false, String::new(), String::new(), 0, None, false),
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
        selected_block: state.neighborhood.selected_block,
        block_count: state.neighborhood.block_count,
        entered_block_id: state.neighborhood.entered_block_id.clone(),
        selected_channel: state.neighborhood.selected_channel,
        channel_count: state.neighborhood.channel_count,
        selected_resident: state.neighborhood.selected_resident,
        resident_count: state.neighborhood.resident_count,
        insert_mode: state.neighborhood.insert_mode,
        input_buffer: state.neighborhood.input_buffer.clone(),
        message_scroll: state.neighborhood.message_scroll,
        message_count: state.neighborhood.message_count,
        steward_actions_enabled: state.neighborhood.steward_actions_enabled,
        // Block create modal
        block_create_modal_visible: block_create_visible,
        block_create_modal_name: block_create_name,
        block_create_modal_description: block_create_description,
        block_create_modal_active_field: block_create_active_field,
        block_create_modal_error: block_create_error,
        block_create_modal_creating: block_create_creating,
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
        use crate::tui::state_machine::CreateChannelModalState;

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
        assert!(props.create_modal_visible);
        assert_eq!(props.create_modal_name, "channel-name");
        // info_modal is not active because create_modal is (only one modal at a time)
        assert!(!props.info_modal_visible);
    }

    #[test]
    fn test_chat_info_modal_props_extraction() {
        use crate::tui::state_machine::ChannelInfoModalState;

        let mut state = TuiState::new();
        // Use queue for modal visibility - use factory constructor
        let info_modal = ChannelInfoModalState::for_channel("ch-123", "info-channel", None);
        state.modal_queue.enqueue(QueuedModal::ChatInfo(info_modal));

        let props = extract_chat_view_props(&state);

        assert!(props.info_modal_visible);
        assert_eq!(props.info_modal_channel_name, "info-channel");
    }

    #[test]
    fn test_contacts_view_props_extraction() {
        use crate::tui::state_machine::NicknameModalState;

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
        assert!(props.nickname_modal_visible);
        assert_eq!(props.nickname_modal_contact_id, "contact-123");
        assert_eq!(props.nickname_modal_value, "new-name");
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
        use crate::tui::state_machine::DisplayNameModalState;

        let mut state = TuiState::new();
        state.settings.section = SettingsSection::Devices;
        state.settings.selected_index = 1;
        state.settings.mfa_policy = MfaPolicy::AlwaysRequired;
        // Use queue for modal visibility (only one modal at a time)
        let display_name_modal = DisplayNameModalState::with_name("new-nick");
        state
            .modal_queue
            .enqueue(QueuedModal::SettingsDisplayName(display_name_modal));

        let props = extract_settings_view_props(&state);

        assert_eq!(props.section, SettingsSection::Devices);
        assert_eq!(props.selected_index, 1);
        assert_eq!(props.mfa_policy, MfaPolicy::AlwaysRequired);
        assert!(props.display_name_modal_visible);
        assert_eq!(props.display_name_modal_value, "new-nick");
        // add_device_modal is not active because display_name_modal is (only one modal at a time)
        assert!(!props.add_device_modal_visible);
    }

    #[test]
    fn test_settings_add_device_modal_props_extraction() {
        use crate::tui::state_machine::AddDeviceModalState;

        let mut state = TuiState::new();
        // Use queue for modal visibility
        let mut add_device_modal = AddDeviceModalState::new();
        add_device_modal.name = "my-device".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::SettingsAddDevice(add_device_modal));

        let props = extract_settings_view_props(&state);

        assert!(props.add_device_modal_visible);
        assert_eq!(props.add_device_modal_name, "my-device");
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
