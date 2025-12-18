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
//! # Deterministic TUI Tests
//!
//! These tests validate TUI behavior using the pure state machine model.
//! They are:
//! - **Fast**: No PTY setup, no sleeps, pure computation
//! - **Deterministic**: Same inputs = same outputs, every time
//! - **Debuggable**: Full state visibility at every step
//!
//! ## Comparison with PTY Tests
//!
//! | Aspect | PTY Tests | State Machine Tests |
//! |--------|-----------|---------------------|
//! | Speed | ~3-10s per test | <1ms per test |
//! | Reliability | Flaky (timing) | Deterministic |
//! | Debugging | Hard (terminal) | Easy (state dumps) |
//! | CI/CD | Requires PTY | Works anywhere |

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_terminal::tui::state_machine::{
    transition, ChatFocus, DispatchCommand, TuiCommand, TuiState,
};
use aura_terminal::tui::types::{RecoveryTab, SettingsSection};
use aura_terminal::tui::Screen;
use proptest::prelude::*;

// ============================================================================
// Test Helpers
// ============================================================================

/// Simple test wrapper for state machine
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

    fn send(&mut self, event: aura_core::effects::terminal::TerminalEvent) {
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

    fn screen(&self) -> Screen {
        self.state.screen()
    }

    fn assert_screen(&self, expected: Screen) {
        assert_eq!(
            self.screen(),
            expected,
            "Expected {:?}, got {:?}",
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

    fn has_dispatch(&self, check: impl Fn(&DispatchCommand) -> bool) -> bool {
        self.commands
            .iter()
            .any(|c| matches!(c, TuiCommand::Dispatch(d) if check(d)))
    }

    fn clear_commands(&mut self) {
        self.commands.clear();
    }
}

// ============================================================================
// Screen Navigation Tests (converted from test_screen_navigation)
// ============================================================================

/// Test screen navigation using number keys
///
/// This is the deterministic version of `test_screen_navigation`.
/// Original PTY test took ~5s with sleeps. This takes <1ms.
#[test]
fn test_screen_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Start at Block screen
    tui.assert_screen(Screen::Block);

    // Navigate to Chat screen (2)
    tui.send_char('2');
    tui.assert_screen(Screen::Chat);

    // Navigate to Contacts screen (3)
    tui.send_char('3');
    tui.assert_screen(Screen::Contacts);

    // Navigate to Neighborhood screen (4)
    tui.send_char('4');
    tui.assert_screen(Screen::Neighborhood);

    // Navigate to Settings screen (5)
    tui.send_char('5');
    tui.assert_screen(Screen::Settings);

    // Navigate to Recovery screen (6)
    tui.send_char('6');
    tui.assert_screen(Screen::Recovery);

    // Navigate back to Block screen (1)
    tui.send_char('1');
    tui.assert_screen(Screen::Block);
}

// ============================================================================
// Tab Navigation Tests (converted from test_tab_navigation)
// ============================================================================

/// Test Tab key cycles through all screens
///
/// Original PTY test took ~5s with sleeps. This takes <1ms.
#[test]
fn test_tab_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Verify starting screen
    tui.assert_screen(Screen::Block);

    // Tab through all screens (6 screens total, Invitations merged into Contacts)
    let expected_order = [
        Screen::Chat,
        Screen::Contacts,
        Screen::Neighborhood,
        Screen::Settings,
        Screen::Recovery,
        Screen::Block, // Wraps around
    ];

    for expected in expected_order.iter() {
        tui.send_tab();
        tui.assert_screen(*expected);
    }
}

// ============================================================================
// Chat Screen Tests (converted from test_chat_keyboard_shortcuts)
// ============================================================================

