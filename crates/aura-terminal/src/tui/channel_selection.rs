use crate::tui::types::Channel;
use aura_app::ui_contract::ChannelBindingWitness;
use parking_lot::RwLock;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedChannelSelection {
    binding: ChannelBindingWitness,
}

impl CommittedChannelSelection {
    #[must_use]
    pub fn new(channel_id: impl Into<String>) -> Self {
        Self {
            binding: ChannelBindingWitness::new(channel_id.into(), None),
        }
    }

    #[must_use]
    pub fn from_binding(binding: &ChannelBindingWitness) -> Self {
        Self {
            binding: binding.clone(),
        }
    }

    #[must_use]
    pub fn channel_id(&self) -> &str {
        &self.binding.channel_id
    }

    #[must_use]
    pub fn binding(&self) -> &ChannelBindingWitness {
        &self.binding
    }
}

pub type SharedCommittedChannelSelection = Arc<RwLock<Option<CommittedChannelSelection>>>;

#[must_use]
pub fn authoritative_channel_binding(channel: &Channel) -> ChannelBindingWitness {
    ChannelBindingWitness::new(channel.id.clone(), channel.context_id.clone())
}

#[must_use]
pub fn authoritative_committed_selection(channel: &Channel) -> CommittedChannelSelection {
    CommittedChannelSelection::from_binding(&authoritative_channel_binding(channel))
}
