//! # Chat Screen
//!
//! Main chat interface using iocraft components.
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to chat state
//! changes via `use_future` and futures-signals. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.

use iocraft::prelude::*;

use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::tui::components::{
    navigate_list, KeyHintsBar, ListNavigation, MessageBubble, MessageInput,
};
use crate::tui::hooks::AppCoreContext;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Channel, KeyHint, Message};

/// Callback type for sending messages
pub type SendCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// Callback type for channel selection (channel_id)
pub type ChannelSelectCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Format a timestamp (ms since epoch) as a human-readable time string
fn format_timestamp(ts_ms: u64) -> String {
    use std::time::{Duration, UNIX_EPOCH};

    if ts_ms == 0 {
        return String::new();
    }

    let timestamp = UNIX_EPOCH + Duration::from_millis(ts_ms);
    if let Ok(duration) = timestamp.duration_since(UNIX_EPOCH) {
        // Simple HH:MM format from the duration
        let total_secs = duration.as_secs();
        let hours = (total_secs / 3600) % 24;
        let minutes = (total_secs / 60) % 60;
        format!("{:02}:{:02}", hours, minutes)
    } else {
        String::new()
    }
}

/// Input text shared between render and event handler (thread-safe)
type SharedText = Arc<RwLock<String>>;

/// Which panel is focused
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatFocus {
    /// Channel list is focused
    #[default]
    Channels,
    /// Message area is focused
    Messages,
    /// Input field is focused
    Input,
}

/// Props for ChannelList
#[derive(Default, Props)]
pub struct ChannelListProps {
    pub channels: Vec<Channel>,
    /// Index of selected channel
    pub selected_index: usize,
    /// Whether this panel is focused
    pub focused: bool,
}

/// A list of channels in the sidebar
#[component]
pub fn ChannelList(props: &ChannelListProps) -> impl Into<AnyElement<'static>> {
    let selected_idx = props.selected_index;
    let border_color = if props.focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Round,
            border_color: border_color,
            padding: Spacing::PANEL_PADDING,
            width: 25pct,
        ) {
            Text(content: "Channels", weight: Weight::Bold, color: Theme::PRIMARY)
            View(
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                overflow: Overflow::Scroll,
                margin_top: Spacing::XS,
            ) {
            #(props.channels.iter().enumerate().map(|(idx, ch)| {
                let is_selected = idx == selected_idx;
                let (bg, fg) = if is_selected {
                    (Theme::BG_SELECTED, Theme::TEXT)
                } else {
                    (Theme::BG_DARK, Theme::TEXT_MUTED)
                };
                let id = ch.id.clone();
                let name = ch.name.clone();
                let badge = if ch.unread_count > 0 {
                    format!(" ({})", ch.unread_count)
                } else {
                    String::new()
                };
                let indicator = if is_selected { "→ " } else { "  " };
                element! {
                    View(key: id, background_color: bg, padding_left: Spacing::XS, padding_right: Spacing::XS) {
                        Text(content: format!("{}# {}{}", indicator, name, badge), color: fg)
                    }
                }
            }))
            }
        }
    }
}

/// Props for MessageList
#[allow(dead_code)] // Retained for future refactoring - ChatScreen currently renders inline
#[derive(Default, Props)]
pub struct MessageListProps {
    pub messages: Vec<Message>,
}

/// A list of messages in the chat area
#[component]
pub fn MessageList(props: &MessageListProps) -> impl Into<AnyElement<'static>> {
    element! {
        View(
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            border_style: BorderStyle::Round,
            border_color: Theme::BORDER,
            padding: Spacing::PANEL_PADDING,
        ) {
            #(props.messages.iter().map(|msg| {
                let id = msg.id.clone();
                let sender = msg.sender.clone();
                let content = msg.content.clone();
                let ts = msg.timestamp.clone();
                element! {
                    MessageBubble(
                        key: id,
                        sender: sender,
                        content: content,
                        timestamp: ts,
                        is_own: msg.is_own,
                        is_sending: false,
                        is_failed: false,
                        is_read: true,
                    )
                }
            }))
        }
    }
}

/// Props for ChatScreen
#[derive(Default, Props)]
pub struct ChatScreenProps {
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    pub hints: Vec<KeyHint>,
    /// Initial selected channel index
    pub initial_channel_index: usize,
    /// Callback when sending a message (channel_id, content)
    pub on_send: Option<SendCallback>,
    /// Callback when selecting a channel (channel_id)
    pub on_channel_select: Option<ChannelSelectCallback>,
}

