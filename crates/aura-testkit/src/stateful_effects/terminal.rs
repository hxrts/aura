//! Mock terminal effect handlers for deterministic TUI/CLI testing
//!
//! This module provides `MockTerminalHandler` for testing TUI components deterministically.
//! It implements the terminal effect traits from aura-core, allowing tests to:
//! - Inject predetermined event sequences
//! - Capture all rendered frames for assertion
//! - Verify TUI behavior without real terminal I/O
//!
//! ## Example
//!
//! ```rust,ignore
//! use aura_testkit::stateful_effects::MockTerminalHandler;
//! use aura_core::effects::terminal::{events, TerminalEvent};
//!
//! #[tokio::test]
//! async fn test_tui_navigation() {
//!     // Create handler with predetermined events
//!     let events = vec![
//!         events::char('1'),  // Navigate to screen 1
//!         events::enter(),    // Confirm
//!         events::escape(),   // Exit
//!     ];
//!     let terminal = MockTerminalHandler::with_events(events);
//!
//!     // Run TUI with mock terminal
//!     run_tui(&terminal).await;
//!
//!     // Assert on captured frames
//!     assert!(terminal.frame_contains("Block"));
//!     assert_eq!(terminal.frame_count(), 3);
//! }
//! ```

use async_trait::async_trait;
use aura_core::effects::terminal::{
    Cell, Color, CursorPosition, CursorShape, TerminalError, TerminalEvent, TerminalFrame,
    TerminalInputEffects, TerminalOutputEffects,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Type alias for event generator function
type EventGenerator = Box<dyn FnMut() -> Option<TerminalEvent> + Send>;

/// Internal state for the mock terminal handler
struct MockTerminalState {
    /// Queue of events to be consumed by `next_event()`
    event_queue: VecDeque<TerminalEvent>,

    /// All frames captured via `render()`
    captured_frames: Vec<TerminalFrame>,

    /// Virtual terminal dimensions
    size: (u16, u16),

    /// Current cursor position
    cursor: CursorPosition,

    /// Whether the terminal is in raw mode
    raw_mode: bool,

    /// Whether the cursor is visible
    cursor_visible: bool,

    /// Whether we're in alternate screen mode
    alternate_screen: bool,

    /// Optional event generator for infinite sequences (e.g., fuzzing)
    event_generator: Option<EventGenerator>,
}

impl std::fmt::Debug for MockTerminalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockTerminalState")
            .field("event_queue", &self.event_queue)
            .field("captured_frames", &self.captured_frames.len())
            .field("size", &self.size)
            .field("cursor", &self.cursor)
            .field("raw_mode", &self.raw_mode)
            .field("cursor_visible", &self.cursor_visible)
            .field("alternate_screen", &self.alternate_screen)
            .field(
                "event_generator",
                &self.event_generator.as_ref().map(|_| "..."),
            )
            .finish()
    }
}

impl Default for MockTerminalState {
    fn default() -> Self {
        Self {
            event_queue: VecDeque::new(),
            captured_frames: Vec::new(),
            size: (80, 24), // Standard terminal size
            cursor: CursorPosition {
                col: 0,
                row: 0,
                shape: CursorShape::Block,
                blinking: false,
            },
            raw_mode: false,
            cursor_visible: true,
            alternate_screen: false,
            event_generator: None,
        }
    }
}

/// Mock terminal handler for deterministic TUI testing
///
/// This handler captures all terminal output (frames) and injects predetermined
/// input events, enabling fully deterministic testing of TUI components.
///
/// ## Features
///
/// - **Event injection**: Provide a sequence of events to be consumed
/// - **Frame capture**: All rendered frames are captured for assertion
/// - **Configurable size**: Set custom terminal dimensions
/// - **Generator support**: Use a function to generate events (for fuzzing/generative testing)
///
/// ## Thread Safety
///
/// The handler uses `Arc<Mutex<>>` internally and can be cloned to share state
/// between the test and the TUI runtime.
#[derive(Debug, Clone)]
pub struct MockTerminalHandler {
    state: Arc<Mutex<MockTerminalState>>,
}

