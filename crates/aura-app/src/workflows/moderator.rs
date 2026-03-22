//! Moderator Workflow - Portable Business Logic
//!
//! This module contains moderator role management operations that are portable across all frontends.
//! Moderators (Admins) have elevated permissions within a home.

#[cfg(test)]
use crate::workflows::home_scope::best_home_for_context_by;
use crate::workflows::home_scope::{identify_materialized_channel_hint, resolve_target_authority};
use crate::workflows::observed_projection::{
    homes_signal_snapshot, replace_homes_projection_observed,
};
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, execute_with_runtime_retry_budget, require_runtime,
    timeout_runtime_call, workflow_retry_policy,
};
use crate::{views::home::HomeRole, AppCore};
use async_lock::RwLock;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::{AuraError, RetryRunError};
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_social::moderation::facts::{HomeGrantModeratorFact, HomeRevokeModeratorFact};
use std::sync::Arc;
use std::time::Duration;

const MODERATOR_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const MODERATOR_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
const MODERATOR_RUNTIME_TIMEOUT: Duration = Duration::from_millis(5_000);

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
            timeout_runtime_call(
                &runtime,
                "send_moderator_fact_with_retry",
                "send_chat_fact",
                MODERATOR_RUNTIME_TIMEOUT,
                || runtime.send_chat_fact(peer, context_id, fact),
            )
            .await?
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

#[aura_macros::strong_reference(domain = "home_scope")]
#[derive(Debug, Clone)]
struct ModeratorScope {
    home_id: ChannelId,
    context_id: ContextId,
    home_state: crate::views::home::HomeState,
    peers: Vec<AuthorityId>,
}

#[cfg(test)]
fn best_home_for_context(
    homes: &crate::views::home::HomesState,
    context_id: ContextId,
) -> Option<(ChannelId, crate::views::home::HomeState)> {
    best_home_for_context_by(homes, context_id, |home| {
        (
            u8::from(home.is_admin()),
            u8::from(!home.members.is_empty()),
            home.member_count as usize,
        )
    })
}

#[cfg(test)]
async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModeratorScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_signal_snapshot(app_core).await?;
    let hinted_channel = match channel_hint {
        Some(hint) => Some(
            identify_materialized_channel_hint(
                app_core,
                hint,
                "identify_materialized_channel_hint",
                "resolve moderator channel",
                MODERATOR_RUNTIME_TIMEOUT,
            )
            .await?,
        ),
        None => None,
    };

    let (home_id, home_state) = if let Some(hinted_channel) = hinted_channel {
        let channel_id = hinted_channel.channel_id;
        if let Some(home) = homes.home_state(&channel_id) {
            (channel_id, home.clone())
        } else {
            let context_id = match hinted_channel.context_id {
                Some(context_id) => context_id,
                None => timeout_runtime_call(
                    &runtime,
                    "resolve_scope",
                    "resolve_amp_channel_context",
                    MODERATOR_RUNTIME_TIMEOUT,
                    || runtime.resolve_amp_channel_context(channel_id),
                )
                .await
                .map_err(|e| super::error::runtime_call("resolve moderator scope context", e))?
                .map_err(|e| super::error::runtime_call("resolve moderator scope context", e))?
                .ok_or_else(|| AuraError::not_found(channel_id.to_string()))?,
            };
            best_home_for_context(&homes, context_id)
                .ok_or_else(|| AuraError::permission_denied(context_id.to_string()))?
        }
    } else {
        homes
            .current_home()
            .map(|home| (home.id, home.clone()))
            .ok_or_else(|| {
                AuraError::permission_denied("Moderator operation requires an active home scope")
            })?
    };

    let context_id = home_state
        .context_id
        .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
    let peers = timeout_runtime_call(
        &runtime,
        "resolve_scope",
        "amp_list_channel_participants",
        MODERATOR_RUNTIME_TIMEOUT,
        || runtime.amp_list_channel_participants(context_id, home_id),
    )
    .await
    .map_err(|e| super::error::runtime_call("list moderator scope participants", e))?
    .map_err(|e| super::error::runtime_call("list moderator scope participants", e))?;

    Ok(ModeratorScope {
        home_id,
        context_id,
        home_state,
        peers,
    })
}

