//! # Screen Router
//!
//! Screen navigation and stack management for the TUI.

use std::collections::VecDeque;

/// Screen identifiers for navigation
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Screen {
    /// Neighborhood navigation (home)
    #[default]
    Neighborhood,
    /// Chat conversations
    Chat,
    /// Contacts, nicknames, and invitations
    Contacts,
    /// Notifications and requests
    Notifications,
    /// Settings and preferences (rightmost in nav bar)
    Settings,
}

impl Screen {
    /// Get the numeric key (1-6) for this screen
    pub fn key_number(&self) -> u8 {
        match self {
            Screen::Neighborhood => 1,
            Screen::Chat => 2,
            Screen::Contacts => 3,
            Screen::Notifications => 4,
            Screen::Settings => 5,
        }
    }

    /// Get screen from numeric key (1-6)
    pub fn from_key(key: u8) -> Option<Self> {
        match key {
            1 => Some(Screen::Neighborhood),
            2 => Some(Screen::Chat),
            3 => Some(Screen::Contacts),
            4 => Some(Screen::Notifications),
            5 => Some(Screen::Settings),
            _ => None,
        }
    }

    /// Get the display name for the screen
    pub fn name(&self) -> &'static str {
        match self {
            Screen::Neighborhood => "Neighborhood",
            Screen::Chat => "Chat",
            Screen::Contacts => "Contacts",
            Screen::Notifications => "Notifications",
            Screen::Settings => "Settings",
        }
    }

    /// Get the icon/emoji for the screen
    pub fn icon(&self) -> &'static str {
        match self {
            Screen::Neighborhood => "⊞",
            Screen::Chat => "◊",
            Screen::Contacts => "∑",
            Screen::Notifications => "✉",
            Screen::Settings => "⚙",
        }
    }

    /// Get all screens in order
    pub fn all() -> &'static [Screen] {
        &[
            Screen::Neighborhood,
            Screen::Chat,
            Screen::Contacts,
            Screen::Notifications,
            Screen::Settings,
        ]
    }

    /// Get next screen in tab order
    pub fn next(&self) -> Screen {
        match self {
            Screen::Neighborhood => Screen::Chat,
            Screen::Chat => Screen::Contacts,
            Screen::Contacts => Screen::Notifications,
            Screen::Notifications => Screen::Settings,
            Screen::Settings => Screen::Neighborhood,
        }
    }

    /// Get previous screen in tab order
    pub fn prev(&self) -> Screen {
        match self {
            Screen::Neighborhood => Screen::Settings,
            Screen::Chat => Screen::Neighborhood,
            Screen::Contacts => Screen::Chat,
            Screen::Notifications => Screen::Contacts,
            Screen::Settings => Screen::Notifications,
        }
    }
}

/// Navigation action for the router
#[derive(Clone, Debug)]
pub enum NavAction {
    /// Go to a specific screen
    GoTo(Screen),
    /// Go back to previous screen in history
    Back,
    /// Go forward (if backed out)
    Forward,
    /// Replace current screen without adding to history
    Replace(Screen),
    /// Clear history and go to screen
    Reset(Screen),
    /// Go to next screen in tab order
    NextTab,
    /// Go to previous screen in tab order
    PrevTab,
}

/// Screen router state
#[derive(Clone, Debug)]
pub struct Router {
    /// Current active screen
    current: Screen,
    /// Navigation history (for back functionality)
    history: VecDeque<Screen>,
    /// Forward history (for forward after back)
    forward: VecDeque<Screen>,
    /// Maximum history length
    max_history: usize,
}

impl Default for Router {
    fn default() -> Self {
        Self::new(Screen::Neighborhood)
    }
}

impl Router {
    /// Create a new router starting at the given screen
    pub fn new(initial: Screen) -> Self {
        Self {
            current: initial,
            history: VecDeque::new(),
            forward: VecDeque::new(),
            max_history: 50,
        }
    }

    /// Get the current screen
    pub fn current(&self) -> Screen {
        self.current
    }

    /// Navigate using an action
    #[allow(clippy::needless_pass_by_value)] // NavAction is small and matched on
    pub fn navigate(&mut self, action: NavAction) {
        match action {
            NavAction::GoTo(screen) => self.go_to(screen),
            NavAction::Back => self.back(),
            NavAction::Forward => self.forward(),
            NavAction::Replace(screen) => self.replace(screen),
            NavAction::Reset(screen) => self.reset(screen),
            NavAction::NextTab => self.next_tab(),
            NavAction::PrevTab => self.prev_tab(),
        }
    }

    /// Go to a specific screen
    pub fn go_to(&mut self, screen: Screen) {
        if screen != self.current {
            // Add current to history
            self.history.push_back(self.current);
            if self.history.len() > self.max_history {
                self.history.pop_front();
            }
            // Clear forward history
            self.forward.clear();
            // Set new current
            self.current = screen;
        }
    }

    /// Go back to previous screen
    pub fn back(&mut self) {
        if let Some(prev) = self.history.pop_back() {
            self.forward.push_front(self.current);
            self.current = prev;
        }
    }

    /// Go forward (after going back)
    pub fn forward(&mut self) {
        if let Some(next) = self.forward.pop_front() {
            self.history.push_back(self.current);
            self.current = next;
        }
    }

    /// Replace current screen without history
    pub fn replace(&mut self, screen: Screen) {
        self.current = screen;
    }

    /// Reset to a screen, clearing all history
    pub fn reset(&mut self, screen: Screen) {
        self.history.clear();
        self.forward.clear();
        self.current = screen;
    }

    /// Go to next screen in tab order
    pub fn next_tab(&mut self) {
        self.go_to(self.current.next());
    }

    /// Go to previous screen in tab order
    pub fn prev_tab(&mut self) {
        self.go_to(self.current.prev());
    }

    /// Check if we can go back
    pub fn can_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// Check if we can go forward
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Get history length
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Handle numeric key (1-8) to navigate
    pub fn handle_number_key(&mut self, key: char) -> bool {
        if let Some(digit) = key.to_digit(10) {
            if let Some(screen) = Screen::from_key(digit as u8) {
                self.go_to(screen);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_router_navigation() {
        let mut router = Router::new(Screen::Neighborhood);
        assert_eq!(router.current(), Screen::Neighborhood);

        router.go_to(Screen::Chat);
        assert_eq!(router.current(), Screen::Chat);
        assert!(router.can_back());

        router.back();
        assert_eq!(router.current(), Screen::Neighborhood);
        assert!(router.can_forward());

        router.forward();
        assert_eq!(router.current(), Screen::Chat);
    }

    #[test]
    fn test_tab_navigation() {
        let mut router = Router::new(Screen::Neighborhood);

        router.next_tab();
        assert_eq!(router.current(), Screen::Chat);

        router.next_tab();
        assert_eq!(router.current(), Screen::Contacts);

        router.prev_tab();
        assert_eq!(router.current(), Screen::Chat);

        // Go all the way back (wraps to Settings)
        let mut r2 = Router::new(Screen::Neighborhood);
        r2.prev_tab();
        assert_eq!(r2.current(), Screen::Settings);
    }

    #[test]
    fn test_screen_keys() {
        assert_eq!(Screen::Neighborhood.key_number(), 1);
        assert_eq!(Screen::from_key(1), Some(Screen::Neighborhood));
        assert_eq!(Screen::from_key(7), None);
    }
}
