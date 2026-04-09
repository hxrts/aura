//! Moderation Workflow - Portable Business Logic
//!
//! This module contains home moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.
use std::time::Duration;

const MODERATION_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const MODERATION_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
const MODERATION_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

mod actions;
mod scope;
mod support;

pub use actions::{
    ban_user, ban_user_resolved, kick_user, kick_user_resolved, mute_user, mute_user_resolved,
    pin_message, unban_user, unban_user_resolved, unmute_user, unmute_user_resolved, unpin_message,
};
#[cfg(test)]
pub(crate) use scope::{current_moderation_scope, resolve_scope};

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime_bridge::OfflineRuntimeBridge;
    use crate::signal_defs::{register_app_signals, HOMES_SIGNAL, HOMES_SIGNAL_NAME};
    use crate::views::{
        home::{HomeRole, HomeState, HomesState},
        Contact, ContactsState,
    };
    use crate::workflows::home_scope::resolve_target_authority;
    use crate::workflows::signals::emit_signal;
    use crate::{AppConfig, AppCore};
    use async_lock::RwLock;
    use aura_core::{
        crypto::hash::hash,
        types::identifiers::{AuthorityId, ChannelId, ContextId},
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn moderation_requires_home() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);

        assert!(ban_user(
            &app_core,
            "authority-00000000-0000-0000-0000-000000000000",
            None,
            0
        )
        .await
        .is_err());
        assert!(kick_user(
            &app_core,
            "channel:test",
            "authority-00000000-0000-0000-0000-000000000000",
            None,
            0
        )
        .await
        .is_err());
        assert!(mute_user(
            &app_core,
            "authority-00000000-0000-0000-0000-000000000000",
            None,
            0
        )
        .await
        .is_err());
        assert!(pin_message(&app_core, "msg-1").await.is_err());
    }

    #[tokio::test]
    async fn resolve_target_authority_supports_contact_lookup() {
        let config = AppConfig::default();
        let app_core = crate::testing::test_app_core(config);
        let bob_id = AuthorityId::new_from_entropy([7u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut contacts = ContactsState::new();
            contacts.apply_contact(Contact {
                id: bob_id,
                nickname: "Bob".to_string(),
                nickname_suggestion: Some("Bobby".to_string()),
                is_guardian: false,
                is_member: false,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: Default::default(),
                relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
            });
            core.views_mut().set_contacts(contacts);
        }

        let resolved_by_name = resolve_target_authority(&app_core, "bob")
            .await
            .expect("resolve by nickname");
        assert_eq!(resolved_by_name, bob_id);

        let id = bob_id.to_string();
        let prefix = id.chars().take(8).collect::<String>();
        let resolved_by_prefix = resolve_target_authority(&app_core, &prefix)
            .await
            .expect("resolve by authority prefix");
        assert_eq!(resolved_by_prefix, bob_id);
    }

    #[tokio::test]
    async fn resolve_scope_uses_named_channel_context_without_falling_back() {
        let config = AppConfig::default();
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [8u8; 32],
        )));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }

        let fallback_context = ContextId::new_from_entropy([21u8; 32]);
        let channel_context = ContextId::new_from_entropy([22u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let peer = AuthorityId::new_from_entropy([2u8; 32]);
        let fallback_home_id = ChannelId::from_bytes(hash(b"moderation-fallback-home"));
        let channel_home_id = ChannelId::from_bytes(hash(b"moderation-channel-home"));

        let mut homes = HomesState::default();
        homes.add_home(HomeState::new(
            fallback_home_id,
            Some("fallback".to_string()),
            owner,
            0,
            fallback_context,
        ));
        let mut channel_home = HomeState::new(
            channel_home_id,
            Some("slash-lab".to_string()),
            owner,
            0,
            channel_context,
        );
        channel_home.my_role = HomeRole::Moderator;
        homes.add_home(channel_home);
        homes.select_home(Some(fallback_home_id));
        emit_signal(&app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        runtime.set_materialized_channel_name_matches("slash-lab", vec![channel_home_id]);
        runtime.set_amp_channel_context(channel_home_id, channel_context);
        runtime.set_amp_channel_participants(channel_context, channel_home_id, vec![owner, peer]);
        {
            let mut core = app_core.write().await;
            core.set_active_home_selection(Some(fallback_home_id));
        }

        let scope = resolve_scope(&app_core, Some("slash-lab"))
            .await
            .expect("scope should resolve");
        assert_eq!(scope.context_id, channel_context);
        assert_eq!(scope.home_id, channel_home_id);
    }

    #[tokio::test]
    async fn resolve_scope_rejects_unknown_named_channel_instead_of_falling_back() {
        let config = AppConfig::default();
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [10u8; 32],
        )));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }

        let fallback_context = ContextId::new_from_entropy([23u8; 32]);
        let owner = AuthorityId::new_from_entropy([3u8; 32]);
        let fallback_home_id = ChannelId::from_bytes(hash(b"moderation-fallback-only-home"));

        let mut homes = HomesState::default();
        homes.add_home(HomeState::new(
            fallback_home_id,
            Some("fallback".to_string()),
            owner,
            0,
            fallback_context,
        ));
        homes.select_home(Some(fallback_home_id));
        emit_signal(&app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        {
            let mut core = app_core.write().await;
            core.set_active_home_selection(Some(fallback_home_id));
        }

        let error = resolve_scope(&app_core, Some("missing-home"))
            .await
            .expect_err("unknown named scope must not fall back to the current home");
        assert!(
            error.to_string().contains("resolve moderation channel"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn resolve_scope_by_channel_id_rejects_unknown_channel_scope() {
        let config = AppConfig::default();
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [9u8; 32],
        )));
        let app_core = crate::testing::test_app_core_with_runtime(config, runtime);
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }
        let error = current_moderation_scope(&app_core)
            .await
            .expect_err("missing active moderation scope must fail");
        assert!(!error.to_string().is_empty());
    }
}