/// Test Chat screen keyboard shortcuts
///
/// Original PTY test took ~5s. This takes <1ms.
#[test]
fn test_chat_keyboard_shortcuts_deterministic() {
    let mut tui = TestTui::new();

    // Go to Chat screen
    tui.send_char('2');
    tui.assert_screen(Screen::Chat);

    // Set up item counts for navigation to work
    tui.state.chat.channel_count = 10;
    tui.state.chat.message_count = 50;

    // Chat starts at Channels by default
    assert_eq!(tui.state.chat.focus, ChatFocus::Channels);

    // Test 'l' for focus right (message area)
    tui.send_char('l');
    assert_eq!(tui.state.chat.focus, ChatFocus::Messages);

    // Test 'h' for focus left (wraps back to channels)
    tui.send_char('h');
    assert_eq!(tui.state.chat.focus, ChatFocus::Channels);

    // Go back to Messages for scroll test
    tui.send_char('l');
    assert_eq!(tui.state.chat.focus, ChatFocus::Messages);

    // Test 'j' for scroll down in messages
    let initial_scroll = tui.state.chat.message_scroll;
    tui.send_char('j');
    assert_eq!(tui.state.chat.message_scroll, initial_scroll + 1);

    // Test 'k' for scroll up in messages
    tui.send_char('k');
    assert_eq!(tui.state.chat.message_scroll, initial_scroll);

    // Test 'i' for insert mode
    assert!(!tui.is_insert_mode());
    tui.send_char('i');
    assert!(tui.is_insert_mode());
    assert_eq!(tui.state.chat.focus, ChatFocus::Input);

    // Escape exits insert mode
    tui.send_escape();
    assert!(!tui.is_insert_mode());
}

/// Test Chat channel selection
#[test]
fn test_chat_channel_selection_deterministic() {
    let mut tui = TestTui::new();

    // Go to Chat - starts at Channels by default
    tui.send_char('2');
    assert_eq!(tui.state.chat.focus, ChatFocus::Channels);

    // Set up item counts for navigation to work
    tui.state.chat.channel_count = 10;
    tui.state.chat.message_count = 50;

    // Navigate channel list
    let initial = tui.state.chat.selected_channel;
    tui.send_char('j'); // Down
    assert_eq!(tui.state.chat.selected_channel, initial + 1);

    tui.send_char('k'); // Up
    assert_eq!(tui.state.chat.selected_channel, initial);
}

// ============================================================================
// Insert Mode Tests
// ============================================================================

/// Test insert mode text entry
#[test]
fn test_insert_mode_text_entry_deterministic() {
    let mut tui = TestTui::new();

    // Go to Chat and enter insert mode
    tui.send_char('2');
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    // Type a message
    for c in "Hello, world!".chars() {
        tui.send_char(c);
    }

    // Verify input buffer
    assert_eq!(tui.state.chat.input_buffer, "Hello, world!");

    // Press Enter to send
    tui.clear_commands();
    tui.send_enter();

    // Verify SendChatMessage command was generated
    assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. })));

    // Input buffer should be cleared
    assert!(tui.state.chat.input_buffer.is_empty());
}

/// Test Escape exits insert mode without sending
///
/// Note: Escape exits insert mode but does NOT clear the buffer.
/// This matches vim behavior where Escape doesn't discard typed content.
#[test]
fn test_escape_exits_insert_mode_deterministic() {
    let mut tui = TestTui::new();

    // Enter insert mode on Block screen
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    // Type something
    tui.send_char('t');
    tui.send_char('e');
    tui.send_char('s');
    tui.send_char('t');
    assert_eq!(tui.state.block.input_buffer, "test");

    // Escape should exit insert mode
    tui.send_escape();
    assert!(!tui.is_insert_mode());

    // Buffer is preserved (not cleared) - vim-style behavior
    assert_eq!(tui.state.block.input_buffer, "test");
}

// ============================================================================
// Modal Tests
// ============================================================================

/// Test help modal opens and closes
#[test]
fn test_help_modal_deterministic() {
    let mut tui = TestTui::new();

    // No modal initially
    assert!(!tui.has_modal());

    // Press '?' for help
    tui.send_char('?');
    assert!(tui.has_modal());

    // Press Escape to close
    tui.send_escape();
    assert!(!tui.has_modal());
}

/// Test modal blocks screen navigation
#[test]
fn test_modal_blocks_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Open help modal
    tui.send_char('?');
    assert!(tui.has_modal());

    // Try to navigate - should be blocked
    let screen_before = tui.screen();
    tui.send_char('2');
    assert_eq!(tui.screen(), screen_before);
    assert!(tui.has_modal()); // Modal still open
}

