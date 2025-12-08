//! TUI Mock Terminal Tests
//!
//! Tests using iocraft's MockTerminalConfig to inject synthetic events
//! directly into components. This approach bypasses terminal/PTY issues
//! that affect tmux-based testing.
//!
//! Run with: cargo test --test tui_mock_test -- --nocapture

mod tui_helpers;

use futures::StreamExt;
use iocraft::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tui_helpers::mock_runner::{key_to_terminal_event, MockTestBuilder, MockTestStep};
use tui_helpers::Key;

/// Simple counter component for testing mock terminal events
#[derive(Default, Props)]
struct CounterProps {
    initial: i32,
}

#[component]
fn Counter(props: &CounterProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let count = hooks.use_state(|| props.initial);
    let should_exit = hooks.use_state(|| false);
    let mut system = hooks.use_context_mut::<SystemContext>();

    // Handle exit outside the event handler
    if should_exit.get() {
        system.exit();
    }

    hooks.use_terminal_events({
        let mut count = count.clone();
        let mut should_exit = should_exit.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent { code, kind, .. }) if kind != KeyEventKind::Release => {
                match code {
                    KeyCode::Up => count.set(count.get() + 1),
                    KeyCode::Down => count.set(count.get() - 1),
                    KeyCode::Char('q') => should_exit.set(true),
                    _ => {}
                }
            }
            _ => {}
        }
    });

    element! {
        View(flex_direction: FlexDirection::Column) {
            Text(content: format!("Count: {}", count.get()))
            Text(content: "Press Up/Down to change, q to quit")
        }
    }
}

/// Test that MockTerminalConfig can inject key events
#[tokio::test]
async fn test_mock_counter_increment() {
    // Create events: Up, Up, Down, q
    let events: Vec<TerminalEvent> = vec![
        key_to_terminal_event(&Key::Up),
        key_to_terminal_event(&Key::Up),
        key_to_terminal_event(&Key::Down),
        key_to_terminal_event(&Key::Char('q')),
    ]
    .into_iter()
    .flatten()
    .collect();

    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    let frames: Vec<String> = element! {
        Counter(initial: 0)
    }
    .mock_terminal_render_loop(config)
    .map(|canvas| canvas.to_string())
    .collect()
    .await;

    // Should have multiple frames (initial + after each press/release)
    assert!(!frames.is_empty(), "Should have at least one frame");

    // Print frames for debugging
    println!("Captured {} frames:", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        println!("Frame {}: {}", i, frame.trim());
    }

    // Final frame should show count of 1 (0 + 2 - 1)
    let final_frame = frames.last().expect("Should have final frame");
    assert!(
        final_frame.contains("Count: 1"),
        "Final frame should show 'Count: 1', got: {}",
        final_frame
    );
}

/// Test callback invocation through mock events
#[tokio::test]
async fn test_mock_callback_invocation() {
    // Track if callback was called
    let callback_called = Arc::new(AtomicBool::new(false));
    let callback_called_clone = callback_called.clone();

    #[derive(Default, Props)]
    struct CallbackProps {
        on_submit: Option<Arc<dyn Fn() + Send + Sync>>,
    }

    #[component]
    fn CallbackTester(props: &CallbackProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let should_exit = hooks.use_state(|| false);
        let mut system = hooks.use_context_mut::<SystemContext>();
        let on_submit = props.on_submit.clone();

        // Handle exit outside the event handler
        if should_exit.get() {
            system.exit();
        }

        hooks.use_terminal_events({
            let mut should_exit = should_exit.clone();
            move |event| match event {
                TerminalEvent::Key(KeyEvent { code, kind, .. }) if kind != KeyEventKind::Release => {
                    match code {
                        KeyCode::Enter => {
                            if let Some(ref callback) = on_submit {
                                callback();
                            }
                        }
                        KeyCode::Char('q') => should_exit.set(true),
                        _ => {}
                    }
                }
                _ => {}
            }
        });

        element! {
            Text(content: "Press Enter to submit, q to quit")
        }
    }

    // Create events: Enter, q
    let events: Vec<TerminalEvent> = vec![
        key_to_terminal_event(&Key::Enter),
        key_to_terminal_event(&Key::Char('q')),
    ]
    .into_iter()
    .flatten()
    .collect();

    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    let on_submit: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        callback_called_clone.store(true, Ordering::SeqCst);
    });

    element! {
        CallbackTester(on_submit: Some(on_submit))
    }
    .mock_terminal_render_loop(config)
    .collect::<Vec<_>>()
    .await;

    assert!(
        callback_called.load(Ordering::SeqCst),
        "Callback should have been invoked"
    );
}

