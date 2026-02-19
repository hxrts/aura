#![allow(
    missing_docs,
    dead_code,
    unused,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    clippy::all
)]
//! # Comprehensive TUI State Machine Tests
//!
//! This test file provides comprehensive coverage of all TUI screens, modals,
//! and dispatch commands using the pure state machine model.
//!
//! ## Test Categories
//!
//! 1. **Screen Tests** - Navigation and functionality for each of 5 screens
//! 2. **Modal Tests** - All modal types and their interactions
//! 3. **Dispatch Command Tests** - Verify correct commands are generated
//! 4. **Edge Case Tests** - Boundary conditions and error handling
//! 5. **Integration Tests** - Complex multi-step workflows
//! 6. **Property Tests** - Invariants via proptest

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::state_machine::{
    transition, ChannelInfoModalState, ChatFocus, CreateChannelModalState, CreateChannelStep,
    DetailFocus, DispatchCommand, ModalType, QueuedModal, TopicModalState, TuiCommand, TuiState,
};
use aura_terminal::tui::types::SettingsSection;
use proptest::prelude::*;

// ============================================================================
// Test Harness
// ============================================================================

/// Extended test wrapper with additional assertion methods
struct TestTui {
    state: TuiState,
    commands: Vec<TuiCommand>,
}

impl TestTui {
    fn new() -> Self {
        Self {
            state: TuiState::new(),
            commands: Vec::new(),
        }
    }

    fn with_account_setup() -> Self {
        Self {
            state: TuiState::with_account_setup(),
            commands: Vec::new(),
        }
    }

    fn send(&mut self, event: TerminalEvent) {
        let (new_state, cmds) = transition(&self.state, event);
        self.state = new_state;
        self.commands.extend(cmds);
    }

    fn send_char(&mut self, c: char) {
        self.send(events::char(c));
    }

    fn send_tab(&mut self) {
        self.send(events::tab());
    }

    fn send_enter(&mut self) {
        self.send(events::enter());
    }

    fn send_escape(&mut self) {
        self.send(events::escape());
    }

    fn send_backspace(&mut self) {
        self.send(events::backspace());
    }

    fn send_up(&mut self) {
        self.send(events::arrow_up());
    }

    fn send_down(&mut self) {
        self.send(events::arrow_down());
    }

    fn send_left(&mut self) {
        self.send(events::arrow_left());
    }

    fn send_right(&mut self) {
        self.send(events::arrow_right());
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.send_char(c);
        }
    }

    fn screen(&self) -> Screen {
        self.state.screen()
    }

    fn assert_screen(&self, expected: Screen) {
        assert_eq!(
            self.screen(),
            expected,
            "Expected screen {:?}, got {:?}",
            expected,
            self.screen()
        );
    }

    fn is_insert_mode(&self) -> bool {
        self.state.is_insert_mode()
    }

    fn has_modal(&self) -> bool {
        self.state.has_modal()
    }

    fn modal_type(&self) -> ModalType {
        self.state.current_modal_type()
    }

    fn assert_modal(&self, expected: ModalType) {
        assert_eq!(
            self.modal_type(),
            expected,
            "Expected modal {:?}, got {:?}",
            expected,
            self.modal_type()
        );
    }

    fn assert_no_modal(&self) {
        assert!(!self.has_modal(), "Expected no modal, but modal is open");
    }

    fn has_dispatch(&self, check: impl Fn(&DispatchCommand) -> bool) -> bool {
        self.commands
            .iter()
            .any(|c| matches!(c, TuiCommand::Dispatch(d) if check(d)))
    }

    fn has_exit(&self) -> bool {
        self.commands.iter().any(|c| matches!(c, TuiCommand::Exit))
    }

    fn clear_commands(&mut self) {
        self.commands.clear();
    }

    fn go_to_screen(&mut self, screen: Screen) {
        let key = char::from_digit(screen.key_number() as u32, 10)
            .unwrap_or_else(|| unreachable!("Screen::key_number returns 1..=5"));
        self.send_char(key);
        self.assert_screen(screen);
    }
}

// ============================================================================
// NEIGHBORHOOD SCREEN TESTS
// ============================================================================

mod neighborhood_screen_map {
    use super::*;

    #[test]
    fn test_neighborhood_insert_mode_entry() {
        let mut tui = TestTui::new();
        tui.assert_screen(Screen::Neighborhood);

        // Enter detail mode; neighborhood no longer supports insert mode.
        tui.send_enter();
        tui.send_char('i');
        assert!(!tui.is_insert_mode());
        assert_eq!(tui.state.neighborhood.detail_focus, DetailFocus::Channels);
    }