/// The main chat screen component with keyboard navigation
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to chat state signals and automatically update when:
/// - New messages arrive
/// - Channels are created/updated
/// - The selected channel changes
///
/// This uses iocraft's `use_future` hook to spawn an async task that subscribes
/// to futures-signals and updates local State<T> when the signal emits.
#[component]
pub fn ChatScreen(props: &ChatScreenProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    // Try to get AppCoreContext for reactive signal subscription
    let app_ctx = hooks.try_use_context::<AppCoreContext>();

    // Initialize reactive state from props (used when no context or as initial values)
    // These will be updated by signal subscription when available
    let reactive_channels = hooks.use_state({
        let initial = props.channels.clone();
        move || initial
    });
    let reactive_messages = hooks.use_state({
        let initial = props.messages.clone();
        move || initial
    });

    // Subscribe to chat signal updates if AppCoreContext is available
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_channels = reactive_channels.clone();
            let mut reactive_messages = reactive_messages.clone();
            let app_core = ctx.app_core.clone();
            async move {
                use futures_signals::signal::SignalExt;

                // Get the signal from AppCore
                // Note: This requires a brief lock to get the signal, then releases it
                let signal = {
                    let core = app_core.read().await;
                    core.chat_signal()
                };

                // Subscribe to signal updates - this runs indefinitely until component unmounts
                signal
                    .for_each(|chat_state| {
                        // Convert aura-app ChatState to TUI types
                        let channels: Vec<Channel> = chat_state
                            .channels
                            .iter()
                            .map(|c| {
                                Channel::new(&c.id, &c.name)
                                    .with_unread(c.unread_count as usize)
                                    .selected(Some(c.id.clone()) == chat_state.selected_channel_id)
                            })
                            .collect();

                        let messages: Vec<Message> = chat_state
                            .messages
                            .iter()
                            .map(|m| {
                                // Convert timestamp from u64 ms to human-readable string
                                let ts_str = format_timestamp(m.timestamp);
                                Message::new(&m.id, &m.sender_name, &m.content)
                                    .with_timestamp(ts_str)
                                    .own(m.is_own)
                            })
                            .collect();

                        // Update reactive state - this triggers re-render
                        reactive_channels.set(channels);
                        reactive_messages.set(messages);

                        async {}
                    })
                    .await;
            }
        });
    }

    // Use reactive state for rendering (updated by signal or initialized from props)
    let channels = reactive_channels.read().clone();
    let messages = reactive_messages.read().clone();
    let hints = props.hints.clone();
    let channel_count = channels.len();
    let on_send = props.on_send.clone();
    let on_channel_select = props.on_channel_select.clone();

    // State for channel selection (usize is Copy, so use_state works)
    let mut channel_idx = hooks.use_state(|| props.initial_channel_index);

    // State for focus (ChatFocus is Copy)
    let mut focus = hooks.use_state(|| ChatFocus::Channels);

    // Input text state - use Arc<RwLock> for thread-safe sharing
    // We create it once and store it via use_state with a wrapper
    let input_text: SharedText = Arc::new(RwLock::new(String::new()));
    let input_text_for_handler = input_text.clone();

    // Version counter to trigger rerenders when input changes
    let mut input_version = hooks.use_state(|| 0usize);

    // Get current channel ID for sending
    let current_channel_id = channels
        .get(channel_idx.get())
        .map(|c| c.id.clone())
        .unwrap_or_default();

    // Clone channels for use in event handler
    let channels_for_handler = channels.clone();

    // Throttle for navigation keys - persists across renders
    let mut nav_throttle = hooks.use_ref(|| Instant::now() - Duration::from_millis(200));
    let throttle_duration = Duration::from_millis(150);

    // Handle keyboard events
    hooks.use_terminal_events({
        let input_text = input_text_for_handler;
        let current_channel_id = current_channel_id.clone();
        let on_send = on_send.clone();
        let on_channel_select = on_channel_select.clone();
        let channels = channels_for_handler;
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code, modifiers, ..
            }) => {
                match code {
                    // Tab cycles focus forward, Shift+Tab cycles backward
                    KeyCode::Tab => {
                        let new_focus = if modifiers.contains(KeyModifiers::SHIFT) {
                            match focus.get() {
                                ChatFocus::Channels => ChatFocus::Input,
                                ChatFocus::Messages => ChatFocus::Channels,
                                ChatFocus::Input => ChatFocus::Messages,
                            }
                        } else {
                            match focus.get() {
                                ChatFocus::Channels => ChatFocus::Messages,
                                ChatFocus::Messages => ChatFocus::Input,
                                ChatFocus::Input => ChatFocus::Channels,
                            }
                        };
                        focus.set(new_focus);
                    }
                    // Arrow keys navigate within focused panel (with throttle)
                    KeyCode::Up => {
                        let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                        if should_move && focus.get() == ChatFocus::Channels && channel_count > 0 {
                            let new_idx =
                                navigate_list(channel_idx.get(), channel_count, ListNavigation::Up);
                            channel_idx.set(new_idx);
                            nav_throttle.set(Instant::now());
                            // Notify AppCore of channel selection for real-time message updates
                            if let Some(ref callback) = on_channel_select {
                                if let Some(ch) = channels.get(new_idx) {
                                    callback(ch.id.clone());
                                }
                            }
                        }
                    }
                    KeyCode::Down => {
                        let should_move = nav_throttle.read().elapsed() >= throttle_duration;
                        if should_move && focus.get() == ChatFocus::Channels && channel_count > 0 {
                            let new_idx = navigate_list(
                                channel_idx.get(),
                                channel_count,
                                ListNavigation::Down,
                            );
                            channel_idx.set(new_idx);
                            nav_throttle.set(Instant::now());
                            // Notify AppCore of channel selection for real-time message updates
                            if let Some(ref callback) = on_channel_select {
                                if let Some(ch) = channels.get(new_idx) {
                                    callback(ch.id.clone());
                                }
                            }
                        }
                    }
                    // Escape exits insert mode (input focus) back to channels
                    KeyCode::Esc => {
                        if focus.get() == ChatFocus::Input {
                            focus.set(ChatFocus::Channels);
                        }
                    }
                    // Enter sends message when input is focused, stays in insert mode
                    KeyCode::Enter => {
                        if focus.get() == ChatFocus::Input {
                            if let Ok(text) = input_text.read() {
                                let text = text.clone();
                                if !text.is_empty() {
                                    // Call the send callback
                                    if let Some(ref callback) = on_send {
                                        callback(current_channel_id.clone(), text);
                                    }
                                    // Clear input but stay in insert mode
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.clear();
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                }
                            }
                            // Note: We stay in insert mode (ChatFocus::Input) after Enter
                        }
                    }
                    // Backspace removes last character
                    KeyCode::Backspace => {
                        if focus.get() == ChatFocus::Input {
                            if let Ok(mut guard) = input_text.write() {
                                guard.pop();
                            }
                            input_version.set(input_version.get().wrapping_add(1));
                        }
                    }
                    // Character input when input is focused, or 'i' to enter insert mode
                    KeyCode::Char(c) => {
                        if focus.get() == ChatFocus::Input
                            && !modifiers.contains(KeyModifiers::CONTROL)
                        {
                            // In insert mode: accept all characters
                            if let Ok(mut guard) = input_text.write() {
                                guard.push(c);
                            }
                            input_version.set(input_version.get().wrapping_add(1));
                        } else if c == 'i' && focus.get() != ChatFocus::Input {
                            // 'i' enters insert mode from any other focus
                            focus.set(ChatFocus::Input);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    });

    // Read input text for display (using version to ensure fresh read)
    let _ = input_version.get(); // Force dependency on version
    let display_input_text = input_text.read().map(|g| g.clone()).unwrap_or_default();

    let current_channel_idx = channel_idx.get();
    let current_focus = focus.get();
    let channels_focused = current_focus == ChatFocus::Channels;
    let messages_focused = current_focus == ChatFocus::Messages;
    let input_focused = current_focus == ChatFocus::Input;

    // Get current channel name for header
    let channel_name = channels
        .get(current_channel_idx)
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "general".to_string());

    // Message list border color based on focus
    let msg_border = if messages_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
        ) {
            // Header with current channel
            View(
                padding: 1,
                border_style: BorderStyle::Single,
                border_edges: Edges::Bottom,
                border_color: Theme::BORDER,
            ) {
                Text(content: format!("Chat - #{}", channel_name), weight: Weight::Bold, color: Theme::PRIMARY)
            }

            // Main content area
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
            ) {
                ChannelList(
                    channels: channels,
                    selected_index: current_channel_idx,
                    focused: channels_focused,
                )
                // Message list with focus indication
                View(
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    border_style: BorderStyle::Round,
                    border_color: msg_border,
                    padding: Spacing::PANEL_PADDING,
                    overflow: Overflow::Scroll,
                ) {
                    #(messages.iter().map(|msg| {
                        let id = msg.id.clone();
                        let sender = msg.sender.clone();
                        let content = msg.content.clone();
                        let ts = msg.timestamp.clone();
                        element! {
                            MessageBubble(
                                key: id,
                                sender: sender,
                                content: content,
                                timestamp: ts,
                                is_own: msg.is_own,
                                is_sending: false,
                                is_failed: false,
                                is_read: true,
                            )
                        }
                    }))
                }
            }

            // Message input
            MessageInput(
                value: display_input_text,
                placeholder: "Type a message...".to_string(),
                focused: input_focused,
                reply_to: None::<String>,
                sending: false,
            )

            // Key hints at bottom
            KeyHintsBar(hints: hints)
        }
    }
}

/// Run the chat screen (demo mode)
pub async fn run_chat_screen() -> std::io::Result<()> {
    // Create sample data
    let channels = vec![
        Channel::new("1", "general").with_unread(3),
        Channel::new("2", "random").with_unread(0),
        Channel::new("3", "dev").with_unread(1),
    ];

    let messages = vec![
        Message::new("1", "Alice", "Hello everyone!")
            .with_timestamp("10:30")
            .own(false),
        Message::new("2", "You", "Hi Alice!")
            .with_timestamp("10:31")
            .own(true),
        Message::new("3", "Bob", "What's up?")
            .with_timestamp("10:32")
            .own(false),
    ];

    let hints = vec![
        KeyHint::new("↑↓", "Channels"),
        KeyHint::new("i", "Insert mode"),
        KeyHint::new("Esc", "Exit insert"),
        KeyHint::new("Enter", "Send"),
        KeyHint::new("q", "Quit"),
    ];

    let initial_idx: usize = 0;
    element! {
        ChatScreen(
            channels: channels,
            messages: messages,
            hints: hints,
            initial_channel_index: initial_idx,
        )
    }
    .fullscreen()
    .await
}
