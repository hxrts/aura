use crate::tui::types::Channel;
use aura_app::ui_contract::ChannelBindingWitness;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedChannelSelection {
    channel_id: String,
}

impl CommittedChannelSelection {
    #[must_use]
    pub fn new(channel_id: impl Into<String>) -> Self {
        Self {
            channel_id: channel_id.into(),
        }
    }

    #[must_use]
    pub fn from_binding(binding: &ChannelBindingWitness) -> Self {
        Self::new(binding.channel_id.clone())
    }

    #[must_use]
    pub fn channel_id(&self) -> &str {
        &self.channel_id
    }
}

pub type SharedCommittedChannelSelection = Arc<RwLock<Option<CommittedChannelSelection>>>;

#[must_use]
pub fn authoritative_channel_binding(channel: &Channel) -> ChannelBindingWitness {
    ChannelBindingWitness::new(channel.id.clone(), channel.context_id.clone())
}