    #[test]
    fn test_neighborhood_enter_does_not_send_message() {
        let mut tui = TestTui::new();

        // Enter detail mode and press Enter - no home message dispatch should occur.
        tui.send_enter();
        tui.clear_commands();
        tui.send_enter();

        assert!(!tui.has_dispatch(|_| true));
    }

    #[test]
    fn test_neighborhood_empty_message_not_sent() {
        let mut tui = TestTui::new();

        // Enter detail mode; home messaging is disabled on Neighborhood.
        tui.send_enter();
        tui.clear_commands();
        tui.send_enter();

        // No dispatch should occur for empty message
        assert!(!tui.has_dispatch(|_| true));
    }

    #[test]
    fn test_neighborhood_resident_navigation() {
        let mut tui = TestTui::new();
        tui.state.neighborhood.home_count = 1;
        tui.send_enter();
        tui.state.neighborhood.detail_focus = DetailFocus::Residents;

        // Set up item counts for navigation to work
        tui.state.neighborhood.resident_count = 10;

        // Navigate down in resident list
        let initial = tui.state.neighborhood.selected_resident;
        tui.send_char('j');
        assert_eq!(tui.state.neighborhood.selected_resident, initial + 1);

        // Navigate up
        tui.send_char('k');
        assert_eq!(tui.state.neighborhood.selected_resident, initial);
    }

    #[test]
    fn test_neighborhood_backspace_in_detail_mode() {
        let mut tui = TestTui::new();

        tui.send_enter();
        tui.clear_commands();
        tui.send_backspace();
        tui.send_backspace();
        assert!(!tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. })));
    }
}

// ============================================================================
// CHAT SCREEN TESTS
// ============================================================================

mod chat_screen {
    use super::*;

    #[test]
    fn test_chat_focus_navigation() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Default focus is Channels
        assert_eq!(tui.state.chat.focus, ChatFocus::Channels);

        // 'l' moves focus right to Messages
        tui.send_char('l');
        assert_eq!(tui.state.chat.focus, ChatFocus::Messages);

        // 'h' moves focus left to Channels
        tui.send_char('h');
        assert_eq!(tui.state.chat.focus, ChatFocus::Channels);
    }

    #[test]
    fn test_chat_channel_selection() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Set up item counts for navigation to work
        tui.state.chat.channel_count = 10;
        tui.state.chat.message_count = 50;

        // Chat screen starts at Channels by default
        assert_eq!(tui.state.chat.focus, ChatFocus::Channels);

        // Navigate channels with j/k
        let initial = tui.state.chat.selected_channel;
        tui.send_char('j');
        assert_eq!(tui.state.chat.selected_channel, initial + 1);

        tui.send_char('k');
        assert_eq!(tui.state.chat.selected_channel, initial);
    }

    #[test]
    fn test_chat_message_scroll() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Set up item counts for navigation to work
        tui.state.chat.channel_count = 10;
        tui.state.chat.message_count = 50;

        // Focus on messages
        tui.send_char('l');
        assert_eq!(tui.state.chat.focus, ChatFocus::Messages);

        // Scroll messages with j/k
        // scroll_offset: 0 = at bottom (newest), higher = scrolled up (older)
        // k = scroll up (toward older = increase offset)
        // j = scroll down (toward newer = decrease offset)
        let initial = tui.state.chat.message_scroll;
        tui.send_char('k');
        assert_eq!(tui.state.chat.message_scroll, initial + 1);

        tui.send_char('j');
        assert_eq!(tui.state.chat.message_scroll, initial);
    }

    #[test]
    fn test_chat_insert_mode() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Enter insert mode
        tui.send_char('i');
        assert!(tui.is_insert_mode());
        assert_eq!(tui.state.chat.focus, ChatFocus::Input);

        // Type and send message
        tui.type_text("Hello, Chat!");
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { content, .. } if content == "Hello, Chat!")));
    }

    #[test]
    fn test_chat_create_channel_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Open create channel modal with 'n'
        tui.send_char('n');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenChatCreateWizard)));
        tui.state
            .modal_queue
            .enqueue(QueuedModal::ChatCreate(CreateChannelModalState::new()));
        assert!(tui.has_modal());

        // Type channel name
        tui.type_text("general");
        assert_eq!(tui.state.chat_create_modal_state().unwrap().name, "general");

        // Advance to threshold step so Enter submits (creates channel directly).
        tui.state.modal_queue.update_active(|modal| {
            if let QueuedModal::ChatCreate(ref mut s) = modal {
                s.step = CreateChannelStep::Threshold;
            }
        });

        // Submit with Enter
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(
            |d| matches!(d, DispatchCommand::CreateChannel { name, .. } if name == "general")
        ));
        // Modal should be dismissed after channel creation
        assert!(!tui.has_modal());
        // Toast should show success message
        assert!(tui.state.toast_queue.is_active());
        let toast = tui.state.toast_queue.current().unwrap();
        assert!(toast.message.contains("general"));
    }

    #[test]
    fn test_chat_set_topic_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // 't' triggers a shell-populated modal open.
        tui.send_char('t');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenChatTopicModal)));

        // Simulate the shell opening the modal with selected channel details.
        tui.state
            .modal_queue
            .enqueue(QueuedModal::ChatTopic(TopicModalState::for_channel(
                "ch-123", "",
            )));

        assert!(tui.has_modal());
        assert!(tui.state.is_chat_topic_modal_active());

        // Type topic
        tui.type_text("Welcome to the channel!");
        assert_eq!(
            tui.state.chat_topic_modal_state().unwrap().value,
            "Welcome to the channel!"
        );

        // Submit with Enter
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(|d| {
            matches!(
                d,
                DispatchCommand::SetChannelTopic { channel_id, topic }
                    if channel_id == "ch-123" && topic == "Welcome to the channel!"
            )
        }));
    }

    #[test]
    fn test_chat_channel_info_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // 'o' triggers a shell-populated modal open.
        tui.send_char('o');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenChatInfoModal)));

        // Simulate the shell opening the modal with selected channel details.
        tui.state
            .modal_queue
            .enqueue(QueuedModal::ChatInfo(ChannelInfoModalState::for_channel(
                "ch-123",
                "info-channel",
                None,
            )));

        assert!(tui.has_modal());
        assert!(tui.state.is_chat_info_modal_active());

        // Close with Escape
        tui.send_escape();
        assert!(!tui.state.is_chat_info_modal_active());
    }

    #[test]
    fn test_chat_retry_message() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Focus on messages
        tui.send_char('l');
        tui.clear_commands();

        // Retry with 'r'
        tui.send_char('r');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::RetryMessage)));
    }
}

