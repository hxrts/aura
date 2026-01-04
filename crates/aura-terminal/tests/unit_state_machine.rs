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

mod support;

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_terminal::tui::navigation::TwoPanelFocus;
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::state_machine::{
    ChatFocus, ChatMemberCandidate, ChatMemberSelectModalState, ContactSelectModalState,
    CreateChannelModalState, CreateChannelStep, DispatchCommand, ModalType, QueuedModal,
    TuiCommand,
};
use aura_terminal::tui::types::SettingsSection;
use proptest::prelude::*;
use support::TestTui;

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

    // Start at Neighborhood screen
    tui.assert_screen(Screen::Neighborhood);

    // Navigate to Chat screen (2)
    tui.send_char('2');
    tui.assert_screen(Screen::Chat);

    // Navigate to Contacts screen (3)
    tui.send_char('3');
    tui.assert_screen(Screen::Contacts);

    // Navigate to Notifications screen (4)
    tui.send_char('4');
    tui.assert_screen(Screen::Notifications);

    // Navigate to Settings screen (5)
    tui.send_char('5');
    tui.assert_screen(Screen::Settings);

    // Navigate back to Neighborhood screen (1)
    tui.send_char('1');
    tui.assert_screen(Screen::Neighborhood);
}

#[test]
fn test_demo_shortcuts_fill_contacts_import_modal() {
    let mut tui = TestTui::new();
    tui.state_mut().contacts.demo_alice_code = "ALICECODE".to_string();
    tui.state_mut().contacts.demo_carol_code = "CAROLCODE".to_string();

    // Go to Contacts screen
    tui.send_char('3');
    tui.assert_screen(Screen::Contacts);

    // Open Contacts import modal
    tui.send_char('a');
    assert!(tui.has_modal());

    // Ctrl+A fills Alice code
    tui.send(events::ctrl('a'));
    match tui.state().modal_queue.current() {
        Some(QueuedModal::ContactsImport(s)) => assert_eq!(s.code, "ALICECODE"),
        other => panic!("Expected ContactsImport modal, got {other:?}"),
    }

    // Ctrl+L fills Carol code
    tui.send(events::ctrl('l'));
    match tui.state().modal_queue.current() {
        Some(QueuedModal::ContactsImport(s)) => assert_eq!(s.code, "CAROLCODE"),
        other => panic!("Expected ContactsImport modal, got {other:?}"),
    }
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
    tui.assert_screen(Screen::Neighborhood);

    // Tab through all screens (5 screens total, Invitations merged into Contacts)
    let expected_order = [
        Screen::Chat,
        Screen::Contacts,
        Screen::Notifications,
        Screen::Settings,
        Screen::Neighborhood, // Wraps around
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
    tui.state_mut().chat.channel_count = 10;
    tui.state_mut().chat.message_count = 50;

    // Chat starts at Channels by default
    assert_eq!(tui.state().chat.focus, ChatFocus::Channels);

    // Test 'l' for focus right (message area)
    tui.send_char('l');
    assert_eq!(tui.state().chat.focus, ChatFocus::Messages);

    // Test 'h' for focus left (wraps back to channels)
    tui.send_char('h');
    assert_eq!(tui.state().chat.focus, ChatFocus::Channels);

    // Go back to Messages for scroll test
    tui.send_char('l');
    assert_eq!(tui.state().chat.focus, ChatFocus::Messages);

    // Test 'j' for scroll down in messages
    let initial_scroll = tui.state().chat.message_scroll;
    tui.send_char('j');
    assert_eq!(tui.state().chat.message_scroll, initial_scroll + 1);

    // Test 'k' for scroll up in messages
    tui.send_char('k');
    assert_eq!(tui.state().chat.message_scroll, initial_scroll);

    // Test 'i' for insert mode
    assert!(!tui.is_insert_mode());
    tui.send_char('i');
    assert!(tui.is_insert_mode());
    assert_eq!(tui.state().chat.focus, ChatFocus::Input);

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
    assert_eq!(tui.state().chat.focus, ChatFocus::Channels);

    // Set up item counts for navigation to work
    tui.state_mut().chat.channel_count = 10;
    tui.state_mut().chat.message_count = 50;

    // Navigate channel list
    let initial = tui.state().chat.selected_channel;
    tui.send_char('j'); // Down
    assert_eq!(tui.state().chat.selected_channel, initial + 1);

    tui.send_char('k'); // Up
    assert_eq!(tui.state().chat.selected_channel, initial);
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
    assert_eq!(tui.state().chat.input_buffer, "Hello, world!");

    // Press Enter to send
    tui.clear_commands();
    tui.send_enter();

    // Verify SendChatMessage command was generated
    assert!(tui.has_dispatch(|d| matches!(d, DispatchCommand::SendChatMessage { .. })));

    // Input buffer should be cleared
    assert!(tui.state().chat.input_buffer.is_empty());
}

/// Test Escape exits insert mode without sending
///
/// Note: Escape exits insert mode but does NOT clear the buffer.
/// This matches vim behavior where Escape doesn't discard typed content.
#[test]
fn test_escape_exits_insert_mode_deterministic() {
    let mut tui = TestTui::new();

    // Enter home detail mode then insert mode on Neighborhood screen
    tui.send_enter();
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    // Type something
    tui.send_char('t');
    tui.send_char('e');
    tui.send_char('s');
    tui.send_char('t');
    assert_eq!(tui.state().neighborhood.input_buffer, "test");

    // Escape should exit insert mode
    tui.send_escape();
    assert!(!tui.is_insert_mode());

    // Buffer is preserved (not cleared) - vim-style behavior
    assert_eq!(tui.state().neighborhood.input_buffer, "test");
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

/// Test modal homes screen navigation
#[test]
fn test_modal_blocks_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Open help modal
    tui.send_char('?');
    assert!(tui.has_modal());

    // Try to navigate - should be blocked
    let screen_before = tui.screen();
    tui.send_char('3');
    assert_eq!(tui.screen(), screen_before);
    assert!(tui.has_modal()); // Modal still open
}

// ============================================================================
// Notifications Screen Tests
// ============================================================================

/// Test Notifications screen focus and list navigation
#[test]
fn test_notifications_navigation_deterministic() {
    let mut tui = TestTui::new();

    // Go to Notifications screen (key '4')
    tui.send_char('4');
    tui.assert_screen(Screen::Notifications);

    // Toggle focus between list/detail
    let initial_focus = tui.state().notifications.focus;
    tui.send_char('l');
    assert_ne!(tui.state().notifications.focus, initial_focus);
    tui.send_char('h');
    assert_eq!(tui.state().notifications.focus, initial_focus);

    // Set up item count for navigation to work
    tui.state_mut().notifications.item_count = 10;

    // Navigate list
    let initial = tui.state().notifications.selected_index;
    tui.send_char('j');
    assert_eq!(tui.state().notifications.selected_index, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state().notifications.selected_index, initial);
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

    // Go to Settings (key '5')
    tui.send_char('5');
    tui.assert_screen(Screen::Settings);

    // Default section is Profile
    assert_eq!(tui.state().settings.section, SettingsSection::Profile);

    // 'j' moves to next section: Profile -> Threshold -> Recovery -> Devices -> Authority -> Profile
    tui.send_char('j');
    assert_eq!(tui.state().settings.section, SettingsSection::Threshold);

    tui.send_char('j');
    assert_eq!(tui.state().settings.section, SettingsSection::Recovery);

    tui.send_char('j');
    assert_eq!(tui.state().settings.section, SettingsSection::Devices);

    tui.send_char('j');
    assert_eq!(tui.state().settings.section, SettingsSection::Authority);

    tui.send_char('j');
    assert_eq!(tui.state().settings.section, SettingsSection::Profile); // Wraps
}

// ============================================================================
// Quit Tests
// ============================================================================

/// Test 'q' triggers exit
#[test]
fn test_quit_deterministic() {
    let mut tui = TestTui::new();

    // Not exiting initially
    assert!(!tui.state().should_exit);

    // Press 'q' to quit
    tui.send_char('q');

    // Should be marked for exit
    assert!(tui.state().should_exit);

    // Exit command should be generated
    assert!(tui.commands().iter().any(|c| matches!(c, TuiCommand::Exit)));
}

/// Test 'q' doesn't quit in insert mode
#[test]
fn test_quit_blocked_in_insert_mode_deterministic() {
    let mut tui = TestTui::new();

    // Enter detail mode then insert mode
    tui.send_enter();
    tui.send_char('i');
    assert!(tui.is_insert_mode());

    // Press 'q' - should type 'q', not quit
    tui.send_char('q');
    assert!(!tui.state().should_exit);
    assert_eq!(tui.state().neighborhood.input_buffer, "q");
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
    tui.state_mut().contacts.contact_count = 10;

    // Navigate contact list
    let initial = tui.state().contacts.selected_index;
    tui.send_char('j');
    assert_eq!(tui.state().contacts.selected_index, initial + 1);

    tui.send_char('k');
    assert_eq!(tui.state().contacts.selected_index, initial);
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
    assert_eq!(tui.state().terminal_size, (80, 24));

    // Resize event
    tui.send(events::resize(120, 40));
    assert_eq!(tui.state().terminal_size, (120, 40));

    // Another resize
    tui.send(events::resize(200, 60));
    assert_eq!(tui.state().terminal_size, (200, 60));
}

// ============================================================================
// Stress Tests (equivalent to freeze diagnostic but instant)
// ============================================================================

/// Stress test: rapid screen switching
#[test]
fn test_rapid_screen_switching_deterministic() {
    let mut tui = TestTui::new();

    // Rapidly switch screens 1000 times (5 screens: 1-5, Invitations merged into Contacts)
    for i in 0..1000 {
        let screen_num = (i % 5) + 1;
        tui.send_char(char::from_digit(screen_num as u32, 10).unwrap());
    }

    // Last iteration: i=999, 999 % 5 = 4, 4 + 1 = 5 (Settings)
    tui.assert_screen(Screen::Settings);
}

/// Stress test: rapid insert mode toggling
#[test]
fn test_rapid_insert_mode_toggling_deterministic() {
    let mut tui = TestTui::new();

    // Toggle insert mode 1000 times (enter detail mode each time)
    for _ in 0..1000 {
        tui.send_enter();
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

    // Enter detail mode then insert mode
    tui.send_enter();
    tui.send_char('i');

    // Type a very long message
    let long_text = "a".repeat(10000);
    for c in long_text.chars() {
        tui.send_char(c);
    }

    // Verify buffer length
    assert_eq!(tui.state().neighborhood.input_buffer.len(), 10000);
}

// ============================================================================
// Property-Based Tests (proptest)
// ============================================================================

/// Strategy for generating valid screen numbers (1-5, Invitations merged into Contacts)
fn screen_key_strategy() -> impl Strategy<Value = char> {
    prop_oneof![Just('1'), Just('2'), Just('3'), Just('4'), Just('5'),]
}

/// Strategy for generating terminal events
fn terminal_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        // Screen navigation keys (1-5)
        (1u8..=5).prop_map(|n| events::char(char::from_digit(n as u32, 10).unwrap())),
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

#[derive(Clone, Copy, Debug)]
enum NavDir {
    Up,
    Down,
}

fn nav_dir_strategy() -> impl Strategy<Value = NavDir> {
    prop_oneof![Just(NavDir::Up), Just(NavDir::Down)]
}

fn nav_dir_event(dir: NavDir) -> TerminalEvent {
    match dir {
        NavDir::Up => events::arrow_up(),
        NavDir::Down => events::arrow_down(),
    }
}

fn nav_dir_inverse(dir: NavDir) -> NavDir {
    match dir {
        NavDir::Up => NavDir::Down,
        NavDir::Down => NavDir::Up,
    }
}

fn nav_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        Just(events::arrow_up()),
        Just(events::arrow_down()),
        Just(events::char('k')),
        Just(events::char('j')),
    ]
}

