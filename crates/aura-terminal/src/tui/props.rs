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
use crate::tui::screens::{BlockFocus as ScreenBlockFocus, ChatFocus as ScreenChatFocus};
use crate::tui::state_machine::{
    BlockFocus, ChatFocus, GuardianCeremonyResponse, GuardianSetupStep, PanelFocus, QueuedModal,
    TuiState,
};
use cfg_if::cfg_if;

// ============================================================================
// Block Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for BlockScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockViewProps {
    pub focus: ScreenBlockFocus,
    pub selected_resident: usize,
    pub message_scroll: usize,
    pub insert_mode: bool,
    pub input_buffer: String,
    pub invite_modal_open: bool,
    pub invite_selection: usize,
}

/// Extract BlockScreen view props from TuiState
///
/// This function extracts all view-related state needed by BlockScreen.
/// Domain data (residents, messages, etc.) is passed separately.
pub fn extract_block_view_props(state: &TuiState) -> BlockViewProps {
    let focus = match state.block.focus {
        BlockFocus::Residents => ScreenBlockFocus::Residents,
        BlockFocus::Messages => ScreenBlockFocus::Messages,
        BlockFocus::Input => ScreenBlockFocus::Input,
    };

    BlockViewProps {
        focus,
        selected_resident: state.block.selected_resident,
        message_scroll: state.block.message_scroll,
        insert_mode: state.block.insert_mode,
        input_buffer: state.block.input_buffer.clone(),
        // Modal visibility from queue, selection from view state (used for navigation)
        invite_modal_open: state.is_block_invite_modal_active(),
        invite_selection: state.block.invite_selection,
    }
}

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
    let (create_visible, create_name, create_topic, create_field) = match state
        .modal_queue
        .current()
    {
        Some(QueuedModal::ChatCreate(s)) => (true, s.name.clone(), s.topic.clone(), s.active_field),
        _ => (false, String::new(), String::new(), 0),
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
    // Petname modal
    pub petname_modal_visible: bool,
    pub petname_modal_contact_id: String,
    pub petname_modal_value: String,
    // Import invitation modal (accept invitation code)
    pub import_modal_visible: bool,
    pub import_modal_code: String,
    pub import_modal_importing: bool,
    // Create invitation modal (send invitation)
    pub create_modal_visible: bool,
    pub create_modal_type_index: usize,
    pub create_modal_message: String,
    pub create_modal_ttl_hours: u64,
    pub create_modal_step: usize,
    // Code display modal (show generated code)
    pub code_modal_visible: bool,
    pub code_modal_invitation_id: String,
    pub code_modal_code: String,
    pub code_modal_loading: bool,
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
    let focus = match state.contacts.focus {
        PanelFocus::List => TwoPanelFocus::List,
        PanelFocus::Detail => TwoPanelFocus::Detail,
    };

    // Extract modal state from queue (all modals now use queue system)
    let (petname_visible, petname_contact_id, petname_value) = match state.modal_queue.current() {
        Some(QueuedModal::ContactsPetname(s)) => (true, s.contact_id.clone(), s.value.clone()),
        _ => (false, String::new(), String::new()),
    };

    let (import_visible, import_code, import_importing) = match state.modal_queue.current() {
        Some(QueuedModal::ContactsImport(s)) => (true, s.code.clone(), s.importing),
        _ => (false, String::new(), false),
    };

    let (create_visible, create_type_index, create_message, create_ttl, create_step) =
        match state.modal_queue.current() {
            Some(QueuedModal::ContactsCreate(s)) => {
                (true, s.type_index, s.message.clone(), s.ttl_hours, s.step)
            }
            _ => (false, 0, String::new(), 24, 0),
        };

    let (code_visible, code_invitation_id, code_code, code_loading) =
        match state.modal_queue.current() {
            Some(QueuedModal::ContactsCode(s)) => {
                (true, s.invitation_id.clone(), s.code.clone(), s.loading)
            }
            _ => (false, String::new(), String::new(), false),
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

    cfg_if! {
        if #[cfg(feature = "development")] {
            ContactsViewProps {
                focus,
                selected_index: state.contacts.selected_index,
                filter: state.contacts.filter.clone(),
                // Petname modal (from queue)
                petname_modal_visible: petname_visible,
                petname_modal_contact_id: petname_contact_id,
                petname_modal_value: petname_value,
                // Import modal (from queue)
                import_modal_visible: import_visible,
                import_modal_code: import_code,
                import_modal_importing: import_importing,
                // Create modal (from queue)
                create_modal_visible: create_visible,
                create_modal_type_index: create_type_index,
                create_modal_message: create_message,
                create_modal_ttl_hours: create_ttl,
                create_modal_step: create_step,
                // Code display modal (from queue)
                code_modal_visible: code_visible,
                code_modal_invitation_id: code_invitation_id,
                code_modal_code: code_code,
                code_modal_loading: code_loading,
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
                // Demo mode
                demo_mode: !state.contacts.demo_alice_code.is_empty(),
                demo_alice_code: state.contacts.demo_alice_code.clone(),
                demo_carol_code: state.contacts.demo_carol_code.clone(),
            }
        } else {
            ContactsViewProps {
                focus,
                selected_index: state.contacts.selected_index,
                filter: state.contacts.filter.clone(),
                // Petname modal (from queue)
                petname_modal_visible: petname_visible,
                petname_modal_contact_id: petname_contact_id,
                petname_modal_value: petname_value,
                // Import modal (from queue)
                import_modal_visible: import_visible,
                import_modal_code: import_code,
                import_modal_importing: import_importing,
                // Create modal (from queue)
                create_modal_visible: create_visible,
                create_modal_type_index: create_type_index,
                create_modal_message: create_message,
                create_modal_ttl_hours: create_ttl,
                create_modal_step: create_step,
                // Code display modal (from queue)
                code_modal_visible: code_visible,
                code_modal_invitation_id: code_invitation_id,
                code_modal_code: code_code,
                code_modal_loading: code_loading,
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
            }
        }
    }
}

