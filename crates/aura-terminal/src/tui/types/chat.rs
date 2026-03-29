use aura_app::ui::types::{
    chat::{Channel as AppChannel, Message as AppMessage},
    format_timestamp,
};

pub use aura_app::ui::types::MessageDeliveryStatus as DeliveryStatus;

/// A chat channel presentation model.
#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub id: String,
    pub context_id: Option<String>,
    pub name: String,
    pub topic: Option<String>,
    pub unread_count: usize,
    pub is_selected: bool,
    pub member_count: u32,
}

impl From<&AppChannel> for Channel {
    fn from(ch: &AppChannel) -> Self {
        Self {
            id: ch.id.to_string(),
            context_id: ch.context_id.map(|id| id.to_string()),
            name: ch.name.clone(),
            topic: ch.topic.clone(),
            unread_count: ch.unread_count as usize,
            is_selected: false,
            member_count: ch.member_count,
        }
    }
}

impl Channel {
    /// Create from aura-app `Channel` with selection state.
    pub fn from_app(ch: &AppChannel, is_selected: bool) -> Self {
        Self {
            is_selected,
            ..Self::from(ch)
        }
    }

    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            context_id: None,
            name: name.into(),
            topic: None,
            unread_count: 0,
            is_selected: false,
            member_count: 0,
        }
    }

    pub fn with_unread(mut self, count: usize) -> Self {
        self.unread_count = count;
        self
    }

    pub fn with_topic(mut self, topic: impl Into<String>) -> Self {
        self.topic = Some(topic.into());
        self
    }

    pub fn selected(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }
}

/// A chat message presentation model.
#[derive(Clone, Debug, Default)]
pub struct Message {
    pub id: String,
    /// Channel this message belongs to.
    pub channel_id: String,
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub is_own: bool,
    /// Delivery status for own messages.
    pub delivery_status: DeliveryStatus,
    /// Whether this message has been finalized by consensus.
    pub is_finalized: bool,
}

impl From<&AppMessage> for Message {
    fn from(msg: &AppMessage) -> Self {
        Self {
            id: msg.id.clone(),
            channel_id: msg.channel_id.to_string(),
            sender: msg.sender_name.clone(),
            content: msg.content.clone(),
            timestamp: format_timestamp(msg.timestamp),
            is_own: msg.is_own,
            delivery_status: msg.delivery_status,
            is_finalized: msg.is_finalized,
        }
    }
}

impl Message {
    pub fn new(
        id: impl Into<String>,
        sender: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: String::new(),
            sender: sender.into(),
            content: content.into(),
            timestamp: String::new(),
            is_own: false,
            delivery_status: DeliveryStatus::default(),
            is_finalized: false,
        }
    }

    /// Create a new message in sending state for optimistic UI.
    pub fn sending(
        id: impl Into<String>,
        channel_id: impl Into<String>,
        sender: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            channel_id: channel_id.into(),
            sender: sender.into(),
            content: content.into(),
            timestamp: String::new(),
            is_own: true,
            delivery_status: DeliveryStatus::Sending,
            is_finalized: false,
        }
    }

    pub fn with_channel(mut self, channel_id: impl Into<String>) -> Self {
        self.channel_id = channel_id.into();
        self
    }

    pub fn with_status(mut self, status: DeliveryStatus) -> Self {
        self.delivery_status = status;
        self
    }

    pub fn with_timestamp(mut self, ts: impl Into<String>) -> Self {
        self.timestamp = ts.into();
        self
    }

    pub fn own(mut self, is_own: bool) -> Self {
        self.is_own = is_own;
        self
    }

    pub fn with_finalized(mut self, is_finalized: bool) -> Self {
        self.is_finalized = is_finalized;
        self
    }
}
