//! # Message Input Component
//!
//! Text input field for composing messages. Supports editing,
//! history navigation, and character limits.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::{Component, InputAction, Styles};

/// A text input component for messages
#[derive(Debug, Clone)]
pub struct MessageInput {
    /// Current input text
    buffer: String,
    /// Cursor position within buffer
    cursor: usize,
    /// Whether the input is focused
    focused: bool,
    /// Hint text shown when empty
    hint: String,
    /// Maximum character limit (None = unlimited)
    max_chars: Option<usize>,
    /// Input history for up/down navigation
    history: Vec<String>,
    /// Current position in history
    history_pos: Option<usize>,
    /// Saved buffer when navigating history
    saved_buffer: Option<String>,
    /// Prefix displayed before input (e.g., "> ")
    prefix: String,
}

impl Default for MessageInput {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageInput {
    /// Create a new message input
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            focused: false,
            hint: "Type a message...".to_string(),
            max_chars: None,
            history: Vec::new(),
            history_pos: None,
            saved_buffer: None,
            prefix: "> ".to_string(),
        }
    }

    /// Set the hint text
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    /// Set maximum character limit
    pub fn with_max_chars(mut self, max: usize) -> Self {
        self.max_chars = Some(max);
        self
    }

    /// Set the prefix
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Get the current input text
    pub fn text(&self) -> &str {
        &self.buffer
    }

    /// Get the current text and clear the buffer
    pub fn take(&mut self) -> String {
        let text = std::mem::take(&mut self.buffer);
        self.cursor = 0;
        text
    }

    /// Set the buffer content
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.buffer = text.into();
        self.cursor = self.buffer.len();
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.history_pos = None;
        self.saved_buffer = None;
    }

    /// Add text to history
    pub fn add_to_history(&mut self, text: impl Into<String>) {
        let text = text.into();
        if !text.is_empty() {
            // Don't add duplicates of the last entry
            if self.history.last() != Some(&text) {
                self.history.push(text);
            }
            // Keep history bounded
            if self.history.len() > 100 {
                self.history.remove(0);
            }
        }
    }

    /// Check if input is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Get character count
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Insert a character at cursor position
    fn insert_char(&mut self, c: char) {
        // Check max chars limit
        if let Some(max) = self.max_chars {
            if self.buffer.len() >= max {
                return;
            }
        }

        if self.cursor >= self.buffer.len() {
            self.buffer.push(c);
        } else {
            self.buffer.insert(self.cursor, c);
        }
        self.cursor += 1;
    }

    /// Delete character before cursor (backspace)
    fn delete_char_before(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    /// Delete character at cursor (delete)
    fn delete_char_at(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    /// Move cursor left
    fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right
    fn cursor_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    /// Move cursor to start
    fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end
    fn cursor_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Navigate up in history
    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_pos {
            None => {
                // Save current buffer and show last history item
                self.saved_buffer = Some(self.buffer.clone());
                self.history_pos = Some(self.history.len() - 1);
                self.buffer = self.history[self.history.len() - 1].clone();
                self.cursor = self.buffer.len();
            }
            Some(pos) if pos > 0 => {
                self.history_pos = Some(pos - 1);
                self.buffer = self.history[pos - 1].clone();
                self.cursor = self.buffer.len();
            }
            _ => {}
        }
    }

    /// Navigate down in history
    fn history_down(&mut self) {
        match self.history_pos {
            Some(pos) if pos + 1 < self.history.len() => {
                self.history_pos = Some(pos + 1);
                self.buffer = self.history[pos + 1].clone();
                self.cursor = self.buffer.len();
            }
            Some(_) => {
                // Restore saved buffer
                self.history_pos = None;
                if let Some(saved) = self.saved_buffer.take() {
                    self.buffer = saved;
                } else {
                    self.buffer.clear();
                }
                self.cursor = self.buffer.len();
            }
            None => {}
        }
    }

    /// Delete word before cursor (Ctrl+W)
    fn delete_word_before(&mut self) {
        // Find start of word
        while self.cursor > 0 && self.buffer.chars().nth(self.cursor - 1) == Some(' ') {
            self.delete_char_before();
        }
        while self.cursor > 0 && self.buffer.chars().nth(self.cursor - 1) != Some(' ') {
            self.delete_char_before();
        }
    }
}