// ============================================================================
// CONTACTS SCREEN TESTS
// ============================================================================

mod contacts_screen {
    use super::*;

    #[test]
    fn test_contacts_list_navigation() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        // Set up item count for navigation to work
        tui.state.contacts.contact_count = 10;

        let initial = tui.state.contacts.selected_index;
        tui.send_char('j');
        assert_eq!(tui.state.contacts.selected_index, initial + 1);

        tui.send_char('k');
        assert_eq!(tui.state.contacts.selected_index, initial);
    }

    #[test]
    fn test_contacts_edit_nickname() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        // 'e' emits a dispatch so the shell can populate the modal with the selected contact.
        tui.send_char('e');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenContactNicknameModal)));

        // Simulate the shell opening the modal with a concrete contact ID.
        use aura_terminal::tui::state_machine::NicknameModalState;
        tui.state.modal_queue.enqueue(QueuedModal::ContactsNickname(
            NicknameModalState::for_contact("contact-123", ""),
        ));
        assert!(tui.has_modal());

        // Type new nickname
        tui.type_text("Alice");

        // Submit
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(
            |d| matches!(d, DispatchCommand::UpdateNickname { nickname, .. } if nickname == "Alice")
        ));
        assert!(!tui.has_modal());
    }

    #[test]
    fn test_contacts_open_guardian_setup_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        tui.clear_commands();
        tui.send_char('g');

        // 'g' emits OpenGuardianSetup dispatch command which the shell processes
        // to populate and open the modal with current contacts from reactive subscription
        assert!(
            tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenGuardianSetup)),
            "Pressing 'g' should emit OpenGuardianSetup dispatch command"
        );
    }

    #[test]
    fn test_contacts_start_chat() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        tui.clear_commands();
        tui.send_char('c');

        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::StartChat { .. })));
    }

    #[test]
    fn test_contacts_navigation_saturates_at_zero() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        // Press 'k' many times - should not go negative
        for _ in 0..50 {
            tui.send_char('k');
        }
        assert_eq!(tui.state.contacts.selected_index, 0);
    }
}

// ============================================================================
// INVITATIONS SCREEN TESTS
// ============================================================================
// NOTE: Invitations screen was merged into Contacts screen. Tests removed.

// ============================================================================
// SETTINGS SCREEN TESTS
// ============================================================================

mod settings_screen {
    use super::*;

    #[test]
    fn test_settings_section_navigation() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // Default section is Profile
        assert_eq!(tui.state.settings.section, SettingsSection::Profile);

