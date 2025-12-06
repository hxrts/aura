//! Mock Terminal Test Runner
//!
//! Uses iocraft's MockTerminalConfig to inject synthetic keyboard events
//! directly into the TUI component, bypassing terminal/PTY issues.
//!
//! This is the proper testing approach for iocraft-based TUIs.

use futures::StreamExt;
use iocraft::prelude::*;

use super::Key;

/// Convert our Key enum to iocraft TerminalEvent
pub fn key_to_terminal_event(key: &Key) -> Vec<TerminalEvent> {
    let (code, modifiers) = match key {
        Key::Char(c) => (KeyCode::Char(*c), KeyModifiers::empty()),
        Key::Enter => (KeyCode::Enter, KeyModifiers::empty()),
        Key::Escape => (KeyCode::Esc, KeyModifiers::empty()),
        Key::Tab => (KeyCode::Tab, KeyModifiers::empty()),
        Key::BackTab => (KeyCode::BackTab, KeyModifiers::SHIFT),
        Key::Backspace => (KeyCode::Backspace, KeyModifiers::empty()),
        Key::Up => (KeyCode::Up, KeyModifiers::empty()),
        Key::Down => (KeyCode::Down, KeyModifiers::empty()),
        Key::Left => (KeyCode::Left, KeyModifiers::empty()),
        Key::Right => (KeyCode::Right, KeyModifiers::empty()),
        Key::F(n) => (KeyCode::F(*n), KeyModifiers::empty()),
        Key::Ctrl(c) => (KeyCode::Char(*c), KeyModifiers::CONTROL),
        Key::Shift(c) => (KeyCode::Char(c.to_ascii_uppercase()), KeyModifiers::SHIFT),
        Key::Num(n) => (KeyCode::Char((b'0' + n) as char), KeyModifiers::empty()),
    };

    // Send both Press and Release events for realistic simulation
    // Create KeyEvent via constructor and mutate public modifiers field
    let mut press = KeyEvent::new(KeyEventKind::Press, code);
    press.modifiers = modifiers;
    let mut release = KeyEvent::new(KeyEventKind::Release, code);
    release.modifiers = modifiers;

    vec![
        TerminalEvent::Key(press),
        TerminalEvent::Key(release),
    ]
}

/// Convert a sequence of Keys to TerminalEvents
pub fn keys_to_events(keys: &[Key]) -> Vec<TerminalEvent> {
    keys.iter().flat_map(key_to_terminal_event).collect()
}

/// Test step with key and expected output verification
#[derive(Debug, Clone)]
pub struct MockTestStep {
    /// Description of what this step does
    pub description: String,
    /// Key to press
    pub key: Key,
    /// Expected text patterns in the rendered output (AND logic)
    pub expect: Vec<String>,
    /// Text patterns that must NOT appear
    pub reject: Vec<String>,
}

impl MockTestStep {
    pub fn new(description: impl Into<String>, key: Key) -> Self {
        Self {
            description: description.into(),
            key,
            expect: Vec::new(),
            reject: Vec::new(),
        }
    }

    pub fn expect(mut self, text: impl Into<String>) -> Self {
        self.expect.push(text.into());
        self
    }

    pub fn reject(mut self, text: impl Into<String>) -> Self {
        self.reject.push(text.into());
        self
    }
}

/// Result of running a mock test
#[derive(Debug)]
pub struct MockTestResult {
    /// Whether the test passed
    pub success: bool,
    /// Rendered canvases at each step (as strings)
    pub frames: Vec<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Which step failed (if any)
    pub failed_step: Option<usize>,
}

