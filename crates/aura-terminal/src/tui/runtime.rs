//! # TUI Runtime
//!
//! Runtime that drives the TUI state machine with terminal effects.
//!
//! This module provides:
//! - `TuiRuntime<T>`: Generic runtime over terminal effects
//! - Command execution infrastructure
//! - Event loop for testing and headless operation
//!
//! ## Architecture
//!
//! The TuiRuntime separates concerns:
//! - **State**: Managed by `TuiState` (pure)
//! - **Transitions**: Computed by `transition()` (pure)
//! - **Effects**: Executed by `TuiRuntime` (impure)
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_terminal::tui::runtime::TuiRuntime;
//! use aura_testkit::stateful_effects::MockTerminalHandler;
//! use aura_core::effects::terminal::events;
//!
//! // Create runtime with mock terminal
//! let terminal = MockTerminalHandler::with_events(vec![
//!     events::char('2'),  // Navigate to Chat
//!     events::char('q'),  // Quit
//! ]);
//! let mut runtime = TuiRuntime::new(terminal);
//!
//! // Run until exit
//! runtime.run().await?;
//!
//! // Assert on captured frames
//! assert!(runtime.terminal().frame_contains("Chat"));
//! ```

use crate::tui::state_machine::{transition, TuiCommand, TuiState};
use aura_core::effects::terminal::{TerminalEffects, TerminalError, TerminalEvent, TerminalFrame};
use std::sync::Arc;

/// Callback for handling dispatch commands from the state machine.
///
/// The runtime doesn't know about app-level commands (like SendMessage),
/// so it delegates to this callback.
pub type DispatchCallback = Arc<dyn Fn(&crate::tui::state_machine::DispatchCommand) + Send + Sync>;

/// Runtime that drives the TUI state machine.
///
/// Generic over `T: TerminalEffects` to support both real terminals
/// and mock handlers for testing.
pub struct TuiRuntime<T: TerminalEffects> {
    /// Terminal effects handler
    terminal: Arc<T>,
    /// Current TUI state
    state: TuiState,
    /// Captured frames for testing
    frames: Vec<TerminalFrame>,
    /// Callback for dispatch commands
    dispatch_callback: Option<DispatchCallback>,
    /// Maximum iterations (None = unlimited)
    max_iterations: Option<usize>,
}

impl<T: TerminalEffects> TuiRuntime<T> {
    /// Create a new runtime with the given terminal handler.
    pub fn new(terminal: T) -> Self {
        Self {
            terminal: Arc::new(terminal),
            state: TuiState::new(),
            frames: Vec::new(),
            dispatch_callback: None,
            max_iterations: None,
        }
    }

    /// Create runtime with initial state.
    pub fn with_state(terminal: T, state: TuiState) -> Self {
        Self {
            terminal: Arc::new(terminal),
            state,
            frames: Vec::new(),
            dispatch_callback: None,
            max_iterations: None,
        }
    }

    /// Set callback for dispatch commands.
    pub fn with_dispatch_callback(mut self, callback: DispatchCallback) -> Self {
        self.dispatch_callback = Some(callback);
        self
    }

    /// Set maximum iterations (for testing to prevent infinite loops).
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Get reference to the terminal handler.
    pub fn terminal(&self) -> &T {
        &self.terminal
    }

    /// Get current state.
    pub fn state(&self) -> &TuiState {
        &self.state
    }

    /// Get mutable reference to state (for testing).
    pub fn state_mut(&mut self) -> &mut TuiState {
        &mut self.state
    }

    /// Get captured frames.
    pub fn frames(&self) -> &[TerminalFrame] {
        &self.frames
    }

    /// Process a single event and return generated commands.
    ///
    /// This is the core method for testing individual transitions.
    pub fn process_event(&mut self, event: TerminalEvent) -> Vec<TuiCommand> {
        let (new_state, commands) = transition(&self.state, event);
        self.state = new_state;
        commands
    }

