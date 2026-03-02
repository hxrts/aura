//! Moderation Workflow - Portable Business Logic
//!
//! This module contains home moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.

use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::workflows::{channel_ref::ChannelRef, snapshot_policy::chat_snapshot};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    effects::amp::ChannelLeaveParams,
    identifiers::{ChannelId, ContextId},
    AuraError,
};
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_social::moderation::facts::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomePinFact, HomeUnbanFact, HomeUnmuteFact,
    HomeUnpinFact,
};
use std::collections::BTreeSet;
use std::sync::Arc;

#[derive(Debug, Clone)]
struct ModerationScope {
    context_id: ContextId,
    home_id: ChannelId,
    is_admin: bool,
    peers: Vec<aura_core::identifiers::AuthorityId>,
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
                u8::from(!home.residents.is_empty()),
                home.resident_count,
            )
        })
}

fn parse_channel_id_from_message_id(message_id: &str) -> Option<ChannelId> {
    let encoded_channel = message_id
        .strip_prefix("msg-")
        .and_then(|value| value.splitn(3, '-').next())?;
    encoded_channel.parse::<ChannelId>().ok()
}

async fn resolve_channel_id(app_core: &Arc<RwLock<AppCore>>, channel: &str) -> ChannelId {
    let parsed = parse_channel_hint(channel);
    let chat = chat_snapshot(app_core).await;
    let resolved = chat
        .all_channels()
        .find(|entry| entry.id == parsed || entry.name.eq_ignore_ascii_case(channel))
        .map(|entry| entry.id)
        .unwrap_or(parsed);
    resolved
}

async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModerationScope, AuraError> {
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let hinted_channel = channel_hint.map(parse_channel_hint).or_else(|| {
        channel_hint.and_then(|hint| {
            chat.all_channels()
                .find(|entry| entry.name.eq_ignore_ascii_case(hint))
                .map(|entry| entry.id)
        })
    });

    let context_from_channel = hinted_channel.and_then(|channel_id| {
        chat.channel(&channel_id)
            .and_then(|channel| channel.context_id)
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

    let (context_id, home_id, is_admin, peers) = if let Some((home_id, home_state)) =
        home_from_channel
    {
        let peers = home_state
            .residents
            .iter()
            .map(|resident| resident.id)
            .collect::<Vec<_>>();
        (
            home_state
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
            home_id,
            home_state.is_admin(),
            peers,
        )
    } else if let Some(context_id) = context_from_channel {
        let fallback_home = ChannelRef::parse("home").to_channel_id();
        let home_id = hinted_channel.unwrap_or(fallback_home);
        let peers = homes
            .current_home()
            .map(|home| home.residents.iter().map(|resident| resident.id).collect())
            .unwrap_or_default();
        let is_admin = homes
            .current_home()
            .map(|home| home.is_admin())
            .unwrap_or(true);
        (context_id, home_id, is_admin, peers)
    } else if let Some(fallback) = homes.current_home() {
        (
            fallback
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
            fallback.id,
            fallback.is_admin(),
            fallback
                .residents
                .iter()
                .map(|resident| resident.id)
                .collect(),
        )
    } else {
        return Err(AuraError::permission_denied(
            "Moderation requires a valid home context and steward privileges",
        ));
    };

    Ok(ModerationScope {
        context_id,
        home_id,
        is_admin,
        peers,
    })
}

async fn commit_and_fanout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    scope: &ModerationScope,
    fact: RelationalFact,
    extra_peers: &[aura_core::identifiers::AuthorityId],
) -> Result<(), AuraError> {
    runtime
        .commit_relational_facts(std::slice::from_ref(&fact))
        .await
        .map_err(|e| AuraError::agent(format!("Failed to commit moderation fact: {e}")))?;

    let actor = runtime.authority_id();
    let mut fanout = BTreeSet::new();
    for peer in &scope.peers {
        if *peer != actor {
            fanout.insert(*peer);
        }
    }
    for peer in extra_peers {
        if *peer != actor {
            fanout.insert(*peer);
        }
    }

    for peer in fanout {
        let _ = runtime.send_chat_fact(peer, scope.context_id, &fact).await;
    }

    Ok(())
}

/// Kick a user from the current home.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    target: &str,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope(app_core, Some(channel)).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can kick residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let channel_id = resolve_channel_id(app_core, channel).await;
    let target_id = parse_authority_id(target)?;
    let fact = HomeKickFact::new_ms(
        scope.context_id,
        channel_id,
        target_id,
        runtime.authority_id(),
        reason.unwrap_or_default().to_string(),
        kicked_at_ms,
    )
    .to_generic();

    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;
    runtime
        .amp_leave_channel(ChannelLeaveParams {
            context: scope.context_id,
            channel: channel_id,
            participant: target_id,
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to enforce kick membership leave: {e}")))?;

    Ok(())
}

/// Ban a user from the current home.
pub async fn ban_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
    reason: Option<&str>,
    banned_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope(app_core, channel_hint).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can ban residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let target_id = parse_authority_id(target)?;
    let fact = HomeBanFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        reason.unwrap_or_default().to_string(),
        banned_at_ms,
        None,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;

    Ok(())
}

/// Unban a user from the current home.
pub async fn unban_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let scope = resolve_scope(app_core, channel_hint).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unban residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    let now_ms = runtime.current_time_ms().await.map_err(|e| {
        AuraError::agent(format!("Failed to read timestamp for unban operation: {e}"))
    })?;
    let fact = HomeUnbanFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        now_ms,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;

    Ok(())
}

/// Mute a user in the current home.
pub async fn mute_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
    duration_secs: Option<u64>,
    muted_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope(app_core, channel_hint).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can mute residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let target_id = parse_authority_id(target)?;
    let expires_at_ms =
        duration_secs.map(|seconds| muted_at_ms.saturating_add(seconds.saturating_mul(1000)));
    let fact = HomeMuteFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        duration_secs,
        muted_at_ms,
        expires_at_ms,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;

    Ok(())
}

/// Unmute a user in the current home.
pub async fn unmute_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let scope = resolve_scope(app_core, channel_hint).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unmute residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    let now_ms = runtime.current_time_ms().await.map_err(|e| {
        AuraError::agent(format!(
            "Failed to read timestamp for unmute operation: {e}"
        ))
    })?;
    let fact = HomeUnmuteFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        now_ms,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;

    Ok(())
}

async fn scope_for_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<ModerationScope, AuraError> {
    if let Some(channel_id) = parse_channel_id_from_message_id(message_id) {
        let channel = channel_id.to_string();
        return resolve_scope(app_core, Some(&channel)).await;
    }
    resolve_scope(app_core, None).await
}

/// Pin a message in the current home.
pub async fn pin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let scope = scope_for_message(app_core, message_id).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can pin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomePinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        runtime.current_time_ms().await.map_err(|e| {
            AuraError::agent(format!("Failed to read timestamp for pin operation: {e}"))
        })?,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[]).await?;

    Ok(())
}

/// Unpin a message in the current home.
pub async fn unpin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let scope = scope_for_message(app_core, message_id).await?;
    if !scope.is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unpin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomeUnpinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        runtime.current_time_ms().await.map_err(|e| {
            AuraError::agent(format!("Failed to read timestamp for unpin operation: {e}"))
        })?,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[]).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn moderation_requires_home() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        assert!(ban_user(
            &app_core,
            None,
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
            None,
            "authority-00000000-0000-0000-0000-000000000000",
            None,
            0
        )
        .await
        .is_err());
        assert!(pin_message(&app_core, "msg-1").await.is_err());
    }
}