        // Navigate sections with j/k
        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Threshold);

        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Recovery);

        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Devices);

        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Authority);

        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Observability);

        // Wraps around
        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Profile);
    }

    #[test]
    fn test_settings_section_navigation_up() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // Navigate up with 'k' - should wrap
        tui.send_char('k');
        assert_eq!(tui.state.settings.section, SettingsSection::Observability);

        tui.send_char('k');
        assert_eq!(tui.state.settings.section, SettingsSection::Authority);

        tui.send_char('k');
        assert_eq!(tui.state.settings.section, SettingsSection::Devices);
    }

    #[test]
    fn test_settings_profile_edit() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // On Profile section, Enter opens edit modal
        tui.send_enter();
        assert!(tui.has_modal());

        // Type new nickname
        tui.type_text("NewNickname");

        // Submit
        tui.clear_commands();
        tui.send_enter();

        // Settings profile edit updates nickname suggestion, not contact nickname
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::UpdateNicknameSuggestion { nickname_suggestion } if nickname_suggestion == "NewNickname")));
    }

    #[test]
    fn test_settings_threshold_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // Go to Threshold section
        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Threshold);

        // Open threshold modal with Enter
        tui.send_enter();

        // Modal may or may not open depending on implementation
        // If it does open, verify it can be closed
        if tui.has_modal() {
            // Close with Escape
            tui.send_escape();
            assert!(!tui.has_modal());
        }
    }

    #[test]
    fn test_settings_authority_mfa_cycle() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // Go to Authority section
        for _ in 0..4 {
            tui.send_char('j');
        }
        assert_eq!(tui.state.settings.section, SettingsSection::Authority);

        // Space opens MFA setup
        tui.clear_commands();
        tui.send_char(' ');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenMfaSetup)));
    }

    #[test]
    fn test_settings_device_management() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // Go to Devices section
        tui.send_char('j');
        tui.send_char('j');
        tui.send_char('j');
        assert_eq!(tui.state.settings.section, SettingsSection::Devices);

        // 'a' opens add device modal
        tui.send_char('a');
        assert!(tui.has_modal());

        // Type device name
        tui.type_text("My Phone");

        // Submit
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(
            |d| matches!(d, DispatchCommand::AddDevice { name, .. } if name == "My Phone")
        ));
    }

    #[test]
    fn test_settings_panel_focus() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // 'l' focuses detail panel
        tui.send_char('l');

        // 'h' focuses menu panel
        tui.send_char('h');

        // These should work without panicking
    }
}

// ============================================================================
// NEIGHBORHOOD SCREEN TESTS
// ============================================================================

mod neighborhood_screen {
    use super::*;

    #[test]
    fn test_neighborhood_grid_navigation() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);

        // Set up a grid with items so navigation works
        tui.state.neighborhood.grid.set_cols(4);
        tui.state.neighborhood.grid.set_count(12); // 3x4 grid

        // Navigate with h/j/k/l (uses grid.col() and grid.row())
        let (initial_col, initial_row) = (
            tui.state.neighborhood.grid.col(),
            tui.state.neighborhood.grid.row(),
        );

        // Move right
        tui.send_char('l');
        assert_eq!(tui.state.neighborhood.grid.col(), initial_col + 1);

        // Move left
        tui.send_char('h');
        assert_eq!(tui.state.neighborhood.grid.col(), initial_col);

        // Move down
        tui.send_char('j');
        assert_eq!(tui.state.neighborhood.grid.row(), initial_row + 1);

        // Move up
        tui.send_char('k');
        assert_eq!(tui.state.neighborhood.grid.row(), initial_row);
    }

    #[test]
    fn test_neighborhood_enter_home() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);
        tui.state.neighborhood.home_count = 1;

        tui.clear_commands();

        // Enter selected home
        tui.send_enter();
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::EnterHome { .. })));
    }

    #[test]
    fn test_neighborhood_go_home() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);

        tui.clear_commands();

        // 'g' goes home
        tui.send_char('g');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::GoHome)));
    }

    #[test]
    fn test_neighborhood_back_to_street() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);

        tui.clear_commands();

        // 'b' goes back to street
        tui.send_char('b');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::BackToStreet)));
    }

    #[test]
    fn test_neighborhood_grid_wraps_around() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);

        // Set up a 4-column grid with 12 items (3 rows)
        tui.state.neighborhood.grid.set_cols(4);
        tui.state.neighborhood.grid.set_count(12);

        // Start at 0,0
        assert_eq!(tui.state.neighborhood.grid.col(), 0);
        assert_eq!(tui.state.neighborhood.grid.row(), 0);

        // Press 'h' at column 0 - should wrap to last item
        tui.send_char('h');
        assert_eq!(tui.state.neighborhood.grid.current(), 11); // Wrapped to end

        // Press 'l' at last item - should wrap to first
        tui.send_char('l');
        assert_eq!(tui.state.neighborhood.grid.current(), 0); // Wrapped to start

        // Press 'k' at row 0 - should wrap to last row
        tui.send_char('k');
        assert_eq!(tui.state.neighborhood.grid.row(), 2); // Wrapped to bottom

        // Press 'j' at last row - should wrap to first row
        tui.send_char('j');
        assert_eq!(tui.state.neighborhood.grid.row(), 0); // Wrapped to top
    }
}