// ============================================================================
// Invitations Screen Props Extraction
// ============================================================================

use crate::tui::types::InvitationFilter;

/// View state extracted from TuiState for InvitationsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InvitationsViewProps {
    pub focus: TwoPanelFocus,
    pub selected_index: usize,
    pub filter: InvitationFilter,
    // Create modal
    pub create_modal_visible: bool,
    pub create_modal_type_index: usize,
    pub create_modal_message: String,
    pub create_modal_ttl_hours: u64,
    pub create_modal_step: usize,
    // Import modal
    pub import_modal_visible: bool,
    pub import_modal_code: String,
    pub import_modal_importing: bool,
    // Code display modal
    pub code_modal_visible: bool,
    pub code_modal_invitation_id: String,
    pub code_modal_code: String,
    pub code_modal_loading: bool,
}

/// Extract InvitationsScreen view props from TuiState
pub fn extract_invitations_view_props(state: &TuiState) -> InvitationsViewProps {
    let focus = match state.invitations.focus {
        PanelFocus::List => TwoPanelFocus::List,
        PanelFocus::Detail => TwoPanelFocus::Detail,
    };

    // Extract modal state from queue (all modals now use queue system)
    let (create_visible, create_type_index, create_message, create_ttl, create_step) =
        match state.modal_queue.current() {
            Some(QueuedModal::InvitationsCreate(s)) => {
                (true, s.type_index, s.message.clone(), s.ttl_hours, s.step)
            }
            _ => (false, 0, String::new(), 24, 0),
        };

    let (import_visible, import_code, import_importing) = match state.modal_queue.current() {
        Some(QueuedModal::InvitationsImport(s)) => (true, s.code.clone(), s.importing),
        _ => (false, String::new(), false),
    };

    let (code_visible, code_invitation_id, code_code, code_loading) =
        match state.modal_queue.current() {
            Some(QueuedModal::InvitationsCode(s)) => {
                (true, s.invitation_id.clone(), s.code.clone(), s.loading)
            }
            _ => (false, String::new(), String::new(), false),
        };

    InvitationsViewProps {
        focus,
        selected_index: state.invitations.selected_index,
        filter: state.invitations.filter,
        // Create modal (from queue)
        create_modal_visible: create_visible,
        create_modal_type_index: create_type_index,
        create_modal_message: create_message,
        create_modal_ttl_hours: create_ttl,
        create_modal_step: create_step,
        // Import modal (from queue)
        import_modal_visible: import_visible,
        import_modal_code: import_code,
        import_modal_importing: import_importing,
        // Code display modal (from queue)
        code_modal_visible: code_visible,
        code_modal_invitation_id: code_invitation_id,
        code_modal_code: code_code,
        code_modal_loading: code_loading,
    }
}

// ============================================================================
// Recovery Screen Props Extraction
// ============================================================================

use crate::tui::types::RecoveryTab;

