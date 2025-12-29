//! Terminal effect interface for deterministic TUI/CLI testing
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3) for production, `aura-testkit` (Layer 8) for testing
//! - **Usage**: `aura-terminal` for TUI and CLI testing
//!
//! This module abstracts terminal I/O to enable:
//! - Deterministic event injection for testing
//! - Frame capture for output verification
//! - State machine modeling for formal verification (Quint)
//! - Reproducible test execution
//!
//! # Architecture
//!
//! The TUI is modeled as a pure state machine:
//! ```text
//! TuiState × TerminalEvent → (TuiState, RenderOutput, Commands)
//! ```
//!
//! By abstracting terminal I/O into effect traits, tests can:
//! 1. Inject predetermined event sequences
//! 2. Capture all rendered frames
//! 3. Assert on exact output
//! 4. Replay failing scenarios deterministically

use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

// ============================================================================
// Terminal Events
// ============================================================================

/// Terminal event - the input to the TUI state machine
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalEvent {
    /// Keyboard event
    Key(KeyEvent),
    /// Mouse event
    Mouse(MouseEvent),
    /// Terminal resize event
    Resize { width: u16, height: u16 },
    /// Focus gained
    FocusGained,
    /// Focus lost
    FocusLost,
    /// Paste event (bracketed paste)
    Paste(String),
    /// Tick event for time-based updates (e.g., animations, timeouts)
    Tick,
}

/// Keyboard event
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyEvent {
    /// The key code
    pub code: KeyCode,
    /// Key modifiers (Ctrl, Alt, Shift)
    pub modifiers: Modifiers,
    /// Event kind (Press, Release, Repeat)
    pub kind: KeyEventKind,
}

impl KeyEvent {
    /// Create a simple key press event
    pub fn press(code: KeyCode) -> Self {
        Self {
            code,
            modifiers: Modifiers::NONE,
            kind: KeyEventKind::Press,
        }
    }

    /// Create a key press with modifiers
    pub fn press_with(code: KeyCode, modifiers: Modifiers) -> Self {
        Self {
            code,
            modifiers,
            kind: KeyEventKind::Press,
        }
    }

    /// Create a character key press
    pub fn char(c: char) -> Self {
        Self::press(KeyCode::Char(c))
    }

    /// Create a character key press with Ctrl modifier
    pub fn ctrl(c: char) -> Self {
        Self::press_with(KeyCode::Char(c), Modifiers::CTRL)
    }
}

/// Key code - matches crossterm/iocraft key codes
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KeyCode {
    /// Backspace key
    Backspace,
    /// Enter/Return key
    Enter,
    /// Left arrow
    Left,
    /// Right arrow
    Right,
    /// Up arrow
    Up,
    /// Down arrow
    Down,
    /// Home key
    Home,
    /// End key
    End,
    /// Page Up
    PageUp,
    /// Page Down
    PageDown,
    /// Tab key
    Tab,
    /// Backtab (Shift+Tab)
    BackTab,
    /// Delete key
    Delete,
    /// Insert key
    Insert,
    /// Function key (F1-F12)
    F(u8),
    /// Character key
    Char(char),
    /// Null character (Ctrl+Space on some terminals)
    Null,
    /// Escape key
    Esc,
    /// Caps Lock
    CapsLock,
    /// Scroll Lock
    ScrollLock,
    /// Num Lock
    NumLock,
    /// Print Screen
    PrintScreen,
    /// Pause
    Pause,
    /// Menu key
    Menu,
    /// Keypad Begin
    KeypadBegin,
}

/// Key event kind
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum KeyEventKind {
    /// Key press
    #[default]
    Press,
    /// Key release
    Release,
    /// Key repeat (held down)
    Repeat,
}

/// Key modifiers (bitflags-style)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Modifiers(u8);

impl Modifiers {
    /// No modifiers
    pub const NONE: Self = Self(0);
    /// Shift key
    pub const SHIFT: Self = Self(1 << 0);
    /// Ctrl key
    pub const CTRL: Self = Self(1 << 1);
    /// Alt key
    pub const ALT: Self = Self(1 << 2);
    /// Super/Meta/Windows key
    pub const SUPER: Self = Self(1 << 3);
    /// Hyper key
    pub const HYPER: Self = Self(1 << 4);
    /// Meta key (distinct from Super on some systems)
    pub const META: Self = Self(1 << 5);

