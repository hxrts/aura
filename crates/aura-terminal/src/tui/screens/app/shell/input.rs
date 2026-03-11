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
) -> Option<InputTransition> {
    let core_event = convert_iocraft_event(event)?;

    let mut current = tui.read_clone();

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
