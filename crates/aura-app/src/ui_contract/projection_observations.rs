//! Observations of canonical view snapshots; these never establish readiness.

use super::{HomeModeSnapshot, RuntimeFact};
use crate::views::chat::{is_note_to_self_channel_name, ChatState};
use crate::views::home::HomesState;

/// Describe the observed chat signal, using an existing selected channel or
/// the deterministic default. No membership, routing, or delivery is inferred.
pub fn observed_chat_projection(chat: &ChatState, selected: Option<&str>) -> RuntimeFact {
    let mut channels = chat.all_channels().collect::<Vec<_>>();
    channels.sort_by_key(|channel| {
        (
            !is_note_to_self_channel_name(&channel.name),
            channel.name.clone(),
            channel.id.to_string(),
        )
    });
    let active = selected
        .and_then(|id| {
            channels
                .iter()
                .find(|channel| channel.id.to_string().eq_ignore_ascii_case(id))
        })
        .copied()
        .or_else(|| channels.first().copied());
    RuntimeFact::ChatSignalUpdated {
        active_channel: active
            .map(|channel| channel.name.clone())
            .unwrap_or_default(),
        channel_count: saturating_count(channels.len()),
        message_count: saturating_count(
            active
                .map(|channel| chat.messages_for_channel(&channel.id).len())
                .unwrap_or_default(),
        ),
    }
}

/// Describe observed contacts/discovery counts. The legacy `RemoteFactsPulled`
/// name does not attest that any transport pull occurred or succeeded.
pub fn observed_contacts_projection(contact_count: usize, lan_peer_count: usize) -> RuntimeFact {
    RuntimeFact::RemoteFactsPulled {
        contact_count: saturating_count(contact_count),
        lan_peer_count: saturating_count(lan_peer_count),
    }
}

/// Export only canonical homes with their authoritative context binding.
/// Missing contexts cannot be repaired from neighborhood labels or raw facts.
pub fn observed_home_modes(homes: &HomesState) -> Vec<HomeModeSnapshot> {
    let mut observations = homes
        .all_homes()
        .filter_map(|home| {
            Some(HomeModeSnapshot {
                channel_id: home.id.to_string(),
                context_id: home.context_id?.to_string(),
                mode_flags: home.mode_flags.clone(),
            })
        })
        .collect::<Vec<_>>();
    observations.sort_by(|left, right| {
        left.channel_id
            .cmp(&right.channel_id)
            .then_with(|| left.context_id.cmp(&right.context_id))
    });
    observations
}

fn saturating_count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_modes_preserve_canonical_bindings_and_require_context() {
        use crate::views::home::HomeState;
        use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
        let owner = AuthorityId::new_from_entropy([1; 32]);
        let context = ContextId::new_from_entropy([2; 32]);
        let mut homes = HomesState::new();
        for byte in [3, 1, 2] {
            let mut home =
                HomeState::new(ChannelId::from_bytes([byte; 32]), None, owner, 0, context);
            home.mode_flags = Some("mi".into());
            if byte == 2 {
                home.context_id = None;
            }
            homes.add_home(home);
        }
        let modes = observed_home_modes(&homes);
        assert_eq!(modes.len(), 2);
        assert!(modes[0].channel_id < modes[1].channel_id);
        assert_eq!(
            modes[0].channel_id,
            ChannelId::from_bytes([1; 32]).to_string()
        );
        assert_eq!(
            modes[1].channel_id,
            ChannelId::from_bytes([3; 32]).to_string()
        );
        assert!(modes
            .iter()
            .all(|home| home.context_id == context.to_string()
                && home.mode_flags.as_deref() == Some("mi")));
    }

    #[test]
    fn snapshot_wire_requires_home_modes_and_rejects_duplicate_bindings() {
        use super::super::{ScreenId, UiSnapshot};
        let mut snapshot = UiSnapshot::loading(ScreenId::Neighborhood);
        let mut wire = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(wire["home_modes"], serde_json::json!([]));
        wire.as_object_mut().unwrap().remove("home_modes");
        assert!(serde_json::from_value::<UiSnapshot>(wire).is_err());
        let mode = HomeModeSnapshot {
            channel_id: "home".into(),
            context_id: "context".into(),
            mode_flags: None,
        };
        snapshot.home_modes = vec![mode.clone(), mode];
        assert!(snapshot.validate_invariants().is_err());
    }

    #[test]
    fn missing_selected_channel_does_not_invent_readiness_or_metadata() {
        assert_eq!(
            observed_chat_projection(&ChatState::default(), Some("missing")),
            RuntimeFact::ChatSignalUpdated {
                active_channel: String::new(),
                channel_count: 0,
                message_count: 0,
            }
        );
    }

    #[test]
    fn projection_counts_are_bounded_observations_even_without_remote_work() {
        assert_eq!(
            observed_contacts_projection(0, 0),
            RuntimeFact::RemoteFactsPulled {
                contact_count: 0,
                lan_peer_count: 0,
            }
        );
        assert_eq!(
            observed_contacts_projection(usize::MAX, usize::MAX),
            RuntimeFact::RemoteFactsPulled {
                contact_count: u32::MAX,
                lan_peer_count: u32::MAX,
            }
        );
    }
}
