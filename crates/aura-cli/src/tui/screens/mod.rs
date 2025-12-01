//! # TUI Screens
//!
//! Screen implementations using the reactive view system.
//! Each screen composes components and connects to view dynamics.

pub mod block;
pub mod block_messages;
pub mod chat;
pub mod contacts;
pub mod guardians;
pub mod help;
pub mod invitations;
pub mod neighborhood;
pub mod recovery;
pub mod welcome;

pub use block::{BlockScreen, BlockStorageBudgetView, MessageChannel, Resident};
pub use block_messages::{BlockMessage, BlockMessagesScreen, SyncProgress};
pub use chat::ChatScreen;
pub use contacts::{Contact, ContactSuggestion, ContactsScreen, EditingField, SuggestionPolicy};
pub use guardians::{GuardiansScreen, ThresholdInfo};
pub use help::HelpScreen;
pub use invitations::{InvitationFilter, InvitationsScreen};
pub use neighborhood::{
    BlockAdjacency, BlockSummary, NeighborhoodScreen, TraversalDepth, TraversalPosition,
};
pub use recovery::RecoveryScreen;
pub use welcome::{OnboardingStep, WelcomeScreen};

use crossterm::event::KeyEvent;
use ratatui::{layout::Rect, Frame};

use super::input::InputAction;
use super::styles::Styles;

/// Screen type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScreenType {
    /// Welcome/home screen
    Welcome,
    /// Chat/messaging screen
    Chat,
    /// Guardian management screen
    Guardians,
    /// Recovery process screen
    Recovery,
    /// Invitations screen
    Invitations,
    /// Block management screen
    Block,
    /// Block message history screen
    BlockMessages,
    /// Neighborhood traversal screen
    Neighborhood,
    /// Contacts and petnames screen
    Contacts,
    /// Help overlay
    Help,
}

impl ScreenType {
    /// Get the screen title
    pub fn title(&self) -> &'static str {
        match self {
            Self::Welcome => "Welcome",
            Self::Chat => "Chat",
            Self::Guardians => "Guardians",
            Self::Recovery => "Recovery",
            Self::Invitations => "Invitations",
            Self::Block => "Block",
            Self::BlockMessages => "Block Messages",
            Self::Neighborhood => "Neighborhood",
            Self::Contacts => "Contacts",
            Self::Help => "Help",
        }
    }

    /// Get keyboard shortcut hint
    pub fn shortcut(&self) -> Option<char> {
        match self {
            Self::Chat => Some('c'),
            Self::Guardians => Some('g'),
            Self::Recovery => Some('r'),
            Self::Invitations => Some('i'),
            Self::Block => Some('b'),
            Self::BlockMessages => Some('m'),
            Self::Neighborhood => Some('n'),
            Self::Contacts => Some('o'), // 'o' for contaOcts (c is taken by Chat)
            Self::Help => Some('?'),
            _ => None,
        }
    }
}

/// Trait for full-screen views
pub trait Screen {
    /// Get the screen type
    fn screen_type(&self) -> ScreenType;

    /// Handle a key event
    ///
    /// Returns an InputAction if the screen handled the event,
    /// None if the event should be passed to the parent.
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction>;

    /// Render the screen
    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles);

    /// Called when the screen becomes active
    fn on_enter(&mut self) {}

    /// Called when the screen is about to be deactivated
    fn on_exit(&mut self) {}

    /// Update screen state (called on tick)
    fn update(&mut self) {}

    /// Check if the screen needs a redraw
    fn needs_redraw(&self) -> bool {
        false
    }
}

/// Screen navigation actions
#[derive(Debug, Clone)]
pub enum ScreenAction {
    /// Stay on current screen
    None,
    /// Navigate to a different screen
    Navigate(ScreenType),
    /// Push a screen onto the stack (overlay)
    Push(ScreenType),
    /// Pop the current screen from the stack
    Pop,
    /// Replace the current screen
    Replace(ScreenType),
}

/// Manager for screen navigation
pub struct ScreenManager {
    /// Stack of active screens
    stack: Vec<ScreenType>,
    /// Default screen
    default: ScreenType,
}

