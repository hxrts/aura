//! Moderator Workflow - Portable Business Logic
//!
//! This module contains moderator role management operations that are portable across all frontends.
//! Moderators (Admins) have elevated permissions within a home.

use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, execute_with_runtime_retry_budget, require_runtime,
    workflow_retry_policy,
};
use crate::workflows::{channel_ref::ChannelRef, snapshot_policy::chat_snapshot};
use crate::{views::home::HomeRole, AppCore};
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::{AuraError, RetryRunError};
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_social::moderation::facts::{HomeGrantModeratorFact, HomeRevokeModeratorFact};
use std::sync::Arc;

const MODERATOR_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const MODERATOR_FACT_SEND_YIELDS_PER_RETRY: usize = 4;

async fn send_moderator_fact_with_retry(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: AuthorityId,
    context_id: ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let retry_policy = workflow_retry_policy(
        MODERATOR_FACT_SEND_MAX_ATTEMPTS as u32,
        std::time::Duration::from_millis(1),
        std::time::Duration::from_millis(1),
    )?;
    execute_with_runtime_retry_budget(runtime, &retry_policy, |attempt| {
        let runtime = Arc::clone(runtime);
        async move {
            if attempt > 0 {
                converge_runtime(&runtime).await;
                for _ in 0..MODERATOR_FACT_SEND_YIELDS_PER_RETRY {
                    cooperative_yield().await;
                }
            }
            runtime
                .send_chat_fact(peer, context_id, fact)
                .await
                .map_err(|error| {
                    AuraError::from(super::error::runtime_call("Send moderator fact", error))
                })
        }
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}

async fn resolve_target_authority(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<AuthorityId, AuraError> {
    if let Ok(contact) = crate::workflows::query::resolve_contact(app_core, target).await {
        return Ok(contact.id);
    }
    parse_authority_id(target)
}

#[derive(Debug, Clone)]
struct ModeratorScope {
    home_id: ChannelId,
    context_id: ContextId,
    home_state: crate::views::home::HomeState,
    peers: Vec<AuthorityId>,
}

fn parse_channel_hint(channel: &str) -> ChannelId {
    ChannelRef::parse(channel).to_channel_id()
}

fn best_home_for_context(
    homes: &crate::views::home::HomesState,
    context_id: ContextId,
) -> Option<(ChannelId, crate::views::home::HomeState)> {
    homes
        .iter()
        .filter(|(_, home)| home.context_id == Some(context_id))
        .map(|(home_id, home)| (*home_id, home.clone()))
        .max_by_key(|(_, home)| {
            (
                u8::from(home.is_admin()),
                u8::from(!home.members.is_empty()),
                home.member_count,
            )
        })
}

#[cfg(test)]
async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModeratorScope, AuraError> {
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let hinted_channel = channel_hint.map(|hint| {
        chat.all_channels()
            .find(|entry| entry.name.eq_ignore_ascii_case(hint))
            .map(|entry| entry.id)
            .unwrap_or_else(|| parse_channel_hint(hint))
    });

    let home_from_channel = hinted_channel.and_then(|channel_id| {
        if let Some(home) = homes.home_state(&channel_id) {
            if let Some(context_id) = home.context_id {
                if !home.is_admin() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.is_admin() {
                            return Some((best_id, best_home));
                        }
                    }
                }
            }
            return Some((channel_id, home.clone()));
        }

        let context_id = chat
            .channel(&channel_id)
            .and_then(|channel| channel.context_id)?;
        best_home_for_context(&homes, context_id)
    });

    let (home_id, home_state) = if let Some((home_id, home_state)) =
        home_from_channel.or_else(|| homes.current_home().map(|home| (home.id, home.clone())))
    {
        (home_id, home_state)
    } else {
        let authority_id = {
            let core = app_core.read().await;
            core.runtime()
                .map(|runtime| runtime.authority_id())
                .or_else(|| core.authority().copied())
        }
        .ok_or_else(|| AuraError::permission_denied("Authority not set"))?;
        let context_id =
            crate::workflows::context::current_home_context_or_fallback(app_core).await?;
        let home_id = hinted_channel.unwrap_or_else(|| ChannelRef::parse("home").to_channel_id());
        (
            home_id,
            crate::views::home::HomeState::new(
                home_id,
                Some("Home".to_string()),
                authority_id,
                0,
                context_id,
            ),
        )
    };

    let context_id = home_state
        .context_id
        .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
    let peers = home_state.members.iter().map(|member| member.id).collect();

    Ok(ModeratorScope {
        home_id,
        context_id,
        home_state,
        peers,
    })
}

async fn resolve_scope_by_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
) -> Result<ModeratorScope, AuraError> {
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let from_home = |home_id: ChannelId, home_state: &crate::views::home::HomeState| {
        let context_id = home_state
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = home_state.members.iter().map(|member| member.id).collect();
        Ok(ModeratorScope {
            home_id,
            context_id,
            home_state: home_state.clone(),
            peers,
        })
    };

    if let Some(channel_id) = channel_hint {
        if let Some(home_state) = homes.home_state(&channel_id) {
            if let Some(context_id) = home_state.context_id {
                if !home_state.is_admin() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.is_admin() {
                            return from_home(best_id, &best_home);
                        }
                    }
                }
            }
            return from_home(channel_id, home_state);
        }

        let context_id = chat
            .channel(&channel_id)
            .and_then(|channel| channel.context_id)
            .ok_or_else(|| AuraError::not_found(channel_id.to_string()))?;

        if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
            return from_home(best_id, &best_home);
        }

        return Err(AuraError::permission_denied(context_id.to_string()));
    }

    if let Ok(active_home_id) =
        crate::workflows::context::current_home_id_or_fallback(app_core).await
    {
        if let Some(home_state) = homes.home_state(&active_home_id) {
            return from_home(active_home_id, home_state);
        }
    }

    Err(AuraError::permission_denied(
        "Moderator operation requires an active home scope",
    ))
}

