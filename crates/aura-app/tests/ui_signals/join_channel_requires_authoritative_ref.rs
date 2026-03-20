use async_lock::RwLock;
use std::sync::Arc;

async fn call_join_channel_with_raw_id(
    app_core: &Arc<RwLock<aura_app::AppCore>>,
    channel_id: aura_core::types::identifiers::ChannelId,
) {
    let _ = aura_app::ui::workflows::messaging::join_channel(app_core, channel_id).await;
}

fn main() {}
