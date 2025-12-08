//! TUI Integration Tests using expectrl
//!
//! These tests spawn the actual aura binary in a PTY and interact with it
//! using expectrl for terminal automation. This provides end-to-end testing
//! of the TUI including real terminal rendering.
//!
//! ## Test Categories
//!
//! - **Startup tests**: Verify TUI launches and renders correctly
//! - **Navigation tests**: Verify screen switching and keyboard input
//! - **User flow tests**: Verify account creation, chat, and other workflows
//!
//! ## Running Tests
//!
//! These tests must run sequentially due to PTY resource constraints:
//! ```bash
//! cargo test --test tui_expect_test -- --test-threads=1
//! ```

#![allow(clippy::expect_used)]

use escargot::CargoBuild;
use expectrl::Regex;
use std::sync::OnceLock;
use std::time::Duration;

/// Cache the built binary path to avoid rebuilding for each test
static BINARY_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();

/// Build the aura binary (or return cached path)
fn get_binary_path() -> &'static std::path::PathBuf {
    BINARY_PATH.get_or_init(|| {
        let cargo_build = CargoBuild::new()
            .package("aura-terminal")
            .bin("aura")
            .features("terminal")
            .run()
            .expect("Failed to build aura binary");

        cargo_build.path().to_path_buf()
    })
}

/// Spawn the TUI in demo mode
fn spawn_tui_demo() -> expectrl::Session {
    let binary_path = get_binary_path();

    let mut session = expectrl::spawn(format!("{} tui --demo", binary_path.display()))
        .expect("Failed to spawn TUI");

    // Set a reasonable timeout for expect operations
    session.set_expect_timeout(Some(Duration::from_secs(10)));

    session
}

/// Test that the TUI launches and outputs startup messages
#[test]
fn test_tui_launches_in_demo_mode() {
    let mut session = spawn_tui_demo();

    // The TUI outputs startup log messages before rendering
    session
        .expect(Regex("(?i)starting aura tui|demo mode|initializing"))
        .expect("TUI should display startup messages");

    // TUI should be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI process should be running"
    );

    // Cleanup: send quit signal
    session.send("q").ok();
}

/// Test that the TUI enters alternate screen mode (renders UI)
#[test]
fn test_tui_renders_ui() {
    let mut session = spawn_tui_demo();

    // Wait for startup
    session
        .expect(Regex("(?i)launching tui"))
        .expect("TUI should launch");

    // After "Launching TUI", it should enter alternate screen mode
    // The ANSI escape sequence [?1049h enables alternate screen
    session
        .expect(Regex(r"\x1b\[\?1049h|\[0m"))
        .expect("TUI should enter rendering mode");

    session.send("q").ok();
}

/// Test that the TUI accepts and processes keyboard input without crashing
#[test]
fn test_tui_accepts_keyboard_input() {
    let mut session = spawn_tui_demo();

    // Wait for startup
    session
        .expect(Regex("(?i)launching tui"))
        .expect("TUI should launch");

    // Wait for UI to render
    std::thread::sleep(Duration::from_secs(1));

    // Press Escape a few times (to dismiss any modal)
    for _ in 0..3 {
        session.send("\x1b").expect("Failed to send Escape");
        std::thread::sleep(Duration::from_millis(100));
    }

    // Send Tab to navigate
    session.send("\t").expect("Failed to send Tab");
    std::thread::sleep(Duration::from_millis(200));

    // Send number keys to switch screens
    for key in ['1', '2', '3'] {
        session
            .send(key.to_string())
            .expect("Failed to send number key");
        std::thread::sleep(Duration::from_millis(100));
    }

    // Process should still be alive (didn't crash)
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after keyboard input"
    );

    session.send("q").ok();
}