/// Grant moderator designation to a home member.
///
/// Authorization: elevated home privileges required.
pub async fn grant_moderator(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = channel_hint.map(parse_channel_hint);
    grant_moderator_resolved(app_core, channel_id, target_id).await
}

/// Grant moderator role to a canonical authority.
pub async fn grant_moderator_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    // Validate current view and collect context/peer fanout.
    let mut scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;

    if !scope.home_state.is_admin() {
        return Err(AuraError::permission_denied(
            "Only moderators can grant moderator role",
        ));
    }

    if let Some(member) = scope.home_state.member(&target_id) {
        if matches!(member.role, HomeRole::Moderator) {
            return Err(AuraError::invalid(
                "Target already has moderator designation",
            ));
        }
        if !matches!(member.role, HomeRole::Member) {
            return Err(AuraError::invalid(
                "Only members can be designated as moderators",
            ));
        }
    } else {
        return Err(AuraError::not_found(target_id.to_string()));
    }

    if !scope.peers.contains(&target_id) {
        scope.peers.push(target_id);
    }

    // Runtime-backed propagation when available. Keep local mutation below for
    // immediate UX even if runtime is not configured (tests/local-only callers).
    if let Ok(runtime) = require_runtime(app_core).await {
        let now_ms = runtime
            .current_time_ms()
            .await
            .map_err(|e| super::error::runtime_call("Grant moderator timestamp", e))?;
        let actor = runtime.authority_id();
        let fact =
            HomeGrantModeratorFact::new_ms(scope.context_id, target_id, actor, now_ms).to_generic();

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| super::error::runtime_call("Commit moderator grant fact", e))?;

        for peer in scope.peers {
            if peer == actor {
                continue;
            }
            send_moderator_fact_with_retry(&runtime, peer, scope.context_id, &fact)
                .await
                .map_err(|e| super::error::runtime_call("Send moderator grant fact", e))?;
        }
    }

    // Local state mutation.
    let mut core = app_core.write().await;
    let local_authority = core
        .runtime()
        .map(|runtime| runtime.authority_id())
        .or_else(|| core.authority().copied());
    let mut homes = core.views().get_homes();
    if !homes.has_home(&scope.home_id) {
        homes.add_home_with_auto_select(scope.home_state.clone());
    }
    let home_state = homes
        .home_mut(&scope.home_id)
        .ok_or_else(|| AuraError::not_found(scope.home_id.to_string()))?;

    let member = home_state
        .member_mut(&target_id)
        .ok_or_else(|| AuraError::not_found(target_id.to_string()))?;

    if matches!(member.role, HomeRole::Moderator) {
        return Err(AuraError::invalid(
            "Target already has moderator designation",
        ));
    }
    if !matches!(member.role, HomeRole::Member) {
        return Err(AuraError::invalid(
            "Only members can be designated as moderators",
        ));
    }

    member.role = HomeRole::Moderator;
    if local_authority == Some(target_id) {
        home_state.my_role = HomeRole::Moderator;
    }
    core.views_mut().set_homes(homes);

    Ok(())
}

