//! Context Workflow - Portable Business Logic
//!
//! This module contains context/navigation operations that are portable across all frontends.
//! It follows the reactive signal pattern and manages neighborhood navigation state.

mod active_home;
mod neighborhood;

pub use crate::workflows::time::current_time_ms;
use crate::AppCore;
pub use active_home::{
    authority_default_relational_context, current_group_context, current_home_context,
    current_home_id, default_relational_context, missing_active_home_message, resolve_active_home,
    ActiveHomeResolution, ActiveHomeSource,
};
use async_lock::RwLock;
use aura_core::AuraError;
pub use neighborhood::{
    add_home_to_neighborhood, create_home, create_home_for_authority, create_neighborhood,
    get_current_position, get_neighborhood_state, initialize_test_home, link_home_one_hop_link,
    move_position,
};
use std::sync::Arc;

/// Set active context for navigation and command targeting
///
/// **What it does**: Sets the active context ID
/// **Returns**: Optional context ID
/// **Signal pattern**: Read-only operation (no signal emission)
///
/// The actual state update is handled by the UI layer when it receives
/// the context change notification.
pub async fn set_context(
    _app_core: &Arc<RwLock<AppCore>>,
    context_id: Option<String>,
) -> Result<Option<String>, AuraError> {
    // Context switching is handled by UI layer
    // This workflow just validates and returns the new context
    Ok(context_id)
}

