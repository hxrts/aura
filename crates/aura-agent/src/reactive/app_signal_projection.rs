use aura_app::views::home::{HomeState, HomesState};
use aura_core::types::identifiers::{ChannelId, ContextId};

pub(crate) fn collect_moderation_homes(
    homes: &HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Vec<HomeState> {
    let mut candidates = Vec::new();

    if let Some(home) = homes.home_state(&channel_id) {
        if home.context_id == Some(context_id) {
            candidates.push(home.clone());
        }
    }

    for (_, home) in homes.iter() {
        if home.context_id == Some(context_id)
            && !candidates
                .iter()
                .any(|candidate: &HomeState| candidate.id == home.id)
        {
            candidates.push(home.clone());
        }
    }

    candidates
}

#[cfg(test)]
pub(crate) fn select_moderation_home(
    homes: &HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Option<HomeState> {
    if let Some(home) = homes.home_state(&channel_id) {
        if home.context_id == Some(context_id) {
            return Some(home.clone());
        }
    }

    let candidates = collect_moderation_homes(homes, context_id, channel_id);
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }

    None
}
