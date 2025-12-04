//! # Chat Screen
//!
//! Main chat interface using iocraft components.

use iocraft::prelude::*;

use std::sync::{Arc, RwLock};

use crate::tui::components::{
    navigate_list, KeyHintsBar, ListNavigation, MessageBubble, MessageInput,
};
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Channel, KeyHint, Message};

/// Callback type for sending messages
pub type SendCallback = Arc<dyn Fn(String, String) + Send + Sync>;

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
            View(height: Spacing::XS)
            #(props.channels.iter().enumerate().map(|(idx, ch)| {
                let is_selected = idx == selected_idx;
                let (bg, fg) = if is_selected {
                    (Theme::BG_SELECTED, Theme::TEXT)
                } else {
                    (Theme::BG_DARK, Theme::TEXT_MUTED)
                };
                let name = ch.name.clone();
                let badge = if ch.unread_count > 0 {
                    format!(" ({})", ch.unread_count)
                } else {
                    String::new()
                };
                let indicator = if is_selected { "→ " } else { "  " };
                element! {
                    View(background_color: bg, padding_left: Spacing::XS, padding_right: Spacing::XS) {
                        Text(content: format!("{}# {}{}", indicator, name, badge), color: fg)
                    }
                }
            }))
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
                let sender = msg.sender.clone();
                let content = msg.content.clone();
                let ts = msg.timestamp.clone();
                element! {
                    MessageBubble(
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
}

/// The main chat screen component with keyboard navigation
#[component]
pub fn ChatScreen(props: &ChatScreenProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    // Clone data for use in element
    let channels = props.channels.clone();
    let messages = props.messages.clone();
    let hints = props.hints.clone();
    let channel_count = channels.len();
    let on_send = props.on_send.clone();

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

    // Handle keyboard events
    hooks.use_terminal_events({
        let input_text = input_text_for_handler;
        let current_channel_id = current_channel_id.clone();
        let on_send = on_send.clone();
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
                    // Arrow keys navigate within focused panel
                    KeyCode::Up => {
                        if focus.get() == ChatFocus::Channels && channel_count > 0 {
                            let new_idx =
                                navigate_list(channel_idx.get(), channel_count, ListNavigation::Up);
                            channel_idx.set(new_idx);
                        }
                    }
                    KeyCode::Down => {
                        if focus.get() == ChatFocus::Channels && channel_count > 0 {
                            let new_idx = navigate_list(
                                channel_idx.get(),
                                channel_count,
                                ListNavigation::Down,
                            );
                            channel_idx.set(new_idx);
                        }
                    }
                    // Enter sends message when input is focused
                    KeyCode::Enter => {
                        if focus.get() == ChatFocus::Input {
                            if let Ok(text) = input_text.read() {
                                let text = text.clone();
                                if !text.is_empty() {
                                    // Call the send callback
                                    if let Some(ref callback) = on_send {
                                        callback(current_channel_id.clone(), text);
                                    }
                                    // Clear input
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.clear();
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                }
                            }
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
                    // Character input when input is focused
                    KeyCode::Char(c) => {
                        if focus.get() == ChatFocus::Input
                            && !modifiers.contains(KeyModifiers::CONTROL)
                        {
                            if let Ok(mut guard) = input_text.write() {
                                guard.push(c);
                            }
                            input_version.set(input_version.get().wrapping_add(1));
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
                padding: Spacing::PANEL_PADDING,
                border_style: BorderStyle::Round,
                border_color: Theme::BORDER,
            ) {
                Text(content: format!("Aura Chat - #{}", channel_name), weight: Weight::Bold, color: Theme::PRIMARY)
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
                ) {
                    #(messages.iter().map(|msg| {
                        let sender = msg.sender.clone();
                        let content = msg.content.clone();
                        let ts = msg.timestamp.clone();
                        element! {
                            MessageBubble(
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
        KeyHint::new("Tab", "Switch panel"),
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