/// Move position in neighborhood view
///
/// **What it does**: Updates neighborhood traversal position
/// **Returns**: Unit result
/// **Signal pattern**: Publishes observed projections
///
/// This operation:
/// 1. Determines target home (home, current, or specific ID)
/// 2. Resolves home name from neighbor list
/// 3. Creates/updates TraversalPosition
/// 4. Updates neighborhood view state
///
/// Depth values:
/// - 0: Limited access
/// - 1: Partial access (default)
/// - 2: Full access
///
/// OWNERSHIP: semantic-workflow-publication
#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME};
    use crate::views::home::HomeState;
    use crate::views::neighborhood::{NeighborHome, OneHopLinkType};
    use crate::workflows::signals::emit_signal;
    use crate::AppConfig;
    use aura_core::crypto::hash::hash;
    use aura_core::{ChannelId, ContextId};

    async fn init_signals_for_test(app_core: &Arc<RwLock<AppCore>>) {
        AppCore::init_signals_with_hooks(app_core).await.unwrap();
    }

    async fn publish_test_homes_signal(app_core: &Arc<RwLock<AppCore>>) {
        emit_signal(
            app_core,
            &*HOMES_SIGNAL,
            {
                let core = app_core.read().await;
                core.views().get_homes()
            },
            HOMES_SIGNAL_NAME,
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_set_context() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        let result = set_context(&app_core, Some("context-123".to_string())).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some("context-123".to_string()));

        let result = set_context(&app_core, None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_get_neighborhood_state() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        let state = get_neighborhood_state(&app_core).await;
        assert!(state.neighbors_is_empty());
    }

    #[tokio::test]
    async fn test_resolve_active_home_uses_selected_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([21u8; 32]);

        let selected_home = ChannelId::from_bytes(hash(b"selected-home"));
        let selected_ctx = ContextId::new_from_entropy(hash(b"selected-ctx"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            let result = homes.add_home(HomeState::new(
                selected_home,
                Some("Selected".to_string()),
                authority,
                1,
                selected_ctx,
            ));
            if result.was_first {
                homes.select_home(Some(result.home_id));
            }
            core.views_mut().set_homes(homes);
        }

        publish_test_homes_signal(&app_core).await;

        let resolved = resolve_active_home(&app_core).await.unwrap();
        assert_eq!(resolved.home_id, selected_home);
        assert_eq!(resolved.context_id, selected_ctx);
        assert_eq!(resolved.source, ActiveHomeSource::Selected);
    }

    #[tokio::test]
    async fn test_resolve_active_home_prefers_authoritative_selection_over_view_current_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([24u8; 32]);
        let selected_home = ChannelId::from_bytes(hash(b"selected-home-authoritative"));
        let selected_ctx = ContextId::new_from_entropy(hash(b"selected-ctx-authoritative"));
        let stale_view_home = ChannelId::from_bytes(hash(b"selected-home-stale-view"));
        let stale_view_ctx = ContextId::new_from_entropy(hash(b"selected-ctx-stale-view"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                selected_home,
                Some("Selected".to_string()),
                authority,
                1,
                selected_ctx,
            ));
            homes.add_home(HomeState::new(
                stale_view_home,
                Some("Stale".to_string()),
                authority,
                2,
                stale_view_ctx,
            ));
            homes.select_home(Some(stale_view_home));
            core.set_active_home_selection(Some(selected_home));
            core.views_mut().set_homes(homes);
        }

        publish_test_homes_signal(&app_core).await;

        let resolved = resolve_active_home(&app_core).await.unwrap();
        assert_eq!(resolved.home_id, selected_home);
        assert_eq!(resolved.context_id, selected_ctx);
        assert_eq!(resolved.source, ActiveHomeSource::Selected);
    }

    #[tokio::test]
    async fn test_resolve_active_home_requires_explicit_or_selected_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([22u8; 32]);

        let home_a = ChannelId::from_bytes(hash(b"home-z"));
        let home_b = ChannelId::from_bytes(hash(b"home-a"));
        let ctx_a = ContextId::new_from_entropy(hash(b"ctx-z"));
        let ctx_b = ContextId::new_from_entropy(hash(b"ctx-a"));
        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                home_a,
                Some("Zeta".to_string()),
                authority,
                1,
                ctx_a,
            ));
            homes.add_home(HomeState::new(
                home_b,
                Some("Alpha".to_string()),
                authority,
                2,
                ctx_b,
            ));
            homes.select_home(None);
            core.set_active_home_selection(None);
            core.views_mut().set_homes(homes);
        }

        publish_test_homes_signal(&app_core).await;

        let error = resolve_active_home(&app_core).await.unwrap_err();
        assert!(matches!(error, AuraError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_resolve_active_home_returns_guidance_when_missing() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        publish_test_homes_signal(&app_core).await;

        let error = resolve_active_home(&app_core).await.unwrap_err();
        assert!(matches!(error, AuraError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_current_home_context_uses_active_home_when_available() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([31u8; 32]);
        let home_id = ChannelId::from_bytes(hash(b"chat-home"));
        let home_ctx = ContextId::new_from_entropy(hash(b"chat-home-ctx"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            let result = homes.add_home(HomeState::new(
                home_id,
                Some("Chat Home".to_string()),
                authority,
                1,
                home_ctx,
            ));
            if result.was_first {
                homes.select_home(Some(result.home_id));
            }
            core.views_mut().set_homes(homes);
        }

        publish_test_homes_signal(&app_core).await;

        let resolved_ctx = current_home_context(&app_core)
            .await
            .expect("context should resolve");
        assert_eq!(resolved_ctx, home_ctx);
    }

    #[tokio::test]
    async fn test_current_home_context_requires_active_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        publish_test_homes_signal(&app_core).await;

        let error = current_home_context(&app_core).await.unwrap_err();
        assert!(matches!(error, AuraError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_current_group_context_uses_authority_default_without_home() {
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([41u8; 32]);
        let app_core = Arc::new(RwLock::new(
            AppCore::with_identity(
                aura_core::types::identifiers::AccountId::new_from_entropy([42u8; 32]),
                authority,
                Vec::new(),
            )
            .expect("identity-bound app core"),
        ));
        init_signals_for_test(&app_core).await;
        publish_test_homes_signal(&app_core).await;

        let resolved_ctx = current_group_context(&app_core)
            .await
            .expect("group context should fall back to authority default");

        assert_eq!(
            resolved_ctx,
            authority_default_relational_context(authority)
        );
    }

    #[tokio::test]
    async fn test_current_group_context_does_not_prefer_active_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([43u8; 32]);

        let home_id = ChannelId::from_bytes(hash(b"group-context-home"));
        let home_ctx = ContextId::new_from_entropy(hash(b"group-context-home-ctx"));
        {
            let mut core = app_core.write().await;
            core.set_authority(authority);
            let mut homes = core.views().get_homes();
            let result = homes.add_home(HomeState::new(
                home_id,
                Some("Neighborhood Home".to_string()),
                authority,
                1,
                home_ctx,
            ));
            if result.was_first {
                homes.select_home(Some(result.home_id));
            }
            core.set_active_home_selection(Some(home_id));
            core.views_mut().set_homes(homes);
        }

        publish_test_homes_signal(&app_core).await;

        let resolved_ctx = current_group_context(&app_core)
            .await
            .expect("group context should ignore active home");

        assert_eq!(
            resolved_ctx,
            authority_default_relational_context(authority)
        );
        assert_ne!(resolved_ctx, home_ctx);
    }

    #[tokio::test]
    async fn test_move_position_selects_known_target_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        init_signals_for_test(&app_core).await;
        let authority = aura_core::types::identifiers::AuthorityId::new_from_entropy([11u8; 32]);

        let home_a = ChannelId::from_bytes(hash(b"home-a"));
        let home_b = ChannelId::from_bytes(hash(b"home-b"));
        let ctx_a = ContextId::new_from_entropy(hash(b"ctx-a"));
        let ctx_b = ContextId::new_from_entropy(hash(b"ctx-b"));

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            let result = homes.add_home(HomeState::new(
                home_a,
                Some("Alpha".to_string()),
                authority,
                1,
                ctx_a,
            ));
            if result.was_first {
                homes.select_home(Some(result.home_id));
            }
            homes.add_home(HomeState::new(
                home_b,
                Some("Beta".to_string()),
                authority,
                2,
                ctx_b,
            ));
            homes.select_home(Some(home_a));
            core.set_active_home_selection(Some(home_a));
            core.views_mut().set_homes(homes);

            let mut neighborhood = core.views().get_neighborhood();
            neighborhood.home_home_id = home_a;
            neighborhood.home_name = "Alpha".to_string();
            neighborhood.add_neighbor(NeighborHome {
                id: home_b,
                name: "Beta".to_string(),
                one_hop_link: OneHopLinkType::Direct,
                shared_contacts: 0,
                member_count: Some(1),
                can_traverse: true,
            });
            core.views_mut().set_neighborhood(neighborhood);
        }
        publish_test_homes_signal(&app_core).await;

        move_position(&app_core, &home_b.to_string(), "full")
            .await
            .unwrap();

        let core = app_core.read().await;
        let homes = core.views().get_homes();
        let neighborhood = core.views().get_neighborhood();

        assert_eq!(homes.current_home_id().copied(), Some(home_b));
        assert_eq!(
            neighborhood.position.as_ref().map(|p| p.current_home_id),
            Some(home_b)
        );
        assert_eq!(neighborhood.position.as_ref().map(|p| p.depth), Some(2));
    }
}