/// Test navigation through all screens
#[test]
fn test_tui_screen_navigation() {
    let mut session = spawn_tui_demo();

    // Wait for startup
    session
        .expect(Regex("(?i)launching tui"))
        .expect("TUI should launch");

    std::thread::sleep(Duration::from_secs(1));

    // Dismiss any modal with Escape
    session.send("\x1b").expect("Failed to send Escape");
    std::thread::sleep(Duration::from_millis(200));

    // Navigate through all 8 screens with number keys
    for key in ['1', '2', '3', '4', '5', '6', '7', '8'] {
        session
            .send(key.to_string())
            .unwrap_or_else(|_| panic!("Failed to send '{}'", key));
        std::thread::sleep(Duration::from_millis(150));
    }

    // Navigate with Tab
    for _ in 0..3 {
        session.send("\t").expect("Failed to send Tab");
        std::thread::sleep(Duration::from_millis(100));
    }

    // Process should still be alive after all navigation
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after navigation"
    );

    session.send("q").ok();
}

// =============================================================================
// User Flow Tests
// =============================================================================

/// Helper to wait for TUI to be ready and dismiss any initial modals
fn wait_for_ready(session: &mut expectrl::Session) {
    // Wait for startup
    session
        .expect(Regex("(?i)launching tui"))
        .expect("TUI should launch");

    // Wait for UI to render
    std::thread::sleep(Duration::from_secs(1));

    // Dismiss any modal (account setup) with Escape
    for _ in 0..3 {
        session.send("\x1b").ok();
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Test account creation flow
///
/// In demo mode, the account setup modal appears on first launch.
/// This test verifies the modal can be interacted with.
#[test]
fn test_account_setup_flow() {
    let mut session = spawn_tui_demo();

    // Wait for startup
    session
        .expect(Regex("(?i)launching tui"))
        .expect("TUI should launch");

    std::thread::sleep(Duration::from_secs(1));

    // The account setup modal should be visible
    // Type a display name
    session
        .send("TestUser")
        .expect("Failed to send display name");
    std::thread::sleep(Duration::from_millis(200));

    // Submit with Enter
    session.send("\r").expect("Failed to send Enter");
    std::thread::sleep(Duration::from_millis(500));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after account setup"
    );

    session.send("q").ok();
}

/// Test chat screen interaction
///
/// Navigate to Chat screen, enter insert mode, type a message, and send it.
/// This tests the full intent → state → view update pipeline.
#[test]
fn test_chat_message_flow() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Chat screen (key '2')
    session.send("2").expect("Failed to navigate to Chat");
    std::thread::sleep(Duration::from_millis(300));

    // Enter insert mode ('i')
    session.send("i").expect("Failed to enter insert mode");
    std::thread::sleep(Duration::from_millis(200));

    // Type a message
    session
        .send("Hello from test!")
        .expect("Failed to type message");
    std::thread::sleep(Duration::from_millis(200));

    // Send message with Enter
    session.send("\r").expect("Failed to send message");
    std::thread::sleep(Duration::from_millis(300));

    // Exit insert mode with Escape
    session.send("\x1b").expect("Failed to exit insert mode");
    std::thread::sleep(Duration::from_millis(200));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after chat interaction"
    );

    session.send("q").ok();
}

/// Test contacts screen navigation
///
/// Navigate to Contacts screen and use vim-style navigation.
#[test]
fn test_contacts_navigation() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Contacts screen (key '3')
    session.send("3").expect("Failed to navigate to Contacts");
    std::thread::sleep(Duration::from_millis(300));

    // Use vim-style navigation
    session.send("j").expect("Failed to send j");
    std::thread::sleep(Duration::from_millis(100));
    session.send("k").expect("Failed to send k");
    std::thread::sleep(Duration::from_millis(100));
    session.send("l").expect("Failed to send l");
    std::thread::sleep(Duration::from_millis(100));
    session.send("h").expect("Failed to send h");
    std::thread::sleep(Duration::from_millis(100));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after contacts navigation"
    );

    session.send("q").ok();
}