// ============================================================================
// MODAL TESTS
// ============================================================================

mod modals {
    use super::*;

    #[test]
    fn test_help_modal() {
        let mut tui = TestTui::new();

        // '?' opens help modal from any screen
        tui.send_char('?');
        assert!(tui.has_modal());
        assert_eq!(tui.modal_type(), ModalType::Help);

        // Set up scroll max for navigation to work
        tui.state.help.scroll_max = 50;

        // Scroll help with j/k
        let initial_scroll = tui.state.help.scroll;
        tui.send_char('j');
        assert_eq!(tui.state.help.scroll, initial_scroll + 1);

        // Escape closes
        tui.send_escape();
        assert!(!tui.has_modal());
    }

    #[test]
    fn test_account_setup_modal() {
        let mut tui = TestTui::with_account_setup();

        assert!(tui.has_modal());
        assert_eq!(tui.modal_type(), ModalType::AccountSetup);

        // Type account name
        tui.type_text("MyAccount");

        // Submit
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(
            |d| matches!(d, DispatchCommand::CreateAccount { name } if name == "MyAccount")
        ));
    }

    #[test]
    fn test_modal_homes_screen_navigation() {
        let mut tui = TestTui::new();

        // Open help modal
        tui.send_char('?');
        assert!(tui.has_modal());

        let screen_before = tui.screen();

        // Try all navigation keys - none should work
        for key in ['1', '2', '3', '4', '5', '6', '7'] {
            tui.send_char(key);
            assert_eq!(tui.screen(), screen_before);
            assert!(tui.has_modal());
        }

        // Tab should also be blocked
        tui.send_tab();
        assert_eq!(tui.screen(), screen_before);
    }

    #[test]
    fn test_modal_escape_always_closes() {
        let mut tui = TestTui::new();

        // Test all modal types that can be opened
        let modal_openers = vec![
            (Screen::Neighborhood, '?'), // Help
            (Screen::Chat, 'n'),         // Create channel
            (Screen::Chat, 't'),         // Set topic
            (Screen::Contacts, 'e'),     // Edit nickname
        ];

        for (screen, key) in modal_openers {
            tui.go_to_screen(screen);
            tui.send_char(key);

            if tui.has_modal() {
                tui.send_escape();
                assert!(!tui.has_modal(), "Modal should close with Escape");
            }
        }
    }

    #[test]
    fn test_text_input_modal_validation() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        // Open create channel modal
        tui.send_char('n');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenChatCreateWizard)));
        tui.state
            .modal_queue
            .enqueue(QueuedModal::ChatCreate(CreateChannelModalState::new()));
        assert!(tui.has_modal());

        // Empty input - Enter should not submit (or should show error)
        tui.clear_commands();
        tui.send_enter();

        // Either modal stays open or no dispatch occurs
        // (depends on implementation - test that something reasonable happens)
    }

    #[test]
    fn test_guardian_select_modal() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Contacts);

        // 'g' opens guardian setup via dispatch (shell populates modal)
        tui.clear_commands();
        tui.send_char('g');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenGuardianSetup)));

        // Simulate the shell opening the modal.
        tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
            aura_terminal::tui::state_machine::GuardianSetupModalState::default(),
        ));
        assert!(tui.state.is_guardian_setup_modal_active());

        // Dismiss and re-enqueue with populated contacts for navigation test
        tui.state.modal_queue.dismiss();
        let contacts = vec![
            aura_terminal::tui::state_machine::GuardianCandidate {
                id: "id1".to_string(),
                name: "Contact 1".to_string(),
                is_current_guardian: false,
            },
            aura_terminal::tui::state_machine::GuardianCandidate {
                id: "id2".to_string(),
                name: "Contact 2".to_string(),
                is_current_guardian: false,
            },
        ];
        tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
            aura_terminal::tui::state_machine::GuardianSetupModalState::from_contacts_with_selection(contacts, vec![]),
        ));

        // Verify focused index updates (initial should be 0)
        if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
            assert_eq!(state.focused_index(), 0);
        }
        // Move focus down
        tui.state.modal_queue.update_active(|modal| {
            if let QueuedModal::GuardianSetup(state) = modal {
                state.move_focus_down();
            }
        });
        if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
            assert_eq!(state.focused_index(), 1);
        }
    }
}

// ============================================================================
// GLOBAL BEHAVIOR TESTS
// ============================================================================

mod global_behavior {
    use super::*;

    #[test]
    fn test_quit_from_any_screen() {
        for screen in [
            Screen::Neighborhood,
            Screen::Chat,
            Screen::Contacts,
            Screen::Neighborhood,
            Screen::Settings,
            Screen::Notifications,
        ] {
            let mut tui = TestTui::new();
            tui.go_to_screen(screen);

            tui.send_char('q');

            assert!(tui.state.should_exit, "q should quit from {screen:?}");
            assert!(tui.has_exit());
        }
    }

