//! # Textarea Component
//!
//! Multi-line text input with cursor positioning.

use iocraft::prelude::*;

use crate::tui::theme::Theme;

/// Props for Textarea
#[derive(Default, Props)]
pub struct TextareaProps {
    /// Current text content (can include newlines)
    pub value: String,
    /// Placeholder text when empty
    pub placeholder: String,
    /// Cursor row position (0-indexed)
    pub cursor_row: usize,
    /// Cursor column position (0-indexed)
    pub cursor_col: usize,
    /// Whether the textarea has focus
    pub focused: bool,
    /// Minimum height in lines
    pub min_height: usize,
    /// Maximum height in lines (0 = unlimited)
    pub max_height: usize,
}

/// A multi-line text input area
///
/// State management handled by parent component.
#[component]
pub fn Textarea(props: &TextareaProps) -> impl Into<AnyElement<'static>> {
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    let is_empty = props.value.is_empty();
    let display_lines: Vec<String> = if is_empty {
        vec![props.placeholder.clone()]
    } else {
        props.value.lines().map(String::from).collect()
    };

    let text_color = if is_empty {
        Theme::TEXT_MUTED
    } else {
        Theme::TEXT
    };

    let min_h = if props.min_height > 0 {
        props.min_height
    } else {
        3
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
            padding: 1,
            min_height: min_h as u32,
        ) {
            #(display_lines.into_iter().map(|line| {
                element! {
                    Text(content: line, color: text_color)
                }
            }))
        }
    }
}

/// State helper for textarea editing
#[derive(Clone, Debug, Default)]
pub struct TextareaState {
    /// The text content
    pub text: String,
    /// Cursor row (0-indexed)
    pub row: usize,
    /// Cursor column (0-indexed)
    pub col: usize,
}

impl TextareaState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            row: 0,
            col: 0,
        }
    }

    /// Get lines as a vector
    #[must_use]
    pub fn lines(&self) -> Vec<&str> {
        self.text.lines().collect()
    }

    /// Get current line content
    #[must_use]
    pub fn current_line(&self) -> Option<&str> {
        self.lines().get(self.row).copied()
    }

    /// Insert a character at cursor position
    pub fn insert_char(&mut self, c: char) {
        let byte_pos = self.cursor_byte_position();
        self.text.insert(byte_pos, c);
        if c == '\n' {
            self.row += 1;
            self.col = 0;
        } else {
            self.col += 1;
        }
    }

    /// Delete character before cursor (backspace)
    pub fn backspace(&mut self) {
        if self.col > 0 {
            let byte_pos = self.cursor_byte_position();
            if byte_pos > 0 {
                self.text.remove(byte_pos - 1);
                self.col -= 1;
            }
        } else if self.row > 0 {
            // Join with previous line
            let byte_pos = self.cursor_byte_position();
            if byte_pos > 0 {
                let prev_line_len = self.lines().get(self.row - 1).map(|l| l.len()).unwrap_or(0);
                self.text.remove(byte_pos - 1); // Remove newline
                self.row -= 1;
                self.col = prev_line_len;
            }
        }
    }

    /// Move cursor up
    pub fn move_up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            // Clamp col to line length
            let line_len = self.current_line().map(|l| l.len()).unwrap_or(0);
            self.col = self.col.min(line_len);
        }
    }

    /// Move cursor down
    pub fn move_down(&mut self) {
        let line_count = self.lines().len();
        if self.row + 1 < line_count {
            self.row += 1;
            // Clamp col to line length
            let line_len = self.current_line().map(|l| l.len()).unwrap_or(0);
            self.col = self.col.min(line_len);
        }
    }

    /// Move cursor left
    pub fn move_left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.current_line().map(|l| l.len()).unwrap_or(0);
        }
    }

    /// Move cursor right
    pub fn move_right(&mut self) {
        let line_len = self.current_line().map(|l| l.len()).unwrap_or(0);
        if self.col < line_len {
            self.col += 1;
        } else if self.row + 1 < self.lines().len() {
            self.row += 1;
            self.col = 0;
        }
    }

    /// Calculate byte position of cursor
    fn cursor_byte_position(&self) -> usize {
        let mut pos = 0;
        for (i, line) in self.text.lines().enumerate() {
            if i == self.row {
                pos += self.col.min(line.len());
                break;
            }
            pos += line.len() + 1; // +1 for newline
        }
        pos.min(self.text.len())
    }

    /// Clear all text
    pub fn clear(&mut self) {
        self.text.clear();
        self.row = 0;
        self.col = 0;
    }
}
