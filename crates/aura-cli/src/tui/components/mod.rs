//! # TUI Components
//!
//! Trait-based component system for the TUI. Components are self-contained
//! UI elements that can handle input and render themselves.
//!
//! This module provides the foundation for building complex UIs from
//! composable, reusable components.

pub mod command_palette;
pub mod message_input;
pub mod modal;
pub mod toast;

pub use command_palette::{CommandPalette, CommandPaletteCategory, PaletteAction, PaletteCommand};
pub use message_input::MessageInput;
pub use modal::{Modal, ModalAction, ModalButton};
pub use toast::{Toast, ToastId, ToastManager};

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

use super::input::InputAction;
use super::styles::Styles;

/// Trait for TUI components
///
/// Components are self-contained UI elements that:
/// - Can render themselves to a terminal frame
/// - Can handle keyboard input
/// - Can be focused or unfocused
/// - Have a consistent styling interface
pub trait Component {
    /// Handle a key event
    ///
    /// Returns an InputAction if the component handled the event,
    /// None if the event should be passed to the parent.
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction>;

    /// Render the component to the given area
    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles);

    /// Check if the component is focused
    fn is_focused(&self) -> bool;

    /// Set the focus state
    fn set_focused(&mut self, focused: bool);

    /// Get the component's minimum size (width, height)
    fn min_size(&self) -> (u16, u16) {
        (1, 1)
    }

    /// Check if the component is visible
    fn is_visible(&self) -> bool {
        true
    }
}

/// A component that can be focused in a focus chain
pub trait Focusable: Component {
    /// Get the tab order index (lower = earlier)
    fn tab_index(&self) -> usize;

    /// Whether this component can receive focus
    fn can_focus(&self) -> bool {
        true
    }
}

/// Focus direction for navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    /// Move to next focusable component
    Next,
    /// Move to previous focusable component
    Previous,
    /// Move up
    Up,
    /// Move down
    Down,
    /// Move left
    Left,
    /// Move right
    Right,
}

/// Manager for a set of focusable components
pub struct FocusManager {
    /// Currently focused component index
    current: Option<usize>,
    /// Total number of focusable components
    count: usize,
}

impl FocusManager {
    /// Create a new focus manager
    pub fn new(count: usize) -> Self {
        Self {
            current: if count > 0 { Some(0) } else { None },
            count,
        }
    }

    /// Get the currently focused index
    pub fn current(&self) -> Option<usize> {
        self.current
    }

    /// Move focus in the given direction
    pub fn move_focus(&mut self, direction: FocusDirection) -> Option<usize> {
        if self.count == 0 {
            return None;
        }

        let new_index = match direction {
            FocusDirection::Next | FocusDirection::Down | FocusDirection::Right => {
                match self.current {
                    Some(i) => (i + 1) % self.count,
                    None => 0,
                }
            }
            FocusDirection::Previous | FocusDirection::Up | FocusDirection::Left => {
                match self.current {
                    Some(i) if i > 0 => i - 1,
                    Some(_) => self.count - 1,
                    None => self.count - 1,
                }
            }
        };

        self.current = Some(new_index);
        self.current
    }

    /// Set focus to a specific index
    pub fn set_focus(&mut self, index: usize) -> bool {
        if index < self.count {
            self.current = Some(index);
            true
        } else {
            false
        }
    }

    /// Clear focus
    pub fn clear_focus(&mut self) {
        self.current = None;
    }

    /// Update the component count
    pub fn set_count(&mut self, count: usize) {
        self.count = count;
        if let Some(current) = self.current {
            if current >= count {
                self.current = if count > 0 { Some(count - 1) } else { None };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_manager_new() {
        let fm = FocusManager::new(3);
        assert_eq!(fm.current(), Some(0));

        let fm_empty = FocusManager::new(0);
        assert_eq!(fm_empty.current(), None);
    }

    #[test]
    fn test_focus_manager_next() {
        let mut fm = FocusManager::new(3);
        assert_eq!(fm.current(), Some(0));

        fm.move_focus(FocusDirection::Next);
        assert_eq!(fm.current(), Some(1));

        fm.move_focus(FocusDirection::Next);
        assert_eq!(fm.current(), Some(2));

        // Wrap around
        fm.move_focus(FocusDirection::Next);
        assert_eq!(fm.current(), Some(0));
    }

    #[test]
    fn test_focus_manager_previous() {
        let mut fm = FocusManager::new(3);
        assert_eq!(fm.current(), Some(0));

        // Wrap around backwards
        fm.move_focus(FocusDirection::Previous);
        assert_eq!(fm.current(), Some(2));

        fm.move_focus(FocusDirection::Previous);
        assert_eq!(fm.current(), Some(1));
    }

    #[test]
    fn test_focus_manager_set_focus() {
        let mut fm = FocusManager::new(3);

        assert!(fm.set_focus(2));
        assert_eq!(fm.current(), Some(2));

        assert!(!fm.set_focus(5)); // Out of bounds
        assert_eq!(fm.current(), Some(2)); // Unchanged
    }

    #[test]
    fn test_focus_manager_clear() {
        let mut fm = FocusManager::new(3);
        fm.clear_focus();
        assert_eq!(fm.current(), None);
    }
}