/// View state extracted from TuiState for RecoveryScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecoveryViewProps {
    pub tab: RecoveryTab,
    pub selected_index: usize,
}

/// Extract RecoveryScreen view props from TuiState
pub fn extract_recovery_view_props(state: &TuiState) -> RecoveryViewProps {
    RecoveryViewProps {
        tab: state.recovery.tab,
        selected_index: state.recovery.selected_index,
    }
}

// ============================================================================
// Settings Screen Props Extraction
// ============================================================================

use crate::tui::types::{MfaPolicy, SettingsSection};

/// View state extracted from TuiState for SettingsScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsViewProps {
    pub section: SettingsSection,
    pub selected_index: usize,
    pub mfa_policy: MfaPolicy,
    // Nickname modal
    pub nickname_modal_visible: bool,
    pub nickname_modal_value: String,
    // Threshold modal
    pub threshold_modal_visible: bool,
    pub threshold_modal_k: u8,
    pub threshold_modal_n: u8,
    pub threshold_modal_active_field: usize,
    // Add device modal
    pub add_device_modal_visible: bool,
    pub add_device_modal_name: String,
    // Confirm remove modal
    pub confirm_remove_modal_visible: bool,
    pub confirm_remove_modal_device_id: String,
    pub confirm_remove_modal_device_name: String,
    pub confirm_remove_modal_confirm_focused: bool,
}

/// Extract SettingsScreen view props from TuiState
pub fn extract_settings_view_props(state: &TuiState) -> SettingsViewProps {
    // Extract modal state from queue (all modals now use queue system)
    let (nickname_visible, nickname_value) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsNickname(s)) => (true, s.value.clone()),
        _ => (false, String::new()),
    };

    let (threshold_visible, threshold_k, threshold_n, threshold_active_field) =
        match state.modal_queue.current() {
            Some(QueuedModal::SettingsThreshold(s)) => (true, s.k, s.n, s.active_field),
            _ => (false, 2, 3, 0),
        };

    let (add_device_visible, add_device_name) = match state.modal_queue.current() {
        Some(QueuedModal::SettingsAddDevice(s)) => (true, s.name.clone()),
        _ => (false, String::new()),
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

    SettingsViewProps {
        section: state.settings.section,
        selected_index: state.settings.selected_index,
        mfa_policy: state.settings.mfa_policy,
        // Nickname modal (from queue)
        nickname_modal_visible: nickname_visible,
        nickname_modal_value: nickname_value,
        // Threshold modal (from queue)
        threshold_modal_visible: threshold_visible,
        threshold_modal_k: threshold_k,
        threshold_modal_n: threshold_n,
        threshold_modal_active_field: threshold_active_field,
        // Add device modal (from queue)
        add_device_modal_visible: add_device_visible,
        add_device_modal_name: add_device_name,
        // Confirm remove modal (from queue)
        confirm_remove_modal_visible: confirm_remove_visible,
        confirm_remove_modal_device_id: confirm_remove_device_id,
        confirm_remove_modal_device_name: confirm_remove_device_name,
        confirm_remove_modal_confirm_focused: confirm_remove_focused,
    }
}

// ============================================================================
// Neighborhood Screen Props Extraction
// ============================================================================

/// View state extracted from TuiState for NeighborhoodScreen
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeighborhoodViewProps {
    pub selected_index: usize,
    pub grid_row: usize,
    pub grid_col: usize,
}

