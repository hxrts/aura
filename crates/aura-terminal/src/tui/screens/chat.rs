//! # Chat Screen
//!
//! Main chat interface using iocraft components.
//!
//! ## Reactive Signal Subscription
//!
//! When `AppCoreContext` is available, this screen subscribes to chat state
//! changes via the unified `ReactiveEffects` system. Updates are pushed to the
//! component automatically, triggering re-renders when data changes.
//!
//! Uses `aura_app::signal_defs::CHAT_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use std::sync::Arc;

use aura_app::signal_defs::CHAT_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::components::{
    ChannelInfoModal, ChatCreateModal, MessageBubble, MessageInput, TextInputModal,
};
use crate::tui::hooks::AppCoreContext;
use crate::tui::layout::dim;
use crate::tui::props::ChatViewProps;
use crate::tui::theme::{Spacing, Theme};
use crate::tui::types::{Channel, Message};

/// Callback type for sending messages
pub type SendCallback = Arc<dyn Fn(String, String) + Send + Sync>;

/// Callback type for channel selection (channel_id)
pub type ChannelSelectCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Callback type for creating new channels (name, topic)
pub type CreateChannelCallback = Arc<dyn Fn(String, Option<String>) + Send + Sync>;

/// Callback type for retrying failed messages (message_id, channel, content)
pub type RetryMessageCallback = Arc<dyn Fn(String, String, String) + Send + Sync>;

/// Callback type for setting channel topic (channel_id, topic)
pub type SetTopicCallback = Arc<dyn Fn(String, String) + Send + Sync>;

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
            width: 30pct,
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
                // Use consistent list item colors
                let (bg, fg) = if is_selected {
                    (Theme::LIST_BG_SELECTED, Theme::LIST_TEXT_SELECTED)
                } else {
                    (Theme::LIST_BG_NORMAL, Theme::LIST_TEXT_NORMAL)
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

/// Props for ChatScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
#[derive(Default, Props)]
pub struct ChatScreenProps {
    // === Domain data (from reactive signals) ===
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,

    // === View state from TuiState (REQUIRED - compile-time enforced) ===
    /// All view state extracted from TuiState via `extract_chat_view_props()`.
    /// This is a single struct field so forgetting any view state is a compile error.
    pub view: ChatViewProps,

    // === Callbacks (still needed for effect dispatch) ===
    /// Callback when sending a message (channel_id, content)
    pub on_send: Option<SendCallback>,
    /// Callback when selecting a channel (channel_id)
    pub on_channel_select: Option<ChannelSelectCallback>,
    /// Callback when creating a new channel (name, topic)
    pub on_create_channel: Option<CreateChannelCallback>,
    /// Callback when retrying a failed message (message_id, channel, content)
    pub on_retry_message: Option<RetryMessageCallback>,
    /// Callback when setting channel topic (channel_id, topic)
    pub on_set_topic: Option<SetTopicCallback>,
}

/// The main chat screen component
///
/// ## Pure View Component
///
/// This screen is a pure view that renders based on props from TuiState.
/// All event handling is done by the parent TuiShell (IoApp) via the state machine.
///
/// ## Reactive Updates
///
/// When `AppCoreContext` is available in the context tree, this component will
/// subscribe to chat state signals and automatically update when:
/// - New messages arrive
/// - Channels are created/updated
/// - The selected channel changes
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
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_channels = reactive_channels.clone();
            let mut reactive_messages = reactive_messages.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // Helper closure to convert ChatState to TUI types
                let convert_chat_state = |chat_state: &aura_app::views::ChatState| {
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
                            let ts_str = format_timestamp(m.timestamp);
                            Message::new(&m.id, &m.sender_name, &m.content)
                                .with_timestamp(ts_str)
                                .own(m.is_own)
                        })
                        .collect();

                    (channels, messages)
                };

                // FIRST: Read current signal value to catch up on any changes
                // that happened while this screen was unmounted
                {
                    let core = app_core.read().await;
                    if let Ok(chat_state) = core.read(&*CHAT_SIGNAL).await {
                        let (channels, messages) = convert_chat_state(&chat_state);
                        reactive_channels.set(channels);
                        reactive_messages.set(messages);
                    }
                }

                // THEN: Subscribe for future updates
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*CHAT_SIGNAL)
                };

                // Subscribe to signal updates - this runs indefinitely until component unmounts
                while let Ok(chat_state) = stream.recv().await {
                    let (channels, messages) = convert_chat_state(&chat_state);
                    reactive_channels.set(channels);
                    reactive_messages.set(messages);
                }
            }
        });
    }

    // Use reactive state for rendering (updated by signal or initialized from props)
    let channels = reactive_channels.read().clone();
    let messages = reactive_messages.read().clone();

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_focus = props.view.focus;
    let current_channel_idx = props.view.selected_channel;
    let display_input_text = props.view.input_buffer.clone();
    let input_focused = props.view.insert_mode || current_focus == ChatFocus::Input;
    let channels_focused = current_focus == ChatFocus::Channels;
    let messages_focused = current_focus == ChatFocus::Messages;

    // Modal visibility from props.view
    let is_create_modal_visible = props.view.create_modal_visible;
    let is_topic_modal_visible = props.view.topic_modal_visible;
    let is_channel_info_visible = props.view.info_modal_visible;

    // Message list border color based on focus
    let msg_border = if messages_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    // === Pure view: No use_terminal_events ===
    // All event handling is done by IoApp (the shell) via the state machine.
    // This component is purely presentational.

    // Layout: Main content (22 rows) + MessageInput (3 rows) = 25 = MIDDLE_HEIGHT
    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: dim::TOTAL_WIDTH,
            height: dim::MIDDLE_HEIGHT,
            overflow: Overflow::Hidden,
        ) {
            // Main content area - fixed 22 rows
            View(
                flex_direction: FlexDirection::Row,
                height: 22,
                overflow: Overflow::Hidden,
                gap: Spacing::XS,
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
                    height: 22,
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
                        let status = msg.delivery_status;
                        element! {
                            MessageBubble(
                                key: id,
                                sender: sender,
                                content: content,
                                timestamp: ts,
                                is_own: msg.is_own,
                                delivery_status: status,
                            )
                        }
                    }))
                }
            }

            // Message input (3 rows) - full width
            View(height: 3, width: dim::TOTAL_WIDTH) {
                MessageInput(
                    value: display_input_text,
                    placeholder: "Type a message...".to_string(),
                    focused: input_focused,
                    reply_to: None::<String>,
                    sending: false,
                )
            }

            // Create channel modal (overlay) - uses props.view from TuiState
            ChatCreateModal(
                visible: is_create_modal_visible,
                focused: is_create_modal_visible,
                name: props.view.create_modal_name.clone(),
                topic: props.view.create_modal_topic.clone(),
                active_field: props.view.create_modal_active_field,
                error: String::new(),
                creating: false,
            )

            // Topic editing modal (overlay) - uses props.view from TuiState
            TextInputModal(
                visible: is_topic_modal_visible,
                title: "Set Channel Topic".to_string(),
                value: props.view.topic_modal_value.clone(),
                placeholder: "Enter topic...".to_string(),
                error: String::new(),
            )

            // Channel info modal (overlay) - uses props.view from TuiState
            ChannelInfoModal(
                visible: is_channel_info_visible,
                channel_name: props.view.info_modal_channel_name.clone(),
                topic: props.view.info_modal_topic.clone(),
                participants: vec!["You".to_string()],
            )
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

    element! {
        ChatScreen(
            channels: channels,
            messages: messages,
        )
    }
    .fullscreen()
    .await
}