    /// Execute a command.
    ///
    /// Some commands are handled internally (like Render),
    /// others are delegated to callbacks (like Dispatch).
    pub async fn execute_command(&mut self, command: TuiCommand) -> Result<(), TerminalError> {
        match command {
            TuiCommand::Exit => {
                // Exit is handled by the run loop checking should_exit
            }
            TuiCommand::ShowToast { message, level } => {
                // Add toast to queue
                let toast_id = self.state.next_toast_id;
                self.state.next_toast_id += 1;
                let toast = crate::tui::state_machine::QueuedToast {
                    id: toast_id,
                    message,
                    level,
                    ticks_remaining: 30, // ~3 seconds at 100ms/tick
                };
                self.state.toast_queue.enqueue(toast);
            }
            TuiCommand::DismissToast { id: _ } => {
                // Dismiss current toast from queue
                self.state.toast_queue.dismiss();
            }
            TuiCommand::ClearAllToasts => {
                self.state.toast_queue.clear();
            }
            TuiCommand::Dispatch(dispatch_cmd) => {
                if let Some(ref callback) = self.dispatch_callback {
                    callback(&dispatch_cmd);
                }
            }
            TuiCommand::Render => {
                // In headless mode, we could render to a frame buffer
                // For now, this is a no-op unless we have frame capture
            }
        }
        Ok(())
    }

    /// Run a single iteration: read event -> transition -> execute commands.
    ///
    /// Returns `Ok(true)` if should continue, `Ok(false)` if should exit.
    pub async fn step(&mut self) -> Result<bool, TerminalError> {
        // Read next event
        let event = self.terminal.next_event().await?;

        // Transition
        let commands = self.process_event(event);

        // Execute commands
        for command in commands {
            self.execute_command(command).await?;
        }

        // Check if we should exit
        Ok(!self.state.should_exit)
    }