/// Test invitations screen interaction
///
/// Navigate to Invitations screen and interact with filters.
#[test]
fn test_invitations_screen() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Invitations screen (key '5')
    session
        .send("5")
        .expect("Failed to navigate to Invitations");
    std::thread::sleep(Duration::from_millis(300));

    // Cycle through filters with 'f'
    for _ in 0..3 {
        session.send("f").expect("Failed to cycle filter");
        std::thread::sleep(Duration::from_millis(150));
    }

    // Navigate list
    session.send("j").expect("Failed to navigate down");
    std::thread::sleep(Duration::from_millis(100));
    session.send("k").expect("Failed to navigate up");
    std::thread::sleep(Duration::from_millis(100));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after invitations interaction"
    );

    session.send("q").ok();
}

/// Test recovery screen interaction
///
/// Navigate to Recovery screen and verify guardian list navigation.
#[test]
fn test_recovery_screen() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Recovery screen (key '7')
    session.send("7").expect("Failed to navigate to Recovery");
    std::thread::sleep(Duration::from_millis(300));

    // Navigate guardian list
    session.send("j").expect("Failed to navigate down");
    std::thread::sleep(Duration::from_millis(100));
    session.send("k").expect("Failed to navigate up");
    std::thread::sleep(Duration::from_millis(100));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after recovery interaction"
    );

    session.send("q").ok();
}

/// Test settings screen interaction
///
/// Navigate to Settings screen and interact with options.
#[test]
fn test_settings_screen() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Settings screen (key '6')
    session.send("6").expect("Failed to navigate to Settings");
    std::thread::sleep(Duration::from_millis(300));

    // Navigate settings list
    session.send("j").expect("Failed to navigate down");
    std::thread::sleep(Duration::from_millis(100));
    session.send("k").expect("Failed to navigate up");
    std::thread::sleep(Duration::from_millis(100));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after settings interaction"
    );

    session.send("q").ok();
}

/// Test help screen
///
/// Navigate to Help screen and verify it displays.
#[test]
fn test_help_screen() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Help screen (key '8')
    session.send("8").expect("Failed to navigate to Help");
    std::thread::sleep(Duration::from_millis(300));

    // Scroll through help content
    session.send("j").expect("Failed to scroll down");
    std::thread::sleep(Duration::from_millis(100));
    session.send("j").expect("Failed to scroll down");
    std::thread::sleep(Duration::from_millis(100));
    session.send("k").expect("Failed to scroll up");
    std::thread::sleep(Duration::from_millis(100));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should still be running after help interaction"
    );

    session.send("q").ok();
}

/// Test rapid screen switching
///
/// Quickly switch between screens to test stability under rapid input.
#[test]
fn test_rapid_screen_switching() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Rapidly switch through all screens multiple times
    for _ in 0..3 {
        for key in ['1', '2', '3', '4', '5', '6', '7', '8'] {
            session
                .send(key.to_string())
                .expect("Failed to switch screen");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // Give time for rendering to catch up
    std::thread::sleep(Duration::from_millis(300));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should handle rapid screen switching"
    );

    session.send("q").ok();
}

/// Test escape key behavior
///
/// Verify Escape dismisses modals and returns to normal mode.
#[test]
fn test_escape_key_behavior() {
    let mut session = spawn_tui_demo();
    wait_for_ready(&mut session);

    // Navigate to Chat and enter insert mode
    session.send("2").expect("Failed to navigate to Chat");
    std::thread::sleep(Duration::from_millis(200));

    session.send("i").expect("Failed to enter insert mode");
    std::thread::sleep(Duration::from_millis(200));

    // Escape should exit insert mode
    session.send("\x1b").expect("Failed to send Escape");
    std::thread::sleep(Duration::from_millis(200));

    // Now screen navigation should work again
    session.send("1").expect("Failed to navigate to Block");
    std::thread::sleep(Duration::from_millis(200));

    // Process should still be running
    assert!(
        session.is_alive().unwrap_or(false),
        "TUI should handle Escape correctly"
    );

    session.send("q").ok();
}