/// Test text input capture through mock events
#[tokio::test]
async fn test_mock_text_input() {
    use std::sync::RwLock;

    #[derive(Default, Props)]
    struct TextInputProps {}

    #[component]
    fn TextInputTester(_props: &TextInputProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>>
    {
        // Use use_state to persist the Arc<RwLock> across renders
        let text: Arc<RwLock<String>> = hooks
            .use_state(|| Arc::new(RwLock::new(String::new())))
            .read()
            .clone();
        let text_for_handler = text.clone();
        let text_for_display = text.clone();
        let version = hooks.use_state(|| 0usize);
        let should_exit = hooks.use_state(|| false);
        let mut system = hooks.use_context_mut::<SystemContext>();

        // Handle exit outside the event handler
        if should_exit.get() {
            system.exit();
        }

        hooks.use_terminal_events({
            let mut version = version.clone();
            let mut should_exit = should_exit.clone();
            move |event| match event {
                TerminalEvent::Key(KeyEvent { code, kind, .. }) if kind != KeyEventKind::Release => {
                    match code {
                        KeyCode::Char(c) if c != 'q' => {
                            if let Ok(mut guard) = text_for_handler.write() {
                                guard.push(c);
                            }
                            version.set(version.get().wrapping_add(1));
                        }
                        KeyCode::Backspace => {
                            if let Ok(mut guard) = text_for_handler.write() {
                                guard.pop();
                            }
                            version.set(version.get().wrapping_add(1));
                        }
                        KeyCode::Char('q') => should_exit.set(true),
                        _ => {}
                    }
                }
                _ => {}
            }
        });

        // Force re-render on version change
        let _ = version.get();
        let current_text = text_for_display
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();

        element! {
            View(flex_direction: FlexDirection::Column) {
                Text(content: format!("Input: {}", current_text))
                Text(content: "Type text, q to quit")
            }
        }
    }

    // Create events: type "hello"
    let events: Vec<TerminalEvent> = vec![
        key_to_terminal_event(&Key::Char('h')),
        key_to_terminal_event(&Key::Char('e')),
        key_to_terminal_event(&Key::Char('l')),
        key_to_terminal_event(&Key::Char('l')),
        key_to_terminal_event(&Key::Char('o')),
        key_to_terminal_event(&Key::Char('q')), // quit
    ]
    .into_iter()
    .flatten()
    .collect();

    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    let frames: Vec<String> = element! {
        TextInputTester()
    }
    .mock_terminal_render_loop(config)
    .map(|canvas| canvas.to_string())
    .collect()
    .await;

    println!("Captured {} frames:", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        println!("Frame {}: {}", i, frame.trim());
    }

    // Find a frame that shows "hello" typed
    // Due to press/release events, we need to check frames after typing is complete
    let has_hello = frames.iter().any(|f| f.contains("Input: hello"));
    assert!(
        has_hello,
        "Should have a frame showing 'Input: hello', frames: {:?}",
        frames
    );
}

/// Test modal interaction pattern (similar to account setup)
#[tokio::test]
async fn test_mock_modal_interaction() {
    use std::sync::RwLock;

    #[derive(Default, Props)]
    struct ModalProps {
        show_modal: bool,
        on_submit: Option<Arc<dyn Fn(String) + Send + Sync>>,
    }

    #[component]
    fn ModalTester(props: &ModalProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
        let modal_visible = hooks.use_state(|| props.show_modal);
        // Use use_state to persist the Arc<RwLock> across renders
        let input_text: Arc<RwLock<String>> = hooks
            .use_state(|| Arc::new(RwLock::new(String::new())))
            .read()
            .clone();
        let input_for_handler = input_text.clone();
        let input_for_display = input_text.clone();
        let version = hooks.use_state(|| 0usize);
        let should_exit = hooks.use_state(|| false);
        let mut system = hooks.use_context_mut::<SystemContext>();
        let on_submit = props.on_submit.clone();

        // Handle exit outside the event handler
        if should_exit.get() {
            system.exit();
        }

        hooks.use_terminal_events({
            let mut modal_visible = modal_visible.clone();
            let mut version = version.clone();
            let mut should_exit = should_exit.clone();
            move |event| {
                // Only process key presses, not releases
                let (code, is_press) = match event {
                    TerminalEvent::Key(KeyEvent { code, kind, .. }) => {
                        (code, kind != KeyEventKind::Release)
                    }
                    _ => return,
                };

                if !is_press {
                    return;
                }

                if !modal_visible.get() {
                    // Not in modal - handle regular navigation
                    match code {
                        KeyCode::Char('q') => should_exit.set(true),
                        _ => {}
                    }
                    return;
                }

                // Modal is visible - capture input
                match code {
                    KeyCode::Char(c) => {
                        if let Ok(mut guard) = input_for_handler.write() {
                            guard.push(c);
                        }
                        version.set(version.get().wrapping_add(1));
                    }
                    KeyCode::Backspace => {
                        if let Ok(mut guard) = input_for_handler.write() {
                            guard.pop();
                        }
                        version.set(version.get().wrapping_add(1));
                    }
                    KeyCode::Enter => {
                        let value = input_for_handler
                            .read()
                            .map(|s| s.clone())
                            .unwrap_or_default();
                        if !value.is_empty() {
                            if let Some(ref callback) = on_submit {
                                callback(value);
                            }
                            modal_visible.set(false);
                            version.set(version.get().wrapping_add(1));
                        }
                    }
                    KeyCode::Esc => {
                        modal_visible.set(false);
                        version.set(version.get().wrapping_add(1));
                    }
                    _ => {}
                }
            }
        });

        let _ = version.get();
        let is_visible = modal_visible.get();
        let current_input = input_for_display
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();

        element! {
            View(flex_direction: FlexDirection::Column) {
                #(if is_visible {
                    vec![element! {
                        View(border_style: BorderStyle::Round) {
                            Text(content: "Enter your name:")
                            Text(content: format!("Name: {}", current_input))
                        }
                    }]
                } else {
                    vec![element! {
                        View {
                            Text(content: "Main Screen - modal dismissed")
                        }
                    }]
                })
            }
        }
    }

    // Track submitted name
    let submitted_name = Arc::new(RwLock::new(String::new()));
    let submitted_for_callback = submitted_name.clone();

    // Create events: type "Bob", press Enter, then q to quit
    let events: Vec<TerminalEvent> = vec![
        key_to_terminal_event(&Key::Char('B')),
        key_to_terminal_event(&Key::Char('o')),
        key_to_terminal_event(&Key::Char('b')),
        key_to_terminal_event(&Key::Enter),
        key_to_terminal_event(&Key::Char('q')), // quit
    ]
    .into_iter()
    .flatten()
    .collect();

    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    let on_submit: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |name: String| {
        if let Ok(mut guard) = submitted_for_callback.write() {
            *guard = name;
        }
    });

    let frames: Vec<String> = element! {
        ModalTester(show_modal: true, on_submit: Some(on_submit))
    }
    .mock_terminal_render_loop(config)
    .map(|canvas| canvas.to_string())
    .collect()
    .await;

    println!("Captured {} frames:", frames.len());
    for (i, frame) in frames.iter().enumerate() {
        println!("Frame {}: {}", i, frame.trim());
    }

    // Check that the name was submitted
    let final_name = submitted_name.read().map(|s| s.clone()).unwrap_or_default();
    assert_eq!(final_name, "Bob", "Should have submitted 'Bob'");

    // Check that modal was dismissed
    let final_frame = frames.last().expect("Should have final frame");
    assert!(
        final_frame.contains("modal dismissed") || final_frame.contains("Main Screen"),
        "Final frame should show main screen after modal dismissal, got: {}",
        final_frame
    );
}

/// Test using the MockTestBuilder helper
#[tokio::test]
async fn test_mock_builder_pattern() {
    let steps = MockTestBuilder::new()
        .press("Increment", Key::Up)
        .press("Increment again", Key::Up)
        .press("Decrement", Key::Down)
        .press("Quit", Key::Char('q'))
        .build();

    // Convert steps to events
    let events: Vec<TerminalEvent> = steps
        .iter()
        .flat_map(|s| key_to_terminal_event(&s.key))
        .collect();

    let config = MockTerminalConfig::with_events(futures::stream::iter(events));

    let frames: Vec<String> = element! {
        Counter(initial: 10)
    }
    .mock_terminal_render_loop(config)
    .map(|canvas| canvas.to_string())
    .collect()
    .await;

    // Final count should be 10 + 2 - 1 = 11
    let final_frame = frames.last().expect("Should have final frame");
    assert!(
        final_frame.contains("Count: 11"),
        "Final frame should show 'Count: 11', got: {}",
        final_frame
    );
}
