use aura_app::views::home::{HomeState, HomesState};
use aura_core::types::identifiers::{ChannelId, ContextId};

const OBSERVED_PROJECTION_MODERATION_HOMES_CAPABILITY: &str =
    "observed_projection_moderation_homes";
#[cfg(test)]
const OBSERVED_PROJECTION_MODERATION_HOME_SELECTION_CAPABILITY: &str =
    "observed_projection_moderation_home_selection";

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "observed_projection_moderation_homes",
    family = "runtime_helper"
)]
pub(crate) fn collect_moderation_homes(
    homes: &HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Vec<HomeState> {
    let _ = OBSERVED_PROJECTION_MODERATION_HOMES_CAPABILITY;
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
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "observed_projection_moderation_home_selection",
    family = "runtime_helper"
)]
pub(crate) fn select_moderation_home(
    homes: &HomesState,
    context_id: ContextId,
    channel_id: ChannelId,
) -> Option<HomeState> {
    let _ = OBSERVED_PROJECTION_MODERATION_HOME_SELECTION_CAPABILITY;
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