    /// Check if shift is pressed
    pub fn shift(self) -> bool {
        self.0 & Self::SHIFT.0 != 0
    }

    /// Check if ctrl is pressed
    pub fn ctrl(self) -> bool {
        self.0 & Self::CTRL.0 != 0
    }

    /// Check if alt is pressed
    pub fn alt(self) -> bool {
        self.0 & Self::ALT.0 != 0
    }

    /// Check if super/command key is pressed
    pub fn super_key(self) -> bool {
        self.0 & Self::SUPER.0 != 0
    }

    /// Combine modifiers
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Mouse event
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MouseEvent {
    /// Mouse button or action
    pub kind: MouseEventKind,
    /// Column (x coordinate)
    pub column: u16,
    /// Row (y coordinate)
    pub row: u16,
    /// Key modifiers held during mouse event
    pub modifiers: Modifiers,
}

/// Mouse event kind
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseEventKind {
    /// Mouse button pressed
    Down(MouseButton),
    /// Mouse button released
    Up(MouseButton),
    /// Mouse moved while button held (drag)
    Drag(MouseButton),
    /// Mouse moved (no button held)
    Moved,
    /// Scroll up
    ScrollUp,
    /// Scroll down
    ScrollDown,
    /// Scroll left
    ScrollLeft,
    /// Scroll right
    ScrollRight,
}

/// Mouse button
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MouseButton {
    /// Left mouse button
    Left,
    /// Right mouse button
    Right,
    /// Middle mouse button
    Middle,
}

// ============================================================================
// Terminal Frame (Output Capture)
// ============================================================================

/// A captured terminal frame - the output of a render cycle
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalFrame {
    /// Frame width in columns
    pub width: u16,
    /// Frame height in rows
    pub height: u16,
    /// Cell buffer (row-major: cells[row][col])
    pub cells: Vec<Vec<Cell>>,
    /// Cursor position (if visible)
    pub cursor: Option<CursorPosition>,
    /// Frame sequence number (for ordering)
    pub sequence: u64,
}

impl TerminalFrame {
    /// Create a new empty frame
    pub fn new(width: u16, height: u16) -> Self {
        let cells = (0..height)
            .map(|_| (0..width).map(|_| Cell::default()).collect())
            .collect();
        Self {
            width,
            height,
            cells,
            cursor: None,
            sequence: 0,
        }
    }

    /// Get a cell at (column, row)
    pub fn get(&self, col: u16, row: u16) -> Option<&Cell> {
        self.cells.get(row as usize)?.get(col as usize)
    }

    /// Set a cell at (column, row)
    pub fn set(&mut self, col: u16, row: u16, cell: Cell) {
        if let Some(row_cells) = self.cells.get_mut(row as usize) {
            if let Some(existing) = row_cells.get_mut(col as usize) {
                *existing = cell;
            }
        }
    }

    /// Extract text content from a row
    pub fn row_text(&self, row: u16) -> String {
        self.cells
            .get(row as usize)
            .map(|cells| cells.iter().map(|c| c.char).collect())
            .unwrap_or_default()
    }

