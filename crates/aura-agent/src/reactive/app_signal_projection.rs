use aura_app::views::{
    home::{HomeState, HomesState},
    invitations::InvitationType,
};
use aura_core::types::identifiers::{ChannelId, ContextId};
use aura_invitation::InvitationType as DomainInvitationType;

const OBSERVED_PROJECTION_INVITATION_TYPE_MAPPING_CAPABILITY: &str =
    "observed_projection_invitation_type_mapping";
const OBSERVED_PROJECTION_CHANNEL_METADATA_CAPABILITY: &str =
    "observed_projection_channel_metadata";
const OBSERVED_PROJECTION_MODERATION_HOMES_CAPABILITY: &str =
    "observed_projection_moderation_homes";
#[cfg(test)]
const OBSERVED_PROJECTION_MODERATION_HOME_SELECTION_CAPABILITY: &str =
    "observed_projection_moderation_home_selection";

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "observed_projection_invitation_type_mapping",
    family = "runtime_helper"
)]
pub(crate) fn map_invitation_type(inv_type: &DomainInvitationType) -> InvitationType {
    let _ = OBSERVED_PROJECTION_INVITATION_TYPE_MAPPING_CAPABILITY;
    match inv_type {
        DomainInvitationType::Contact { .. } => InvitationType::Contact,
        DomainInvitationType::Guardian { .. } => InvitationType::Guardian,
        DomainInvitationType::Channel { .. } => InvitationType::Chat,
        DomainInvitationType::DeviceEnrollment { .. } => InvitationType::Home,
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "observed_projection_channel_metadata",
    family = "runtime_helper"
)]
pub(crate) fn map_channel_metadata(
    inv_type: &DomainInvitationType,
) -> (Option<ChannelId>, Option<String>) {
    let _ = OBSERVED_PROJECTION_CHANNEL_METADATA_CAPABILITY;
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

/// Display name of an invitation's sender: the nickname a contact invitation
/// carries, or "Unknown" when the invitation names no sender.
pub(crate) fn invitation_sender_name(inv_type: &DomainInvitationType) -> String {
    match inv_type {
        DomainInvitationType::Contact {
            nickname: Some(nickname),
        } if !nickname.trim().is_empty() => nickname.trim().to_string(),
        _ => "Unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contact_invitations_name_their_sender() {
        assert_eq!(
            invitation_sender_name(&DomainInvitationType::Contact {
                nickname: Some(" Alex ".to_string())
            }),
            "Alex"
        );
        assert_eq!(
            invitation_sender_name(&DomainInvitationType::Contact { nickname: None }),
            "Unknown"
        );
    }

    #[test]
    fn invitation_kinds_project_to_distinct_view_types() {
        assert_eq!(
            map_invitation_type(&DomainInvitationType::Contact { nickname: None }),
            InvitationType::Contact
        );
        assert_eq!(
            map_invitation_type(&DomainInvitationType::Channel {
                home_id: aura_core::types::identifiers::ChannelId::from_bytes([1; 32]),
                nickname_suggestion: None,
                bootstrap: None,
            }),
            InvitationType::Chat
        );
    }
}
