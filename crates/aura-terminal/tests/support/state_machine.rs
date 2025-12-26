//! Pure state machine test helpers.
//!
//! This module provides [`TestTui`], a wrapper around [`TuiState`] for
//! deterministic, fast testing of TUI behavior without PTY automation.
//!
//! # Benefits
//!
//! - **Fast**: No PTY setup, no sleeps, pure computation (<1ms per test)
//! - **Deterministic**: Same inputs always produce same outputs
//! - **Debuggable**: Full state visibility at every step
//!
//! # Example
//!
//! ```ignore
//! use crate::support::state_machine::TestTui;
//! use aura_terminal::tui::screens::Screen;
//!
//! #[test]
//! fn test_navigation() {
//!     let mut tui = TestTui::new();
//!     tui.assert_screen(Screen::Neighborhood);
//!
//!     tui.send_char('2');
//!     tui.assert_screen(Screen::Neighborhood);
//! }
//! ```

use aura_core::effects::terminal::{events, TerminalEvent};
use aura_terminal::tui::screens::Screen;
use aura_terminal::tui::state_machine::{
    transition, DispatchCommand, QueuedModal, TuiCommand, TuiState,
};

/// Test wrapper for the TUI state machine.
///
/// Provides convenient methods for sending events and asserting state,
/// while collecting all emitted commands for verification.
pub struct TestTui {
    state: TuiState,
    commands: Vec<TuiCommand>,
}

impl TestTui {
    /// Create a new TestTui with default initial state.
    pub fn new() -> Self {
        Self {
            state: TuiState::new(),
            commands: Vec::new(),
        }
    }

    /// Create a TestTui with custom initial state.
    pub fn with_state(state: TuiState) -> Self {
        Self {
            state,
            commands: Vec::new(),
        }
    }

    // ========================================================================
    // Event Sending
    // ========================================================================

    /// Send a terminal event and update state.
    pub fn send(&mut self, event: TerminalEvent) {
        let (new_state, cmds) = transition(&self.state, event);
        self.state = new_state;
        self.commands.extend(cmds);
    }

    /// Send a character key event.
    pub fn send_char(&mut self, c: char) {
        self.send(events::char(c));
    }

    /// Send Tab key event.
    pub fn send_tab(&mut self) {
        self.send(events::tab());
    }

    /// Send Enter key event.
    pub fn send_enter(&mut self) {
        self.send(events::enter());
    }

    /// Send Escape key event.
    pub fn send_escape(&mut self) {
        self.send(events::escape());
    }

    /// Send Backspace key event.
    pub fn send_backspace(&mut self) {
        self.send(events::backspace());
    }

    /// Send Up arrow key event.
    pub fn send_up(&mut self) {
        self.send(events::arrow_up());
    }

    /// Send Down arrow key event.
    pub fn send_down(&mut self) {
        self.send(events::arrow_down());
    }

    /// Send Left arrow key event.
    pub fn send_left(&mut self) {
        self.send(events::arrow_left());
    }

    /// Send Right arrow key event.
    pub fn send_right(&mut self) {
        self.send(events::arrow_right());
    }

    /// Type a string by sending individual character events.
    pub fn type_str(&mut self, s: &str) {
        for c in s.chars() {
            self.send_char(c);
        }
    }

    // ========================================================================
    // State Access
    // ========================================================================

    /// Get the current screen.
    pub fn screen(&self) -> Screen {
        self.state.screen()
    }

    /// Get a reference to the full state.
    pub fn state(&self) -> &TuiState {
        &self.state
    }

    /// Get a mutable reference to the full state for test setup.
    pub fn state_mut(&mut self) -> &mut TuiState {
        &mut self.state
    }

    /// Check if in insert mode.
    pub fn is_insert_mode(&self) -> bool {
        self.state.is_insert_mode()
    }

    /// Check if there's an active modal.
    pub fn has_modal(&self) -> bool {
        self.state.has_modal()
    }

    /// Get the current modal, if any.
    pub fn current_modal(&self) -> Option<&QueuedModal> {
        self.state.modal_queue.current()
    }

    // ========================================================================
    // Command Access
    // ========================================================================

    /// Get all collected commands.
    pub fn commands(&self) -> &[TuiCommand] {
        &self.commands
    }

    /// Get only dispatch commands.
    pub fn dispatch_commands(&self) -> Vec<&DispatchCommand> {
        self.commands
            .iter()
            .filter_map(|c| match c {
                TuiCommand::Dispatch(d) => Some(d),
                _ => None,
            })
            .collect()
    }

    /// Check if any dispatch command matches a predicate.
    pub fn has_dispatch(&self, check: impl Fn(&DispatchCommand) -> bool) -> bool {
        self.commands
            .iter()
            .any(|c| matches!(c, TuiCommand::Dispatch(d) if check(d)))
    }

    /// Clear collected commands.
    pub fn clear_commands(&mut self) {
        self.commands.clear();
    }

    /// Take and clear collected commands.
    pub fn take_commands(&mut self) -> Vec<TuiCommand> {
        std::mem::take(&mut self.commands)
    }

    // ========================================================================
    // Assertions
    // ========================================================================

    /// Assert the current screen matches expected.
    pub fn assert_screen(&self, expected: Screen) {
        assert_eq!(
            self.screen(),
            expected,
            "Expected screen {:?}, got {:?}",
            expected,
            self.screen()
        );
    }

    /// Assert currently in insert mode.
    pub fn assert_insert_mode(&self) {
        assert!(
            self.is_insert_mode(),
            "Expected insert mode, but not in insert mode"
        );
    }

    /// Assert currently in normal mode.
    pub fn assert_normal_mode(&self) {
        assert!(
            !self.is_insert_mode(),
            "Expected normal mode, but in insert mode"
        );
    }

    /// Assert a modal is currently shown.
    pub fn assert_has_modal(&self) {
        assert!(
            self.has_modal(),
            "Expected modal to be shown, but no modal present"
        );
    }

    /// Assert no modal is shown.
    pub fn assert_no_modal(&self) {
        assert!(!self.has_modal(), "Expected no modal, but modal is present");
    }
}

impl Default for TestTui {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Proptest Strategies
// ============================================================================

#[cfg(feature = "proptest")]
pub mod strategies {
    use super::*;
    use proptest::prelude::*;

    /// Generate arbitrary terminal events for property testing.
    pub fn any_event() -> impl Strategy<Value = TerminalEvent> {
        prop_oneof![
            // Character keys (alphanumeric + common symbols)
            any::<char>()
                .prop_filter("printable", |c| c.is_ascii_graphic() || *c == ' ')
                .prop_map(events::char),
            // Navigation keys
            Just(events::tab()),
            Just(events::enter()),
            Just(events::escape()),
            Just(events::backspace()),
            Just(events::up()),
            Just(events::down()),
            Just(events::left()),
            Just(events::right()),
        ]
    }

    /// Generate a sequence of events.
    pub fn event_sequence(max_len: usize) -> impl Strategy<Value = Vec<TerminalEvent>> {
        prop::collection::vec(any_event(), 0..max_len)
    }
}