async fn current_moderator_scope(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ModeratorScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_signal_snapshot(app_core).await?;

    if let Ok(active_home_id) = crate::workflows::context::current_home_id(app_core).await {
        if let Some(home_state) = homes.home_state(&active_home_id) {
            let context_id = home_state
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
            let peers = timeout_runtime_call(
                &runtime,
                "current_moderator_scope",
                "amp_list_channel_participants",
                MODERATOR_RUNTIME_TIMEOUT,
                || runtime.amp_list_channel_participants(context_id, active_home_id),
            )
            .await
            .map_err(|e| super::error::runtime_call("list moderator scope participants", e))?
            .map_err(|e| super::error::runtime_call("list moderator scope participants", e))?;
            return Ok(ModeratorScope {
                home_id: active_home_id,
                context_id,
                home_state: home_state.clone(),
                peers,
            });
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
    let channel_id = match channel_hint {
        Some(hint) => Some(
            identify_materialized_channel_hint(
                app_core,
                hint,
                "identify_materialized_channel_hint",
                "resolve moderator channel",
                MODERATOR_RUNTIME_TIMEOUT,
            )
            .await?
            .channel_id,
        ),
        None => None,
    };
    grant_moderator_resolved(app_core, channel_id, target_id).await
}

/// Grant moderator role to a canonical authority.
pub async fn grant_moderator_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    // Validate current view and collect context/peer fanout.
    if channel_hint.is_some() {
        return Err(AuraError::invalid(
            "explicit moderator scope hints are no longer supported; select the target home first",
        ));
    }

    let mut scope = current_moderator_scope(app_core).await?;
    let runtime = require_runtime(app_core).await?;

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

    let now_ms = timeout_runtime_call(
        &runtime,
        "grant_moderator_resolved",
        "current_time_ms",
        MODERATOR_RUNTIME_TIMEOUT,
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|e| super::error::runtime_call("Grant moderator timestamp", e))?
    .map_err(|e| super::error::runtime_call("Grant moderator timestamp", e))?;
    let actor = runtime.authority_id();
    let fact =
        HomeGrantModeratorFact::new_ms(scope.context_id, target_id, actor, now_ms).to_generic();

    timeout_runtime_call(
        &runtime,
        "grant_moderator_resolved",
        "commit_relational_facts",
        MODERATOR_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
    )
    .await
    .map_err(|e| super::error::runtime_call("Commit moderator grant fact", e))?
    .map_err(|e| super::error::runtime_call("Commit moderator grant fact", e))?;

    for peer in scope.peers {
        if peer == actor {
            continue;
        }
        send_moderator_fact_with_retry(&runtime, peer, scope.context_id, &fact)
            .await
            .map_err(|e| super::error::runtime_call("Send moderator grant fact", e))?;
    }

    // Observed UI mirror.
    let mut homes = homes_signal_snapshot(app_core).await?;
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
    if actor == target_id {
        home_state.my_role = HomeRole::Moderator;
    }

    replace_homes_projection_observed(app_core, homes).await
}

/// Revoke moderator designation from a home member.
pub async fn revoke_moderator(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = match channel_hint {
        Some(hint) => Some(
            identify_materialized_channel_hint(
                app_core,
                hint,
                "identify_materialized_channel_hint",
                "resolve moderator channel",
                MODERATOR_RUNTIME_TIMEOUT,
            )
            .await?
            .channel_id,
        ),
        None => None,
    };
    revoke_moderator_resolved(app_core, channel_id, target_id).await
}

/// Revoke moderator role from a canonical authority.
pub async fn revoke_moderator_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    if channel_hint.is_some() {
        return Err(AuraError::invalid(
            "explicit moderator scope hints are no longer supported; select the target home first",
        ));
    }

    let mut scope = current_moderator_scope(app_core).await?;
    let runtime = require_runtime(app_core).await?;

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

    let now_ms = timeout_runtime_call(
        &runtime,
        "revoke_moderator_resolved",
        "current_time_ms",
        MODERATOR_RUNTIME_TIMEOUT,
        || runtime.current_time_ms(),
    )
    .await
    .map_err(|e| super::error::runtime_call("Revoke moderator timestamp", e))?
    .map_err(|e| super::error::runtime_call("Revoke moderator timestamp", e))?;
    let actor = runtime.authority_id();
    let fact =
        HomeRevokeModeratorFact::new_ms(scope.context_id, target_id, actor, now_ms).to_generic();

    timeout_runtime_call(
        &runtime,
        "revoke_moderator_resolved",
        "commit_relational_facts",
        MODERATOR_RUNTIME_TIMEOUT,
        || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
    )
    .await
    .map_err(|e| super::error::runtime_call("Commit moderator revoke fact", e))?
    .map_err(|e| super::error::runtime_call("Commit moderator revoke fact", e))?;

    for peer in scope.peers {
        if peer == actor {
            continue;
        }
        send_moderator_fact_with_retry(&runtime, peer, scope.context_id, &fact)
            .await
            .map_err(|e| super::error::runtime_call("Send moderator revoke fact", e))?;
    }

    let mut homes = homes_signal_snapshot(app_core).await?;
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
    if actor == target_id {
        home_state.my_role = HomeRole::Member;
    }

    replace_homes_projection_observed(app_core, homes).await
}

/// Check if current user is admin in current home.
pub async fn is_admin(app_core: &Arc<RwLock<AppCore>>) -> bool {
    let Ok(homes) = homes_signal_snapshot(app_core).await else {
        return false;
    };

    homes
        .current_home()
        .map(|home_state| home_state.is_admin())
        .unwrap_or(false)
}

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
    use crate::workflows::signals::emit_signal;
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
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [7u8; 32],
        )));
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime.clone()).unwrap(),
        ));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }

        let context_id = ContextId::new_from_entropy([9u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let peer = AuthorityId::new_from_entropy([2u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"scope-prefers-admin-channel"));
        let placeholder_id = ChannelId::from_bytes(hash(b"scope-prefers-admin-placeholder"));

        let mut homes = HomesState::default();
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
        emit_signal(&app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME)
            .await
            .unwrap();
        runtime.set_materialized_channel_name_matches("slash-lab", vec![channel_id]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, vec![owner, peer]);
        {
            let mut core = app_core.write().await;
            core.set_active_home_selection(Some(channel_id));
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

        let fallback_context = ContextId::new_from_entropy([31u8; 32]);
        let channel_context = ContextId::new_from_entropy([32u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let peer = AuthorityId::new_from_entropy([2u8; 32]);
        let fallback_home_id = ChannelId::from_bytes(hash(b"moderator-fallback-home"));
        let channel_home_id = ChannelId::from_bytes(hash(b"moderator-channel-home"));

        let mut homes = HomesState::default();
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
    async fn test_resolve_scope_by_channel_id_rejects_unknown_channel_scope() {
        let config = AppConfig::default();
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [9u8; 32],
        )));
        let app_core = Arc::new(RwLock::new(AppCore::with_runtime(config, runtime).unwrap()));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }
        let error = current_moderator_scope(&app_core)
            .await
            .expect_err("missing active moderator scope must fail");
        assert!(!error.to_string().is_empty());
    }
}