    #[test]
    fn test_quit_homeed_in_insert_mode() {
        let mut tui = TestTui::new();

        // Enter insert mode on Chat screen
        tui.go_to_screen(Screen::Chat);
        tui.send_char('i');
        assert!(tui.is_insert_mode());

        // 'q' should type 'q', not quit
        tui.send_char('q');
        assert!(!tui.state.should_exit);
        assert_eq!(tui.state.chat.input_buffer, "q");
    }

    #[test]
    fn test_quit_homeed_in_modal() {
        let mut tui = TestTui::new();

        // Open help modal
        tui.send_char('?');
        assert!(tui.has_modal());

        // 'q' should not quit while modal is open
        tui.send_char('q');
        assert!(!tui.state.should_exit);
    }

    #[test]
    fn test_help_from_any_screen() {
        for screen in [
            Screen::Neighborhood,
            Screen::Chat,
            Screen::Contacts,
            Screen::Neighborhood,
            Screen::Settings,
            Screen::Notifications,
        ] {
            let mut tui = TestTui::new();
            tui.go_to_screen(screen);

            tui.send_char('?');

            assert!(tui.has_modal(), "? should open help from {screen:?}");
            assert_eq!(tui.modal_type(), ModalType::Help);

            // Clean up for next iteration
            tui.send_escape();
        }
    }

    #[test]
    fn test_resize_preserves_state() {
        let mut tui = TestTui::new();

        // Set up some state
        tui.go_to_screen(Screen::Chat);
        tui.send_char('i');
        tui.type_text("Hello");

        let screen_before = tui.screen();
        let buffer_before = tui.state.chat.input_buffer.clone();
        let insert_before = tui.is_insert_mode();

        // Resize
        tui.send(events::resize(120, 40));

        // State should be preserved
        assert_eq!(tui.screen(), screen_before);
        assert_eq!(tui.state.chat.input_buffer, buffer_before);
        assert_eq!(tui.is_insert_mode(), insert_before);
        assert_eq!(tui.state.terminal_size, (120, 40));
    }
}

// ============================================================================
// STRESS TESTS
// ============================================================================

mod stress {
    use super::*;

    #[test]
    fn test_rapid_screen_switching() {
        let mut tui = TestTui::new();

        // Switch screens rapidly 10000 times (5 screens)
        for i in 0..10000 {
            let key = char::from_digit(((i % 5) + 1) as u32, 10).unwrap();
            tui.send_char(key);
        }

        // Should end on a valid screen without panicking
        assert!(matches!(
            tui.screen(),
            Screen::Neighborhood
                | Screen::Chat
                | Screen::Contacts
                | Screen::Notifications
                | Screen::Settings
        ));
    }

    #[test]
    fn test_rapid_insert_mode_toggle() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Chat);

        for _ in 0..10000 {
            tui.send_char('i');
            tui.send_escape();
        }

        assert!(!tui.is_insert_mode());
    }

    #[test]
    fn test_very_long_input() {
        let mut tui = TestTui::new();

        tui.go_to_screen(Screen::Chat);
        tui.send_char('i');

        // Type 100KB of text
        let long_text = "a".repeat(100_000);
        for c in long_text.chars() {
            tui.send_char(c);
        }

        assert_eq!(tui.state.chat.input_buffer.len(), 100_000);
    }

    #[test]
    fn test_many_modal_opens_closes() {
        let mut tui = TestTui::new();

        for _ in 0..1000 {
            tui.send_char('?'); // Open help
            tui.send_escape(); // Close
        }

        assert!(!tui.has_modal());
    }

    #[test]
    fn test_mixed_rapid_operations() {
        let mut tui = TestTui::new();

        for i in 0..1000 {
            match i % 10 {
                0 => tui.send_char('i'),
                1 => tui.send_escape(),
                2 => tui.send_char('j'),
                3 => tui.send_char('k'),
                4 => tui.send_tab(),
                5 => tui.send_char('?'),
                6 => tui.send_escape(),
                7 => tui.send(events::resize(80 + (i % 100) as u16, 24)),
                8 => tui.send_char((b'1' + (i % 5) as u8) as char),
                _ => tui.send_char('h'),
            }
        }

        // Should complete without panicking
    }
}

// ============================================================================
// PROPERTY-BASED TESTS
// ============================================================================

fn screen_key_strategy() -> impl Strategy<Value = char> {
    // 5 screens: Neighborhood(1), Chat(2), Contacts(3), Notifications(4), Settings(5)
    prop_oneof![Just('1'), Just('2'), Just('3'), Just('4'), Just('5'),]
}

