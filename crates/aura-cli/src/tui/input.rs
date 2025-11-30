//! # TUI Input Handling
//!
//! Extensible input handling for the TUI. Supports Normal, Editing, and Command modes.
//! Architecture is designed to support future vim-mode extension.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Input mode for the TUI
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum InputMode {
    /// Normal navigation mode (Tab, arrow keys, single-key shortcuts)
    #[default]
    Normal,
    /// Text editing mode (typing into input fields)
    Editing,
    /// Command palette mode (IRC-style /commands)
    Command,
    // Future: Vim(VimMode) for j/k/h/l navigation
}

impl InputMode {
    /// Display name for the mode
    pub fn as_str(&self) -> &'static str {
        match self {
            InputMode::Normal => "NORMAL",
            InputMode::Editing => "EDIT",
            InputMode::Command => "COMMAND",
        }
    }

    /// Whether this mode accepts text input
    pub fn accepts_text(&self) -> bool {
        matches!(self, InputMode::Editing | InputMode::Command)
    }
}

/// Result of handling a key event
#[derive(Debug, Clone)]
pub enum InputAction {
    /// No action taken
    None,
    /// Quit the application
    Quit,
    /// Navigate to next element
    FocusNext,
    /// Navigate to previous element
    FocusPrev,
    /// Show/hide help overlay
    ToggleHelp,
    /// Advance to next phase/step
    Advance,
    /// Go back to previous phase/step
    Back,
    /// Reset to initial state
    Reset,
    /// Enter editing mode
    EnterEditMode,
    /// Enter command mode
    EnterCommandMode,
    /// Exit to normal mode
    ExitToNormal,
    /// Submit the current input
    Submit(String),
    /// Execute an IRC command
    Command(crate::tui::commands::IrcCommand),
    /// Display an error message
    Error(String),
    /// Switch between screens
    SwitchScreen,
    /// Custom action with key code
    Custom(KeyCode),
    /// Text input (character added)
    TextInput(char),
    /// Backspace (delete character)
    Backspace,
}

/// Trait for input handlers
pub trait InputHandler {
    /// Handle a key event and return the resulting action
    fn handle_key(&mut self, key: KeyEvent, mode: &InputMode) -> InputAction;

    /// Get the current input buffer (for editing/command modes)
    fn input_buffer(&self) -> &str;

    /// Clear the input buffer
    fn clear_buffer(&mut self);

    /// Get the current input mode
    fn mode(&self) -> &InputMode;

    /// Set the input mode
    fn set_mode(&mut self, mode: InputMode);
}

/// Default input handler implementation
#[derive(Debug, Clone, Default)]
pub struct DefaultInputHandler {
    /// Current input mode
    mode: InputMode,
    /// Input buffer for editing/command modes
    buffer: String,
    /// Command history
    history: Vec<String>,
    /// Current position in history (for up/down navigation)
    history_pos: Option<usize>,
}

impl DefaultInputHandler {
    /// Create a new default input handler
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a command to history
    pub fn add_to_history(&mut self, command: String) {
        if !command.is_empty() {
            self.history.push(command);
            // Keep only last 100 commands
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
        self.history_pos = None;
    }

    /// Navigate up in history
    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_pos {
            None => {
                self.history_pos = Some(self.history.len() - 1);
                self.buffer = self.history[self.history.len() - 1].clone();
            }
            Some(pos) if pos > 0 => {
                self.history_pos = Some(pos - 1);
                self.buffer = self.history[pos - 1].clone();
            }
            _ => {}
        }
    }

    /// Navigate down in history
    fn history_down(&mut self) {
        if let Some(pos) = self.history_pos {
            if pos + 1 < self.history.len() {
                self.history_pos = Some(pos + 1);
                self.buffer = self.history[pos + 1].clone();
            } else {
                self.history_pos = None;
                self.buffer.clear();
            }
        }
    }