    /// Extract all text content (rows joined by newlines)
    pub fn text(&self) -> String {
        self.cells
            .iter()
            .map(|row| row.iter().map(|c| c.char).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Check if frame contains text anywhere
    pub fn contains(&self, text: &str) -> bool {
        // Check each row
        for row in &self.cells {
            let row_text: String = row.iter().map(|c| c.char).collect();
            if row_text.contains(text) {
                return true;
            }
        }
        // Also check full text (for multi-line)
        self.text().contains(text)
    }

    /// Find position of text in frame
    pub fn find(&self, text: &str) -> Option<(u16, u16)> {
        for (row_idx, row) in self.cells.iter().enumerate() {
            let row_text: String = row.iter().map(|c| c.char).collect();
            if let Some(col_idx) = row_text.find(text) {
                return Some((col_idx as u16, row_idx as u16));
            }
        }
        None
    }
}

impl fmt::Display for TerminalFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for row in &self.cells {
            for cell in row {
                write!(f, "{}", cell.char)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

/// A single cell in the terminal
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cell {
    /// The character displayed
    pub char: char,
    /// Foreground color
    pub fg: Color,
    /// Background color
    pub bg: Color,
    /// Text style modifiers
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            char: ' ',
            fg: Color::Reset,
            bg: Color::Reset,
            style: Style::empty(),
        }
    }
}

impl Cell {
    /// Create a cell with a character
    pub fn new(c: char) -> Self {
        Self {
            char: c,
            ..Default::default()
        }
    }

    /// Create a cell with character and foreground color
    #[must_use]
    pub fn with_fg(c: char, fg: Color) -> Self {
        Self {
            char: c,
            fg,
            ..Default::default()
        }
    }
}

/// Terminal colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Color {
    /// Reset to default
    #[default]
    Reset,
    /// Black
    Black,
    /// Dark red
    DarkRed,
    /// Dark green
    DarkGreen,
    /// Dark yellow
    DarkYellow,
    /// Dark blue
    DarkBlue,
    /// Dark magenta
    DarkMagenta,
    /// Dark cyan
    DarkCyan,
    /// Gray
    Gray,
    /// Dark gray
    DarkGray,
    /// Red
    Red,
    /// Green
    Green,
    /// Yellow
    Yellow,
    /// Blue
    Blue,
    /// Magenta
    Magenta,
    /// Cyan
    Cyan,
    /// White
    White,
    /// 256-color palette
    Indexed(u8),
    /// True color RGB
    Rgb { r: u8, g: u8, b: u8 },
}

/// Text style modifiers (bitflags-style)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Style(u16);

impl Style {
    /// No style
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Bold text
    pub const BOLD: Self = Self(1 << 0);
    /// Dim text
    pub const DIM: Self = Self(1 << 1);
    /// Italic text
    pub const ITALIC: Self = Self(1 << 2);
    /// Underlined text
    pub const UNDERLINED: Self = Self(1 << 3);
    /// Slow blink
    pub const SLOW_BLINK: Self = Self(1 << 4);
    /// Rapid blink
    pub const RAPID_BLINK: Self = Self(1 << 5);
    /// Reversed colors
    pub const REVERSED: Self = Self(1 << 6);
    /// Hidden text
    pub const HIDDEN: Self = Self(1 << 7);
    /// Crossed out text
    pub const CROSSED_OUT: Self = Self(1 << 8);

    /// Check if bold
    pub fn is_bold(self) -> bool {
        self.0 & Self::BOLD.0 != 0
    }

    /// Check if underlined
    pub fn is_underlined(self) -> bool {
        self.0 & Self::UNDERLINED.0 != 0
    }

    /// Combine styles
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for Style {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Cursor position and state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CursorPosition {
    /// Column (x)
    pub col: u16,
    /// Row (y)
    pub row: u16,
    /// Cursor shape
    pub shape: CursorShape,
    /// Whether cursor is blinking
    pub blinking: bool,
}

/// Cursor shape
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum CursorShape {
    /// Block cursor (full cell)
    #[default]
    Block,
    /// Underline cursor
    Underline,
    /// Bar/line cursor (vertical line)
    Bar,
}

// ============================================================================
// Terminal Error
// ============================================================================

/// Terminal operation error
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalError {
    /// No more events available (for testing)
    EndOfInput,
    /// Terminal I/O error
    IoError(String),
    /// Terminal not available (headless mode without mock)
    NotAvailable,
    /// Invalid operation for current state
    InvalidOperation(String),
    /// Timeout waiting for event
    Timeout,
}

impl fmt::Display for TerminalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EndOfInput => write!(f, "end of input"),
            Self::IoError(msg) => write!(f, "terminal I/O error: {}", msg),
            Self::NotAvailable => write!(f, "terminal not available"),
            Self::InvalidOperation(msg) => write!(f, "invalid operation: {}", msg),
            Self::Timeout => write!(f, "timeout waiting for terminal event"),
        }
    }
}

impl std::error::Error for TerminalError {}

impl From<TerminalError> for AuraError {
    fn from(err: TerminalError) -> Self {
        AuraError::Terminal(err.to_string())
    }
}

// ============================================================================
// Terminal Effect Traits
// ============================================================================

/// Terminal input effects - abstracts reading events from terminal
///
/// # Implementations
///
/// - **Production**: Wraps iocraft/crossterm terminal event stream
/// - **Testing**: `MockTerminalHandler` with predetermined event queue
/// - **Simulation**: Seeded random event generation
#[async_trait]
pub trait TerminalInputEffects: Send + Sync {
    /// Get the next terminal event, blocking until available
    ///
    /// Returns `Err(TerminalError::EndOfInput)` when no more events
    /// are available (useful for testing with finite event sequences).
    async fn next_event(&self) -> Result<TerminalEvent, TerminalError>;

