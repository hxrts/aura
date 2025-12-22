//! State transition logic
//!
//! Contains the pure transition function that coordinates all input handlers.

use aura_core::effects::terminal::TerminalEvent;

use super::commands::TuiCommand;
use super::handlers::{handle_key_event, handle_mouse_event, handle_paste_event};
use super::TuiState;

// Pure Transition Function
// ============================================================================

/// Pure state transition function
///
/// Given the current state and an input event, produces a new state and
/// a list of commands to execute. This function has no side effects.
///
/// # Arguments
///
/// * `state` - Current TUI state
/// * `event` - Terminal event to process
///
/// # Returns
///
/// A tuple of (new state, commands to execute)
pub fn transition(state: &TuiState, event: TerminalEvent) -> (TuiState, Vec<TuiCommand>) {
    let mut new_state = state.clone();
    let mut commands = Vec::new();

    match event {
        TerminalEvent::Key(key) => {
            handle_key_event(&mut new_state, &mut commands, key);
        }
        TerminalEvent::Resize { width, height } => {
            new_state.terminal_size = (width, height);
        }
        TerminalEvent::Tick => {
            // Time-based updates: tick the toast queue (handles decrement and auto-dismiss)
            new_state.toast_queue.tick();
        }
        TerminalEvent::Mouse(mouse) => {
            handle_mouse_event(&mut new_state, &mut commands, mouse);
        }
        TerminalEvent::FocusGained => {
            new_state.window_focused = true;
        }
        TerminalEvent::FocusLost => {
            new_state.window_focused = false;
        }
        TerminalEvent::Paste(text) => {
            handle_paste_event(&mut new_state, &mut commands, text);
        }
    }

    (new_state, commands)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::screens::Screen;
    use crate::tui::state::commands::DispatchCommand;
    use crate::tui::state::ModalType;
    use aura_core::effects::terminal::events;

    #[test]
    fn test_initial_state() {
        let state = TuiState::new();
        assert_eq!(state.screen(), Screen::Block);
        assert!(!state.has_modal());
        assert!(!state.is_insert_mode());
    }

    #[test]
    fn test_screen_navigation() {
        let state = TuiState::new();

        // Press '3' to go to Chat (see Screen::from_key mapping)
        let (new_state, _) = transition(&state, events::char('3'));
        assert_eq!(new_state.screen(), Screen::Chat);

        // Press Tab to go to next screen
        let (new_state, _) = transition(&new_state, events::tab());
        assert_eq!(new_state.screen(), Screen::Contacts);
    }

    #[test]
    fn test_quit() {
        let state = TuiState::new();

        // Press 'q' to quit
        let (new_state, commands) = transition(&state, events::char('q'));
        assert!(new_state.should_exit);
        assert!(commands.iter().any(|c| matches!(c, TuiCommand::Exit)));
    }

    #[test]
    fn test_insert_mode() {
        let state = TuiState::new();

        // Press 'i' to enter insert mode
        let (new_state, _) = transition(&state, events::char('i'));
        assert!(new_state.block.insert_mode);
        assert!(new_state.is_insert_mode());

        // Type some text
        let (new_state, _) = transition(&new_state, events::char('h'));
        let (new_state, _) = transition(&new_state, events::char('i'));
        assert_eq!(new_state.block.input_buffer, "hi");

        // Press Escape to exit insert mode
        let (new_state, _) = transition(&new_state, events::escape());
        assert!(!new_state.block.insert_mode);
        assert!(!new_state.is_insert_mode());
    }

    #[test]
    fn test_help_modal() {
        let state = TuiState::new();

        // Press '?' to open help
        let (new_state, _) = transition(&state, events::char('?'));
        assert!(new_state.has_modal());
        assert_eq!(new_state.current_modal_type(), ModalType::Help);

        // Press Escape to close
        let (new_state, _) = transition(&new_state, events::escape());
        assert!(!new_state.has_modal());
    }

    #[test]
    fn test_send_message_command() {
        let mut state = TuiState::new();
        state.block.insert_mode = true;
        state.block.input_buffer = "hello".to_string();

        // Press Enter to send
        let (new_state, commands) = transition(&state, events::enter());
        assert!(new_state.block.input_buffer.is_empty());
        assert!(commands.iter().any(|c| matches!(
            c,
            TuiCommand::Dispatch(DispatchCommand::SendBlockMessage { content })
            if content == "hello"
        )));
    }

    #[test]
    fn test_resize_event() {
        let state = TuiState::new();

        let (new_state, _) = transition(&state, events::resize(120, 40));
        assert_eq!(new_state.terminal_size, (120, 40));
    }

    #[test]
    fn test_account_setup_modal() {
        let state = TuiState::with_account_setup();

        // Modal should be visible
        assert!(state.has_modal());
        assert_eq!(state.current_modal_type(), ModalType::AccountSetup);

        // Type a name
        let (state, _) = transition(&state, events::char('A'));
        let (state, _) = transition(&state, events::char('l'));
        let (state, _) = transition(&state, events::char('i'));
        let (state, _) = transition(&state, events::char('c'));
        let (state, _) = transition(&state, events::char('e'));
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alice");

        // Submit should dispatch CreateAccount and set creating flag
        let (state, commands) = transition(&state, events::enter());
        assert!(state.account_setup_state().unwrap().creating);
        assert!(commands.iter().any(|c| matches!(
            c,
            TuiCommand::Dispatch(DispatchCommand::CreateAccount { name })
            if name == "Alice"
        )));
    }

    #[test]
    fn test_account_setup_async_feedback() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();
        state.account_setup_state_mut().unwrap().creating = true;

        // Simulate success callback
        state.account_created();
        assert!(state.account_setup_state().unwrap().success);
        assert!(!state.account_setup_state().unwrap().creating);

        // Enter should close modal
        let (state, _) = transition(&state, events::enter());
        assert!(!state.has_modal());
    }

    #[test]
    fn test_account_setup_error_recovery() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();
        state.account_setup_state_mut().unwrap().creating = true;

        // Simulate error callback
        state.account_creation_failed("Network error".to_string());
        assert!(!state.account_setup_state().unwrap().creating);
        assert_eq!(
            state.account_setup_state().unwrap().error,
            Some("Network error".to_string())
        );

        // Enter should reset to input state
        let (state, _) = transition(&state, events::enter());
        assert!(state.account_setup_state().unwrap().error.is_none());
        assert!(!state.account_setup_state().unwrap().success);
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alice"); // Name preserved
    }

    #[test]
    fn test_account_setup_escape() {
        let state = TuiState::with_account_setup();

        // Escape should close modal
        let (state, _) = transition(&state, events::escape());
        assert!(!state.has_modal());
    }

    #[test]
    fn test_account_setup_backspace() {
        let mut state = TuiState::with_account_setup();
        state.account_setup_state_mut().unwrap().display_name = "Alice".to_string();

        // Backspace should remove character
        let (state, _) = transition(&state, events::backspace());
        assert_eq!(state.account_setup_state().unwrap().display_name, "Alic");
    }

    #[test]
    fn test_threshold_modal_arrow_keys() {
        use crate::tui::state::modal_queue::QueuedModal;
        use crate::tui::state::views::ThresholdModalState;

        let mut state = TuiState::new();
        // Navigate to Settings screen
        state.router.go_to(Screen::Settings);

        // Enqueue threshold modal with k=2, n=3
        state.modal_queue.enqueue(QueuedModal::SettingsThreshold(
            ThresholdModalState::with_threshold(2, 3),
        ));

        // Verify modal is active and initial values
        assert!(state.has_queued_modal());
        if let Some(QueuedModal::SettingsThreshold(modal_state)) = state.modal_queue.current() {
            assert_eq!(modal_state.k, 2);
            assert_eq!(modal_state.n, 3);
            assert_eq!(modal_state.active_field, 0); // k field is active
        } else {
            panic!("Expected SettingsThreshold modal");
        }

        // Press Right arrow to increment k (should go from 2 to 3)
        let (state, _) = transition(&state, events::arrow_right());
        if let Some(QueuedModal::SettingsThreshold(modal_state)) = state.modal_queue.current() {
            assert_eq!(modal_state.k, 3, "Right arrow should increment k from 2 to 3");
        } else {
            panic!("Expected SettingsThreshold modal after Right key");
        }

        // Press Left arrow to decrement k (should go from 3 to 2)
        let (state, _) = transition(&state, events::arrow_left());
        if let Some(QueuedModal::SettingsThreshold(modal_state)) = state.modal_queue.current() {
            assert_eq!(modal_state.k, 2, "Left arrow should decrement k from 3 to 2");
        } else {
            panic!("Expected SettingsThreshold modal after Left key");
        }

        // Press Left again to decrement k (should go from 2 to 1)
        let (state, _) = transition(&state, events::arrow_left());
        if let Some(QueuedModal::SettingsThreshold(modal_state)) = state.modal_queue.current() {
            assert_eq!(modal_state.k, 1, "Left arrow should decrement k from 2 to 1");
        } else {
            panic!("Expected SettingsThreshold modal after second Left key");
        }

        // Press Left again - k should stay at 1 (minimum)
        let (state, _) = transition(&state, events::arrow_left());
        if let Some(QueuedModal::SettingsThreshold(modal_state)) = state.modal_queue.current() {
            assert_eq!(modal_state.k, 1, "Left arrow should not decrement k below 1");
        } else {
            panic!("Expected SettingsThreshold modal after third Left key");
        }

        // Press Escape to dismiss
        let (state, _) = transition(&state, events::escape());
        assert!(!state.has_queued_modal(), "Escape should dismiss modal");
    }
}
