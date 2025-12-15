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
    BlockFocus, ChatFocus, GuardianCeremonyResponse, GuardianSetupStep, PanelFocus, TuiState,
};

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
        invite_modal_open: state.block.invite_modal_open,
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

    ChatViewProps {
        focus,
        selected_channel: state.chat.selected_channel,
        message_scroll: state.chat.message_scroll,
        insert_mode: state.chat.insert_mode,
        input_buffer: state.chat.input_buffer.clone(),
        // Create modal
        create_modal_visible: state.chat.create_modal.visible,
        create_modal_name: state.chat.create_modal.name.clone(),
        create_modal_topic: state.chat.create_modal.topic.clone(),
        create_modal_active_field: state.chat.create_modal.active_field,
        // Topic modal
        topic_modal_visible: state.chat.topic_modal.visible,
        topic_modal_value: state.chat.topic_modal.value.clone(),
        // Info modal
        info_modal_visible: state.chat.info_modal.visible,
        info_modal_channel_name: state.chat.info_modal.channel_name.clone(),
        info_modal_topic: state.chat.info_modal.topic.clone(),
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
    pub demo_mode: bool,
    pub demo_alice_code: String,
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

    ContactsViewProps {
        focus,
        selected_index: state.contacts.selected_index,
        filter: state.contacts.filter.clone(),
        // Petname modal
        petname_modal_visible: state.contacts.petname_modal.visible,
        petname_modal_contact_id: state.contacts.petname_modal.contact_id.clone(),
        petname_modal_value: state.contacts.petname_modal.value.clone(),
        // Import modal
        import_modal_visible: state.contacts.import_modal.visible,
        import_modal_code: state.contacts.import_modal.code.clone(),
        import_modal_importing: state.contacts.import_modal.importing,
        // Create modal
        create_modal_visible: state.contacts.create_modal.visible,
        create_modal_type_index: state.contacts.create_modal.type_index,
        create_modal_message: state.contacts.create_modal.message.clone(),
        create_modal_ttl_hours: state.contacts.create_modal.ttl_hours,
        create_modal_step: state.contacts.create_modal.step,
        // Code display modal
        code_modal_visible: state.contacts.code_modal.visible,
        code_modal_invitation_id: state.contacts.code_modal.invitation_id.clone(),
        code_modal_code: state.contacts.code_modal.code.clone(),
        code_modal_loading: state.contacts.code_modal.loading,
        // Guardian setup modal
        guardian_setup_modal_visible: state.contacts.guardian_setup_modal.visible,
        guardian_setup_modal_step: state.contacts.guardian_setup_modal.step.clone(),
        guardian_setup_modal_contacts: state
            .contacts
            .guardian_setup_modal
            .contacts
            .iter()
            .map(|c| GuardianCandidateViewProps {
                id: c.id.clone(),
                name: c.name.clone(),
                is_current_guardian: c.is_current_guardian,
            })
            .collect(),
        guardian_setup_modal_selected_indices: state
            .contacts
            .guardian_setup_modal
            .selected_indices
            .clone(),
        guardian_setup_modal_focused_index: state.contacts.guardian_setup_modal.focused_index,
        guardian_setup_modal_threshold_k: state.contacts.guardian_setup_modal.threshold_k,
        guardian_setup_modal_threshold_n: state.contacts.guardian_setup_modal.threshold_n(),
        guardian_setup_modal_ceremony_responses: state
            .contacts
            .guardian_setup_modal
            .ceremony_responses
            .clone(),
        guardian_setup_modal_error: state
            .contacts
            .guardian_setup_modal
            .error
            .clone()
            .unwrap_or_default(),
        // Demo mode
        demo_mode: !state.contacts.demo_alice_code.is_empty(),
        demo_alice_code: state.contacts.demo_alice_code.clone(),
        demo_carol_code: state.contacts.demo_carol_code.clone(),
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

    InvitationsViewProps {
        focus,
        selected_index: state.invitations.selected_index,
        filter: state.invitations.filter,
        // Create modal
        create_modal_visible: state.invitations.create_modal.visible,
        create_modal_type_index: state.invitations.create_modal.type_index,
        create_modal_message: state.invitations.create_modal.message.clone(),
        create_modal_ttl_hours: state.invitations.create_modal.ttl_hours,
        create_modal_step: state.invitations.create_modal.step,
        // Import modal
        import_modal_visible: state.invitations.import_modal.visible,
        import_modal_code: state.invitations.import_modal.code.clone(),
        import_modal_importing: state.invitations.import_modal.importing,
        // Code display modal
        code_modal_visible: state.invitations.code_modal.visible,
        code_modal_invitation_id: state.invitations.code_modal.invitation_id.clone(),
        code_modal_code: state.invitations.code_modal.code.clone(),
        code_modal_loading: state.invitations.code_modal.loading,
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
    SettingsViewProps {
        section: state.settings.section,
        selected_index: state.settings.selected_index,
        mfa_policy: state.settings.mfa_policy,
        // Nickname modal
        nickname_modal_visible: state.settings.nickname_modal.visible,
        nickname_modal_value: state.settings.nickname_modal.value.clone(),
        // Threshold modal
        threshold_modal_visible: state.settings.threshold_modal.visible,
        threshold_modal_k: state.settings.threshold_modal.k,
        threshold_modal_n: state.settings.threshold_modal.n,
        threshold_modal_active_field: state.settings.threshold_modal.active_field,
        // Add device modal
        add_device_modal_visible: state.settings.add_device_modal.visible,
        add_device_modal_name: state.settings.add_device_modal.name.clone(),
        // Confirm remove modal
        confirm_remove_modal_visible: state.settings.confirm_remove_modal.visible,
        confirm_remove_modal_device_id: state.settings.confirm_remove_modal.device_id.clone(),
        confirm_remove_modal_device_name: state.settings.confirm_remove_modal.device_name.clone(),
        confirm_remove_modal_confirm_focused: state.settings.confirm_remove_modal.confirm_focused,
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
        let mut state = TuiState::new();
        state.block.insert_mode = true;
        state.block.focus = BlockFocus::Input;
        state.block.input_buffer = "hello".to_string();
        state.block.selected_resident = 3;
        state.block.invite_modal_open = true;
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
        let mut state = TuiState::new();
        state.chat.insert_mode = true;
        state.chat.focus = ChatFocus::Input;
        state.chat.input_buffer = "test message".to_string();
        state.chat.selected_channel = 5;
        state.chat.message_scroll = 10;
        state.chat.create_modal.visible = true;
        state.chat.create_modal.name = "channel-name".to_string();
        state.chat.info_modal.visible = true;
        state.chat.info_modal.channel_name = "info-channel".to_string();

        let props = extract_chat_view_props(&state);

        assert!(props.insert_mode, "insert_mode must be extracted");
        assert_eq!(props.focus, ScreenChatFocus::Input);
        assert_eq!(props.input_buffer, "test message");
        assert_eq!(props.selected_channel, 5);
        assert_eq!(props.message_scroll, 10);
        assert!(props.create_modal_visible);
        assert_eq!(props.create_modal_name, "channel-name");
        assert!(props.info_modal_visible);
        assert_eq!(props.info_modal_channel_name, "info-channel");
    }

    #[test]
    fn test_contacts_view_props_extraction() {
        let mut state = TuiState::new();
        state.contacts.selected_index = 7;
        state.contacts.filter = "search".to_string();
        state.contacts.petname_modal.visible = true;
        state.contacts.petname_modal.contact_id = "contact-123".to_string();
        state.contacts.petname_modal.value = "new-name".to_string();

        let props = extract_contacts_view_props(&state);

        assert_eq!(props.selected_index, 7);
        assert_eq!(props.filter, "search");
        assert!(props.petname_modal_visible);
        assert_eq!(props.petname_modal_contact_id, "contact-123");
        assert_eq!(props.petname_modal_value, "new-name");
    }

    #[test]
    fn test_invitations_view_props_extraction() {
        let mut state = TuiState::new();
        state.invitations.selected_index = 3;
        state.invitations.filter = InvitationFilter::Sent;
        state.invitations.create_modal.visible = true;
        state.invitations.import_modal.visible = true;
        state.invitations.import_modal.code = "ABC123".to_string();

        let props = extract_invitations_view_props(&state);

        assert_eq!(props.selected_index, 3);
        assert_eq!(props.filter, InvitationFilter::Sent);
        assert!(props.create_modal_visible);
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
        let mut state = TuiState::new();
        state.settings.section = SettingsSection::Devices;
        state.settings.selected_index = 1;
        state.settings.mfa_policy = MfaPolicy::AlwaysRequired;
        state.settings.nickname_modal.visible = true;
        state.settings.nickname_modal.value = "new-nick".to_string();
        state.settings.add_device_modal.visible = true;
        state.settings.add_device_modal.name = "my-device".to_string();

        let props = extract_settings_view_props(&state);

        assert_eq!(props.section, SettingsSection::Devices);
        assert_eq!(props.selected_index, 1);
        assert_eq!(props.mfa_policy, MfaPolicy::AlwaysRequired);
        assert!(props.nickname_modal_visible);
        assert_eq!(props.nickname_modal_value, "new-nick");
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
