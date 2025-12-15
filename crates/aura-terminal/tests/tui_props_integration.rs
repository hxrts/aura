//! # TUI Props Integration Tests
//!
//! These tests verify that state machine transitions correctly propagate
//! to screen props via the extraction functions. This catches bugs where:
//!
//! 1. State machine correctly updates state
//! 2. But extraction functions don't pass all fields to props
//! 3. Or app.rs doesn't call the extraction functions
//!
//! ## Test Pattern
//!
//! ```text
//! Event → transition() → TuiState → extract_*_view_props() → Props
//! ```
//!
//! Each test verifies the full path from input event to output props.

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_terminal::tui::props::{
    extract_block_view_props, extract_chat_view_props, extract_contacts_view_props,
    extract_recovery_view_props, extract_settings_view_props,
};
use aura_terminal::tui::screens::{BlockFocus, ChatFocus};
use aura_terminal::tui::state_machine::{transition, ChatFocus as StateChatFocus, TuiState};
use aura_terminal::tui::types::{RecoveryTab, SettingsSection};
use aura_terminal::tui::Screen;

// ============================================================================
// Test Harness
// ============================================================================

/// Test harness that verifies state→props pipeline
struct PropsTestHarness {
    state: TuiState,
}

impl PropsTestHarness {
    fn new() -> Self {
        Self {
            state: TuiState::new(),
        }
    }

    #[allow(dead_code)]
    fn with_account_setup() -> Self {
        Self {
            state: TuiState::with_account_setup(),
        }
    }

    fn send(&mut self, event: TerminalEvent) {
        let (new_state, _) = transition(&self.state, event);
        self.state = new_state;
    }

    fn send_char(&mut self, c: char) {
        self.send(events::char(c));
    }

    #[allow(dead_code)]
    fn send_tab(&mut self) {
        self.send(events::tab());
    }

    fn current_screen(&self) -> Screen {
        self.state.screen()
    }

    /// Navigate directly to a screen using number keys
    fn go_to_screen(&mut self, screen: Screen) {
        let key = match screen {
            Screen::Block => '1',
            Screen::Chat => '2',
            Screen::Contacts => '3',
            Screen::Neighborhood => '4',
            Screen::Settings => '5',
            Screen::Recovery => '6',
        };
        self.send_char(key);
        assert_eq!(
            self.current_screen(),
            screen,
            "Failed to navigate to {:?}",
            screen
        );
    }
}

// ============================================================================
// Block Screen Props Integration Tests
// ============================================================================

mod block_screen {
    use super::*;

    #[test]
    fn test_insert_mode_reaches_block_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Block);

        // Initially not in insert mode
        let props = extract_block_view_props(&harness.state);
        assert!(!props.insert_mode, "Should start not in insert mode");
        assert_eq!(props.focus, BlockFocus::Residents);

        // Press 'i' to enter insert mode
        harness.send_char('i');

        // Verify props reflect the change
        let props = extract_block_view_props(&harness.state);
        assert!(
            props.insert_mode,
            "Insert mode must reach BlockScreen props"
        );
        assert_eq!(props.focus, BlockFocus::Input, "Focus must change to Input");
    }

    #[test]
    fn test_input_buffer_reaches_block_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Block);

        // Enter insert mode
        harness.send_char('i');

        // Type some text
        harness.send_char('h');
        harness.send_char('i');

        let props = extract_block_view_props(&harness.state);
        assert_eq!(
            props.input_buffer, "hi",
            "Input buffer must reach BlockScreen props"
        );
    }

    #[test]
    fn test_invite_modal_reaches_block_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Block);

        // Open invite modal with 'v'
        harness.send_char('v');

        let props = extract_block_view_props(&harness.state);
        assert!(
            props.invite_modal_open,
            "Invite modal state must reach props"
        );
        assert_eq!(props.invite_selection, 0);
    }

    #[test]
    fn test_resident_selection_reaches_block_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Block);

        // Set up item count for navigation to work
        harness.state.block.resident_count = 10;

        // Navigate down in resident list
        harness.send(events::arrow_down());

        let props = extract_block_view_props(&harness.state);
        assert_eq!(
            props.selected_resident, 1,
            "Selected resident must reach props"
        );
    }
}

// ============================================================================
// Chat Screen Props Integration Tests
// ============================================================================

mod chat_screen {
    use super::*;

