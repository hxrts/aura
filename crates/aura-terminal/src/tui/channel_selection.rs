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

#[must_use]
pub fn strongest_authoritative_binding_for_channel(
    channel: &Channel,
    selected: Option<&CommittedChannelSelection>,
) -> Option<ChannelBindingWitness> {
    if let Some(binding) = selected
        .map(CommittedChannelSelection::binding)
        .filter(|binding| binding.channel_id == channel.id)
        .filter(|binding| binding.context_id.is_some())
    {
        return Some(binding.clone());
    }

    channel
        .context_id
        .clone()
        .map(|context_id| ChannelBindingWitness::new(channel.id.clone(), Some(context_id)))
}

#[cfg(test)]
mod tests {
    use super::{strongest_authoritative_binding_for_channel, CommittedChannelSelection};
    use crate::tui::types::Channel;
    use aura_app::ui_contract::ChannelBindingWitness;

    #[test]
    fn strongest_binding_prefers_selected_authoritative_context() {
        let mut channel = Channel::new("chan-1", "General");
        channel.context_id = Some("projection-ctx".to_string());
        let selected = CommittedChannelSelection::from_binding(&ChannelBindingWitness::new(
            "chan-1",
            Some("selected-ctx".to_string()),
        ));

        let binding = strongest_authoritative_binding_for_channel(&channel, Some(&selected))
            .expect("binding");

        assert_eq!(binding.context_id.as_deref(), Some("selected-ctx"));
    }

    #[test]
    fn strongest_binding_falls_back_to_channel_projection_context() {
        let mut channel = Channel::new("chan-1", "General");
        channel.context_id = Some("projection-ctx".to_string());
        let selected = CommittedChannelSelection::from_binding(&ChannelBindingWitness::new(
            "chan-1",
            None::<String>,
        ));

        let binding = strongest_authoritative_binding_for_channel(&channel, Some(&selected))
            .expect("binding");

        assert_eq!(binding.context_id.as_deref(), Some("projection-ctx"));
    }
}