/// Revoke moderator designation from a home member.
pub async fn revoke_moderator(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = channel_hint.map(parse_channel_hint);
    revoke_moderator_resolved(app_core, channel_id, target_id).await
}

/// Revoke moderator role from a canonical authority.
pub async fn revoke_moderator_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    let mut scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;

    if !scope.home_state.is_admin() {
        return Err(AuraError::permission_denied(
            "Only moderators can revoke moderator role",
        ));
    }

    if let Some(member) = scope.home_state.member(&target_id) {
        if !matches!(member.role, HomeRole::Moderator) {
            return Err(AuraError::invalid("Target is not a moderator"));
        }
    } else {
        return Err(AuraError::not_found(target_id.to_string()));
    }

    if !scope.peers.contains(&target_id) {
        scope.peers.push(target_id);
    }

    if let Ok(runtime) = require_runtime(app_core).await {
        let now_ms = runtime
            .current_time_ms()
            .await
            .map_err(|e| super::error::runtime_call("Revoke moderator timestamp", e))?;
        let actor = runtime.authority_id();
        let fact = HomeRevokeModeratorFact::new_ms(scope.context_id, target_id, actor, now_ms)
            .to_generic();

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| super::error::runtime_call("Commit moderator revoke fact", e))?;

        for peer in scope.peers {
            if peer == actor {
                continue;
            }
            send_moderator_fact_with_retry(&runtime, peer, scope.context_id, &fact)
                .await
                .map_err(|e| super::error::runtime_call("Send moderator revoke fact", e))?;
        }
    }

    let mut core = app_core.write().await;
    let local_authority = core
        .runtime()
        .map(|runtime| runtime.authority_id())
        .or_else(|| core.authority().copied());
    let mut homes = core.views().get_homes();
    if !homes.has_home(&scope.home_id) {
        homes.add_home_with_auto_select(scope.home_state.clone());
    }
    let home_state = homes
        .home_mut(&scope.home_id)
        .ok_or_else(|| AuraError::not_found(scope.home_id.to_string()))?;

    let member = home_state
        .member_mut(&target_id)
        .ok_or_else(|| AuraError::not_found(target_id.to_string()))?;

    if !matches!(member.role, HomeRole::Moderator) {
        return Err(AuraError::invalid("Target is not a moderator"));
    }

    member.role = HomeRole::Member;
    if local_authority == Some(target_id) {
        home_state.my_role = HomeRole::Member;
    }
    core.views_mut().set_homes(homes);

    Ok(())
}