// ============================================================================
// Recovery Screen Tests
// ============================================================================

/// Test Recovery screen tab navigation
///
/// Note: Tab order is Guardians -> Recovery -> Requests (wrapping).
#[test]
fn test_recovery_tabs_deterministic() {
    let mut tui = TestTui::new();

    // Go to Recovery screen (key '6' after Invitations merged into Contacts)
    tui.send_char('6');
    tui.assert_screen(Screen::Recovery);

    // Default tab is Guardians
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Guardians);

    // Navigate right: Guardians -> Recovery
    tui.send_char('l');
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Recovery);

    // Navigate right: Recovery -> Requests
    tui.send_char('l');
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Requests);

    // Navigate left: Requests -> Recovery
    tui.send_char('h');
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Recovery);

    // Navigate left: Recovery -> Guardians
    tui.send_char('h');
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Guardians);
}

/// Test Recovery screen guardian selection
#[test]
fn test_recovery_guardian_list_deterministic() {
    let mut tui = TestTui::new();

    // Go to Recovery, Guardians tab (key '6' after Invitations merged into Contacts)
    tui.send_char('6');
    assert_eq!(tui.state.recovery.tab, RecoveryTab::Guardians);

    // Set up item count for navigation to work
    tui.state.recovery.item_count = 10;

    // Navigate list
    let initial = tui.state.recovery.selected_index;
    tui.send_char('j');
    assert_eq!(tui.state.recovery.selected_index, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state.recovery.selected_index, initial);
}

// ============================================================================
// Settings Screen Tests
// ============================================================================

/// Test Settings screen section navigation
///
/// Note: Sections cycle with j/k (down/up), not Tab. Tab is global navigation.
#[test]
fn test_settings_sections_deterministic() {
    let mut tui = TestTui::new();

    // Go to Settings (key '5' after Invitations merged into Contacts)
    tui.send_char('5');
    tui.assert_screen(Screen::Settings);

    // Default section is Profile
    assert_eq!(tui.state.settings.section, SettingsSection::Profile);

    // 'j' moves to next section: Profile -> Threshold -> Devices -> Mfa -> Profile
    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Threshold);

    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Devices);

    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Mfa);

    tui.send_char('j');
    assert_eq!(tui.state.settings.section, SettingsSection::Profile); // Wraps
}

// ============================================================================
// Quit Tests
// ============================================================================

/// Test 'q' triggers exit
#[test]
fn test_quit_deterministic() {
    let mut tui = TestTui::new();

    // Not exiting initially
    assert!(!tui.state.should_exit);

    // Press 'q' to quit
    tui.send_char('q');

    // Should be marked for exit
    assert!(tui.state.should_exit);

    // Exit command should be generated
    assert!(tui.commands.iter().any(|c| matches!(c, TuiCommand::Exit)));
}

/// Test 'q' doesn't quit in insert mode
#[test]
fn test_quit_blocked_in_insert_mode_deterministic() {
    let mut tui = TestTui::new();

    // Enter insert mode
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    // Press 'q' - should type 'q', not quit
    tui.send_char('q');
    assert!(!tui.state.should_exit);
    assert_eq!(tui.state.block.input_buffer, "q");
}

// ============================================================================
// Contacts Screen Tests
// ============================================================================

/// Test Contacts screen navigation
#[test]
fn test_contacts_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Go to Contacts
    tui.send_char('3');
    tui.assert_screen(Screen::Contacts);

    // Set up item count for navigation to work
    tui.state.contacts.contact_count = 10;

    // Navigate contact list
    let initial = tui.state.contacts.selected_index;
    tui.send_char('j');
    assert_eq!(tui.state.contacts.selected_index, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state.contacts.selected_index, initial);
}

// ============================================================================
// Invitations Screen Tests
// ============================================================================
// NOTE: Invitations screen was merged into Contacts screen. Test removed.

// ============================================================================
// Resize Event Tests
// ============================================================================

