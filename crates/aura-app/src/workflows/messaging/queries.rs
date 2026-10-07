//! Read-side chat queries shared by every frontend: channel lookup by
//! user input, history and message search over the observed chat state.

use super::*;
use crate::signal_defs::CHAT_SIGNAL;
use crate::views::chat::{Channel, Message};
use crate::workflows::signals::read_signal_or_default;

/// The chat state frontends observe: `CHAT_SIGNAL`, which the runtime's
/// reactive pipeline keeps current with inbound messages and membership.
// OWNERSHIP: observed
pub async fn observed_chat(app_core: &Arc<RwLock<AppCore>>) -> ChatState {
    read_signal_or_default(app_core, &*CHAT_SIGNAL).await
}

/// Find the observed channel a user typed: its canonical id, or its name
/// (case-insensitive, with or without a leading `#`).
///
/// Fails with `NotFound` when nothing matches and `Invalid` when a name
/// matches several channels.
// OWNERSHIP: observed
pub async fn resolve_channel(
    app_core: &Arc<RwLock<AppCore>>,
    selector: &str,
) -> Result<Channel, AuraError> {
    let chat = observed_chat(app_core).await;
    select_channel(chat.all_channels(), selector)
}

fn select_channel<'a>(
    channels: impl Iterator<Item = &'a Channel>,
    selector: &str,
) -> Result<Channel, AuraError> {
    let raw = selector.trim();
    if raw.is_empty() {
        return Err(AuraError::invalid("Channel selector cannot be empty"));
    }
    let by_id = raw.parse::<ChannelId>().ok();
    let name = raw.trim_start_matches('#').trim().to_lowercase();
    let matches: Vec<&Channel> = channels
        .filter(|channel| match by_id {
            Some(id) => channel.id == id,
            None => channel.name.to_lowercase() == name,
        })
        .collect();
    match matches.as_slice() {
        [channel] => Ok((*channel).clone()),
        [] => {
            let error = ChannelSelectorError::NotFound {
                selector: raw.to_string(),
            };
            Err(AuraError::NotFound {
                message: error.to_string(),
                source: Some(Arc::new(error)),
            })
        }
        several => {
            let error = ChannelSelectorError::Ambiguous {
                selector: raw.to_string(),
                matches: several.len(),
            };
            Err(AuraError::Invalid {
                message: error.to_string(),
                source: Some(Arc::new(error)),
            })
        }
    }
}

/// Why a typed channel selector does not name exactly one channel.
#[derive(Debug, thiserror::Error)]
pub enum ChannelSelectorError {
    /// No observed channel has this name or id.
    #[error("Channel {selector} not found")]
    NotFound { selector: String },
    /// Several observed channels share this name.
    #[error("Channel name {selector} matches {matches} channels; use its id")]
    Ambiguous { selector: String, matches: usize },
}

/// The last `limit` messages of a channel, oldest first, optionally only
/// those from `sender`.
// OWNERSHIP: observed
pub async fn channel_history(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    limit: Option<usize>,
    sender: Option<AuthorityId>,
) -> Vec<Message> {
    let chat = observed_chat(app_core).await;
    let messages: Vec<Message> = chat
        .messages_for_channel(&channel_id)
        .iter()
        .filter(|message| sender.is_none_or(|s| message.sender_id == s))
        .cloned()
        .collect();
    let skip = limit.map_or(0, |limit| messages.len().saturating_sub(limit));
    messages.into_iter().skip(skip).collect()
}

/// Messages whose text contains `query` (case-insensitive), in one channel
/// or all of them, optionally only from `sender`; at most `limit`, newest
/// first.
// OWNERSHIP: observed
pub async fn search_messages(
    app_core: &Arc<RwLock<AppCore>>,
    query: &str,
    channel_id: Option<ChannelId>,
    sender: Option<AuthorityId>,
    limit: usize,
) -> Vec<Message> {
    let chat = observed_chat(app_core).await;
    let needle = query.to_lowercase();
    let mut found: Vec<Message> = chat
        .all_channels()
        .filter(|channel| channel_id.is_none_or(|id| channel.id == id))
        .flat_map(|channel| chat.messages_for_channel(&channel.id).iter())
        .filter(|message| sender.is_none_or(|s| message.sender_id == s))
        .filter(|message| message.content.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    found.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| a.id.cmp(&b.id)));
    found.truncate(limit);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(seed: u8, name: &str) -> Channel {
        Channel {
            id: ChannelId::from_bytes([seed; 32]),
            name: name.to_string(),
            ..Channel::default()
        }
    }

    #[test]
    fn channels_resolve_by_id_or_case_insensitive_name() {
        let channels = [channel(1, "General"), channel(2, "random")];
        assert_eq!(
            select_channel(channels.iter(), "#general").unwrap().id,
            channels[0].id
        );
        assert_eq!(
            select_channel(channels.iter(), &channels[1].id.to_string())
                .unwrap()
                .name,
            "random"
        );
        assert!(matches!(
            select_channel(channels.iter(), "missing"),
            Err(AuraError::NotFound { .. })
        ));
    }

    #[test]
    fn ambiguous_names_are_rejected() {
        let channels = [channel(1, "dup"), channel(2, "Dup")];
        assert!(matches!(
            select_channel(channels.iter(), "dup"),
            Err(AuraError::Invalid { .. })
        ));
    }
}