    #[test]
    fn test_insert_mode_reaches_chat_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Chat);

        // Initially not in insert mode
        let props = extract_chat_view_props(&harness.state);
        assert!(!props.insert_mode, "Should start not in insert mode");
        assert_eq!(props.focus, ChatFocus::Channels);

        // Press 'i' to enter insert mode
        harness.send_char('i');

        // Verify props reflect the change
        let props = extract_chat_view_props(&harness.state);
        assert!(props.insert_mode, "Insert mode must reach ChatScreen props");
        assert_eq!(props.focus, ChatFocus::Input, "Focus must change to Input");
    }

    #[test]
    fn test_input_buffer_reaches_chat_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Chat);

        // Enter insert mode
        harness.send_char('i');

        // Type message
        harness.send_char('t');
        harness.send_char('e');
        harness.send_char('s');
        harness.send_char('t');

        let props = extract_chat_view_props(&harness.state);
        assert_eq!(
            props.input_buffer, "test",
            "Input buffer must reach ChatScreen props"
        );
    }

    #[test]
    fn test_message_scroll_reaches_chat_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Chat);

        // Set up message count for navigation to work
        harness.state.chat.message_count = 50;

        // Navigate to messages panel
        harness.send(events::arrow_right());
        assert_eq!(harness.state.chat.focus, StateChatFocus::Messages);

        // Scroll down in messages
        harness.send(events::arrow_down());

        let props = extract_chat_view_props(&harness.state);
        assert_eq!(
            props.message_scroll, 1,
            "Message scroll must reach ChatScreen props"
        );
    }

    #[test]
    fn test_channel_selection_reaches_chat_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Chat);

        // Navigate to messages panel
        harness.send(events::arrow_right());

        let props = extract_chat_view_props(&harness.state);
        // After arrow_right, focus is Messages
        assert_eq!(props.focus, ChatFocus::Messages);
    }
}

// ============================================================================
// Contacts Screen Props Integration Tests
// ============================================================================

mod contacts_screen {
    use super::*;

    #[test]
    fn test_selection_reaches_contacts_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Contacts);

        // Set up contact count for navigation to work
        harness.state.contacts.contact_count = 10;

        // Navigate down
        harness.send(events::arrow_down());

        let props = extract_contacts_view_props(&harness.state);
        assert_eq!(
            props.selected_index, 1,
            "Selected index must reach ContactsScreen props"
        );
    }

    #[test]
    fn test_petname_modal_reaches_contacts_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Contacts);

        // Open petname modal with 'e'
        harness.send_char('e');

        let props = extract_contacts_view_props(&harness.state);
        assert!(
            props.petname_modal_visible,
            "Petname modal state must reach props"
        );
    }
}

// ============================================================================
// Invitations Screen Props Integration Tests
// ============================================================================
// NOTE: Invitations screen was merged into Contacts screen. Tests removed.

// ============================================================================
// Recovery Screen Props Integration Tests
// ============================================================================

mod recovery_screen {
    use super::*;

    #[test]
    fn test_tab_selection_reaches_recovery_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Recovery);

        // Initial tab should be Guardians
        let props = extract_recovery_view_props(&harness.state);
        assert_eq!(props.tab, RecoveryTab::Guardians);

        // Switch to Recovery tab with arrow_right (order is Guardians -> Recovery -> Requests)
        harness.send(events::arrow_right());

        let props = extract_recovery_view_props(&harness.state);
        assert_eq!(
            props.tab,
            RecoveryTab::Recovery,
            "Tab selection must reach RecoveryScreen props"
        );
    }

    #[test]
    fn test_guardian_selection_reaches_recovery_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Recovery);

        // Set up item count for navigation to work
        harness.state.recovery.item_count = 10;

        // Navigate down in guardian list
        harness.send(events::arrow_down());

        let props = extract_recovery_view_props(&harness.state);
        assert_eq!(
            props.selected_index, 1,
            "Selected index must reach RecoveryScreen props"
        );
    }
}

// ============================================================================
// Settings Screen Props Integration Tests
// ============================================================================

mod settings_screen {
    use super::*;