fn modal_safe_event_strategy() -> impl Strategy<Value = TerminalEvent> {
    prop_oneof![
        Just(events::tab()),
        Just(events::arrow_up()),
        Just(events::arrow_down()),
        Just(events::arrow_left()),
        Just(events::arrow_right()),
    ]
}

proptest! {
    /// Property: Tab cycle (5 times) returns to starting screen (5 screens total)
    #[test]
    fn prop_tab_cycle_returns_to_start(start_screen in 1u8..=5) {
        let mut tui = TestTui::new();

        // Go to a specific screen
        tui.send_char(char::from_digit(start_screen as u32, 10).unwrap());
        let initial_screen = tui.screen();

        // Tab 5 times should cycle back
        for _ in 0..5 {
            tui.send_tab();
        }

        prop_assert_eq!(tui.screen(), initial_screen,
            "After 5 tabs, should return to initial screen {:?}", initial_screen);
    }

    /// Property: Screen navigation keys always work in normal mode
    #[test]
    fn prop_screen_nav_always_works(screen_key in screen_key_strategy()) {
        let mut tui = TestTui::new();

        // Navigate to various screens first
        tui.send_char('4');
        tui.send_char('5');

        // Should be able to navigate with number key
        tui.send_char(screen_key);

        let expected_screen = match screen_key {
            '1' => Screen::Neighborhood,
            '2' => Screen::Chat,
            '3' => Screen::Contacts,
            '4' => Screen::Notifications,
            '5' => Screen::Settings,
            _ => unreachable!(),
        };

        prop_assert_eq!(tui.screen(), expected_screen);
    }

    /// Property: Modal homes all screen navigation
    #[test]
    fn prop_modal_blocks_all_navigation(
        initial_screen in 1u8..=5,
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

        // Enter detail mode then insert mode
        tui.send_enter();
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
        prop_assert_eq!(tui1.state().terminal_size, tui2.state().terminal_size);
        prop_assert_eq!(tui1.state().should_exit, tui2.state().should_exit);
    }

    /// Property: Resize events update terminal size correctly
    #[test]
    fn prop_resize_updates_size(width in 10u16..500, height in 10u16..200) {
        let mut tui = TestTui::new();

        tui.send(events::resize(width, height));

        prop_assert_eq!(tui.state().terminal_size, (width, height));
    }

    /// Property: Insert mode only available on Neighborhood and Chat screens
    #[test]
    fn prop_insert_mode_only_on_valid_screens(screen_key in screen_key_strategy()) {
        let mut tui = TestTui::new();

        tui.send_char(screen_key);
        if tui.screen() == Screen::Neighborhood {
            tui.send_enter();
        }
        tui.send_char('i'); // Try to enter insert mode

        let should_have_insert_mode = matches!(tui.screen(), Screen::Neighborhood | Screen::Chat);
        prop_assert_eq!(tui.is_insert_mode(), should_have_insert_mode,
            "Insert mode should only be available on Neighborhood/Chat, got {:?}", tui.screen());
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
        prop_assert_eq!(tui.state().contacts.selected_index, 0);
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

    /// Property: Contacts selection stays within list bounds
    #[test]
    fn prop_contacts_selection_in_bounds(
        count in 1usize..50,
        start in 0usize..50,
        events in prop::collection::vec(nav_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('3');
        tui.state_mut().contacts.contact_count = count;
        tui.state_mut().contacts.selected_index = start % count;
        tui.state_mut().contacts.focus = TwoPanelFocus::List;

        for event in events {
            tui.send(event);
        }

        prop_assert!(tui.state().contacts.selected_index < count);
    }

    /// Property: Notifications selection stays within list bounds
    #[test]
    fn prop_notifications_selection_in_bounds(
        count in 1usize..50,
        start in 0usize..50,
        events in prop::collection::vec(nav_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('4');
        tui.state_mut().notifications.item_count = count;
        tui.state_mut().notifications.selected_index = start % count;

        for event in events {
            tui.send(event);
        }

        prop_assert!(tui.state().notifications.selected_index < count);
    }

    /// Property: Neighborhood resident selection stays within list bounds
    #[test]
    fn prop_neighborhood_resident_selection_in_bounds(
        count in 1usize..50,
        start in 0usize..50,
        events in prop::collection::vec(nav_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('1');
        tui.send_enter();
        tui.state_mut().neighborhood.resident_count = count;
        tui.state_mut().neighborhood.selected_resident = start % count;
        tui.state_mut().neighborhood.detail_focus = aura_terminal::tui::state_machine::DetailFocus::Residents;

        for event in events {
            tui.send(event);
        }

        prop_assert!(tui.state().neighborhood.selected_resident < count);
    }

    /// Property: Chat channel selection stays within list bounds
    #[test]
    fn prop_chat_channel_selection_in_bounds(
        count in 1usize..50,
        start in 0usize..50,
        events in prop::collection::vec(nav_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('2');
        tui.state_mut().chat.channel_count = count;
        tui.state_mut().chat.selected_channel = start % count;
        tui.state_mut().chat.focus = ChatFocus::Channels;

        for event in events {
            tui.send(event);
        }

        prop_assert!(tui.state().chat.selected_channel < count);
    }

    /// Property: Chat message scroll stays within list bounds
    #[test]
    fn prop_chat_message_scroll_in_bounds(
        count in 1usize..50,
        start in 0usize..50,
        events in prop::collection::vec(nav_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('2');
        tui.state_mut().chat.message_count = count;
        tui.state_mut().chat.message_scroll = start % count;
        tui.state_mut().chat.focus = ChatFocus::Messages;

        for event in events {
            tui.send(event);
        }

        prop_assert!(tui.state().chat.message_scroll < count);
    }

    /// Property: Modal queue state is internally consistent
    #[test]
    fn prop_modal_queue_consistent(
        events in prop::collection::vec(terminal_event_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();

        for event in events {
            tui.send(event);
        }

        let modal = tui.state().modal_queue.current();
        let modal_type = tui.state().current_modal_type();
        let is_global_modal = matches!(
            modal,
            Some(QueuedModal::AccountSetup(_))
                | Some(QueuedModal::Help { .. })
                | Some(QueuedModal::GuardianSelect(_))
                | Some(QueuedModal::ContactSelect(_))
                | Some(QueuedModal::Confirm { .. })
        );

        prop_assert_eq!(tui.has_modal(), modal.is_some());
        prop_assert_eq!(modal_type != ModalType::None, is_global_modal);

        if let Some(QueuedModal::ContactSelect(state))
        | Some(QueuedModal::GuardianSelect(state)) = modal
        {
            if state.is_empty() {
                prop_assert_eq!(state.selected_index, 0);
            } else {
                prop_assert!(state.selected_index < state.contact_count());
            }
        }

        if let Some(QueuedModal::ChatMemberSelect(state)) = modal {
            if state.picker.is_empty() {
                prop_assert_eq!(state.picker.selected_index, 0);
            } else {
                prop_assert!(state.picker.selected_index < state.picker.contact_count());
            }
        }
    }

    /// Property: Navigation reversibility for list navigation
    #[test]
    fn prop_contacts_nav_reversible(
        count in 1usize..50,
        dirs in prop::collection::vec(nav_dir_strategy(), 0..50)
    ) {
        let mut tui = TestTui::new();
        tui.send_char('3');
        tui.state_mut().contacts.contact_count = count;
        tui.state_mut().contacts.focus = TwoPanelFocus::List;
        tui.state_mut().contacts.selected_index = count / 2;

        let start = tui.state().contacts.selected_index;
        for dir in &dirs {
            tui.send(nav_dir_event(*dir));
        }
        for dir in dirs.iter().rev() {
            tui.send(nav_dir_event(nav_dir_inverse(*dir)));
        }

        prop_assert_eq!(tui.state().contacts.selected_index, start);
    }

    /// Property: Chat create modal validity stays true under non-editing events
    #[test]
    fn prop_chat_create_validity_stable(
        name in "[a-zA-Z0-9][a-zA-Z0-9 _-]{0,23}",
        events in prop::collection::vec(modal_safe_event_strategy(), 0..30)
    ) {
        let mut state = aura_terminal::tui::state_machine::TuiState::new();
        let mut modal_state = aura_terminal::tui::state_machine::CreateChannelModalState::new();
        modal_state.name = name.clone();
        state.modal_queue.enqueue(QueuedModal::ChatCreate(modal_state));
        let mut tui = TestTui::with_state(state);

        for event in events {
            tui.send(event);
        }

        let modal_state = tui
            .state()
            .chat_create_modal_state()
            .expect("chat create modal should remain active");
        prop_assert!(modal_state.can_submit());
        prop_assert_eq!(modal_state.name.as_str(), name.as_str());
    }

    /// Property: Tick is a no-op when no toasts are active
    #[test]
    fn prop_tick_noop_without_toasts(ticks in 0usize..20) {
        let mut tui = TestTui::new();
        prop_assume!(!tui.state().toast_queue.is_active());

        let before = format!("{:?}", tui.state());
        for _ in 0..ticks {
            tui.send(events::tick());
        }
        let after = format!("{:?}", tui.state());

        prop_assert_eq!(before, after);
    }
}

#[test]
fn test_chat_create_select_members_dispatches_create_channel_with_members() {
    let mut tui = TestTui::new();

    // Go to Chat screen
    tui.send_char('2');
    tui.assert_screen(Screen::Chat);

    // Open create chat wizard (dispatch) then inject modal (shell would enqueue)
    tui.send_char('n');
    assert!(tui.has_dispatch(|cmd| matches!(cmd, DispatchCommand::OpenChatCreateWizard)));
    tui.state_mut()
        .modal_queue
        .enqueue(QueuedModal::ChatCreate(CreateChannelModalState::new()));

    // Fill in name + topic
    tui.type_str("group");
    tui.send_tab();
    tui.type_str("topic");

    // Replace active modal with ChatMemberSelect (shell normally populates contacts)
    let draft = match tui.current_modal() {
        Some(QueuedModal::ChatCreate(s)) => s.clone(),
        other => panic!("Expected ChatCreate modal, got {other:?}"),
    };

    let contacts = vec![
        ("alice".to_string(), "Alice".to_string()),
        ("carol".to_string(), "Carol".to_string()),
    ];
    let picker = ContactSelectModalState::multi("Select chat members", contacts);

    tui.state_mut().modal_queue.update_active(|modal| {
        *modal = QueuedModal::ChatMemberSelect(ChatMemberSelectModalState { picker, draft });
    });

    // Select both contacts
    tui.send_char(' ');
    tui.send_down();
    tui.send_char(' ');

    // Confirm selection; should return to ChatCreate with member_ids populated
    tui.send_enter();
    match tui.current_modal() {
        Some(QueuedModal::ChatCreate(s)) => {
            assert_eq!(s.member_ids.len(), 2);
            assert!(s.member_ids.contains(&"alice".to_string()));
            assert!(s.member_ids.contains(&"carol".to_string()));
        }
        other => panic!("Expected ChatCreate modal, got {other:?}"),
    }

    // Jump to threshold step and seed selected members to simulate wizard progression.
    // Channel is now created directly from Threshold step (Review step removed).
    tui.state_mut().modal_queue.update_active(|modal| {
        if let QueuedModal::ChatCreate(ref mut s) = modal {
            s.step = CreateChannelStep::Threshold;
            s.contacts = vec![
                ChatMemberCandidate {
                    id: "alice".to_string(),
                    name: "Alice".to_string(),
                },
                ChatMemberCandidate {
                    id: "carol".to_string(),
                    name: "Carol".to_string(),
                },
            ];
            s.selected_indices = vec![0, 1];
        }
    });

    tui.clear_commands();
    tui.send_enter();

    assert!(tui.has_dispatch(|d| {
        matches!(
            d,
            DispatchCommand::CreateChannel { name, topic, members, .. }
                if name == "group"
                    && topic.as_deref() == Some("topic")
                    && members.len() == 2
                    && members.contains(&"alice".to_string())
                    && members.contains(&"carol".to_string())
        )
    }));
}
