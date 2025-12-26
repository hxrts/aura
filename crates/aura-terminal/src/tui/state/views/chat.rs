//! Chat screen view state

/// Chat screen focus
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ChatFocus {
    /// Channel list has focus
    #[default]
    Channels,
    /// Message list has focus
    Messages,
    /// Input field has focus
    Input,
}

/// Chat screen state
#[derive(Clone, Debug, Default)]
pub struct ChatViewState {
    /// Current focus (channels, messages, input)
    pub focus: ChatFocus,
    /// Selected channel index
    pub selected_channel: usize,
    /// Total channel count (for wrap-around navigation)
    pub channel_count: usize,
    /// Scroll position in message list
    pub message_scroll: usize,
    /// Total message count (for wrap-around navigation)
    pub message_count: usize,
    /// Input buffer for message composition
    pub input_buffer: String,
    /// Whether in insert mode
    pub insert_mode: bool,
    /// Character used to enter insert mode (to prevent it being typed)
    pub insert_mode_entry_char: Option<char>,
    // Note: Modal state is now stored in ModalQueue, not here.
    // Use modal_queue.enqueue(QueuedModal::ChatCreate/Topic/Info(...)) to show modals.
}

/// State for create channel modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
/// Use `modal_queue.enqueue(QueuedModal::ChatCreate(state))` to show.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CreateChannelStep {
    #[default]
    Details,
    Members,
    Threshold,
    Review,
    Waiting,
}

#[derive(Clone, Debug, Default)]
pub struct ChatMemberCandidate {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default)]
pub struct CreateChannelModalState {
    /// Wizard step
    pub step: CreateChannelStep,
    /// Channel name input
    pub name: String,
    /// Optional topic input
    pub topic: String,
    /// Selected member contact/authority IDs to add to the channel
    pub member_ids: Vec<String>,
    /// Current input field (0 = name, 1 = topic)
    pub active_field: usize,
    /// Available contacts
    pub contacts: Vec<ChatMemberCandidate>,
    /// Selected contact indices
    pub selected_indices: Vec<usize>,
    /// Focused contact index
    pub focused_index: usize,
    /// Threshold k (m-of-n)
    pub threshold_k: u8,
    /// Whether the threshold was manually adjusted
    pub threshold_custom: bool,
    /// Status message (e.g., waiting for acceptances)
    pub status: Option<String>,
    /// Error message if any
    pub error: Option<String>,
}

impl CreateChannelModalState {
    /// Create a new modal state ready to be enqueued
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset state (called when dismissed or re-opened)
    pub fn reset(&mut self) {
        self.step = CreateChannelStep::Details;
        self.name.clear();
        self.topic.clear();
        self.member_ids.clear();
        self.active_field = 0;
        self.contacts.clear();
        self.selected_indices.clear();
        self.focused_index = 0;
        self.threshold_k = 1;
        self.threshold_custom = false;
        self.status = None;
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        !self.name.trim().is_empty()
    }

    pub fn selected_member_ids(&self) -> Vec<String> {
        self.selected_indices
            .iter()
            .filter_map(|idx| self.contacts.get(*idx))
            .map(|c| c.id.clone())
            .collect()
    }

    pub fn total_participants(&self) -> u8 {
        (self.selected_indices.len() + 1) as u8
    }

    pub fn default_threshold(total_n: u8) -> u8 {
        if total_n <= 1 {
            return 1;
        }
        let f = total_n.saturating_sub(1) / 3;
        let k = (2 * f) + 1;
        k.clamp(1, total_n)
    }

    pub fn ensure_threshold(&mut self) {
        let total_n = self.total_participants();
        if !self.threshold_custom {
            self.threshold_k = Self::default_threshold(total_n);
        } else {
            self.threshold_k = self.threshold_k.clamp(1, total_n.max(1));
        }
    }

    pub fn toggle_selection(&mut self) {
        if let Some(pos) = self
            .selected_indices
            .iter()
            .position(|&i| i == self.focused_index)
        {
            self.selected_indices.remove(pos);
        } else {
            self.selected_indices.push(self.focused_index);
        }
        self.member_ids = self.selected_member_ids();
        self.ensure_threshold();
    }
}

/// State for topic edit modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct TopicModalState {
    /// Topic input value
    pub value: String,
    /// Channel ID being edited
    pub channel_id: String,
    /// Error message if any
    pub error: Option<String>,
}

impl TopicModalState {
    /// Create initialized state for a channel topic edit
    pub fn for_channel(channel_id: &str, current_topic: &str) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            value: current_topic.to_string(),
            error: None,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.value.clear();
        self.channel_id.clear();
        self.error = None;
    }
}

/// State for channel info modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct ChannelInfoModalState {
    /// Channel ID
    pub channel_id: String,
    /// Channel name
    pub channel_name: String,
    /// Channel topic
    pub topic: String,
    /// Participants
    pub participants: Vec<String>,
}

impl ChannelInfoModalState {
    /// Create initialized state for channel info display
    pub fn for_channel(channel_id: &str, name: &str, topic: Option<&str>) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            channel_name: name.to_string(),
            topic: topic.unwrap_or("").to_string(),
            participants: Vec::new(),
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.channel_id.clear();
        self.channel_name.clear();
        self.topic.clear();
        self.participants.clear();
    }
}