/// Test terminal resize updates state
#[test]
fn test_resize_event_deterministic() {
    let mut tui = TestTui::new();

    // Initial size is default
    assert_eq!(tui.state.terminal_size, (80, 24));

    // Resize event
    tui.send(events::resize(120, 40));
    assert_eq!(tui.state.terminal_size, (120, 40));

    // Another resize
    tui.send(events::resize(200, 60));
    assert_eq!(tui.state.terminal_size, (200, 60));
}

// ============================================================================
// Stress Tests (equivalent to freeze diagnostic but instant)
// ============================================================================

/// Stress test: rapid screen switching
#[test]
fn test_rapid_screen_switching_deterministic() {
    let mut tui = TestTui::new();

    // Rapidly switch screens 1000 times (6 screens: 1-6, Invitations merged into Contacts)
    for i in 0..1000 {
        let screen_num = (i % 6) + 1;
        tui.send_char(char::from_digit(screen_num as u32, 10).unwrap());
    }

    // Last iteration: i=999, 999 % 6 = 3, 3 + 1 = 4 (Neighborhood)
    tui.assert_screen(Screen::Neighborhood);
}

/// Stress test: rapid insert mode toggling
#[test]
fn test_rapid_insert_mode_toggling_deterministic() {
    let mut tui = TestTui::new();

    // Toggle insert mode 1000 times
    for _ in 0..1000 {
        tui.send_char('i');
        tui.send_escape();
    }

    // Should end in normal mode
    assert!(!tui.is_insert_mode());
}

/// Stress test: long text input
#[test]
fn test_long_text_input_deterministic() {
    let mut tui = TestTui::new();

    // Enter insert mode
    tui.send_char('i');

    // Type a very long message
    let long_text = "a".repeat(10000);
    for c in long_text.chars() {
        tui.send_char(c);
    }

    // Verify buffer length
    assert_eq!(tui.state.block.input_buffer.len(), 10000);
}

// ============================================================================
// Property-Based Tests (proptest)
// ============================================================================

/// Strategy for generating valid screen numbers (1-6, Invitations merged into Contacts)
fn screen_key_strategy() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('1'),
        Just('2'),
        Just('3'),
        Just('4'),
        Just('5'),
        Just('6'),
    ]
}

/// Strategy for generating terminal events
fn terminal_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        // Screen navigation keys (1-7)
        (1u8..=7).prop_map(|n| events::char(char::from_digit(n as u32, 10).unwrap())),
        // Vim-style navigation
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
        Just(events::char('f')),
        Just(events::char('n')),
        // Resize events
        (10u16..200, 10u16..100).prop_map(|(w, h)| events::resize(w, h)),
    ]
}

