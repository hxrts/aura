use aura_app::views::{
    home::{HomeState, HomesState},
    invitations::InvitationType,
};
use aura_core::types::identifiers::{ChannelId, ContextId};
use aura_invitation::InvitationType as DomainInvitationType;

pub(crate) fn map_invitation_type(inv_type: &DomainInvitationType) -> InvitationType {
    match inv_type {
        DomainInvitationType::Contact { .. } => InvitationType::Home,
        DomainInvitationType::Guardian { .. } => InvitationType::Guardian,
        DomainInvitationType::Channel { .. } => InvitationType::Chat,
        DomainInvitationType::DeviceEnrollment { .. } => InvitationType::Home,
    }
}

pub(crate) fn map_channel_metadata(
    inv_type: &DomainInvitationType,
) -> (Option<ChannelId>, Option<String>) {
    match inv_type {
        DomainInvitationType::Channel {
            home_id,
            nickname_suggestion,
            ..
        } => (
            Some(*home_id),
            nickname_suggestion
                .clone()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
        ),
        _ => (None, None),
    }
}

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