    /// Handle key in normal mode
    fn handle_normal(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Char('q') => InputAction::Quit,
            KeyCode::Char('h') => InputAction::ToggleHelp,
            KeyCode::Char('n') | KeyCode::Right => InputAction::Advance,
            KeyCode::Char('p') | KeyCode::Left => InputAction::Back,
            KeyCode::Char('r') => InputAction::Reset,
            KeyCode::Char('i') => {
                self.mode = InputMode::Editing;
                InputAction::EnterEditMode
            }
            KeyCode::Char(':') | KeyCode::Char('/') => {
                self.mode = InputMode::Command;
                self.buffer.clear();
                InputAction::EnterCommandMode
            }
            KeyCode::Tab => InputAction::SwitchScreen,
            KeyCode::Up => InputAction::FocusPrev,
            KeyCode::Down => InputAction::FocusNext,
            _ => InputAction::Custom(key.code),
        }
    }

    /// Handle key in editing mode
    fn handle_editing(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Enter => {
                let content = self.buffer.clone();
                self.buffer.clear();
                self.mode = InputMode::Normal;
                InputAction::Submit(content)
            }
            KeyCode::Esc => {
                self.buffer.clear();
                self.mode = InputMode::Normal;
                InputAction::ExitToNormal
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                InputAction::TextInput(c)
            }
            KeyCode::Backspace => {
                self.buffer.pop();
                InputAction::Backspace
            }
            KeyCode::Up => {
                self.history_up();
                InputAction::None
            }
            KeyCode::Down => {
                self.history_down();
                InputAction::None
            }
            _ => InputAction::None,
        }
    }

    /// Handle key in command mode
    fn handle_command(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Enter => {
                let command = self.buffer.clone();
                self.add_to_history(command.clone());
                self.buffer.clear();
                self.mode = InputMode::Normal;
                InputAction::Submit(command)
            }
            KeyCode::Esc => {
                self.buffer.clear();
                self.mode = InputMode::Normal;
                InputAction::ExitToNormal
            }
            KeyCode::Char(c) => {
                self.buffer.push(c);
                InputAction::TextInput(c)
            }
            KeyCode::Backspace => {
                if self.buffer.is_empty() {
                    self.mode = InputMode::Normal;
                    InputAction::ExitToNormal
                } else {
                    self.buffer.pop();
                    InputAction::Backspace
                }
            }
            KeyCode::Up => {
                self.history_up();
                InputAction::None
            }
            KeyCode::Down => {
                self.history_down();
                InputAction::None
            }
            KeyCode::Tab => {
                // Future: command completion
                InputAction::None
            }
            _ => InputAction::None,
        }
    }
}

impl InputHandler for DefaultInputHandler {
    fn handle_key(&mut self, key: KeyEvent, _mode: &InputMode) -> InputAction {
        // Handle Ctrl+C globally
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return InputAction::Quit;
        }

        match self.mode {
            InputMode::Normal => self.handle_normal(key),
            InputMode::Editing => self.handle_editing(key),
            InputMode::Command => self.handle_command(key),
        }
    }

    fn input_buffer(&self) -> &str {
        &self.buffer
    }

    fn clear_buffer(&mut self) {
        self.buffer.clear();
    }

    fn mode(&self) -> &InputMode {
        &self.mode
    }

    fn set_mode(&mut self, mode: InputMode) {
        let clear_buffer = mode == InputMode::Normal;
        self.mode = mode;
        if clear_buffer {
            self.buffer.clear();
        }
    }
}

/// Parse IRC-style commands
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// /help - Show help
    Help,
    /// /quit - Quit application
    Quit,
    /// /join <channel> - Join a channel
    Join(String),
    /// /leave - Leave current channel
    Leave,
    /// /msg <user> <message> - Direct message
    DirectMessage {
        /// Target user
        user: String,

        /// Message content
        message: String,
    },
    /// /invite <user> - Invite user to channel
    Invite(String),
    /// /recovery - Start recovery process
    Recovery,
    /// /status - Show status
    Status,
    /// /reset - Reset to initial state
    Reset,
    /// Unknown command
    Unknown(String),
}