impl Default for MockTerminalHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl MockTerminalHandler {
    /// Create a new empty mock terminal handler
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MockTerminalState::default())),
        }
    }

    /// Create a handler with a predetermined event sequence
    ///
    /// Events will be consumed in order by `next_event()`. When all events
    /// are consumed, `next_event()` returns `TerminalError::EndOfInput`.
    pub fn with_events(events: Vec<TerminalEvent>) -> Self {
        let state = MockTerminalState {
            event_queue: events.into(),
            ..Default::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Create a handler with custom terminal size
    pub fn with_size(width: u16, height: u16) -> Self {
        let state = MockTerminalState {
            size: (width, height),
            ..Default::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Create a handler with events and custom size
    pub fn with_events_and_size(events: Vec<TerminalEvent>, width: u16, height: u16) -> Self {
        let state = MockTerminalState {
            event_queue: events.into(),
            size: (width, height),
            ..Default::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Create a handler with an event generator function
    ///
    /// The generator is called when the event queue is empty. Return `None`
    /// to signal end of input.
    ///
    /// Useful for generative testing and fuzzing.
    pub fn with_generator<F>(generator: F) -> Self
    where
        F: FnMut() -> Option<TerminalEvent> + Send + 'static,
    {
        let state = MockTerminalState {
            event_generator: Some(Box::new(generator)),
            ..Default::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    // ==================== Query Methods ====================

    /// Get all captured frames
    pub fn captured_frames(&self) -> Vec<TerminalFrame> {
        self.state.lock().unwrap().captured_frames.clone()
    }

    /// Get the number of captured frames
    pub fn frame_count(&self) -> usize {
        self.state.lock().unwrap().captured_frames.len()
    }

    /// Get the last captured frame (if any)
    pub fn last_frame(&self) -> Option<TerminalFrame> {
        self.state.lock().unwrap().captured_frames.last().cloned()
    }

    /// Get a specific frame by index
    pub fn frame_at(&self, index: usize) -> Option<TerminalFrame> {
        self.state
            .lock()
            .unwrap()
            .captured_frames
            .get(index)
            .cloned()
    }

    /// Get number of remaining events in queue
    pub fn remaining_events(&self) -> usize {
        self.state.lock().unwrap().event_queue.len()
    }

    /// Check if all events have been consumed
    pub fn all_events_consumed(&self) -> bool {
        let state = self.state.lock().unwrap();
        state.event_queue.is_empty() && state.event_generator.is_none()
    }

    // ==================== Assertion Methods ====================

    /// Check if the last frame contains the given text
    ///
    /// Searches all cells in the frame for the text string.
    pub fn frame_contains(&self, text: &str) -> bool {
        if let Some(frame) = self.last_frame() {
            frame.contains(text)
        } else {
            false
        }
    }

    /// Check if any captured frame contains the given text
    pub fn any_frame_contains(&self, text: &str) -> bool {
        let frames = self.captured_frames();
        frames.iter().any(|frame| frame.contains(text))
    }

    /// Assert that the last frame contains the given text
    ///
    /// Panics with a descriptive message if assertion fails.
    pub fn assert_frame_contains(&self, text: &str) {
        assert!(
            self.frame_contains(text),
            "Expected last frame to contain '{}', but it was not found.\nFrame content:\n{}",
            text,
            self.last_frame()
                .map(|f| f.to_string())
                .unwrap_or_else(|| "No frames captured".to_string())
        );
    }

    /// Assert that a specific frame contains the given text
    pub fn assert_frame_at_contains(&self, index: usize, text: &str) {
        if let Some(frame) = self.frame_at(index) {
            assert!(
                frame.contains(text),
                "Expected frame {} to contain '{}', but it was not found.\nFrame content:\n{}",
                index,
                text,
                frame
            );
        } else {
            panic!(
                "No frame at index {}. Total frames: {}",
                index,
                self.frame_count()
            );
        }
    }

    /// Assert that the frame count matches expected
    pub fn assert_frame_count(&self, expected: usize) {
        let actual = self.frame_count();
        assert_eq!(
            actual, expected,
            "Expected {} frames, but got {}",
            expected, actual
        );
    }

    /// Assert using a custom predicate on the last frame
    pub fn assert_frame<F>(&self, predicate: F)
    where
        F: FnOnce(&TerminalFrame) -> bool,
    {
        if let Some(frame) = self.last_frame() {
            assert!(
                predicate(&frame),
                "Frame assertion failed.\nFrame content:\n{}",
                frame
            );
        } else {
            panic!("No frames captured");
        }
    }

    // ==================== Mutation Methods ====================

    /// Push additional events to the queue
    pub fn push_events(&self, events: impl IntoIterator<Item = TerminalEvent>) {
        let mut state = self.state.lock().unwrap();
        state.event_queue.extend(events);
    }

    /// Push a single event to the queue
    pub fn push_event(&self, event: TerminalEvent) {
        self.state.lock().unwrap().event_queue.push_back(event);
    }

    /// Clear all captured frames
    pub fn clear_frames(&self) {
        self.state.lock().unwrap().captured_frames.clear();
    }

    /// Set the terminal size
    pub fn set_size(&self, width: u16, height: u16) {
        self.state.lock().unwrap().size = (width, height);
    }

    /// Reset the handler to initial state
    pub fn reset(&self) {
        let mut state = self.state.lock().unwrap();
        state.event_queue.clear();
        state.captured_frames.clear();
        state.cursor = CursorPosition {
            col: 0,
            row: 0,
            shape: CursorShape::Block,
            blinking: false,
        };
        state.raw_mode = false;
        state.cursor_visible = true;
        state.alternate_screen = false;
    }
}

// ==================== Trait Implementations ====================

#[async_trait]
impl TerminalInputEffects for MockTerminalHandler {
    async fn next_event(&self) -> Result<TerminalEvent, TerminalError> {
        let mut state = self.state.lock().unwrap();

        // Try queue first
        if let Some(event) = state.event_queue.pop_front() {
            return Ok(event);
        }

        // Try generator
        if let Some(ref mut gen) = state.event_generator {
            if let Some(event) = gen() {
                return Ok(event);
            }
        }

        // No more events - signal end of input
        Err(TerminalError::EndOfInput)
    }

    async fn poll_event(&self, _timeout_ms: u64) -> Result<Option<TerminalEvent>, TerminalError> {
        let mut state = self.state.lock().unwrap();

        // In mock mode, we don't actually wait - just check if event available
        if let Some(event) = state.event_queue.pop_front() {
            return Ok(Some(event));
        }

        // Try generator
        if let Some(ref mut gen) = state.event_generator {
            if let Some(event) = gen() {
                return Ok(Some(event));
            }
        }

        Ok(None)
    }

    async fn has_input(&self) -> bool {
        let state = self.state.lock().unwrap();
        !state.event_queue.is_empty() || state.event_generator.is_some()
    }
}

#[async_trait]
impl TerminalOutputEffects for MockTerminalHandler {
    async fn render(&self, frame: TerminalFrame) -> Result<(), TerminalError> {
        self.state.lock().unwrap().captured_frames.push(frame);
        Ok(())
    }

    async fn size(&self) -> Result<(u16, u16), TerminalError> {
        Ok(self.state.lock().unwrap().size)
    }

    async fn clear(&self) -> Result<(), TerminalError> {
        // In mock mode, clear doesn't affect captured frames
        // (tests want to see all frames including those before clear)
        Ok(())
    }

    async fn set_cursor(&self, col: u16, row: u16) -> Result<(), TerminalError> {
        let mut state = self.state.lock().unwrap();
        state.cursor.col = col;
        state.cursor.row = row;
        Ok(())
    }

    async fn set_cursor_visible(&self, visible: bool) -> Result<(), TerminalError> {
        self.state.lock().unwrap().cursor_visible = visible;
        Ok(())
    }

    async fn set_cursor_shape(&self, shape: CursorShape) -> Result<(), TerminalError> {
        self.state.lock().unwrap().cursor.shape = shape;
        Ok(())
    }

    async fn enter_alternate_screen(&self) -> Result<(), TerminalError> {
        self.state.lock().unwrap().alternate_screen = true;
        Ok(())
    }

    async fn leave_alternate_screen(&self) -> Result<(), TerminalError> {
        self.state.lock().unwrap().alternate_screen = false;
        Ok(())
    }

    async fn enable_raw_mode(&self) -> Result<(), TerminalError> {
        self.state.lock().unwrap().raw_mode = true;
        Ok(())
    }

    async fn disable_raw_mode(&self) -> Result<(), TerminalError> {
        self.state.lock().unwrap().raw_mode = false;
        Ok(())
    }
}

// ==================== Helper Functions ====================

/// Extract all text content from a frame as a single string
pub fn frame_to_string(frame: &TerminalFrame) -> String {
    frame.to_string()
}

/// Check if a frame contains a given text string
pub fn frame_contains_text(frame: &TerminalFrame, text: &str) -> bool {
    frame.contains(text)
}

/// Find the position of text in a frame (returns col, row of first match)
pub fn find_text_in_frame(frame: &TerminalFrame, text: &str) -> Option<(u16, u16)> {
    frame.find(text)
}

/// Get a specific row from the frame as a string
pub fn get_frame_row(frame: &TerminalFrame, row: u16) -> String {
    frame.row_text(row)
}

/// Count occurrences of a character in the frame
pub fn count_char_in_frame(frame: &TerminalFrame, ch: char) -> usize {
    frame
        .cells
        .iter()
        .flat_map(|row| row.iter())
        .filter(|cell| cell.char == ch)
        .count()
}

/// Check if a cell at a specific position has a given foreground color
pub fn cell_has_fg_color(frame: &TerminalFrame, row: u16, col: u16, color: Color) -> bool {
    frame
        .get(col, row)
        .map(|cell| cell.fg == color)
        .unwrap_or(false)
}

/// Create an empty frame with the given dimensions
pub fn empty_frame(width: u16, height: u16) -> TerminalFrame {
    TerminalFrame::new(width, height)
}

/// Create a frame from a string (each line is a row)
///
/// Useful for creating expected frames in tests.
pub fn frame_from_string(content: &str, width: u16, height: u16) -> TerminalFrame {
    let mut frame = TerminalFrame::new(width, height);

    for (row_idx, line) in content.lines().enumerate() {
        if row_idx >= height as usize {
            break;
        }
        for (col_idx, ch) in line.chars().enumerate() {
            if col_idx >= width as usize {
                break;
            }
            frame.set(col_idx as u16, row_idx as u16, Cell::new(ch));
        }
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::terminal::{events, KeyCode};

    #[tokio::test]
    async fn test_event_consumption() {
        let handler = MockTerminalHandler::with_events(vec![
            events::char('a'),
            events::char('b'),
            events::enter(),
        ]);

        // Consume all events
        let e1 = handler.next_event().await.unwrap();
        assert!(matches!(e1, TerminalEvent::Key(k) if k.code == KeyCode::Char('a')));

        let e2 = handler.next_event().await.unwrap();
        assert!(matches!(e2, TerminalEvent::Key(k) if k.code == KeyCode::Char('b')));

        let e3 = handler.next_event().await.unwrap();
        assert!(matches!(e3, TerminalEvent::Key(k) if k.code == KeyCode::Enter));

        // Should get EndOfInput
        let result = handler.next_event().await;
        assert!(matches!(result, Err(TerminalError::EndOfInput)));
    }

    #[tokio::test]
    async fn test_frame_capture() {
        let handler = MockTerminalHandler::new();

        // Capture some frames
        let frame1 = frame_from_string("Hello", 80, 24);
        let frame2 = frame_from_string("World", 80, 24);

        handler.render(frame1).await.unwrap();
        handler.render(frame2).await.unwrap();

        assert_eq!(handler.frame_count(), 2);
        assert!(handler.frame_contains("World"));
        assert!(handler.any_frame_contains("Hello"));
    }

    #[tokio::test]
    async fn test_frame_text_search() {
        let handler = MockTerminalHandler::new();

        let frame = frame_from_string("Block Screen\nPress 1 for home", 80, 24);
        handler.render(frame).await.unwrap();

        assert!(handler.frame_contains("Block"));
        assert!(handler.frame_contains("Press 1"));
        assert!(!handler.frame_contains("Chat"));
    }

    #[tokio::test]
    async fn test_size_configuration() {
        let handler = MockTerminalHandler::with_size(120, 40);

        let (w, h) = handler.size().await.unwrap();
        assert_eq!(w, 120);
        assert_eq!(h, 40);
    }

    #[tokio::test]
    async fn test_push_events() {
        let handler = MockTerminalHandler::new();

        handler.push_event(events::char('x'));
        handler.push_events(vec![events::enter(), events::escape()]);

        assert_eq!(handler.remaining_events(), 3);
    }

    #[tokio::test]
    async fn test_poll_event() {
        let handler = MockTerminalHandler::with_events(vec![events::char('a')]);

        // Poll should return event
        let result = handler.poll_event(100).await.unwrap();
        assert!(result.is_some());

        // Poll should return None when empty
        let result = handler.poll_event(100).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_generator() {
        let mut counter = 0;
        let handler = MockTerminalHandler::with_generator(move || {
            counter += 1;
            if counter <= 3 {
                Some(events::char((b'a' + counter as u8 - 1) as char))
            } else {
                None
            }
        });

        // Should generate a, b, c
        let e1 = handler.next_event().await.unwrap();
        assert!(matches!(e1, TerminalEvent::Key(k) if k.code == KeyCode::Char('a')));

        let e2 = handler.next_event().await.unwrap();
        assert!(matches!(e2, TerminalEvent::Key(k) if k.code == KeyCode::Char('b')));

        let e3 = handler.next_event().await.unwrap();
        assert!(matches!(e3, TerminalEvent::Key(k) if k.code == KeyCode::Char('c')));

        // Generator exhausted
        let result = handler.next_event().await;
        assert!(matches!(result, Err(TerminalError::EndOfInput)));
    }

    #[test]
    fn test_frame_to_string() {
        let frame = frame_from_string("Line 1\nLine 2\nLine 3", 80, 24);
        let content = frame.to_string();
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
        assert!(content.contains("Line 3"));
    }

    #[test]
    fn test_find_text_in_frame() {
        let frame = frame_from_string("  Hello World  ", 80, 24);
        let pos = frame.find("Hello");
        assert_eq!(pos, Some((2, 0)));
    }
}