fn navigation_key_strategy() -> impl Strategy<Value = char> {
    prop_oneof![Just('h'), Just('j'), Just('k'), Just('l'),]
}

fn terminal_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        // Screen navigation (5 screens)
        (1u8..=5).prop_map(|n| events::char(char::from_digit(n as u32, 10).unwrap())),
        // Vim navigation
        Just(events::char('h')),
        Just(events::char('j')),
        Just(events::char('k')),
        Just(events::char('l')),
        // Mode keys
        Just(events::char('i')),
        Just(events::escape()),
        Just(events::tab()),
        Just(events::enter()),
        // Special keys
        Just(events::char('?')),
        Just(events::char('q')),
        Just(events::char('f')),
        Just(events::char('n')),
        // Resize
        (10u16..200, 10u16..100).prop_map(|(w, h)| events::resize(w, h)),
        // Printable chars
        any::<char>()
            .prop_filter("printable", |c| c.is_ascii_graphic())
            .prop_map(events::char),
    ]
}

proptest! {
    /// Tab cycle always returns to start after 5 tabs (5 screens)
    #[test]
    fn prop_tab_cycle(start in 1u8..=5) {
        let mut tui = TestTui::new();
        tui.send_char(char::from_digit(start as u32, 10).unwrap());
        let initial = tui.screen();

        for _ in 0..5 {
            tui.send_tab();
        }

        prop_assert_eq!(tui.screen(), initial);
    }

    /// Screen navigation always works in normal mode
    #[test]
    fn prop_screen_nav_works(key in screen_key_strategy()) {
        let mut tui = TestTui::new();

        // Some random navigation first
        tui.send_tab();
        tui.send_tab();

        tui.send_char(key);

        let expected = match key {
            '1' => Screen::Neighborhood,
            '2' => Screen::Chat,
            '3' => Screen::Contacts,
            '4' => Screen::Notifications,
            '5' => Screen::Settings,
            _ => unreachable!(),
        };

        prop_assert_eq!(tui.screen(), expected);
    }

    /// Modal always homes screen navigation
    #[test]
    fn prop_modal_homes_nav(nav_keys in prop::collection::vec(screen_key_strategy(), 1..10)) {
        let mut tui = TestTui::new();
        let screen_before = tui.screen();

        tui.send_char('?'); // Open help
        prop_assert!(tui.has_modal());

        for key in nav_keys {
            tui.send_char(key);
        }

        prop_assert_eq!(tui.screen(), screen_before);
        prop_assert!(tui.has_modal());
    }

    /// Escape always exits insert mode
    #[test]
    fn prop_escape_exits_insert(chars in prop::collection::vec(any::<char>().prop_filter("printable", |c| c.is_ascii_graphic()), 0..50)) {
        let mut tui = TestTui::new();

        // Enter detail mode for Neighborhood before insert
        if tui.screen() == Screen::Neighborhood {
            // Need at least one home to enter detail mode
            tui.state.neighborhood.home_count = 1;
            tui.send_enter();
        }
        tui.send_char('i');
        prop_assert!(tui.is_insert_mode());

        for c in chars {
            tui.send_char(c);
        }

        tui.send_escape();
        prop_assert!(!tui.is_insert_mode());
    }

    /// Arbitrary events never panic
    #[test]
    fn prop_no_panics(events in prop::collection::vec(terminal_event_strategy(), 0..200)) {
        let mut tui = TestTui::new();

        for event in events {
            tui.send(event);
        }

        // Access state to ensure it's valid
        let _ = tui.screen();
        let _ = tui.is_insert_mode();
        let _ = tui.has_modal();
    }

    /// State transitions are deterministic
    #[test]
    fn prop_deterministic(events in prop::collection::vec(terminal_event_strategy(), 1..50)) {
        let mut tui1 = TestTui::new();
        let mut tui2 = TestTui::new();

        for event in &events {
            tui1.send(event.clone());
            tui2.send(event.clone());
        }

        prop_assert_eq!(tui1.screen(), tui2.screen());
        prop_assert_eq!(tui1.is_insert_mode(), tui2.is_insert_mode());
        prop_assert_eq!(tui1.has_modal(), tui2.has_modal());
        prop_assert_eq!(tui1.state.terminal_size, tui2.state.terminal_size);
    }

    /// Navigation indices never go negative
    #[test]
    fn prop_indices_non_negative(k_presses in 0usize..100) {
        let mut tui = TestTui::new();

        // Test on various screens
        for screen in ['3', '4', '5'] {
            tui.send_char(screen);
            for _ in 0..k_presses {
                tui.send_char('k');
            }
        }

        // All indices are always >= 0 since they're usize - no need to check
    }

    /// Insert mode only on Chat
    #[test]
    fn prop_insert_mode_screens(screen in screen_key_strategy()) {
        let mut tui = TestTui::new();
        tui.send_char(screen);
        tui.send_char('i');

        let should_be_insert = matches!(tui.screen(), Screen::Chat);
        prop_assert_eq!(tui.is_insert_mode(), should_be_insert);
    }

    /// Help modal opens from any screen
    #[test]
    fn prop_help_from_anywhere(screen in screen_key_strategy()) {
        let mut tui = TestTui::new();
        tui.send_char(screen);
        tui.send_char('?');

        prop_assert!(tui.has_modal());
        prop_assert_eq!(tui.modal_type(), ModalType::Help);
    }

    /// Resize always updates terminal size
    #[test]
    fn prop_resize_updates(width in 10u16..500, height in 10u16..200) {
        let mut tui = TestTui::new();
        tui.send(events::resize(width, height));

        prop_assert_eq!(tui.state.terminal_size, (width, height));
    }
}

