//! # Terminal Test Utilities
//!
//! Utilities for deterministic TUI and CLI testing without PTY dependencies.
//!
//! ## Overview
//!
//! This module provides:
//! - `TestTui`: A wrapper around `TuiRuntime` with test-friendly methods
//! - `CliTestHarness`: A wrapper around `CliHandler` with output capture
//! - Event builders for common key sequences
//! - State assertion helpers
//! - Frame capture and assertion utilities
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_terminal::testing::{TestTui, events};
//!
//! #[test]
//! fn test_navigation() {
//!     let mut tui = TestTui::new();
//!
//!     // Navigate to Chat screen
//!     tui.send_events(vec![events::char('2')]);
//!     tui.assert_screen(Screen::Chat);
//!
//!     // Type a message
//!     tui.send_events(vec![events::char('i')]); // Enter insert mode
//!     tui.type_text("Hello, world!");
//!     tui.send_events(vec![events::enter()]);
//!
//!     // Verify the state
//!     assert!(!tui.state().chat.insert_mode);
//! }
//! ```
//!
//! ## Why Deterministic Tests?
//!
//! PTY-based tests are:
//! - **Flaky**: Timing-dependent, fail randomly
//! - **Slow**: Require real terminal setup
//! - **Hard to debug**: Output is interleaved with test output
//!
//! State machine tests are:
//! - **Deterministic**: Same inputs = same outputs, every time
//! - **Fast**: No I/O, pure computation
//! - **Easy to debug**: Full state visibility

pub mod cli;
pub mod itf_replay;

use crate::tui::runtime::TuiRuntime;
use crate::tui::screens::Screen;
use crate::tui::state_machine::{TuiCommand, TuiState};
use aura_core::effects::terminal::{
    events, CursorShape, TerminalError, TerminalEvent, TerminalFrame,
};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Re-export events module for convenience
pub use aura_core::effects::terminal::events as event_builders;

/// Test-focused TUI wrapper with convenient assertion methods.
pub struct TestTui {
    state: TuiState,
    /// Captured commands for assertion
    commands: Vec<TuiCommand>,
}

impl Default for TestTui {
    fn default() -> Self {
        Self::new()
    }
}

impl TestTui {
    /// Create a new test TUI with default state.
    pub fn new() -> Self {
        Self {
            state: TuiState::new(),
            commands: Vec::new(),
        }
    }

    /// Create a test TUI with custom initial state.
    pub fn with_state(state: TuiState) -> Self {
        Self {
            state,
            commands: Vec::new(),
        }
    }

    /// Get current state.
    pub fn state(&self) -> &TuiState {
        &self.state
    }

    /// Get mutable state (for setup).
    pub fn state_mut(&mut self) -> &mut TuiState {
        &mut self.state
    }

    /// Get all captured commands.
    pub fn commands(&self) -> &[TuiCommand] {
        &self.commands
    }

    /// Clear captured commands.
    pub fn clear_commands(&mut self) {
        self.commands.clear();
    }

    /// Send a single event and return generated commands.
    pub fn send_event(&mut self, event: TerminalEvent) -> Vec<TuiCommand> {
        let (new_state, commands) = crate::tui::state_machine::transition(&self.state, event);
        self.state = new_state;
        self.commands.extend(commands.clone());
        commands
    }

    /// Send multiple events in sequence.
    pub fn send_events(&mut self, events: Vec<TerminalEvent>) {
        for event in events {
            self.send_event(event);
        }
    }

