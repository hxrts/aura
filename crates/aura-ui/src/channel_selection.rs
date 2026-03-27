use crate::UiController;
use aura_app::ui::signals::CHAT_SIGNAL;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::types::identifiers::{ChannelId, ContextId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthoritativeSelectedChannel {
    pub channel_id: ChannelId,
    pub context_id: ContextId,
    pub channel_name: String,
}

pub async fn selected_authoritative_channel(
    controller: &UiController,
) -> Result<AuthoritativeSelectedChannel, String> {
    let selected_channel_id = controller
        .selected_channel_id()
        .ok_or_else(|| "Select a channel first".to_string())?;
    let channel_id = selected_channel_id
        .parse::<ChannelId>()
        .map_err(|error| format!("Invalid selected channel id: {error}"))?;
    let channel = {
        let core = controller.app_core().read().await;
        let chat = core.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        chat.channel(&channel_id)
            .cloned()
            .ok_or_else(|| format!("Selected channel is stale or unavailable: {channel_id}"))?
    };
    let context_id = channel
        .context_id
        .ok_or_else(|| format!("Selected channel lacks authoritative context: {channel_id}"))?;
    Ok(AuthoritativeSelectedChannel {
        channel_id,
        context_id,
        channel_name: channel.name,
    })
}
