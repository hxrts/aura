use super::*;

pub(super) struct InputTransition {
    pub(super) current: TuiState,
    pub(super) new_state: TuiState,
    pub(super) commands: Vec<TuiCommand>,
}

pub(super) fn transition_from_terminal_event(
    event: iocraft::prelude::TerminalEvent,
    tui: &TuiStateHandle,
    shared_channels: &Arc<std::sync::RwLock<Vec<Channel>>>,
    shared_homes: &Arc<std::sync::RwLock<Vec<String>>>,
    shared_home_meta: &SharedNeighborhoodHomeMeta,
    selected_channel_id: &Arc<std::sync::RwLock<Option<String>>>,
) -> Option<InputTransition> {
    let core_event = convert_iocraft_event(event)?;

    let mut current = tui.read_clone();
    let current_channels = match shared_channels.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some(channel) = current_channels.get(current.chat.selected_channel) {
        if let Ok(mut selected_id) = selected_channel_id.write() {
            *selected_id = Some(channel.id.clone());
        }
    }

    sync_neighborhood_navigation_state(
        &mut current,
        shared_homes,
        shared_channels,
        shared_home_meta,
    );

    let (mut new_state, commands) = transition(&current, core_event);

    sync_neighborhood_navigation_state(
        &mut new_state,
        shared_homes,
        shared_channels,
        shared_home_meta,
    );

    Some(InputTransition {
        current,
        new_state,
        commands,
    })
}
