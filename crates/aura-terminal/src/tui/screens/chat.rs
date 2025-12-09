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

use iocraft::prelude::*;

use std::sync::{Arc, RwLock};

use aura_app::signal_defs::CHAT_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;

use crate::tui::components::{
    ChannelInfoModal, ChatCreateModal, ChatCreateState, MessageBubble, MessageInput, TextInputModal,
};
use crate::tui::hooks::AppCoreContext;
use crate::tui::navigation::{is_nav_key_press, navigate_list, InputThrottle, NavKey, NavThrottle};
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

/// State for topic editing modal
#[derive(Clone, Debug, Default)]
pub struct TopicModalState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Current input value
    pub value: String,
    /// Channel ID being edited
    pub channel_id: String,
    /// Error message if any
    pub error: String,
}

impl TopicModalState {
    /// Show the modal with the current topic
    pub fn show(&mut self, channel_id: &str, current_topic: &str) {
        self.visible = true;
        self.channel_id = channel_id.to_string();
        self.value = current_topic.to_string();
        self.error.clear();
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.value.clear();
        self.channel_id.clear();
        self.error.clear();
    }

    /// Push a character to the input
    pub fn push_char(&mut self, c: char) {
        self.value.push(c);
    }

    /// Delete the last character
    pub fn backspace(&mut self) {
        self.value.pop();
    }
}

/// State for channel info modal
#[derive(Clone, Debug, Default)]
pub struct ChannelInfoModalState {
    /// Whether the modal is visible
    pub visible: bool,
    /// Channel ID being shown
    pub channel_id: String,
    /// Channel name
    pub channel_name: String,
    /// Channel topic
    pub topic: String,
    /// Participants in the channel
    pub participants: Vec<String>,
}

impl ChannelInfoModalState {
    /// Show the modal with channel info
    pub fn show(&mut self, channel_id: &str, channel_name: &str, topic: Option<&str>) {
        self.visible = true;
        self.channel_id = channel_id.to_string();
        self.channel_name = channel_name.to_string();
        self.topic = topic.unwrap_or("").to_string();
        self.participants.clear(); // Will be populated by callback
    }

    /// Set the participants list
    pub fn set_participants(&mut self, participants: Vec<String>) {
        self.participants = participants;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.channel_id.clear();
        self.channel_name.clear();
        self.topic.clear();
        self.participants.clear();
    }
}

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
}

/// Props for ChatScreen
#[derive(Default, Props)]
pub struct ChatScreenProps {
    pub channels: Vec<Channel>,
    pub messages: Vec<Message>,
    /// Initial selected channel index
    pub initial_channel_index: usize,
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
    // Uses the unified ReactiveEffects system from aura-core
    if let Some(ctx) = app_ctx {
        hooks.use_future({
            let mut reactive_channels = reactive_channels.clone();
            let mut reactive_messages = reactive_messages.clone();
            let app_core = ctx.app_core.clone();
            async move {
                // Get a subscription to the chat signal via ReactiveEffects
                let mut stream = {
                    let core = app_core.read().await;
                    core.subscribe(&*CHAT_SIGNAL)
                };

                // Subscribe to signal updates - this runs indefinitely until component unmounts
                while let Ok(chat_state) = stream.recv().await {
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
                }
            }
        });
    }

    // Use reactive state for rendering (updated by signal or initialized from props)
    let channels = reactive_channels.read().clone();
    let messages = reactive_messages.read().clone();
    let channel_count = channels.len();
    let on_send = props.on_send.clone();
    let on_channel_select = props.on_channel_select.clone();

    // State for channel selection (usize is Copy, so use_state works)
    let mut channel_idx = hooks.use_state(|| props.initial_channel_index);

    // State for focus (ChatFocus is Copy)
    let mut focus = hooks.use_state(|| ChatFocus::Channels);

    // Modal state for creating new channels
    let create_modal_state = hooks.use_state(ChatCreateState::new);

    // Topic modal state
    let topic_modal_state = hooks.use_state(TopicModalState::default);

    // Channel info modal state
    let channel_info_state = hooks.use_state(ChannelInfoModalState::default);

    // Input text state - use Arc<RwLock> for thread-safe sharing
    // Use use_state to persist across renders
    let input_text: SharedText = hooks
        .use_state(|| Arc::new(RwLock::new(String::new())))
        .read()
        .clone();
    let input_text_for_handler = input_text.clone();