    /// Poll for an event with timeout
    ///
    /// Returns `Ok(None)` if no event within timeout.
    /// Returns `Ok(Some(event))` if event received.
    /// Returns `Err(...)` on error.
    async fn poll_event(&self, timeout_ms: u64) -> Result<Option<TerminalEvent>, TerminalError>;

    /// Check if input is available without blocking
    async fn has_input(&self) -> bool;
}

/// Terminal output effects - abstracts rendering to terminal
///
/// # Implementations
///
/// - **Production**: Wraps iocraft terminal rendering
/// - **Testing**: `MockTerminalHandler` captures frames for assertion
/// - **Simulation**: Tracks render calls without actual output
#[async_trait]
pub trait TerminalOutputEffects: Send + Sync {
    /// Render a frame to the terminal
    ///
    /// In production, this writes to the real terminal.
    /// In testing, this captures the frame for later assertion.
    async fn render(&self, frame: TerminalFrame) -> Result<(), TerminalError>;

    /// Get terminal dimensions
    async fn size(&self) -> Result<(u16, u16), TerminalError>;

    /// Clear the terminal
    async fn clear(&self) -> Result<(), TerminalError>;

    /// Set cursor position
    async fn set_cursor(&self, col: u16, row: u16) -> Result<(), TerminalError>;

    /// Show or hide cursor
    async fn set_cursor_visible(&self, visible: bool) -> Result<(), TerminalError>;

    /// Set cursor shape
    async fn set_cursor_shape(&self, shape: CursorShape) -> Result<(), TerminalError>;

    /// Enter alternate screen buffer
    async fn enter_alternate_screen(&self) -> Result<(), TerminalError>;

    /// Leave alternate screen buffer
    async fn leave_alternate_screen(&self) -> Result<(), TerminalError>;

    /// Enable raw mode (no line buffering, no echo)
    async fn enable_raw_mode(&self) -> Result<(), TerminalError>;

    /// Disable raw mode
    async fn disable_raw_mode(&self) -> Result<(), TerminalError>;
}

/// Combined terminal effects
pub trait TerminalEffects: TerminalInputEffects + TerminalOutputEffects {}

/// Blanket implementation for types that implement both traits
impl<T: TerminalInputEffects + TerminalOutputEffects> TerminalEffects for T {}

// ============================================================================
// Blanket Implementations for Arc<T>
// ============================================================================

#[async_trait]
impl<T: TerminalInputEffects + ?Sized> TerminalInputEffects for std::sync::Arc<T> {
    async fn next_event(&self) -> Result<TerminalEvent, TerminalError> {
        (**self).next_event().await
    }

    async fn poll_event(&self, timeout_ms: u64) -> Result<Option<TerminalEvent>, TerminalError> {
        (**self).poll_event(timeout_ms).await
    }

    async fn has_input(&self) -> bool {
        (**self).has_input().await
    }
}

#[async_trait]
impl<T: TerminalOutputEffects + ?Sized> TerminalOutputEffects for std::sync::Arc<T> {
    async fn render(&self, frame: TerminalFrame) -> Result<(), TerminalError> {
        (**self).render(frame).await
    }

    async fn size(&self) -> Result<(u16, u16), TerminalError> {
        (**self).size().await
    }

    async fn clear(&self) -> Result<(), TerminalError> {
        (**self).clear().await
    }

    async fn set_cursor(&self, col: u16, row: u16) -> Result<(), TerminalError> {
        (**self).set_cursor(col, row).await
    }

    async fn set_cursor_visible(&self, visible: bool) -> Result<(), TerminalError> {
        (**self).set_cursor_visible(visible).await
    }

    async fn set_cursor_shape(&self, shape: CursorShape) -> Result<(), TerminalError> {
        (**self).set_cursor_shape(shape).await
    }