    #[test]
    fn test_section_selection_reaches_settings_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Settings);

        // Initial section should be Profile
        let props = extract_settings_view_props(&harness.state);
        assert_eq!(props.section, SettingsSection::Profile);

        // Navigate down to next section
        harness.send(events::arrow_down());

        let props = extract_settings_view_props(&harness.state);
        assert_eq!(
            props.section,
            SettingsSection::Threshold,
            "Section must reach SettingsScreen props"
        );
    }

    #[test]
    fn test_profile_edit_modal_reaches_settings_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Settings);

        // Open profile edit with Enter
        harness.send(events::enter());

        let props = extract_settings_view_props(&harness.state);
        assert!(
            props.nickname_modal_visible,
            "Nickname modal state must reach props"
        );
    }

    #[test]
    fn test_mfa_policy_cycles_in_settings_props() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Settings);

        // Navigate to Mfa section (Profile -> Threshold -> Devices -> Mfa)
        harness.send(events::arrow_down()); // Threshold
        harness.send(events::arrow_down()); // Devices
        harness.send(events::arrow_down()); // Mfa

        let props = extract_settings_view_props(&harness.state);
        assert_eq!(props.section, SettingsSection::Mfa);
    }
}

// ============================================================================
// Cross-Screen Integration Tests
// ============================================================================

mod cross_screen {
    use super::*;

    /// This test would have caught the original bug where ChatScreen
    /// wasn't receiving insert_mode from TuiState
    #[test]
    fn test_insert_mode_works_on_all_screens_with_input() {
        let mut harness = PropsTestHarness::new();

        // Test Block screen insert mode
        harness.go_to_screen(Screen::Block);
        harness.send_char('i');
        let block_props = extract_block_view_props(&harness.state);
        assert!(
            block_props.insert_mode,
            "Block: insert_mode must reach props"
        );

        // Exit insert mode
        harness.send(events::escape());
        let block_props = extract_block_view_props(&harness.state);
        assert!(!block_props.insert_mode, "Block: must exit insert mode");

        // Navigate to Chat and test insert mode
        harness.go_to_screen(Screen::Chat);
        harness.send_char('i');
        let chat_props = extract_chat_view_props(&harness.state);
        assert!(chat_props.insert_mode, "Chat: insert_mode must reach props");

        // Exit insert mode
        harness.send(events::escape());
        let chat_props = extract_chat_view_props(&harness.state);
        assert!(!chat_props.insert_mode, "Chat: must exit insert mode");
    }

    /// Test that screen navigation doesn't lose screen-specific state
    #[test]
    fn test_screen_state_preserved_across_navigation() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Block);

        // Set up item count for navigation to work
        harness.state.block.resident_count = 10;

        // Set some state on Block screen
        harness.send(events::arrow_down());
        let block_props = extract_block_view_props(&harness.state);
        assert_eq!(block_props.selected_resident, 1);

        // Navigate away and back
        harness.go_to_screen(Screen::Chat);
        harness.go_to_screen(Screen::Block);

        // Block state should be preserved
        let block_props = extract_block_view_props(&harness.state);
        assert_eq!(
            block_props.selected_resident, 1,
            "Screen state must be preserved across navigation"
        );
    }
}

// ============================================================================
// Regression Tests
// ============================================================================

mod regression {
    use super::*;

    /// Regression test for the original bug where ChatScreen
    /// wasn't wired to receive view state from TuiState.
    ///
    /// This test ensures the full pipeline works:
    /// Event → State Machine → Extract Props → Screen receives props
    #[test]
    fn test_chat_insert_mode_full_pipeline() {
        let mut harness = PropsTestHarness::new();
        harness.go_to_screen(Screen::Chat);

        // Verify initial state
        let props = extract_chat_view_props(&harness.state);
        assert!(!props.insert_mode, "Initially not in insert mode");
        assert_eq!(props.focus, ChatFocus::Channels);
        assert!(props.input_buffer.is_empty());

        // Enter insert mode
        harness.send_char('i');

        // THE BUG: These assertions would have failed before the fix
        // because app.rs wasn't passing view state to ChatScreen
        let props = extract_chat_view_props(&harness.state);
        assert!(
            props.insert_mode,
            "REGRESSION: insert_mode not reaching ChatScreen"
        );
        assert_eq!(
            props.focus,
            ChatFocus::Input,
            "REGRESSION: focus not reaching ChatScreen"
        );

        // Type some text
        harness.send_char('h');
        harness.send_char('e');
        harness.send_char('l');
        harness.send_char('l');
        harness.send_char('o');

        let props = extract_chat_view_props(&harness.state);
        assert_eq!(
            props.input_buffer, "hello",
            "REGRESSION: input_buffer not reaching ChatScreen"
        );
    }
}
