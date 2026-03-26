use aura_app::ui::contract::ListId;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_app::ui_contract::ChannelBindingWitness;
use aura_core::types::identifiers::ChannelId;
use aura_effects::ReactiveEffects;
use aura_ui::UiController;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WeakChannelSelection(ChannelId);

impl WeakChannelSelection {
    #[must_use]
    pub(crate) fn new(channel_id: ChannelId) -> Self {
        Self(channel_id)
    }

    #[must_use]
    pub(crate) fn channel_id(&self) -> &ChannelId {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelectionError {
    NoSelectedChannel,
    InvalidSelectedChannelId(String),
    MissingSelectedChannelContext(String),
    NoSelectedDevice,
}

impl SelectionError {
    #[must_use]
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::NoSelectedChannel => "no channel is selected".to_string(),
            Self::InvalidSelectedChannelId(error) => {
                format!("invalid selected channel id: {error}")
            }
            Self::MissingSelectedChannelContext(channel_id) => {
                format!("selected channel lacks authoritative context: {channel_id}")
            }
            Self::NoSelectedDevice => "no device is selected".to_string(),
        }
    }
}

pub(crate) fn selected_channel_id(
    controller: &UiController,
) -> Result<WeakChannelSelection, SelectionError> {
    let selected = controller
        .selected_channel_id()
        .ok_or(SelectionError::NoSelectedChannel)?;
    let channel_id = selected
        .parse::<ChannelId>()
        .map_err(|error| SelectionError::InvalidSelectedChannelId(error.to_string()))?;
    Ok(WeakChannelSelection::new(channel_id))
}

pub(crate) async fn selected_channel_binding(
    controller: &UiController,
) -> Result<ChannelBindingWitness, SelectionError> {
    let selection = selected_channel_id(controller)?;
    let channel_id = selection.channel_id().clone();
    let context_id = {
        let core = controller.app_core().read().await;
        let chat = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        chat.channel(&channel_id)
            .and_then(|channel| channel.context_id)
    }
    .ok_or_else(|| SelectionError::MissingSelectedChannelContext(channel_id.to_string()))?;

    Ok(ChannelBindingWitness::new(
        channel_id.to_string(),
        Some(context_id.to_string()),
    ))
}

pub(crate) fn selected_device_id(controller: &UiController) -> Result<String, SelectionError> {
    let snapshot = controller.ui_snapshot();
    snapshot
        .selected_item_id(ListId::Devices)
        .map(str::to_string)
        .ok_or(SelectionError::NoSelectedDevice)
}

pub(crate) fn selected_authority_id(controller: &UiController) -> Option<String> {
    controller.selected_authority_id().map(|id| id.to_string())
}