impl Command {
    /// Parse a command string (without leading /)
    pub fn parse(input: &str) -> Self {
        let input = input.trim();
        let parts: Vec<&str> = input.splitn(3, ' ').collect();

        match parts.first().map(|s| s.to_lowercase()).as_deref() {
            Some("help") | Some("h") | Some("?") => Command::Help,
            Some("quit") | Some("q") | Some("exit") => Command::Quit,
            Some("join") | Some("j") => Command::Join(parts.get(1).unwrap_or(&"").to_string()),
            Some("leave") | Some("l") | Some("part") => Command::Leave,
            Some("msg") | Some("m") | Some("dm") => {
                if parts.len() >= 3 {
                    Command::DirectMessage {
                        user: parts[1].to_string(),
                        message: parts[2].to_string(),
                    }
                } else {
                    Command::Unknown(input.to_string())
                }
            }
            Some("invite") | Some("inv") => {
                Command::Invite(parts.get(1).unwrap_or(&"").to_string())
            }
            Some("recovery") | Some("recover") => Command::Recovery,
            Some("status") | Some("stat") | Some("s") => Command::Status,
            Some("reset") => Command::Reset,
            Some(_) | None => Command::Unknown(input.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_mode_default() {
        let mode = InputMode::default();
        assert_eq!(mode, InputMode::Normal);
    }

    #[test]
    fn test_input_mode_display() {
        assert_eq!(InputMode::Normal.as_str(), "NORMAL");
        assert_eq!(InputMode::Editing.as_str(), "EDIT");
        assert_eq!(InputMode::Command.as_str(), "COMMAND");
    }

    #[test]
    fn test_input_mode_accepts_text() {
        assert!(!InputMode::Normal.accepts_text());
        assert!(InputMode::Editing.accepts_text());
        assert!(InputMode::Command.accepts_text());
    }

    #[test]
    fn test_command_parse_help() {
        assert_eq!(Command::parse("help"), Command::Help);
        assert_eq!(Command::parse("h"), Command::Help);
        assert_eq!(Command::parse("?"), Command::Help);
    }

    #[test]
    fn test_command_parse_quit() {
        assert_eq!(Command::parse("quit"), Command::Quit);
        assert_eq!(Command::parse("q"), Command::Quit);
        assert_eq!(Command::parse("exit"), Command::Quit);
    }

    #[test]
    fn test_command_parse_join() {
        assert_eq!(
            Command::parse("join general"),
            Command::Join("general".to_string())
        );
        assert_eq!(Command::parse("j dev"), Command::Join("dev".to_string()));
    }

    #[test]
    fn test_command_parse_dm() {
        assert_eq!(
            Command::parse("msg alice hello there"),
            Command::DirectMessage {
                user: "alice".to_string(),
                message: "hello there".to_string(),
            }
        );
    }

    #[test]
    fn test_command_parse_unknown() {
        assert_eq!(
            Command::parse("foobar"),
            Command::Unknown("foobar".to_string())
        );
    }

    #[test]
    fn test_default_handler_mode_transitions() {
        let mut handler = DefaultInputHandler::new();
        assert_eq!(handler.mode(), &InputMode::Normal);

        // Enter editing mode
        handler.set_mode(InputMode::Editing);
        assert_eq!(handler.mode(), &InputMode::Editing);

        // Enter command mode
        handler.set_mode(InputMode::Command);
        assert_eq!(handler.mode(), &InputMode::Command);

        // Back to normal
        handler.set_mode(InputMode::Normal);
        assert_eq!(handler.mode(), &InputMode::Normal);
    }

    #[test]
    fn test_default_handler_buffer() {
        let mut handler = DefaultInputHandler::new();
        handler.set_mode(InputMode::Editing);

        // Simulate typing
        let key = KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE);
        handler.handle_key(key, &InputMode::Editing);
        assert_eq!(handler.input_buffer(), "h");

        let key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        handler.handle_key(key, &InputMode::Editing);
        assert_eq!(handler.input_buffer(), "hi");

        // Backspace
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        handler.handle_key(key, &InputMode::Editing);
        assert_eq!(handler.input_buffer(), "h");

        // Clear
        handler.clear_buffer();
        assert_eq!(handler.input_buffer(), "");
    }

    #[test]
    fn test_command_history() {
        let mut handler = DefaultInputHandler::new();
        handler.add_to_history("help".to_string());
        handler.add_to_history("status".to_string());

        // Navigate up
        handler.history_up();
        assert_eq!(handler.input_buffer(), "status");

        handler.history_up();
        assert_eq!(handler.input_buffer(), "help");

        // Navigate down
        handler.history_down();
        assert_eq!(handler.input_buffer(), "status");

        handler.history_down();
        assert_eq!(handler.input_buffer(), "");
    }
}