impl Component for MessageInput {
    fn handle_key(&mut self, key: KeyEvent) -> Option<InputAction> {
        if !self.focused {
            return None;
        }

        // Handle ctrl combinations
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('w') => {
                    self.delete_word_before();
                    return Some(InputAction::None);
                }
                KeyCode::Char('a') => {
                    self.cursor_home();
                    return Some(InputAction::None);
                }
                KeyCode::Char('e') => {
                    self.cursor_end();
                    return Some(InputAction::None);
                }
                KeyCode::Char('u') => {
                    // Clear from cursor to start
                    self.buffer = self.buffer[self.cursor..].to_string();
                    self.cursor = 0;
                    return Some(InputAction::None);
                }
                KeyCode::Char('k') => {
                    // Clear from cursor to end
                    self.buffer.truncate(self.cursor);
                    return Some(InputAction::None);
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Enter => {
                if !self.buffer.is_empty() {
                    let text = self.take();
                    self.add_to_history(text.clone());
                    return Some(InputAction::Submit(text));
                }
                Some(InputAction::None)
            }
            KeyCode::Esc => {
                self.clear();
                Some(InputAction::ExitToNormal)
            }
            KeyCode::Char(c) => {
                self.insert_char(c);
                Some(InputAction::TextInput(c))
            }
            KeyCode::Backspace => {
                self.delete_char_before();
                Some(InputAction::Backspace)
            }
            KeyCode::Delete => {
                self.delete_char_at();
                Some(InputAction::None)
            }
            KeyCode::Left => {
                self.cursor_left();
                Some(InputAction::None)
            }
            KeyCode::Right => {
                self.cursor_right();
                Some(InputAction::None)
            }
            KeyCode::Home => {
                self.cursor_home();
                Some(InputAction::None)
            }
            KeyCode::End => {
                self.cursor_end();
                Some(InputAction::None)
            }
            KeyCode::Up => {
                self.history_up();
                Some(InputAction::None)
            }
            KeyCode::Down => {
                self.history_down();
                Some(InputAction::None)
            }
            _ => Some(InputAction::None),
        }
    }

    fn render(&self, f: &mut Frame<'_>, area: Rect, styles: &Styles) {
        let border_style = if self.focused {
            styles.border_focused()
        } else {
            styles.border()
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style);

        // Build display text
        let display_text = if self.buffer.is_empty() && !self.focused {
            Line::from(vec![
                Span::raw(&self.prefix),
                Span::styled(&self.hint, styles.text_muted()),
            ])
        } else {
            let mut spans = vec![Span::raw(&self.prefix)];

            if self.focused {
                // Show cursor
                let before = &self.buffer[..self.cursor];
                let cursor_char = self.buffer.chars().nth(self.cursor);
                let after = if self.cursor < self.buffer.len() {
                    &self.buffer[self.cursor + 1..]
                } else {
                    ""
                };

                spans.push(Span::styled(before, styles.text()));

                // Cursor character (or space if at end)
                let cursor_str = cursor_char
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| " ".to_string());
                spans.push(Span::styled(
                    cursor_str,
                    styles.text().add_modifier(Modifier::REVERSED),
                ));

                spans.push(Span::styled(after, styles.text()));
            } else {
                spans.push(Span::styled(&self.buffer, styles.text()));
            }

            // Show character count if limited
            if let Some(max) = self.max_chars {
                let count_str = format!(" {}/{}", self.buffer.len(), max);
                let count_style = if self.buffer.len() >= max {
                    styles.text_warning()
                } else {
                    styles.text_muted()
                };
                spans.push(Span::styled(count_str, count_style));
            }

            Line::from(spans)
        };

        let paragraph = Paragraph::new(display_text).block(block);

        f.render_widget(paragraph, area);
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            // Reset history navigation when losing focus
            self.history_pos = None;
            self.saved_buffer = None;
        }
    }

    fn min_size(&self) -> (u16, u16) {
        (20, 3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_input_new() {
        let input = MessageInput::new();
        assert!(input.is_empty());
        assert!(!input.is_focused());
    }

    #[test]
    fn test_message_input_typing() {
        let mut input = MessageInput::new();
        input.set_focused(true);

        input.insert_char('h');
        input.insert_char('i');
        assert_eq!(input.text(), "hi");
        assert_eq!(input.cursor, 2);
    }

    #[test]
    fn test_message_input_backspace() {
        let mut input = MessageInput::new();
        input.set_text("hello");

        input.delete_char_before();
        assert_eq!(input.text(), "hell");
    }

    #[test]
    fn test_message_input_cursor_movement() {
        let mut input = MessageInput::new();
        input.set_text("hello");
        assert_eq!(input.cursor, 5);

        input.cursor_left();
        assert_eq!(input.cursor, 4);

        input.cursor_home();
        assert_eq!(input.cursor, 0);

        input.cursor_end();
        assert_eq!(input.cursor, 5);
    }

    #[test]
    fn test_message_input_history() {
        let mut input = MessageInput::new();
        input.add_to_history("first");
        input.add_to_history("second");
        input.add_to_history("third");

        // Navigate up through history
        input.history_up();
        assert_eq!(input.text(), "third");

        input.history_up();
        assert_eq!(input.text(), "second");

        input.history_up();
        assert_eq!(input.text(), "first");

        // Navigate down
        input.history_down();
        assert_eq!(input.text(), "second");

        input.history_down();
        assert_eq!(input.text(), "third");

        input.history_down();
        assert!(input.is_empty()); // Back to empty
    }

    #[test]
    fn test_message_input_max_chars() {
        let mut input = MessageInput::new().with_max_chars(5);
        input.set_focused(true);

        for c in "hello world".chars() {
            input.insert_char(c);
        }

        assert_eq!(input.text(), "hello");
        assert_eq!(input.len(), 5);
    }

    #[test]
    fn test_message_input_take() {
        let mut input = MessageInput::new();
        input.set_text("message");

        let text = input.take();
        assert_eq!(text, "message");
        assert!(input.is_empty());
    }

    #[test]
    fn test_message_input_word_delete() {
        let mut input = MessageInput::new();
        input.set_text("hello world test");

        input.delete_word_before();
        assert_eq!(input.text(), "hello world ");

        input.delete_word_before();
        assert_eq!(input.text(), "hello ");
    }
}
