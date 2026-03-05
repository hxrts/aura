use super::*;

pub(super) fn handle_channel_selection_change(
    current: &TuiState,
    new_state: &TuiState,
    shared_channels: &Arc<std::sync::RwLock<Vec<Channel>>>,
    tui_selected: &Arc<std::sync::RwLock<usize>>,
    selected_channel_id: &Arc<std::sync::RwLock<Option<String>>>,
    callbacks: &CallbackRegistry,
) {
    if new_state.chat.selected_channel == current.chat.selected_channel {
        return;
    }

    let idx = new_state.chat.selected_channel;

    if let Ok(mut guard) = tui_selected.write() {
        *guard = idx;
    }

    let channels = match shared_channels.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some(channel) = channels.get(idx) {
        if let Ok(mut selected_id) = selected_channel_id.write() {
            *selected_id = Some(channel.id.clone());
        }
        (callbacks.chat.on_channel_select)(channel.id.clone());
    }
}
