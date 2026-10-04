//! Observations of canonical view snapshots; these never establish readiness.

use super::RuntimeFact;
use crate::views::chat::{is_note_to_self_channel_name, ChatState};

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

fn saturating_count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

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