/// Run a mock terminal test with a component
///
/// This function:
/// 1. Creates a stream of TerminalEvents from the test steps
/// 2. Runs the component with mock_terminal_render_loop
/// 3. Collects rendered frames
/// 4. Verifies each frame against expected patterns
pub async fn run_mock_test<E: Into<AnyElement<'static>>>(
    element: E,
    steps: &[MockTestStep],
) -> MockTestResult {
    // Convert steps to events
    let events: Vec<TerminalEvent> = steps
        .iter()
        .flat_map(|s| key_to_terminal_event(&s.key))
        .collect();

    // Create mock terminal config with events
    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    // Run the render loop and collect all frames
    let frames: Vec<String> = element
        .into()
        .mock_terminal_render_loop(config)
        .map(|canvas| canvas.to_string())
        .collect()
        .await;

    // Verify each step against corresponding frame
    // Note: Each key press generates 2 events (press + release), so we need to
    // account for that when matching frames to steps
    let mut failed_step = None;
    let mut error = None;

    for (i, step) in steps.iter().enumerate() {
        // Get the frame for this step (accounting for press/release events)
        // Each step generates 2 events, plus initial render, so frame index is 2*i + 1 for after press
        let frame_idx = 2 * i + 1;
        if frame_idx >= frames.len() {
            failed_step = Some(i);
            error = Some(format!(
                "Step {}: '{}' - No frame captured (only {} frames)",
                i + 1,
                step.description,
                frames.len()
            ));
            break;
        }

        let frame = &frames[frame_idx];

        // Check expect patterns
        for pattern in &step.expect {
            if !frame.contains(pattern) {
                failed_step = Some(i);
                error = Some(format!(
                    "Step {}: '{}' - Expected '{}' not found in frame",
                    i + 1,
                    step.description,
                    pattern
                ));
                break;
            }
        }

        if error.is_some() {
            break;
        }

        // Check reject patterns
        for pattern in &step.reject {
            if frame.contains(pattern) {
                failed_step = Some(i);
                error = Some(format!(
                    "Step {}: '{}' - Rejected pattern '{}' found in frame",
                    i + 1,
                    step.description,
                    pattern
                ));
                break;
            }
        }

        if error.is_some() {
            break;
        }
    }

    MockTestResult {
        success: error.is_none(),
        frames,
        error,
        failed_step,
    }
}

/// Builder for mock test sequences
pub struct MockTestBuilder {
    steps: Vec<MockTestStep>,
}

impl MockTestBuilder {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// Add a key press step
    pub fn press(mut self, description: impl Into<String>, key: Key) -> Self {
        self.steps.push(MockTestStep::new(description, key));
        self
    }

    /// Add a key press step with expected text
    pub fn press_expect(
        mut self,
        description: impl Into<String>,
        key: Key,
        expect: impl Into<String>,
    ) -> Self {
        self.steps
            .push(MockTestStep::new(description, key).expect(expect));
        self
    }

    /// Type a string as individual characters
    pub fn type_text(mut self, description: impl Into<String>, text: &str) -> Self {
        let desc = description.into();
        for (i, c) in text.chars().enumerate() {
            let step_desc = if i == 0 {
                desc.clone()
            } else {
                format!("{} (continued)", desc)
            };
            self.steps.push(MockTestStep::new(step_desc, Key::Char(c)));
        }
        self
    }

    /// Build the test sequence
    pub fn build(self) -> Vec<MockTestStep> {
        self.steps
    }
}

impl Default for MockTestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_to_event_char() {
        let events = key_to_terminal_event(&Key::Char('a'));
        assert_eq!(events.len(), 2);
        // First should be Press
        match &events[0] {
            TerminalEvent::Key(ke) => {
                assert_eq!(ke.code, KeyCode::Char('a'));
                assert_eq!(ke.kind, KeyEventKind::Press);
            }
            _ => panic!("Expected Key event"),
        }
        // Second should be Release
        match &events[1] {
            TerminalEvent::Key(ke) => {
                assert_eq!(ke.code, KeyCode::Char('a'));
                assert_eq!(ke.kind, KeyEventKind::Release);
            }
            _ => panic!("Expected Key event"),
        }
    }

    #[test]
    fn test_key_to_event_enter() {
        let events = key_to_terminal_event(&Key::Enter);
        assert_eq!(events.len(), 2);
        match &events[0] {
            TerminalEvent::Key(ke) => {
                assert_eq!(ke.code, KeyCode::Enter);
            }
            _ => panic!("Expected Key event"),
        }
    }

    #[test]
    fn test_key_to_event_ctrl() {
        let events = key_to_terminal_event(&Key::Ctrl('c'));
        assert_eq!(events.len(), 2);
        match &events[0] {
            TerminalEvent::Key(ke) => {
                assert_eq!(ke.code, KeyCode::Char('c'));
                assert!(ke.modifiers.contains(KeyModifiers::CONTROL));
            }
            _ => panic!("Expected Key event"),
        }
    }

    #[test]
    fn test_builder() {
        let steps = MockTestBuilder::new()
            .press("Press a", Key::Char('a'))
            .type_text("Type hello", "hi")
            .press_expect("Press Enter", Key::Enter, "result")
            .build();

        assert_eq!(steps.len(), 4); // a + h + i + Enter
        assert_eq!(steps[0].description, "Press a");
        assert_eq!(steps[1].description, "Type hello");
        assert_eq!(steps[2].description, "Type hello (continued)");
        assert_eq!(steps[3].description, "Press Enter");
        assert_eq!(steps[3].expect, vec!["result"]);
    }
}