    async fn enter_alternate_screen(&self) -> Result<(), TerminalError> {
        (**self).enter_alternate_screen().await
    }

    async fn leave_alternate_screen(&self) -> Result<(), TerminalError> {
        (**self).leave_alternate_screen().await
    }

    async fn enable_raw_mode(&self) -> Result<(), TerminalError> {
        (**self).enable_raw_mode().await
    }

    async fn disable_raw_mode(&self) -> Result<(), TerminalError> {
        (**self).disable_raw_mode().await
    }
}

// ============================================================================
// Event Builder Helpers (for testing convenience)
// ============================================================================

/// Helper functions for constructing terminal events in tests
pub mod events {
    use super::*;

    /// Create a key press event for a character
    pub fn char(c: char) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::char(c))
    }

    /// Create a key press event
    pub fn key(code: KeyCode) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::press(code))
    }

    /// Create an Enter key press
    pub fn enter() -> TerminalEvent {
        key(KeyCode::Enter)
    }

    /// Create an Escape key press
    pub fn escape() -> TerminalEvent {
        key(KeyCode::Esc)
    }

    /// Create a Tab key press
    pub fn tab() -> TerminalEvent {
        key(KeyCode::Tab)
    }

    /// Create a Backspace key press
    pub fn backspace() -> TerminalEvent {
        key(KeyCode::Backspace)
    }

    /// Create an arrow key press
    pub fn arrow_up() -> TerminalEvent {
        key(KeyCode::Up)
    }

    /// Create an arrow key press
    pub fn arrow_down() -> TerminalEvent {
        key(KeyCode::Down)
    }

    /// Create an arrow key press
    pub fn arrow_left() -> TerminalEvent {
        key(KeyCode::Left)
    }

    /// Create an arrow key press
    pub fn arrow_right() -> TerminalEvent {
        key(KeyCode::Right)
    }

    /// Create a Ctrl+key press
    pub fn ctrl(c: char) -> TerminalEvent {
        TerminalEvent::Key(KeyEvent::ctrl(c))
    }

    /// Create a function key press (F1-F12)
    pub fn function(n: u8) -> TerminalEvent {
        key(KeyCode::F(n))
    }

    /// Create a resize event
    pub fn resize(width: u16, height: u16) -> TerminalEvent {
        TerminalEvent::Resize { width, height }
    }

    /// Create a tick event
    pub fn tick() -> TerminalEvent {
        TerminalEvent::Tick
    }

    /// Type a string as a sequence of character events
    pub fn type_str(s: &str) -> Vec<TerminalEvent> {
        s.chars().map(char).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_event_creation() {
        let event = KeyEvent::char('a');
        assert_eq!(event.code, KeyCode::Char('a'));
        assert_eq!(event.modifiers, Modifiers::NONE);
        assert_eq!(event.kind, KeyEventKind::Press);
    }

    #[test]
    fn test_ctrl_key() {
        let event = KeyEvent::ctrl('c');
        assert_eq!(event.code, KeyCode::Char('c'));
        assert!(event.modifiers.ctrl());
    }

    #[test]
    fn test_frame_contains() {
        let mut frame = TerminalFrame::new(20, 5);
        // Write "Hello" to row 1
        for (i, c) in "Hello".chars().enumerate() {
            frame.set(i as u16, 1, Cell::new(c));
        }

        assert!(frame.contains("Hello"));
        assert!(frame.contains("ell"));
        assert!(!frame.contains("World"));
    }

    #[test]
    fn test_frame_find() {
        let mut frame = TerminalFrame::new(20, 5);
        // Write "Test" to row 2, column 5
        for (i, c) in "Test".chars().enumerate() {
            frame.set(5 + i as u16, 2, Cell::new(c));
        }

        assert_eq!(frame.find("Test"), Some((5, 2)));
        assert_eq!(frame.find("Missing"), None);
    }

    #[test]
    fn test_event_helpers() {
        use events::*;

        let events = [char('h'), char('i'), enter(), escape()];

        assert_eq!(events.len(), 4);

        let typed = type_str("hello");
        assert_eq!(typed.len(), 5);
    }

    #[test]
    fn test_modifiers() {
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        assert!(ctrl_shift.ctrl());
        assert!(ctrl_shift.shift());
        assert!(!ctrl_shift.alt());
    }
}
