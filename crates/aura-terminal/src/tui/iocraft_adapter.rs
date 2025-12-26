//! # Iocraft Terminal Adapter
//!
//! Adapter that bridges iocraft's terminal hooks to the `TerminalEffects` trait.
//!
//! ## Overview
//!
//! Iocraft uses a component-based model with hooks like `use_terminal_events()`.
//! This adapter provides a bridge so that:
//! - Production TUI uses iocraft's native rendering and event handling
//! - Test TUI uses `TuiRuntime<T>` with mock `TerminalEffects`
//!
//! ## Usage
//!
//! The adapter enables two modes of operation:
//!
//! 1. **Production**: IoApp component runs with iocraft, events flow through hooks
//! 2. **Headless**: TuiRuntime drives the state machine directly
//!
//! ```rust,ignore
//! // Headless mode with state machine
//! use aura_terminal::tui::runtime::TuiRuntime;
//! use aura_terminal::testing::TestTerminal;
//!
//! let terminal = TestTerminal::new(vec![events::char('2'), events::char('q')]);
//! let mut runtime = TuiRuntime::new(terminal);
//! runtime.run().await?;
//! ```

use aura_core::effects::terminal::{
    CursorShape, TerminalError, TerminalEvent, TerminalFrame, TerminalInputEffects,
    TerminalOutputEffects,
};
use iocraft::prelude::*;
use std::collections::VecDeque;
use std::sync::Mutex;
use tokio::sync::mpsc;

/// Adapter that bridges iocraft events to the TerminalEffects trait.
///
/// This allows using the same `TuiRuntime` with either:
/// - Mock terminals for testing
/// - Real iocraft terminals for production
///
/// ## Architecture
///
/// ```text
/// Production Mode:
///   IoApp (iocraft) --[use_terminal_events]--> IoApp event handler
///                                               |
///                                               v
///                                          state updates, callbacks
///
/// Headless Mode:
///   TuiRuntime --[TerminalEffects]--> IocraftAdapter --[channel]--> event queue
///       |                                   |
///       v                                   v
///   TuiState transitions              frame capture
/// ```
pub struct IocraftTerminalAdapter {
    /// Event queue for headless operation
    event_queue: Mutex<VecDeque<TerminalEvent>>,
    /// Channel for receiving events from external source (async-aware)
    event_receiver: tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<TerminalEvent>>>,
    /// Captured frames for testing
    frames: Mutex<Vec<TerminalFrame>>,
    /// Terminal size
    size: (u16, u16),
}

impl IocraftTerminalAdapter {
    /// Create a new adapter with default 80x24 terminal size.
    pub fn new() -> Self {
        Self {
            event_queue: Mutex::new(VecDeque::new()),
            event_receiver: tokio::sync::Mutex::new(None),
            frames: Mutex::new(Vec::new()),
            size: (80, 24),
        }
    }

    /// Create an adapter with predetermined events (for testing).
    pub fn with_events(events: Vec<TerminalEvent>) -> Self {
        Self {
            event_queue: Mutex::new(events.into()),
            event_receiver: tokio::sync::Mutex::new(None),
            frames: Mutex::new(Vec::new()),
            size: (80, 24),
        }
    }

    /// Create an adapter with custom size.
    pub fn with_size(width: u16, height: u16) -> Self {
        Self {
            event_queue: Mutex::new(VecDeque::new()),
            event_receiver: tokio::sync::Mutex::new(None),
            frames: Mutex::new(Vec::new()),
            size: (width, height),
        }
    }

    /// Create an adapter with an event channel (for receiving external events).
    pub fn with_channel(receiver: mpsc::UnboundedReceiver<TerminalEvent>) -> Self {
        Self {
            event_queue: Mutex::new(VecDeque::new()),
            event_receiver: tokio::sync::Mutex::new(Some(receiver)),
            frames: Mutex::new(Vec::new()),
            size: (80, 24),
        }
    }

    /// Push an event to the queue (for external injection).
    pub fn push_event(&self, event: TerminalEvent) {
        let mut queue = self.event_queue.lock().unwrap();
        queue.push_back(event);
    }

    /// Get captured frames.
    pub fn frames(&self) -> Vec<TerminalFrame> {
        self.frames.lock().unwrap().clone()
    }

    /// Get the last captured frame.
    pub fn last_frame(&self) -> Option<TerminalFrame> {
        self.frames.lock().unwrap().last().cloned()
    }

