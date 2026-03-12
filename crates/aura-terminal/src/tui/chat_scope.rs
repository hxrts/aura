//! Chat scoping helpers tied to neighborhood traversal state.

use aura_app::ui::types::{
    chat::is_note_to_self_channel_name, Channel as AppChannel, ChannelType, ChatState,
    NeighborhoodState,
};
use aura_core::identifiers::ChannelId;

/// Resolve the active home scope for chat from neighborhood traversal state.
#[must_use]
pub fn active_home_scope_id(neighborhood: &NeighborhoodState) -> String {
    let scope_id = neighborhood
        .position
        .as_ref()
        .map(|p| p.current_home_id)
        .unwrap_or(neighborhood.home_home_id);

    // Query-backed neighborhood snapshots may carry a zero placeholder home.
    // In that case, return empty scope so chat falls back to unfiltered channels.
    if scope_id == ChannelId::default() {
        String::new()
    } else {
        scope_id.to_string()
    }
}

/// Resolve the effective home scope for chat publication and rendering.
///
/// An explicit selected non-DM shared channel takes precedence over neighborhood
/// traversal because the chat view may already have committed to a shared channel
/// before the neighborhood projection catches up.
#[must_use]
pub fn effective_home_scope_id(
    chat_state: &ChatState,
    active_home_scope: Option<&str>,
    selected_channel_id: Option<&str>,
) -> Option<String> {
    let selected_channel = selected_channel_id.and_then(|selected_id| {
        chat_state
            .all_channels()
            .find(|channel| channel.id.to_string() == selected_id)
    });

    if let Some(channel) = selected_channel.filter(|channel| !is_pinned_channel(channel)) {
        if channel.channel_type == ChannelType::Home {
            return Some(channel.id.to_string());
        }

        if let Some(context_id) = channel.context_id {
            if let Some(home_channel) = chat_state.all_channels().find(|candidate| {
                candidate.channel_type == ChannelType::Home
                    && !is_pinned_channel(candidate)
                    && candidate.context_id == Some(context_id)
            }) {
                return Some(home_channel.id.to_string());
            }
        }

        return Some(channel.id.to_string());
    }

    active_home_scope
        .map(str::trim)
        .filter(|scope| !scope.is_empty())
        .map(str::to_owned)
}

/// Returns true when the channel is a DM-like stream that should stay visible.
#[must_use]
pub fn is_dm_like_channel(channel: &AppChannel) -> bool {
    channel.is_dm
        || channel.name.to_ascii_lowercase().starts_with("dm:")
        || channel
            .topic
            .as_deref()
            .map(|topic| topic.to_ascii_lowercase().starts_with("direct messages"))
            .unwrap_or(false)
}

#[must_use]
pub fn is_pinned_channel(channel: &AppChannel) -> bool {
    is_dm_like_channel(channel) || is_note_to_self_channel_name(&channel.name)
}

/// Returns scoped chat channels while preserving visibility when scope metadata is incomplete.
#[must_use]
pub fn scoped_channels<'a>(
    chat_state: &'a ChatState,
    active_home_scope: Option<&str>,
) -> Vec<&'a AppChannel> {
    let active_home_scope = active_home_scope
        .map(str::trim)
        .filter(|scope| !scope.is_empty());
    let active_home_channel = active_home_scope.and_then(|scope| {
        chat_state
            .all_channels()
            .find(|channel| channel.id.to_string() == scope)
    });
    let has_active_home_channel = active_home_channel.is_some();
    let active_home_context = active_home_channel.and_then(|channel| channel.context_id);

    let mut channels: Vec<_> = chat_state
        .all_channels()
        .filter(|channel| {
            if is_pinned_channel(channel) {
                return true;
            }

            match active_home_scope {
                None => true,
                Some(scope) => {
                    let id_match = channel.id.to_string() == scope;
                    let context_match = active_home_context
                        .map(|ctx| channel.context_id == Some(ctx))
                        .unwrap_or(false);
                    // If home metadata has not landed in CHAT_SIGNAL yet, keep channels visible.
                    id_match || context_match || !has_active_home_channel
                }
            }
        })
        .collect();

    channels.sort_by(|left, right| {
        match (
            is_note_to_self_channel_name(&left.name),
            is_note_to_self_channel_name(&right.name),
        ) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.name.cmp(&right.name),
        }
    });

    channels
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::types::{
        Channel as AppChannel, ChannelType, ChatState, NeighborhoodState, TraversalPosition,
    };
    use aura_core::crypto::hash::hash;
    use aura_core::identifiers::ChannelId;

    fn test_channel_id(seed: &str) -> ChannelId {
        ChannelId::from_bytes(hash(seed.as_bytes()))
    }

    #[test]
    fn active_scope_uses_home_when_not_traversing() {
        let home_id = test_channel_id("home");
        let state = NeighborhoodState::from_parts(home_id, "Home".to_string(), []);
        assert_eq!(active_home_scope_id(&state), home_id.to_string());
    }

    #[test]
    fn active_scope_uses_traversal_position_when_present() {
        let home_id = test_channel_id("home");
        let target_id = test_channel_id("target");
        let mut state = NeighborhoodState::from_parts(home_id, "Home".to_string(), []);
        state.position = Some(TraversalPosition {
            current_home_id: target_id,
            current_home_name: "Target".to_string(),
            depth: 1,
            path: vec![home_id, target_id],
        });
        assert_eq!(active_home_scope_id(&state), target_id.to_string());
    }

    #[test]
    fn active_scope_is_empty_for_zero_placeholder_home() {
        let state = NeighborhoodState::default();
        assert_eq!(active_home_scope_id(&state), "");
    }

    #[test]
    fn channel_scope_match_defaults_to_true_without_scope() {
        let home_id = test_channel_id("home");
        let channel_id = test_channel_id("porch");
        let state = ChatState::from_channels([test_channel(channel_id, "porch")]);

        let scoped = scoped_channels(&state, Some(home_id.to_string().as_str()));
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, channel_id);
    }

    #[test]
    fn scoped_channels_filters_to_home_when_root_home_channel_present() {
        let home_id = test_channel_id("home");
        let other_id = test_channel_id("other");
        let state = ChatState::from_channels([
            test_channel(home_id, "home"),
            test_channel(other_id, "other"),
        ]);

        let scope = home_id.to_string();
        let scoped = scoped_channels(&state, Some(scope.as_str()));
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, home_id);
    }

    fn test_channel(id: ChannelId, name: &str) -> AppChannel {
        AppChannel {
            id,
            context_id: None,
            name: name.to_string(),
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: Vec::new(),
            member_count: 1,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        }
    }
}