/// Extract NeighborhoodScreen view props from TuiState
pub fn extract_neighborhood_view_props(state: &TuiState) -> NeighborhoodViewProps {
    NeighborhoodViewProps {
        selected_index: state.neighborhood.grid.current(),
        grid_row: state.neighborhood.grid.row(),
        grid_col: state.neighborhood.grid.col(),
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
mod tests {
    use super::*;

    #[test]
    fn test_block_view_props_extraction() {
        use crate::tui::state_machine::ContactSelectModalState;

        let mut state = TuiState::new();
        state.block.insert_mode = true;
        state.block.focus = BlockFocus::Input;
        state.block.input_buffer = "hello".to_string();
        state.block.selected_resident = 3;
        // Use queue for modal visibility
        state
            .modal_queue
            .enqueue(QueuedModal::BlockInvite(ContactSelectModalState::default()));
        state.block.invite_selection = 2;

        let props = extract_block_view_props(&state);

        assert!(props.insert_mode, "insert_mode must be extracted");
        assert_eq!(props.focus, ScreenBlockFocus::Input);
        assert_eq!(props.input_buffer, "hello");
        assert_eq!(props.selected_resident, 3);
        assert!(props.invite_modal_open);
        assert_eq!(props.invite_selection, 2);
    }

    #[test]
    fn test_chat_view_props_extraction() {
        use crate::tui::state_machine::CreateChannelModalState;

        let mut state = TuiState::new();
        state.chat.insert_mode = true;
        state.chat.focus = ChatFocus::Input;
        state.chat.input_buffer = "test message".to_string();
        state.chat.selected_channel = 5;
        state.chat.message_scroll = 10;
        // Use queue for modal visibility
        let mut create_modal = CreateChannelModalState::default();
        create_modal.visible = true;
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
        // Use queue for modal visibility
        let mut info_modal = ChannelInfoModalState::default();
        info_modal.visible = true;
        info_modal.channel_name = "info-channel".to_string();
        state.modal_queue.enqueue(QueuedModal::ChatInfo(info_modal));

        let props = extract_chat_view_props(&state);

        assert!(props.info_modal_visible);
        assert_eq!(props.info_modal_channel_name, "info-channel");
    }

    #[test]
    fn test_contacts_view_props_extraction() {
        use crate::tui::state_machine::PetnameModalState;

        let mut state = TuiState::new();
        state.contacts.selected_index = 7;
        state.contacts.filter = "search".to_string();
        // Use queue for modal visibility
        let mut petname_modal = PetnameModalState::default();
        petname_modal.visible = true;
        petname_modal.contact_id = "contact-123".to_string();
        petname_modal.value = "new-name".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::ContactsPetname(petname_modal));

        let props = extract_contacts_view_props(&state);

        assert_eq!(props.selected_index, 7);
        assert_eq!(props.filter, "search");
        assert!(props.petname_modal_visible);
        assert_eq!(props.petname_modal_contact_id, "contact-123");
        assert_eq!(props.petname_modal_value, "new-name");
    }

    #[test]
    fn test_invitations_view_props_extraction() {
        use crate::tui::state_machine::ImportInvitationModalState;

        let mut state = TuiState::new();
        state.invitations.selected_index = 3;
        state.invitations.filter = InvitationFilter::Sent;
        // Use queue for modal visibility (only one modal at a time)
        let mut import_modal = ImportInvitationModalState::default();
        import_modal.visible = true;
        import_modal.code = "ABC123".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::InvitationsImport(import_modal));

        let props = extract_invitations_view_props(&state);

        assert_eq!(props.selected_index, 3);
        assert_eq!(props.filter, InvitationFilter::Sent);
        // Only import modal is active (not create)
        assert!(!props.create_modal_visible);
        assert!(props.import_modal_visible);
        assert_eq!(props.import_modal_code, "ABC123");
    }

    #[test]
    fn test_recovery_view_props_extraction() {
        let mut state = TuiState::new();
        state.recovery.tab = RecoveryTab::Requests;
        state.recovery.selected_index = 2;

        let props = extract_recovery_view_props(&state);

        assert_eq!(props.tab, RecoveryTab::Requests);
        assert_eq!(props.selected_index, 2);
    }

    #[test]
    fn test_settings_view_props_extraction() {
        use crate::tui::state_machine::NicknameModalState;

        let mut state = TuiState::new();
        state.settings.section = SettingsSection::Devices;
        state.settings.selected_index = 1;
        state.settings.mfa_policy = MfaPolicy::AlwaysRequired;
        // Use queue for modal visibility (only one modal at a time)
        let mut nickname_modal = NicknameModalState::default();
        nickname_modal.visible = true;
        nickname_modal.value = "new-nick".to_string();
        state
            .modal_queue
            .enqueue(QueuedModal::SettingsNickname(nickname_modal));

        let props = extract_settings_view_props(&state);

        assert_eq!(props.section, SettingsSection::Devices);
        assert_eq!(props.selected_index, 1);
        assert_eq!(props.mfa_policy, MfaPolicy::AlwaysRequired);
        assert!(props.nickname_modal_visible);
        assert_eq!(props.nickname_modal_value, "new-nick");
        // add_device_modal is not active because nickname_modal is (only one modal at a time)
        assert!(!props.add_device_modal_visible);
    }

    #[test]
    fn test_settings_add_device_modal_props_extraction() {
        use crate::tui::state_machine::AddDeviceModalState;

        let mut state = TuiState::new();
        // Use queue for modal visibility
        let mut add_device_modal = AddDeviceModalState::default();
        add_device_modal.visible = true;
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