impl ScreenManager {
    /// Create a new screen manager
    pub fn new(default: ScreenType) -> Self {
        Self {
            stack: vec![default],
            default,
        }
    }

    /// Get the current active screen type
    pub fn current(&self) -> ScreenType {
        *self.stack.last().unwrap_or(&self.default)
    }

    /// Navigate to a screen (replaces stack)
    pub fn navigate(&mut self, screen: ScreenType) {
        self.stack.clear();
        self.stack.push(screen);
    }

    /// Push a screen onto the stack
    pub fn push(&mut self, screen: ScreenType) {
        self.stack.push(screen);
    }

    /// Pop the top screen from the stack
    pub fn pop(&mut self) -> Option<ScreenType> {
        if self.stack.len() > 1 {
            self.stack.pop()
        } else {
            None
        }
    }

    /// Replace the current screen
    pub fn replace(&mut self, screen: ScreenType) {
        if !self.stack.is_empty() {
            self.stack.pop();
        }
        self.stack.push(screen);
    }

    /// Get the stack depth
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Check if a screen is in the stack
    pub fn contains(&self, screen: ScreenType) -> bool {
        self.stack.contains(&screen)
    }

    /// Process a screen action
    pub fn process(&mut self, action: ScreenAction) {
        match action {
            ScreenAction::None => {}
            ScreenAction::Navigate(screen) => self.navigate(screen),
            ScreenAction::Push(screen) => self.push(screen),
            ScreenAction::Pop => {
                self.pop();
            }
            ScreenAction::Replace(screen) => self.replace(screen),
        }
    }
}

impl Default for ScreenManager {
    fn default() -> Self {
        Self::new(ScreenType::Welcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_type_title() {
        assert_eq!(ScreenType::Chat.title(), "Chat");
        assert_eq!(ScreenType::Guardians.title(), "Guardians");
    }

    #[test]
    fn test_screen_type_shortcut() {
        assert_eq!(ScreenType::Chat.shortcut(), Some('c'));
        assert_eq!(ScreenType::Welcome.shortcut(), None);
    }

    #[test]
    fn test_screen_manager_navigate() {
        let mut manager = ScreenManager::new(ScreenType::Welcome);
        assert_eq!(manager.current(), ScreenType::Welcome);

        manager.navigate(ScreenType::Chat);
        assert_eq!(manager.current(), ScreenType::Chat);
        assert_eq!(manager.depth(), 1);
    }

    #[test]
    fn test_screen_manager_push_pop() {
        let mut manager = ScreenManager::new(ScreenType::Welcome);

        manager.push(ScreenType::Chat);
        assert_eq!(manager.current(), ScreenType::Chat);
        assert_eq!(manager.depth(), 2);

        manager.push(ScreenType::Help);
        assert_eq!(manager.current(), ScreenType::Help);
        assert_eq!(manager.depth(), 3);

        manager.pop();
        assert_eq!(manager.current(), ScreenType::Chat);

        manager.pop();
        assert_eq!(manager.current(), ScreenType::Welcome);

        // Can't pop last screen
        assert!(manager.pop().is_none());
        assert_eq!(manager.current(), ScreenType::Welcome);
    }

    #[test]
    fn test_screen_manager_replace() {
        let mut manager = ScreenManager::new(ScreenType::Welcome);
        manager.push(ScreenType::Chat);

        manager.replace(ScreenType::Guardians);
        assert_eq!(manager.current(), ScreenType::Guardians);
        assert_eq!(manager.depth(), 2);
    }

    #[test]
    fn test_screen_manager_process() {
        let mut manager = ScreenManager::new(ScreenType::Welcome);

        manager.process(ScreenAction::Push(ScreenType::Chat));
        assert_eq!(manager.current(), ScreenType::Chat);

        manager.process(ScreenAction::Pop);
        assert_eq!(manager.current(), ScreenType::Welcome);

        manager.process(ScreenAction::Navigate(ScreenType::Guardians));
        assert_eq!(manager.current(), ScreenType::Guardians);
    }
}