proptest! {
    /// Property: Tab cycle (6 times) returns to starting screen (6 screens total)
    #[test]
    fn prop_tab_cycle_returns_to_start(start_screen in 1u8..=6) {
        let mut tui = TestTui::new();

        // Go to a specific screen
        tui.send_char(char::from_digit(start_screen as u32, 10).unwrap());
        let initial_screen = tui.screen();

        // Tab 6 times should cycle back
        for _ in 0..6 {
            tui.send_tab();
        }

        prop_assert_eq!(tui.screen(), initial_screen,
            "After 6 tabs, should return to initial screen {:?}", initial_screen);
    }

    /// Property: Screen navigation keys always work in normal mode
    #[test]
    fn prop_screen_nav_always_works(screen_key in screen_key_strategy()) {
        let mut tui = TestTui::new();

        // Navigate to various screens first
        tui.send_char('3');
        tui.send_char('5');

        // Should be able to navigate with number key
        tui.send_char(screen_key);

        let expected_screen = match screen_key {
            '1' => Screen::Block,
            '2' => Screen::Chat,
            '3' => Screen::Contacts,
            '4' => Screen::Neighborhood,
            '5' => Screen::Settings,
            '6' => Screen::Recovery,
            _ => unreachable!(),
        };

        prop_assert_eq!(tui.screen(), expected_screen);
    }

    /// Property: Modal blocks all screen navigation
    #[test]
    fn prop_modal_blocks_all_navigation(
        initial_screen in 1u8..=6,
        nav_attempts in prop::collection::vec(screen_key_strategy(), 1..10)
    ) {
        let mut tui = TestTui::new();

        // Go to initial screen
        tui.send_char(char::from_digit(initial_screen as u32, 10).unwrap());
        let screen_before = tui.screen();

        // Open help modal
        tui.send_char('?');
        prop_assert!(tui.has_modal());

        // Try various navigation keys
        for key in nav_attempts {
            tui.send_char(key);
        }

        // Screen should not have changed
        prop_assert_eq!(tui.screen(), screen_before);
        // Modal should still be open
        prop_assert!(tui.has_modal());
    }

    /// Property: Escape always exits insert mode
    #[test]
    fn prop_escape_exits_insert_mode(text_length in 0usize..100) {
        let mut tui = TestTui::new();

        // Enter insert mode
        tui.send_char('i');
        prop_assert!(tui.is_insert_mode());

        // Type some text
        for _ in 0..text_length {
            tui.send_char('a');
        }

        // Escape should always exit insert mode
        tui.send_escape();
        prop_assert!(!tui.is_insert_mode());
    }

    /// Property: Arbitrary event sequences never panic
    #[test]
    fn prop_no_panics_on_arbitrary_events(
        events in prop::collection::vec(terminal_event_strategy(), 0..100)
    ) {
        let mut tui = TestTui::new();

        // Apply all events - should never panic
        for event in events {
            tui.send(event);
        }

        // State should still be valid
        let _ = tui.screen();
        let _ = tui.is_insert_mode();
        let _ = tui.has_modal();
    }

    /// Property: State transitions are deterministic
    #[test]
    fn prop_transitions_are_deterministic(
        events in prop::collection::vec(terminal_event_strategy(), 1..20)
    ) {
        // First run
        let mut tui1 = TestTui::new();
        for event in &events {
            tui1.send(event.clone());
        }

        // Second run with same events
        let mut tui2 = TestTui::new();
        for event in &events {
            tui2.send(event.clone());
        }

        // States should be identical
        prop_assert_eq!(tui1.screen(), tui2.screen());
        prop_assert_eq!(tui1.is_insert_mode(), tui2.is_insert_mode());
        prop_assert_eq!(tui1.has_modal(), tui2.has_modal());
        prop_assert_eq!(tui1.state.terminal_size, tui2.state.terminal_size);
        prop_assert_eq!(tui1.state.should_exit, tui2.state.should_exit);
    }

    /// Property: Resize events update terminal size correctly
    #[test]
    fn prop_resize_updates_size(width in 10u16..500, height in 10u16..200) {
        let mut tui = TestTui::new();

        tui.send(events::resize(width, height));

        prop_assert_eq!(tui.state.terminal_size, (width, height));
    }

    /// Property: Insert mode only available on Block and Chat screens
    #[test]
    fn prop_insert_mode_only_on_valid_screens(screen_key in screen_key_strategy()) {
        let mut tui = TestTui::new();

        tui.send_char(screen_key);
        tui.send_char('i'); // Try to enter insert mode

        let should_have_insert_mode = matches!(tui.screen(), Screen::Block | Screen::Chat);
        prop_assert_eq!(tui.is_insert_mode(), should_have_insert_mode,
            "Insert mode should only be available on Block/Chat, got {:?}", tui.screen());
    }

    /// Property: Navigation index never goes negative (saturating_sub)
    #[test]
    fn prop_navigation_saturates_at_zero(k_presses in 0usize..50) {
        let mut tui = TestTui::new();

        // Go to Contacts screen
        tui.send_char('3');

        // Press 'k' (up) many times
        for _ in 0..k_presses {
            tui.send_char('k');
        }

        // Index should be 0, not negative
        prop_assert_eq!(tui.state.contacts.selected_index, 0);
    }

    /// Property: Escape closes any modal
    #[test]
    fn prop_escape_closes_modal(nav_count in 0usize..10) {
        let mut tui = TestTui::new();

        // Open help modal
        tui.send_char('?');
        prop_assert!(tui.has_modal());

        // Try some navigation
        for _ in 0..nav_count {
            tui.send_char('j');
        }

        // Escape should always close modal
        tui.send_escape();
        prop_assert!(!tui.has_modal());
    }
}