    /// Run the event loop until exit or max iterations.
    pub async fn run(&mut self) -> Result<(), TerminalError> {
        let mut iterations = 0;

        loop {
            match self.step().await {
                Ok(true) => {
                    // Continue
                    iterations += 1;
                    if let Some(max) = self.max_iterations {
                        if iterations >= max {
                            return Ok(());
                        }
                    }
                }
                Ok(false) => {
                    // Exit requested
                    return Ok(());
                }
                Err(TerminalError::EndOfInput) => {
                    // No more events (test completed)
                    return Ok(());
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }
}

/// Builder for creating TuiRuntime instances with common configurations.
pub struct TuiRuntimeBuilder<T: TerminalEffects> {
    terminal: T,
    state: Option<TuiState>,
    dispatch_callback: Option<DispatchCallback>,
    max_iterations: Option<usize>,
}

impl<T: TerminalEffects> TuiRuntimeBuilder<T> {
    /// Create a new builder with the given terminal handler.
    pub fn new(terminal: T) -> Self {
        Self {
            terminal,
            state: None,
            dispatch_callback: None,
            max_iterations: None,
        }
    }

    /// Set initial state.
    pub fn with_state(mut self, state: TuiState) -> Self {
        self.state = Some(state);
        self
    }

    /// Set dispatch callback.
    pub fn with_dispatch_callback(mut self, callback: DispatchCallback) -> Self {
        self.dispatch_callback = Some(callback);
        self
    }

    /// Set max iterations.
    pub fn with_max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Build the runtime.
    pub fn build(self) -> TuiRuntime<T> {
        let mut runtime = if let Some(state) = self.state {
            TuiRuntime::with_state(self.terminal, state)
        } else {
            TuiRuntime::new(self.terminal)
        };

        if let Some(callback) = self.dispatch_callback {
            runtime.dispatch_callback = Some(callback);
        }

        if let Some(max) = self.max_iterations {
            runtime.max_iterations = Some(max);
        }

        runtime
    }
}

/// Snapshot of runtime state for assertions.
#[derive(Clone, Debug)]
pub struct RuntimeSnapshot {
    /// Current state
    pub state: TuiState,
    /// Number of frames captured
    pub frame_count: usize,
}

impl<T: TerminalEffects> TuiRuntime<T> {
    /// Take a snapshot of current runtime state.
    pub fn snapshot(&self) -> RuntimeSnapshot {
        RuntimeSnapshot {
            state: self.state.clone(),
            frame_count: self.frames.len(),
        }
    }
}

/// Test utilities for TuiRuntime
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::screens::Screen;
    use aura_core::effects::terminal::events;

    /// Simple mock terminal for testing
    struct SimpleMockTerminal {
        events: std::sync::Mutex<Vec<TerminalEvent>>,
    }

    impl SimpleMockTerminal {
        fn new(events: Vec<TerminalEvent>) -> Self {
            Self {
                events: std::sync::Mutex::new(events.into_iter().rev().collect()),
            }
        }
    }

    #[async_trait::async_trait]
    impl aura_core::effects::terminal::TerminalInputEffects for SimpleMockTerminal {
        async fn next_event(&self) -> Result<TerminalEvent, TerminalError> {
            let mut events = self.events.lock().unwrap();
            events.pop().ok_or(TerminalError::EndOfInput)
        }

        async fn poll_event(
            &self,
            _timeout_ms: u64,
        ) -> Result<Option<TerminalEvent>, TerminalError> {
            let mut events = self.events.lock().unwrap();
            Ok(events.pop())
        }

        async fn has_input(&self) -> bool {
            let events = self.events.lock().unwrap();
            !events.is_empty()
        }
    }

    #[async_trait::async_trait]
    impl aura_core::effects::terminal::TerminalOutputEffects for SimpleMockTerminal {
        async fn render(&self, _frame: TerminalFrame) -> Result<(), TerminalError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), TerminalError> {
            Ok(())
        }

        async fn size(&self) -> Result<(u16, u16), TerminalError> {
            Ok((80, 24))
        }

        async fn set_cursor(&self, _col: u16, _row: u16) -> Result<(), TerminalError> {
            Ok(())
        }

        async fn set_cursor_visible(&self, _visible: bool) -> Result<(), TerminalError> {
            Ok(())
        }

        async fn set_cursor_shape(
            &self,
            _shape: aura_core::effects::terminal::CursorShape,
        ) -> Result<(), TerminalError> {
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

    #[tokio::test]
    async fn test_runtime_screen_navigation() {
        let terminal = SimpleMockTerminal::new(vec![
            events::char('2'), // Navigate to Chat
            events::char('q'), // Quit
        ]);

        let mut runtime = TuiRuntime::new(terminal);
        runtime.run().await.unwrap();

        assert_eq!(runtime.state().screen(), Screen::Chat);
        assert!(runtime.state().should_exit);
    }

    #[tokio::test]
    async fn test_runtime_max_iterations() {
        let terminal = SimpleMockTerminal::new(vec![
            events::char('1'),
            events::char('2'),
            events::char('3'),
            events::char('4'),
            events::char('5'),
        ]);

        let mut runtime = TuiRuntime::new(terminal).with_max_iterations(3);
        runtime.run().await.unwrap();

        // Should have stopped after 3 iterations
        assert!(!runtime.state().should_exit);
    }

    #[tokio::test]
    async fn test_runtime_step_by_step() {
        let terminal = SimpleMockTerminal::new(vec![
            events::char('2'), // Navigate to Chat
            events::char('3'), // Navigate to Contacts
        ]);

        let mut runtime = TuiRuntime::new(terminal);

        // First step
        assert!(runtime.step().await.unwrap());
        assert_eq!(runtime.state().screen(), Screen::Chat);

        // Second step
        assert!(runtime.step().await.unwrap());
        assert_eq!(runtime.state().screen(), Screen::Contacts);

        // Third step - end of input
        assert!(runtime.step().await.is_err());
    }

    #[tokio::test]
    async fn test_runtime_builder() {
        let terminal = SimpleMockTerminal::new(vec![events::char('q')]);

        let mut runtime = TuiRuntimeBuilder::new(terminal)
            .with_max_iterations(10)
            .build();

        runtime.run().await.unwrap();
        assert!(runtime.state().should_exit);
    }
}