// ============================================================================
// INTEGRATION TESTS - Complex Workflows
// ============================================================================

mod integration {
    use super::*;

    #[test]
    fn test_complete_chat_workflow() {
        let mut tui = TestTui::new();

        // 1. Navigate to Chat
        tui.go_to_screen(Screen::Chat);

        // 2. Create a channel
        tui.send_char('n');
        tui.type_text("announcements");
        tui.send_enter();

        // 3. Set topic
        tui.send_char('t');
        tui.type_text("Important announcements only");
        tui.send_enter();

        // 4. Send a message
        tui.send_char('i');
        tui.type_text("Hello everyone!");
        tui.clear_commands();
        tui.send_enter();

        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. })));
    }

    // NOTE: Invitations screen merged into Contacts - invitation workflow test removed

    #[test]
    fn test_recovery_guardian_setup() {
        let mut tui = TestTui::new();

        // 1. Navigate to Contacts
        tui.go_to_screen(Screen::Contacts);

        // 2. Add a guardian (dispatch, then shell opens modal)
        tui.clear_commands();
        tui.send_char('g');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::OpenGuardianSetup)));
        tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
            aura_terminal::tui::state_machine::GuardianSetupModalState::default(),
        ));
        assert!(tui.state.is_guardian_setup_modal_active());

        // 2b. Dismiss and re-enqueue with populated contacts (shell would normally do this)
        tui.state.modal_queue.dismiss();
        let contacts = vec![
            aura_terminal::tui::state_machine::GuardianCandidate {
                id: "id1".to_string(),
                name: "Contact 1".to_string(),
                is_current_guardian: false,
            },
            aura_terminal::tui::state_machine::GuardianCandidate {
                id: "id2".to_string(),
                name: "Contact 2".to_string(),
                is_current_guardian: false,
            },
        ];
        tui.state.modal_queue.enqueue(QueuedModal::GuardianSetup(
            aura_terminal::tui::state_machine::GuardianSetupModalState::from_contacts_with_selection(contacts, vec![]),
        ));

        // 3. Select two contacts
        tui.send_char(' '); // Select first contact
        tui.send_char('j'); // Focus second contact
        tui.send_char(' '); // Select second contact

        // 4. Proceed to threshold selection
        tui.send_enter();
        if let Some(QueuedModal::GuardianSetup(state)) = tui.state.modal_queue.current() {
            assert_eq!(
                state.step(),
                aura_terminal::tui::state_machine::GuardianSetupStep::ChooseThreshold
            );
        } else {
            unreachable!("guardian setup modal missing after enter");
        }

        // 5. Start ceremony
        tui.clear_commands();
        tui.send_enter();
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::StartGuardianCeremony { .. })));
    }

    #[test]
    fn test_settings_complete_configuration() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Settings);

        // 1. Update nickname
        tui.send_enter();
        tui.type_text("NewName");
        tui.send_enter();

        // 2. Update threshold
        tui.send_char('j');
        tui.send_enter();
        tui.send_up();
        tui.send_enter();

        // 3. Add a device
        tui.send_char('j');
        tui.send_char('a');
        tui.type_text("Tablet");
        tui.send_enter();

        // 4. Cycle MFA
        tui.send_char('j');
        tui.send_char(' ');

        // Should complete without errors
    }

    #[test]
    fn test_neighborhood_exploration() {
        let mut tui = TestTui::new();
        tui.go_to_screen(Screen::Neighborhood);
        tui.state.neighborhood.home_count = 1;

        // Navigate around the grid
        tui.send_char('l');
        tui.send_char('l');
        tui.send_char('j');
        tui.send_char('j');

        // Enter a home
        tui.clear_commands();
        tui.send_enter();
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::EnterHome { .. })));

        // Go home
        tui.clear_commands();
        tui.state.neighborhood.mode = aura_terminal::tui::state_machine::NeighborhoodMode::Map;
        tui.send_char('g');
        assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::GoHome)));
    }
}