    /// Type a string (sends each character as a key event).
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.send_event(events::char(c));
        }
    }

    /// Assert current screen.
    pub fn assert_screen(&self, expected: Screen) {
        assert_eq!(
            self.state.screen(),
            expected,
            "Expected screen {:?}, got {:?}",
            expected,
            self.state.screen()
        );
    }

    /// Assert insert mode is active.
    pub fn assert_insert_mode(&self) {
        assert!(
            self.state.is_insert_mode(),
            "Expected insert mode to be active"
        );
    }

    /// Assert insert mode is inactive.
    pub fn assert_normal_mode(&self) {
        assert!(
            !self.state.is_insert_mode(),
            "Expected normal mode (insert mode inactive)"
        );
    }

    /// Assert modal is shown.
    pub fn assert_has_modal(&self) {
        assert!(self.state.has_modal(), "Expected a modal to be shown");
    }

    /// Assert no modal is shown.
    pub fn assert_no_modal(&self) {
        assert!(!self.state.has_modal(), "Expected no modal to be shown");
    }

    /// Assert an Exit command was generated.
    pub fn assert_exit_requested(&self) {
        assert!(
            self.commands.iter().any(|c| matches!(c, TuiCommand::Exit)),
            "Expected Exit command to be generated"
        );
    }

    /// Assert a specific toast message was shown.
    pub fn assert_toast_shown(&self, message_contains: &str) {
        let has_toast = self.commands.iter().any(|c| {
            matches!(c, TuiCommand::ShowToast { message, .. } if message.contains(message_contains))
        });
        assert!(
            has_toast,
            "Expected toast containing '{}' to be shown",
            message_contains
        );
    }

    /// Check if a specific dispatch command was generated.
    pub fn has_dispatch(
        &self,
        check: impl Fn(&crate::tui::state_machine::DispatchCommand) -> bool,
    ) -> bool {
        self.commands
            .iter()
            .any(|c| matches!(c, TuiCommand::Dispatch(cmd) if check(cmd)))
    }
}

/// Simple mock terminal for testing TuiRuntime directly.
pub struct TestTerminal {
    events: Mutex<VecDeque<TerminalEvent>>,
    frames: Mutex<Vec<TerminalFrame>>,
    size: (u16, u16),
}

impl TestTerminal {
    /// Create a new test terminal with predetermined events.
    pub fn new(events: Vec<TerminalEvent>) -> Self {
        Self {
            events: Mutex::new(events.into()),
            frames: Mutex::new(Vec::new()),
            size: (80, 24),
        }
    }

    /// Create a test terminal with custom size.
    pub fn with_size(events: Vec<TerminalEvent>, width: u16, height: u16) -> Self {
        Self {
            events: Mutex::new(events.into()),
            frames: Mutex::new(Vec::new()),
            size: (width, height),
        }
    }

    /// Add more events to the queue.
    pub fn add_events(&self, new_events: Vec<TerminalEvent>) {
        let mut events = self.events.lock().unwrap();
        events.extend(new_events);
    }

    /// Get captured frames.
    pub fn frames(&self) -> Vec<TerminalFrame> {
        self.frames.lock().unwrap().clone()
    }

    /// Get the last captured frame.
    pub fn last_frame(&self) -> Option<TerminalFrame> {
        self.frames.lock().unwrap().last().cloned()
    }
}

#[async_trait::async_trait]
impl aura_core::effects::terminal::TerminalInputEffects for TestTerminal {
    async fn next_event(&self) -> Result<TerminalEvent, TerminalError> {
        let mut events = self.events.lock().unwrap();
        events.pop_front().ok_or(TerminalError::EndOfInput)
    }

    async fn poll_event(&self, _timeout_ms: u64) -> Result<Option<TerminalEvent>, TerminalError> {
        let mut events = self.events.lock().unwrap();
        Ok(events.pop_front())
    }

    async fn has_input(&self) -> bool {
        let events = self.events.lock().unwrap();
        !events.is_empty()
    }
}

#[async_trait::async_trait]
impl aura_core::effects::terminal::TerminalOutputEffects for TestTerminal {
    async fn render(&self, frame: TerminalFrame) -> Result<(), TerminalError> {
        let mut frames = self.frames.lock().unwrap();
        frames.push(frame);
        Ok(())
    }