    /// Clear captured frames.
    pub fn clear_frames(&self) {
        self.frames.lock().unwrap().clear();
    }
}

impl Default for IocraftTerminalAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl TerminalInputEffects for IocraftTerminalAdapter {
    async fn next_event(&self) -> Result<TerminalEvent, TerminalError> {
        // First try the event queue (sync, fast path)
        {
            let mut queue = self.event_queue.lock().unwrap();
            if let Some(event) = queue.pop_front() {
                return Ok(event);
            }
        }

        // Then try the channel receiver (async)
        {
            let mut receiver_guard = self.event_receiver.lock().await;
            if let Some(ref mut receiver) = *receiver_guard {
                match receiver.recv().await {
                    Some(event) => return Ok(event),
                    None => return Err(TerminalError::EndOfInput),
                }
            }
        }

        // No events available
        Err(TerminalError::EndOfInput)
    }

    async fn poll_event(&self, _timeout_ms: u64) -> Result<Option<TerminalEvent>, TerminalError> {
        let mut queue = self.event_queue.lock().unwrap();
        Ok(queue.pop_front())
    }

    async fn has_input(&self) -> bool {
        let queue = self.event_queue.lock().unwrap();
        !queue.is_empty()
    }
}

#[async_trait::async_trait]
impl TerminalOutputEffects for IocraftTerminalAdapter {
    async fn render(&self, frame: TerminalFrame) -> Result<(), TerminalError> {
        let mut frames = self.frames.lock().unwrap();
        frames.push(frame);
        Ok(())
    }