/// Check if current user is admin in current home.
pub async fn is_admin(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let core = app_core.read().await;
    let homes = core.views().get_homes();

    homes
        .current_home()
        .map(|home_state| home_state.is_admin())
        .unwrap_or(false)
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::views::{
        chat::{Channel, ChannelType, ChatState},
        home::HomeState,
        Contact, ContactsState,
    };
    use crate::AppConfig;
    use aura_core::crypto::hash::hash;

    #[tokio::test]
    async fn test_is_admin_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let is_admin_result = is_admin(&app_core).await;
        assert!(!is_admin_result);
    }

    #[tokio::test]
    async fn test_grant_moderator_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = grant_moderator(&app_core, None, "user-123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_revoke_moderator_no_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let result = revoke_moderator(&app_core, None, "user-123").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resolve_scope_prefers_admin_home_for_context() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let context_id = ContextId::new_from_entropy([9u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let peer = AuthorityId::new_from_entropy([2u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"scope-prefers-admin-channel"));
        let placeholder_id = ChannelId::from_bytes(hash(b"scope-prefers-admin-placeholder"));

        {
            let mut core = app_core.write().await;

            let mut chat = ChatState::new();
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![owner, peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
            core.views_mut().set_chat(chat);

            let mut homes = core.views().get_homes();
            let mut placeholder = HomeState::new(
                placeholder_id,
                Some("placeholder".to_string()),
                peer,
                0,
                context_id,
            );
            placeholder.my_role = HomeRole::Participant;
            homes.add_home(placeholder);
            homes.add_home(HomeState::new(
                channel_id,
                Some("slash-lab".to_string()),
                owner,
                0,
                context_id,
            ));
            homes.select_home(Some(channel_id));
            core.views_mut().set_homes(homes);
        }

        let scope = resolve_scope(&app_core, Some("slash-lab"))
            .await
            .expect("scope should resolve");
        assert_eq!(scope.home_id, channel_id);
        assert!(
            scope.home_state.is_admin(),
            "resolve_scope should pick the admin-capable home"
        );
    }

    #[tokio::test]
    async fn test_resolve_target_authority_supports_contact_lookup() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let bob_id = AuthorityId::new_from_entropy([7u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut contacts = ContactsState::new();
            contacts.apply_contact(Contact {
                id: bob_id,
                nickname: "Bob".to_string(),
                nickname_suggestion: Some("Bobby".to_string()),
                is_guardian: false,
                is_member: true,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: Default::default(),
            });
            core.views_mut().set_contacts(contacts);
        }

        let resolved_by_name = resolve_target_authority(&app_core, "bobby")
            .await
            .expect("resolve by nickname suggestion");
        assert_eq!(resolved_by_name, bob_id);

        let id = bob_id.to_string();
        let prefix = id.chars().take(8).collect::<String>();
        let resolved_by_prefix = resolve_target_authority(&app_core, &prefix)
            .await
            .expect("resolve by authority prefix");
        assert_eq!(resolved_by_prefix, bob_id);
    }

    #[tokio::test]
    async fn test_resolve_scope_prefers_named_channel_context_over_current_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let fallback_context = ContextId::new_from_entropy([31u8; 32]);
        let channel_context = ContextId::new_from_entropy([32u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let peer = AuthorityId::new_from_entropy([2u8; 32]);
        let fallback_home_id = ChannelId::from_bytes(hash(b"moderator-fallback-home"));
        let channel_home_id = ChannelId::from_bytes(hash(b"moderator-channel-home"));

        {
            let mut core = app_core.write().await;

            let mut chat = ChatState::new();
            chat.upsert_channel(Channel {
                id: channel_home_id,
                context_id: Some(channel_context),
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![owner, peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
            core.views_mut().set_chat(chat);

            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                fallback_home_id,
                Some("fallback".to_string()),
                owner,
                0,
                fallback_context,
            ));
            homes.add_home(HomeState::new(
                channel_home_id,
                Some("slash-lab".to_string()),
                owner,
                0,
                channel_context,
            ));
            homes.select_home(Some(fallback_home_id));
            core.views_mut().set_homes(homes);
        }

        let scope = resolve_scope(&app_core, Some("slash-lab"))
            .await
            .expect("scope should resolve");
        assert_eq!(scope.context_id, channel_context);
        assert_eq!(scope.home_id, channel_home_id);
    }

    #[tokio::test]
    async fn test_resolve_scope_by_channel_id_rejects_unknown_channel_scope() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let unknown = ChannelId::from_bytes(hash(b"moderator-unknown-scope"));

        let error = resolve_scope_by_channel_id(&app_core, Some(unknown))
            .await
            .expect_err("unknown channel scope must fail");
        assert!(error.to_string().contains(&unknown.to_string()));
    }
}