    async fn clear(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn size(&self) -> Result<(u16, u16), TerminalError> {
        Ok(self.size)
    }

    async fn set_cursor(&self, _col: u16, _row: u16) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn set_cursor_visible(&self, _visible: bool) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn set_cursor_shape(&self, _shape: CursorShape) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn enter_alternate_screen(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn leave_alternate_screen(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn enable_raw_mode(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn disable_raw_mode(&self) -> Result<(), TerminalError> {
        Ok(())
    }
}

/// Create a TuiRuntime with test events for async testing.
pub fn create_test_runtime(events: Vec<TerminalEvent>) -> TuiRuntime<TestTerminal> {
    TuiRuntime::new(TestTerminal::new(events))
}

/// Macro for building event sequences more ergonomically.
///
/// # Example
///
/// ```rust,ignore
/// use aura_terminal::testing::events;
///
/// let evts = events![
///     char '2',      // Navigate to Chat
///     char 'i',      // Insert mode
///     text "hello",  // Type text
///     enter,         // Submit
///     esc,           // Exit insert mode
///     char 'q',      // Quit
/// ];
/// ```
#[macro_export]
macro_rules! test_events {
    ($($event:tt)*) => {{
        #[allow(clippy::vec_init_then_push)]
        {
            let mut events = Vec::new();
            $crate::test_events_inner!(events, $($event)*);
            events
        }
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! test_events_inner {
    ($events:ident,) => {};

    ($events:ident, char $c:literal $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::char($c));
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, text $s:literal $(, $($rest:tt)*)?) => {
        for c in $s.chars() {
            $events.push($crate::testing::event_builders::char(c));
        }
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, enter $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::enter());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, esc $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::escape());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, tab $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::tab());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, up $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::up());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, down $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::down());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, left $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::left());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };

    ($events:ident, right $(, $($rest:tt)*)?) => {
        $events.push($crate::testing::event_builders::right());
        $($crate::test_events_inner!($events, $($rest)*);)?
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_navigation() {
        let mut tui = TestTui::new();

        // Initial state
        tui.assert_screen(Screen::Neighborhood);
        tui.assert_normal_mode();
        tui.assert_no_modal();

        // Navigate to Neighborhood
        tui.send_event(events::char('2'));
        tui.assert_screen(Screen::Chat);

        // Navigate to Chat
        tui.send_event(events::char('3'));
        tui.assert_screen(Screen::Contacts);

        // Navigate to Contacts
        tui.send_event(events::char('4'));
        tui.assert_screen(Screen::Notifications);

        // Use Tab to cycle
        tui.send_event(events::tab());
        tui.assert_screen(Screen::Settings);
    }

    #[test]
    fn test_tui_insert_mode() {
        let mut tui = TestTui::new();

        // Start in Neighborhood screen
        tui.assert_normal_mode();

        // Enter home detail mode, then insert mode
        tui.send_event(events::enter());
        tui.send_event(events::char('i'));
        tui.assert_insert_mode();

        // Type some text
        tui.type_text("Hello");

        // Exit with Escape
        tui.send_event(events::escape());
        tui.assert_normal_mode();
    }

    #[test]
    fn test_tui_quit() {
        let mut tui = TestTui::new();

        // Press q to quit
        tui.send_event(events::char('q'));
        tui.assert_exit_requested();
    }

    #[test]
    fn test_tui_help_modal() {
        let mut tui = TestTui::new();

        tui.assert_no_modal();

        // Press ? for help
        tui.send_event(events::char('?'));
        tui.assert_has_modal();

        // Dismiss with Escape
        tui.send_event(events::escape());
        tui.assert_no_modal();
    }

    #[tokio::test]
    async fn test_runtime_with_test_terminal() {
        let terminal = TestTerminal::new(vec![
            events::char('2'), // Navigate to Chat
            events::char('q'), // Quit
        ]);

        let mut runtime = TuiRuntime::new(terminal);
        runtime.run().await.unwrap();

        assert_eq!(runtime.state().screen(), Screen::Chat);
        assert!(runtime.state().should_exit);
    }

    #[test]
    fn test_events_macro() {
        let events = test_events![
            char '2',
            tab,
            enter,
            esc,
        ];
        assert_eq!(events.len(), 4);
    }
}