    // Version counter to trigger rerenders when input changes
    let input_version = hooks.use_state(|| 0usize);

    // Get current channel ID for sending
    let current_channel_id = channels
        .get(channel_idx.get())
        .map(|c| c.id.clone())
        .unwrap_or_default();

    // Clone channels for use in event handler
    let channels_for_handler = channels.clone();

    // Throttle for navigation keys - persists across renders
    let mut nav_throttle = hooks.use_ref(NavThrottle::new);

    // Throttle for text input - persists across renders
    let mut input_throttle = hooks.use_ref(InputThrottle::new);

    // Clone create callback for event handler
    let on_create_channel = props.on_create_channel.clone();
    let on_retry_message = props.on_retry_message.clone();
    let on_set_topic = props.on_set_topic.clone();

    // Message index for selecting failed messages to retry
    let message_idx = hooks.use_state(|| 0usize);

    // Clone messages for event handler
    let messages_for_handler = messages.clone();

    // Handle keyboard events
    hooks.use_terminal_events({
        let input_text = input_text_for_handler;
        let current_channel_id = current_channel_id.clone();
        let on_send = on_send.clone();
        let on_channel_select = on_channel_select.clone();
        let on_create_channel = on_create_channel.clone();
        let on_retry_message = on_retry_message.clone();
        let on_set_topic = on_set_topic.clone();
        let mut create_modal_state = create_modal_state.clone();
        let mut topic_modal_state = topic_modal_state.clone();
        let mut channel_info_state = channel_info_state.clone();
        let channels = channels_for_handler;
        let messages_for_retry = messages_for_handler;
        let mut input_version = input_version.clone();
        move |event| {
            // Check if any modal is visible
            let create_modal_visible = create_modal_state.read().visible;
            let topic_modal_visible = topic_modal_state.read().visible;
            let channel_info_visible = channel_info_state.read().visible;
            let modal_visible = create_modal_visible || topic_modal_visible || channel_info_visible;
            let current_focus = focus.get();

            // Handle navigation keys first (only in normal mode, not in modal or input mode)
            if !modal_visible && current_focus != ChatFocus::Input {
                if let Some(nav_key) = is_nav_key_press(&event) {
                    if nav_throttle.write().try_navigate() {
                        match nav_key {
                            // Horizontal: cycle focus between panels
                            NavKey::Left => {
                                let new_focus = match current_focus {
                                    ChatFocus::Channels => ChatFocus::Input,
                                    ChatFocus::Messages => ChatFocus::Channels,
                                    ChatFocus::Input => ChatFocus::Messages,
                                };
                                focus.set(new_focus);
                            }
                            NavKey::Right => {
                                let new_focus = match current_focus {
                                    ChatFocus::Channels => ChatFocus::Messages,
                                    ChatFocus::Messages => ChatFocus::Input,
                                    ChatFocus::Input => ChatFocus::Channels,
                                };
                                focus.set(new_focus);
                            }
                            // Vertical: navigate within channel list when focused
                            NavKey::Up | NavKey::Down => {
                                if current_focus == ChatFocus::Channels && channel_count > 0 {
                                    let new_idx =
                                        navigate_list(channel_idx.get(), channel_count, nav_key);
                                    channel_idx.set(new_idx);
                                    if let Some(ref callback) = on_channel_select {
                                        if let Some(ch) = channels.get(new_idx) {
                                            callback(ch.id.clone());
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return;
                }
            }

            match event {
                TerminalEvent::Key(KeyEvent {
                    code, modifiers, ..
                }) => {
                    if channel_info_visible {
                        // Handle channel info modal keys
                        match code {
                            KeyCode::Esc => {
                                // Close modal
                                let mut state = channel_info_state.read().clone();
                                state.hide();
                                channel_info_state.set(state);
                            }
                            KeyCode::Char('t') => {
                                // Open topic editing from info modal
                                let info_state = channel_info_state.read().clone();
                                // Close info modal
                                let mut state = channel_info_state.read().clone();
                                state.hide();
                                channel_info_state.set(state);
                                // Open topic modal with channel info
                                let mut state = topic_modal_state.read().clone();
                                state.show(&info_state.channel_id, &info_state.topic);
                                topic_modal_state.set(state);
                            }
                            _ => {}
                        }
                    } else if topic_modal_visible {
                        // Handle topic modal keys
                        match code {
                            KeyCode::Esc => {
                                // Close modal
                                let mut state = topic_modal_state.read().clone();
                                state.hide();
                                topic_modal_state.set(state);
                            }
                            KeyCode::Enter => {
                                // Submit topic
                                let state = topic_modal_state.read().clone();
                                if let Some(ref callback) = on_set_topic {
                                    callback(state.channel_id.clone(), state.value.clone());
                                }
                                // Close modal
                                let mut state = topic_modal_state.read().clone();
                                state.hide();
                                topic_modal_state.set(state);
                            }
                            KeyCode::Backspace => {
                                // Delete character (with throttle)
                                if input_throttle.write().try_input() {
                                    let mut state = topic_modal_state.read().clone();
                                    state.backspace();
                                    topic_modal_state.set(state);
                                }
                            }
                            KeyCode::Char(c) => {
                                // Add character (with throttle)
                                if input_throttle.write().try_input() {
                                    let mut state = topic_modal_state.read().clone();
                                    state.push_char(c);
                                    topic_modal_state.set(state);
                                }
                            }
                            _ => {}
                        }
                    } else if create_modal_visible {
                        // Handle create channel modal keys
                        match code {
                            KeyCode::Esc => {
                                // Close modal
                                let mut state = create_modal_state.read().clone();
                                state.hide();
                                create_modal_state.set(state);
                            }
                            KeyCode::Tab => {
                                // Switch field
                                let mut state = create_modal_state.read().clone();
                                state.next_field();
                                create_modal_state.set(state);
                            }
                            KeyCode::BackTab => {
                                // Switch field backwards
                                let mut state = create_modal_state.read().clone();
                                state.prev_field();
                                create_modal_state.set(state);
                            }
                            KeyCode::Enter => {
                                // Submit
                                let state = create_modal_state.read().clone();
                                if state.can_submit() {
                                    if let Some(ref callback) = on_create_channel {
                                        let name = state.get_name().to_string();
                                        let topic = state.get_topic().map(|s| s.to_string());
                                        callback(name, topic);
                                    }
                                    // Close modal
                                    let mut state = create_modal_state.read().clone();
                                    state.hide();
                                    create_modal_state.set(state);
                                }
                            }
                            KeyCode::Backspace => {
                                // Delete character (with throttle)
                                if input_throttle.write().try_input() {
                                    let mut state = create_modal_state.read().clone();
                                    state.pop_char();
                                    create_modal_state.set(state);
                                }
                            }
                            KeyCode::Char(c) => {
                                // Add character (with throttle)
                                if input_throttle.write().try_input() {
                                    let mut state = create_modal_state.read().clone();
                                    state.push_char(c);
                                    create_modal_state.set(state);
                                }
                            }
                            _ => {}
                        }
                    } else if focus.get() == ChatFocus::Input {
                        // Insert mode: only handle Escape, Enter, Backspace, and character input
                        match code {
                            // Escape exits insert mode back to channels
                            KeyCode::Esc => {
                                focus.set(ChatFocus::Channels);
                            }
                            // Shift+Enter adds newline, plain Enter sends message
                            KeyCode::Enter => {
                                if modifiers.contains(KeyModifiers::SHIFT) {
                                    // Shift+Enter: add newline
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.push('\n');
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                } else {
                                    // Plain Enter: send message
                                    if let Ok(text) = input_text.read() {
                                        let text = text.clone();
                                        if !text.is_empty() {
                                            if let Some(ref callback) = on_send {
                                                callback(current_channel_id.clone(), text);
                                            }
                                            if let Ok(mut guard) = input_text.write() {
                                                guard.clear();
                                            }
                                            input_version.set(input_version.get().wrapping_add(1));
                                        }
                                    }
                                }
                            }
                            // Backspace removes last character (with throttle)
                            KeyCode::Backspace => {
                                if input_throttle.write().try_input() {
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.pop();
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                }
                            }
                            // Character input (including "/" for commands) with throttle
                            KeyCode::Char(c) => {
                                if !modifiers.contains(KeyModifiers::CONTROL)
                                    && input_throttle.write().try_input()
                                {
                                    if let Ok(mut guard) = input_text.write() {
                                        guard.push(c);
                                    }
                                    input_version.set(input_version.get().wrapping_add(1));
                                }
                            }
                            // All other keys ignored in insert mode
                            _ => {}
                        }
                    } else {
                        // Normal mode: navigation and hotkeys
                        // First handle hotkeys that shouldn't be captured by nav detection
                        match code {
                            // 'i' enters insert mode
                            KeyCode::Char('i') => {
                                focus.set(ChatFocus::Input);
                            }
                            // 'n' opens new channel modal
                            KeyCode::Char('n') => {
                                let mut state = create_modal_state.read().clone();
                                state.show();
                                create_modal_state.set(state);
                            }
                            // 'r' retries failed message (when in messages focus)
                            KeyCode::Char('r') => {
                                if focus.get() == ChatFocus::Messages {
                                    // Find the current message and retry if it's failed
                                    let msg_idx = message_idx.get();
                                    if let Some(msg) = messages_for_retry.get(msg_idx) {
                                        use crate::tui::types::DeliveryStatus;
                                        if msg.delivery_status == DeliveryStatus::Failed {
                                            if let Some(ref callback) = on_retry_message {
                                                callback(
                                                    msg.id.clone(),
                                                    current_channel_id.clone(),
                                                    msg.content.clone(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            // 't' opens topic editing modal (when in channels focus)
                            KeyCode::Char('t') => {
                                if focus.get() == ChatFocus::Channels {
                                    // Get current channel's topic (or empty string)
                                    let current_topic = channels
                                        .get(channel_idx.get())
                                        .and_then(|c| c.topic.as_ref())
                                        .map(|t| t.as_str())
                                        .unwrap_or("");
                                    let mut state = topic_modal_state.read().clone();
                                    state.show(&current_channel_id, current_topic);
                                    topic_modal_state.set(state);
                                }
                            }
                            // 'o' opens channel info modal (when in channels focus)
                            KeyCode::Char('o') => {
                                if focus.get() == ChatFocus::Channels {
                                    if let Some(ch) = channels.get(channel_idx.get()) {
                                        let mut state = channel_info_state.read().clone();
                                        state.show(&ch.id, &ch.name, ch.topic.as_deref());
                                        // Set default participants (You + placeholder for others)
                                        // Full participant list can be loaded via ListParticipants command
                                        state.set_participants(vec!["You".to_string()]);
                                        channel_info_state.set(state);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
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

    // Message list border color based on focus
    let msg_border = if messages_focused {
        Theme::BORDER_FOCUS
    } else {
        Theme::BORDER
    };

    // Check if modals are visible
    let is_create_modal_visible = create_modal_state.read().visible;
    let is_topic_modal_visible = topic_modal_state.read().visible;
    let is_channel_info_visible = channel_info_state.read().visible;

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: 100pct,
            height: 100pct,
            flex_grow: 1.0,
            flex_shrink: 1.0,
            overflow: Overflow::Hidden,
        ) {
            // Main content area
            View(
                flex_direction: FlexDirection::Row,
                flex_grow: 1.0,
                flex_shrink: 1.0,
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

            // Message input
            MessageInput(
                value: display_input_text,
                placeholder: "Type a message...".to_string(),
                focused: input_focused,
                reply_to: None::<String>,
                sending: false,
            )

            // Create channel modal (overlay)
            ChatCreateModal(
                visible: is_create_modal_visible,
                focused: is_create_modal_visible,
                name: create_modal_state.read().name.clone(),
                topic: create_modal_state.read().topic.clone(),
                active_field: create_modal_state.read().active_field,
                error: create_modal_state.read().error.clone().unwrap_or_default(),
                creating: create_modal_state.read().creating,
            )

            // Topic editing modal (overlay)
            TextInputModal(
                visible: is_topic_modal_visible,
                title: "Set Channel Topic".to_string(),
                value: topic_modal_state.read().value.clone(),
                placeholder: "Enter topic...".to_string(),
                error: topic_modal_state.read().error.clone(),
            )

            // Channel info modal (overlay)
            ChannelInfoModal(
                visible: is_channel_info_visible,
                channel_name: channel_info_state.read().channel_name.clone(),
                topic: channel_info_state.read().topic.clone(),
                participants: channel_info_state.read().participants.clone(),
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

    let initial_idx: usize = 0;
    element! {
        ChatScreen(
            channels: channels,
            messages: messages,
            initial_channel_index: initial_idx,
        )
    }
    .fullscreen()
    .await
}
