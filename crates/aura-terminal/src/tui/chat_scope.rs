//! Chat scoping helpers tied to neighborhood traversal state.

use aura_app::ui::types::NeighborhoodState;

/// Resolve the active home scope for chat from neighborhood traversal state.
#[must_use]
pub fn active_home_scope_id(neighborhood: &NeighborhoodState) -> String {
    neighborhood
        .position
        .as_ref()
        .map(|p| p.current_home_id.to_string())
        .unwrap_or_else(|| neighborhood.home_home_id.to_string())
}

/// Returns true when a channel belongs to the active traversal scope.
#[must_use]
pub fn channel_matches_scope(channel_id: &str, active_home_scope: Option<&str>) -> bool {
    active_home_scope
        .map(|home_id| channel_id == home_id)
        .unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::ui::types::{NeighborhoodState, TraversalPosition};
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
    fn channel_scope_match_defaults_to_true_without_scope() {
        assert!(channel_matches_scope("abc", None));
    }

    #[test]
    fn channel_scope_match_requires_exact_home_channel() {
        assert!(channel_matches_scope("home-1", Some("home-1")));
        assert!(!channel_matches_scope("home-2", Some("home-1")));
    }
}
