//! Handler submodule for state machine transitions
//!
//! Organizes key/mouse event handlers into logical groups:
//! - `input`: Mouse, paste, and insert mode handlers
//! - `modal`: Modal keyboard handlers
//! - `screen`: Screen-specific keyboard handlers

mod input;
mod modal;
mod screen;

use aura_core::effects::terminal::{KeyCode, KeyEvent};

use crate::tui::screens::Screen;

use super::commands::TuiCommand;
use super::modal_queue::QueuedModal;
use super::TuiState;

// Re-export handler functions for use by transition.rs
pub use input::{handle_insert_mode_key, handle_mouse_event, handle_paste_event};
pub use modal::handle_queued_modal_key;
pub use screen::{
    handle_block_key, handle_chat_key, handle_contacts_key, handle_neighborhood_key,
    handle_recovery_key, handle_settings_key,
};

/// Handle a key event
///
/// Routes key events to the appropriate handler based on current state:
/// 1. Modal handlers (if modal is active)
/// 2. Insert mode handlers (if in insert mode)
/// 3. Global key handlers
/// 4. Screen-specific handlers
pub fn handle_key_event(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Queued modal gets priority (all modals are now queue-based)
    if state.has_queued_modal() {
        handle_modal_key(state, commands, key);
        return;
    }

    // Insert mode gets priority
    if state.is_insert_mode() {
        handle_insert_mode_key(state, commands, key);
        return;
    }

    // Global keys
    if handle_global_key(state, commands, &key) {
        return;
    }

    // Screen-specific keys (exhaustive match on all Screen variants)
    match state.screen() {
        Screen::Block => handle_block_key(state, commands, key),
        Screen::Chat => handle_chat_key(state, commands, key),
        Screen::Contacts => handle_contacts_key(state, commands, key),
        Screen::Neighborhood => handle_neighborhood_key(state, commands, key),
        Screen::Settings => handle_settings_key(state, commands, key),
        Screen::Recovery => handle_recovery_key(state, commands, key),
    }
}

/// Handle modal key events (queue-based only)
fn handle_modal_key(state: &mut TuiState, commands: &mut Vec<TuiCommand>, key: KeyEvent) {
    // Handle queued modal key events
    if let Some(queued_modal) = state.modal_queue.current().cloned() {
        handle_queued_modal_key(state, commands, key, queued_modal);
    }
}

/// Handle global keys (available in all screens)
pub fn handle_global_key(
    state: &mut TuiState,
    commands: &mut Vec<TuiCommand>,
    key: &KeyEvent,
) -> bool {
    // Quit
    if key.code == KeyCode::Char('q') && !key.modifiers.shift() {
        state.should_exit = true;
        commands.push(TuiCommand::Exit);
        return true;
    }

    // Ctrl+C - force quit
    if key.code == KeyCode::Char('c') && key.modifiers.ctrl() {
        state.should_exit = true;
        commands.push(TuiCommand::Exit);
        return true;
    }

    // Escape - dismiss ONE toast at a time (when no modal is open)
    // Note: Modal escape handling is in handle_modal_key, so this only fires
    // when there's no modal open
    if key.code == KeyCode::Esc {
        if state.toast_queue.is_active() {
            // Dismiss the current toast (queue automatically shows next one)
            state.toast_queue.dismiss();
        }
        // If no toasts, Esc does nothing here (modals handled in handle_modal_key)
        return true;
    }

    // Help (?)
    if key.code == KeyCode::Char('?') {
        state.modal_queue.enqueue(QueuedModal::Help {
            current_screen: Some(state.screen()),
        });
        return true;
    }

    // Number keys for screen navigation (1-7)
    if let KeyCode::Char(c) = key.code {
        if let Some(digit) = c.to_digit(10) {
            if let Some(screen) = Screen::from_key(digit as u8) {
                state.router.go_to(screen);
                return true;
            }
        }
    }

    // Tab - next screen
    if key.code == KeyCode::Tab && !key.modifiers.shift() {
        state.router.next_tab();
        return true;
    }

    // Shift+Tab - previous screen
    if key.code == KeyCode::BackTab || (key.code == KeyCode::Tab && key.modifiers.shift()) {
        state.router.prev_tab();
        return true;
    }

    false
}
