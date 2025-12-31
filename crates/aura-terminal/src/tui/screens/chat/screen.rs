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
//! Uses `aura_app::ui::signals::CHAT_SIGNAL` with `ReactiveEffects::subscribe()`.
//!
//! ## Pure View Component
//!
//! This screen is a pure view that renders based on props from TuiState.
//! All event handling is done by the parent TuiShell (IoApp) via the state machine.

use iocraft::prelude::*;

use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::views::display::format_timestamp;

use crate::tui::callbacks::{
    ChannelSelectCallback, CreateChannelCallback, RetryMessageCallback, SendCallback,
    SetTopicCallback,
};
use crate::tui::components::{ListPanel, MessageInput, MessagePanel};
use crate::tui::hooks::{subscribe_signal_with_retry, AppCoreContext};
use crate::tui::layout::dim;
use crate::tui::props::ChatViewProps;
use crate::tui::theme::{list_item_colors, Spacing, Theme};
use crate::tui::types::{Channel, Message};

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
    let items: Vec<AnyElement<'static>> = props
        .channels
        .iter()
        .enumerate()
        .map(|(idx, ch)| {
            let is_selected = idx == selected_idx;
            let (bg, fg) = list_item_colors(is_selected);
            let id = ch.id.clone();
            let name = ch.name.clone();
            let badge = if ch.unread_count > 0 {
                format!(" ({})", ch.unread_count)
            } else {
                String::new()
            };
            // Selection indicator: colored triangle when selected, space otherwise
            let (indicator, indicator_color) = if is_selected {
                ("➤ ", Theme::PRIMARY)
            } else {
                ("  ", fg)
            };
            element! {
                View(key: id, flex_direction: FlexDirection::Row, background_color: bg, padding_right: Spacing::XS) {
                    Text(content: indicator, color: indicator_color)
                    Text(content: format!("# {}{}", name, badge), color: fg)
                }
            }
            .into_any()
        })
        .collect();

    element! {
        View(width: dim::TWO_PANEL_LEFT_WIDTH) {
            ListPanel(
                title: "Channels".to_string(),
                count: props.channels.len(),
                focused: props.focused,
                items: items,
                empty_message: "No channels yet".to_string(),
            )
        }
    }
}

/// Props for ChatScreen
///
/// ## Compile-Time Safety
///
/// The `view` field is a required struct that embeds all view state from TuiState.
/// This makes it a **compile-time error** to forget any view state field.
///
/// ## Reactive Data Model
///
/// Domain data (channels, messages) is NOT passed as props.
/// Instead, the component subscribes to CHAT_SIGNAL directly via AppCoreContext.
/// This ensures a single source of truth and prevents stale data bugs.
#[derive(Default, Props)]
pub struct ChatScreenProps {
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
    // Get AppCoreContext for reactive signal subscription (required for domain data)
    let app_ctx = hooks.use_context::<AppCoreContext>();

    // Initialize reactive state with defaults - will be populated by signal subscriptions
    let reactive_channels = hooks.use_state(Vec::new);
    let reactive_messages = hooks.use_state(Vec::new);

    // Subscribe to chat signal updates
    // Uses the unified ReactiveEffects system from aura-core
    hooks.use_future({
        let mut reactive_channels = reactive_channels.clone();
        let mut reactive_messages = reactive_messages.clone();
        let app_core = app_ctx.app_core.clone();
        async move {
            // Helper closure to convert ChatState to TUI types
            let convert_chat_state = |chat_state: &aura_app::ui::types::ChatState| {
                let channels: Vec<Channel> = chat_state
                    .channels
                    .iter()
                    .map(|c| {
                        Channel::new(c.id.to_string(), &c.name)
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

            subscribe_signal_with_retry(app_core, &*CHAT_SIGNAL, move |chat_state| {
                let (channels, messages) = convert_chat_state(&chat_state);
                reactive_channels.set(channels);
                reactive_messages.set(messages);
            })
            .await;
        }
    });

    // Use reactive state for rendering (populated by signal subscription)
    let channels = reactive_channels.read().clone();
    let messages = reactive_messages.read().clone();

    let empty_message = if channels.is_empty() {
        "Select a channel to view messages.".to_string()
    } else {
        "No messages yet.".to_string()
    };

    // === Pure view: Use props.view from TuiState instead of local state ===
    let current_channel_idx = props.view.selected_channel;
    let display_input_text = props.view.input_buffer.clone();
    let input_focused = props.view.insert_mode;

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
                gap: dim::TWO_PANEL_GAP,
            ) {
                ChannelList(
                    channels: channels,
                    selected_index: current_channel_idx,
                    focused: false,
                )
                MessagePanel(
                    messages: messages,
                    title: Some("Messages".to_string()),
                    empty_message: Some(empty_message),
                )
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
        }
    }
}

/// Run the chat screen (requires AppCoreContext for domain data)
pub async fn run_chat_screen() -> std::io::Result<()> {
    // Note: This standalone runner won't have domain data without AppCoreContext.
    // Domain data is obtained via signal subscriptions when context is available.
    element! {
        ChatScreen(
            view: ChatViewProps::default(),
        )
    }
    .fullscreen()
    .await
}
