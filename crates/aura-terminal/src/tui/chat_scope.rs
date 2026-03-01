//! Chat scoping helpers tied to neighborhood traversal state.

use aura_app::ui::types::{Channel as AppChannel, ChatState, NeighborhoodState};
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

    chat_state
        .all_channels()
        .filter(|channel| {
            if is_dm_like_channel(channel) {
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
        .collect()
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