    async fn clear(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn size(&self) -> Result<(u16, u16), TerminalError> {
        Ok(self.size)
    }

    async fn set_cursor(&self, _col: u16, _row: u16) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn set_cursor_visible(&self, _visible: bool) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn set_cursor_shape(&self, _shape: CursorShape) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn enter_alternate_screen(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn leave_alternate_screen(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn enable_raw_mode(&self) -> Result<(), TerminalError> {
        Ok(())
    }

    async fn disable_raw_mode(&self) -> Result<(), TerminalError> {
        Ok(())
    }
}

/// Convert iocraft TerminalEvent to our TerminalEvent
///
/// This bridges between iocraft's event model and our effect trait's model.
/// Only Press events are converted - Release and Repeat events are filtered out
/// to prevent duplicate character input.
pub fn convert_iocraft_event(event: iocraft::prelude::TerminalEvent) -> Option<TerminalEvent> {
    use aura_core::effects::terminal::KeyEvent as AuraKeyEvent;

    match event {
        iocraft::prelude::TerminalEvent::Key(key_event) => {
            // Only process Press events - ignore Release and Repeat to avoid duplicates
            // On Windows, crossterm sends both Press and Release events for each keystroke
            // On some terminals, Repeat events may also be sent for held keys
            if key_event.kind != KeyEventKind::Press {
                return None;
            }
            let code = convert_key_code(key_event.code);
            let modifiers = convert_modifiers(key_event.modifiers);
            Some(TerminalEvent::Key(AuraKeyEvent {
                code,
                modifiers,
                kind: aura_core::effects::terminal::KeyEventKind::Press,
            }))
        }
        iocraft::prelude::TerminalEvent::Resize(width, height) => {
            Some(TerminalEvent::Resize { width, height })
        }
        _ => None, // Mouse events, paste events, etc. not needed for TUI
    }
}

/// Convert iocraft KeyCode to our KeyCode
fn convert_key_code(code: KeyCode) -> aura_core::effects::terminal::KeyCode {
    use aura_core::effects::terminal::KeyCode as AuraKeyCode;

    match code {
        KeyCode::Char(c) => AuraKeyCode::Char(c),
        KeyCode::Enter => AuraKeyCode::Enter,
        KeyCode::Esc => AuraKeyCode::Esc,
        KeyCode::Tab => AuraKeyCode::Tab,
        KeyCode::Backspace => AuraKeyCode::Backspace,
        KeyCode::Delete => AuraKeyCode::Delete,
        KeyCode::Up => AuraKeyCode::Up,
        KeyCode::Down => AuraKeyCode::Down,
        KeyCode::Left => AuraKeyCode::Left,
        KeyCode::Right => AuraKeyCode::Right,
        KeyCode::Home => AuraKeyCode::Home,
        KeyCode::End => AuraKeyCode::End,
        KeyCode::PageUp => AuraKeyCode::PageUp,
        KeyCode::PageDown => AuraKeyCode::PageDown,
        KeyCode::F(n) => AuraKeyCode::F(n),
        _ => AuraKeyCode::Null,
    }
}

/// Convert iocraft KeyModifiers to our Modifiers
fn convert_modifiers(modifiers: KeyModifiers) -> aura_core::effects::terminal::Modifiers {
    use aura_core::effects::terminal::Modifiers;
    let mut result = Modifiers::NONE;
    if modifiers.contains(KeyModifiers::SHIFT) {
        result = result | Modifiers::SHIFT;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        result = result | Modifiers::CTRL;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        result = result | Modifiers::ALT;
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        result = result | Modifiers::SUPER;
    }
    if modifiers.contains(KeyModifiers::META) {
        result = result | Modifiers::META;
    }
    result
}

/// Event bridge for forwarding iocraft events to TuiRuntime
///
/// This allows running the state machine alongside iocraft's rendering.
pub struct EventBridge {
    sender: mpsc::UnboundedSender<TerminalEvent>,
}

impl EventBridge {
    /// Create a new event bridge and return the adapter it connects to.
    pub fn new() -> (Self, IocraftTerminalAdapter) {
        let (sender, receiver) = mpsc::unbounded_channel();
        let adapter = IocraftTerminalAdapter::with_channel(receiver);
        (Self { sender }, adapter)
    }

    /// Forward an iocraft event to the adapter.
    pub fn forward(&self, event: iocraft::prelude::TerminalEvent) {
        if let Some(converted) = convert_iocraft_event(event) {
            let _ = self.sender.send(converted);
        }
    }

    /// Send a custom event.
    pub fn send(&self, event: TerminalEvent) {
        let _ = self.sender.send(event);
    }
}

impl Default for EventBridge {
    fn default() -> Self {
        let (sender, _) = mpsc::unbounded_channel();
        Self { sender }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::terminal::events;
    use aura_core::effects::terminal::KeyCode as AuraKeyCode;

    #[tokio::test]
    async fn test_adapter_with_events() {
        let adapter = IocraftTerminalAdapter::with_events(vec![
            events::char('a'),
            events::char('b'),
            events::char('c'),
        ]);

        assert_eq!(adapter.next_event().await.unwrap(), events::char('a'));
        assert_eq!(adapter.next_event().await.unwrap(), events::char('b'));
        assert_eq!(adapter.next_event().await.unwrap(), events::char('c'));
        assert!(adapter.next_event().await.is_err());
    }

    #[tokio::test]
    async fn test_adapter_push_event() {
        let adapter = IocraftTerminalAdapter::new();
        adapter.push_event(events::char('x'));

        assert_eq!(adapter.next_event().await.unwrap(), events::char('x'));
    }

    #[tokio::test]
    async fn test_adapter_frame_capture() {
        let adapter = IocraftTerminalAdapter::new();

        let frame = TerminalFrame::new(80, 24);

        adapter.render(frame.clone()).await.unwrap();
        assert_eq!(adapter.frames().len(), 1);
        assert!(adapter.last_frame().is_some());

        adapter.clear_frames();
        assert!(adapter.frames().is_empty());
    }

    #[tokio::test]
    async fn test_adapter_size() {
        let adapter = IocraftTerminalAdapter::with_size(120, 40);
        assert_eq!(adapter.size().await.unwrap(), (120, 40));
    }

    #[test]
    fn test_convert_key_code() {
        assert_eq!(convert_key_code(KeyCode::Char('a')), AuraKeyCode::Char('a'));
        assert_eq!(convert_key_code(KeyCode::Enter), AuraKeyCode::Enter);
        assert_eq!(convert_key_code(KeyCode::Esc), AuraKeyCode::Esc);
        assert_eq!(convert_key_code(KeyCode::Tab), AuraKeyCode::Tab);
        assert_eq!(convert_key_code(KeyCode::Up), AuraKeyCode::Up);
        assert_eq!(convert_key_code(KeyCode::F(1)), AuraKeyCode::F(1));
    }

    #[test]
    fn test_convert_modifiers() {
        let result = convert_modifiers(KeyModifiers::SHIFT | KeyModifiers::CONTROL);
        assert!(result.shift());
        assert!(result.ctrl());
        assert!(!result.alt());
    }

    #[test]
    fn test_event_bridge() {
        let (bridge, _adapter) = EventBridge::new();

        // Just verify creation works
        bridge.send(events::char('x'));
    }
}
