//! Moderation Workflow - Portable Business Logic
//!
//! This module contains home moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.

use crate::workflows::channel_ref::ChannelSelector;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, execute_with_runtime_retry_budget, require_runtime,
    workflow_retry_policy,
};
use crate::workflows::signals::{emit_signal, read_signal};
use crate::{
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    views::home::HomesState,
    AppCore,
};
use async_lock::RwLock;
use aura_core::{
    effects::amp::ChannelLeaveParams,
    types::identifiers::{ChannelId, ContextId},
    AuraError, RetryRunError,
};
use aura_journal::{fact::RelationalFact, DomainFact};
use aura_social::moderation::facts::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomePinFact, HomeUnbanFact, HomeUnmuteFact,
    HomeUnpinFact,
};
use std::collections::BTreeSet;
use std::sync::Arc;

const MODERATION_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const MODERATION_FACT_SEND_YIELDS_PER_RETRY: usize = 4;

async fn send_moderation_fact_with_retry(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: aura_core::types::identifiers::AuthorityId,
    context_id: ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let retry_policy = workflow_retry_policy(
        MODERATION_FACT_SEND_MAX_ATTEMPTS as u32,
        std::time::Duration::from_millis(1),
        std::time::Duration::from_millis(1),
    )?;
    execute_with_runtime_retry_budget(runtime, &retry_policy, |attempt| {
        let runtime = Arc::clone(runtime);
        async move {
            if attempt > 0 {
                converge_runtime(&runtime).await;
                for _ in 0..MODERATION_FACT_SEND_YIELDS_PER_RETRY {
                    cooperative_yield().await;
                }
            }
            runtime
                .send_chat_fact(peer, context_id, fact)
                .await
                .map_err(|error| {
                    AuraError::from(super::error::WorkflowError::DeliveryFailed {
                        peer: peer.to_string(),
                        attempts: MODERATION_FACT_SEND_MAX_ATTEMPTS,
                        source: AuraError::agent(error.to_string()),
                    })
                })
        }
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { last_error, .. } => last_error,
    })
}

#[derive(Debug, Clone)]
struct ModerationScope {
    context_id: ContextId,
    home_id: ChannelId,
    can_moderate: bool,
    peers: Vec<aura_core::types::identifiers::AuthorityId>,
}

fn parse_channel_hint(channel: &str) -> Result<ChannelSelector, AuraError> {
    ChannelSelector::parse(channel)
}

async fn homes_state_signal_snapshot(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<HomesState, AuraError> {
    read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME).await
}

async fn emit_homes_state_observed(
    app_core: &Arc<RwLock<AppCore>>,
    homes: HomesState,
) -> Result<(), AuraError> {
    {
        let mut core = app_core.write().await;
        // OWNERSHIP: observed-display-update
        core.views_mut().set_homes(homes.clone());
    }
    emit_signal(app_core, &*HOMES_SIGNAL, homes, HOMES_SIGNAL_NAME).await
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
                u8::from(home.can_moderate()),
                u8::from(!home.members.is_empty()),
                home.member_count,
            )
        })
}

fn parse_channel_id_from_message_id(message_id: &str) -> Option<ChannelId> {
    let encoded_channel = message_id
        .strip_prefix("msg-")
        .and_then(|value| value.split('-').next())?;
    encoded_channel.parse::<ChannelId>().ok()
}

async fn resolve_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
) -> Result<ChannelId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let parsed = parse_channel_hint(channel)?;
    match parsed {
        ChannelSelector::Id(channel_id) => Ok(channel_id),
        ChannelSelector::Name(name) => {
            let resolved = runtime
                .resolve_authoritative_channel_ids_by_name(&name)
                .await
                .map_err(|e| super::error::runtime_call("resolve moderation channel", e))?;
            match resolved.as_slice() {
                [] => Err(AuraError::not_found(channel.to_string())),
                [channel_id] => Ok(*channel_id),
                _ => Err(AuraError::invalid(format!(
                    "Ambiguous moderation channel hint: {name}"
                ))),
            }
        }
    }
}

#[cfg(test)]
async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModerationScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_state_signal_snapshot(app_core).await?;

    let hinted_channel = if let Some(hint) = channel_hint {
        Some(resolve_channel_id(app_core, hint).await?)
    } else {
        None
    };

    let home_from_channel = if let Some(channel_id) = hinted_channel {
        if let Some(home) = homes.home_state(&channel_id) {
            if let Some(context_id) = home.context_id {
                if !home.can_moderate() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.can_moderate() {
                            Some((best_id, best_home))
                        } else {
                            Some((channel_id, home.clone()))
                        }
                    } else {
                        Some((channel_id, home.clone()))
                    }
                } else {
                    Some((channel_id, home.clone()))
                }
            } else {
                Some((channel_id, home.clone()))
            }
        } else if let Some(context_id) = runtime
            .resolve_amp_channel_context(channel_id)
            .await
            .map_err(|e| super::error::runtime_call("resolve moderation scope context", e))?
        {
            best_home_for_context(&homes, context_id)
        } else {
            None
        }
    } else {
        None
    };

    let (context_id, home_id, can_moderate, peers) =
        if let Some((home_id, home_state)) = home_from_channel {
            let peers = runtime
                .amp_list_channel_participants(
                    home_state
                        .context_id
                        .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
                    home_id,
                )
                .await
                .map_err(|e| super::error::runtime_call("list moderation scope participants", e))?;
            (
                home_state
                    .context_id
                    .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
                home_id,
                home_state.can_moderate(),
                peers,
            )
        } else if let Some(fallback) = homes.current_home() {
            let context_id = fallback
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
            let peers = runtime
                .amp_list_channel_participants(context_id, fallback.id)
                .await
                .map_err(|e| super::error::runtime_call("list moderation scope participants", e))?;
            (context_id, fallback.id, fallback.can_moderate(), peers)
        } else {
            return Err(AuraError::permission_denied(
                "Moderation requires a valid home context and moderator privileges",
            ));
        };

    Ok(ModerationScope {
        context_id,
        home_id,
        can_moderate,
        peers,
    })
}

async fn resolve_scope_by_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
) -> Result<ModerationScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_state_signal_snapshot(app_core).await?;

    if let Some(channel_id) = channel_hint {
        if let Some(home) = homes.home_state(&channel_id) {
            let context_id = home
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
            let peers = runtime
                .amp_list_channel_participants(context_id, channel_id)
                .await
                .map_err(|e| super::error::runtime_call("list moderation scope participants", e))?;
            if let Some(context_id) = home.context_id {
                if !home.can_moderate() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.can_moderate() {
                            let best_context = best_home
                                .context_id
                                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
                            let best_peers = runtime
                                .amp_list_channel_participants(best_context, best_id)
                                .await
                                .map_err(|e| {
                                    super::error::runtime_call(
                                        "list moderation scope participants",
                                        e,
                                    )
                                })?;
                            return Ok(ModerationScope {
                                context_id: best_context,
                                home_id: best_id,
                                can_moderate: best_home.can_moderate(),
                                peers: best_peers,
                            });
                        }
                    }
                }
            }
            return Ok(ModerationScope {
                context_id,
                home_id: channel_id,
                can_moderate: home.can_moderate(),
                peers,
            });
        }

        let context_id = runtime
            .resolve_amp_channel_context(channel_id)
            .await
            .map_err(|e| super::error::runtime_call("resolve moderation scope context", e))?
            .ok_or_else(|| AuraError::not_found(channel_id.to_string()))?;

        if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
            let best_context = best_home
                .context_id
                .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
            let peers = runtime
                .amp_list_channel_participants(best_context, best_id)
                .await
                .map_err(|e| super::error::runtime_call("list moderation scope participants", e))?;
            return Ok(ModerationScope {
                context_id: best_context,
                home_id: best_id,
                can_moderate: best_home.can_moderate(),
                peers,
            });
        }

        return Err(AuraError::permission_denied(context_id.to_string()));
    }

    if let Some(current_home) = homes.current_home() {
        let context_id = current_home
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = runtime
            .amp_list_channel_participants(context_id, current_home.id)
            .await
            .map_err(|e| super::error::runtime_call("list moderation scope participants", e))?;
        return Ok(ModerationScope {
            context_id,
            home_id: current_home.id,
            can_moderate: current_home.can_moderate(),
            peers,
        });
    }

    Err(AuraError::permission_denied(
        "Moderation requires a valid home context and moderator privileges",
    ))
}

async fn commit_and_fanout(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    scope: &ModerationScope,
    fact: RelationalFact,
    extra_peers: &[aura_core::types::identifiers::AuthorityId],
) -> Result<(), AuraError> {
    runtime
        .commit_relational_facts(std::slice::from_ref(&fact))
        .await
        .map_err(|e| super::error::runtime_call("commit moderation fact", e))?;

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
        send_moderation_fact_with_retry(runtime, peer, scope.context_id, &fact).await?;
    }

    Ok(())
}

async fn apply_local_home_projection<F>(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &ModerationScope,
    actor: aura_core::types::identifiers::AuthorityId,
    timestamp_ms: u64,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut crate::views::home::HomeState),
{
    let mut homes = homes_state_signal_snapshot(app_core).await?;
    if homes.home_state(&scope.home_id).is_none() {
        homes.add_home(crate::views::home::HomeState::new(
            scope.home_id,
            None,
            actor,
            timestamp_ms,
            scope.context_id,
        ));
    }

    let Some(home) = homes.home_mut(&scope.home_id) else {
        return Err(AuraError::not_found(scope.home_id.to_string()));
    };
    update(home);
    emit_homes_state_observed(app_core, homes).await
}

async fn resolve_target_authority(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<aura_core::types::identifiers::AuthorityId, AuraError> {
    if let Ok(contact) = crate::workflows::query::resolve_contact(app_core, target).await {
        return Ok(contact.id);
    }
    parse_authority_id(target)
}

/// Kick a user from the current home.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    target: &str,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_channel_id(app_core, channel).await?;
    let target_id = resolve_target_authority(app_core, target).await?;
    kick_user_resolved(app_core, channel_id, target_id, reason, kicked_at_ms).await
}

/// Kick a canonical authority from a canonical channel.
pub async fn kick_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    target_id: aura_core::types::identifiers::AuthorityId,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, Some(channel_id)).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with kick capability can kick members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomeKickFact::new_ms(
        scope.context_id,
        channel_id,
        target_id,
        runtime.authority_id(),
        reason.map_or_else(String::new, str::to_string),
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
        .map_err(|e| super::error::runtime_call("enforce kick membership leave", e))?;

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
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = match channel_hint {
        Some(hint) => Some(resolve_channel_id(app_core, hint).await?),
        None => None,
    };
    ban_user_resolved(app_core, channel_id, target_id, reason, banned_at_ms).await
}

/// Ban a canonical authority, optionally scoped by canonical channel.
pub async fn ban_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: aura_core::types::identifiers::AuthorityId,
    reason: Option<&str>,
    banned_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with ban capability can ban members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomeBanFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        reason.map_or_else(String::new, str::to_string),
        banned_at_ms,
        None,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;
    apply_local_home_projection(
        app_core,
        &scope,
        runtime.authority_id(),
        banned_at_ms,
        |home| {
            home.add_ban(crate::views::home::BanRecord {
                authority_id: target_id,
                reason: reason.map_or_else(String::new, str::to_string),
                actor: runtime.authority_id(),
                banned_at: banned_at_ms,
            });
        },
    )
    .await?;

    Ok(())
}

/// Unban a user from the current home.
pub async fn unban_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = match channel_hint {
        Some(hint) => Some(resolve_channel_id(app_core, hint).await?),
        None => None,
    };
    unban_user_resolved(app_core, channel_id, target_id).await
}

/// Unban a canonical authority, optionally scoped by canonical channel.
pub async fn unban_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: aura_core::types::identifiers::AuthorityId,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with ban capability can unban members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let now_ms = runtime
        .current_time_ms()
        .await
        .map_err(|e| super::error::runtime_call("read timestamp for unban", e))?;
    let fact = HomeUnbanFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        now_ms,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;
    apply_local_home_projection(app_core, &scope, runtime.authority_id(), now_ms, |home| {
        home.remove_ban(&target_id);
    })
    .await?;

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
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = match channel_hint {
        Some(hint) => Some(resolve_channel_id(app_core, hint).await?),
        None => None,
    };
    mute_user_resolved(app_core, channel_id, target_id, duration_secs, muted_at_ms).await
}

/// Mute a canonical authority, optionally scoped by canonical channel.
pub async fn mute_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: aura_core::types::identifiers::AuthorityId,
    duration_secs: Option<u64>,
    muted_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with mute capability can mute members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
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
    apply_local_home_projection(
        app_core,
        &scope,
        runtime.authority_id(),
        muted_at_ms,
        |home| {
            home.add_mute(crate::views::home::MuteRecord {
                authority_id: target_id,
                duration_secs,
                muted_at: muted_at_ms,
                expires_at: expires_at_ms,
                actor: runtime.authority_id(),
            });
        },
    )
    .await?;

    Ok(())
}

/// Unmute a user in the current home.
pub async fn unmute_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
    target: &str,
) -> Result<(), AuraError> {
    let target_id = resolve_target_authority(app_core, target).await?;
    let channel_id = match channel_hint {
        Some(hint) => Some(resolve_channel_id(app_core, hint).await?),
        None => None,
    };
    unmute_user_resolved(app_core, channel_id, target_id).await
}

/// Unmute a canonical authority, optionally scoped by canonical channel.
pub async fn unmute_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<ChannelId>,
    target_id: aura_core::types::identifiers::AuthorityId,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with mute capability can unmute members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let now_ms = runtime
        .current_time_ms()
        .await
        .map_err(|e| super::error::runtime_call("read timestamp for unmute", e))?;
    let fact = HomeUnmuteFact::new_ms(
        scope.context_id,
        None,
        target_id,
        runtime.authority_id(),
        now_ms,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[target_id]).await?;
    apply_local_home_projection(app_core, &scope, runtime.authority_id(), now_ms, |home| {
        home.remove_mute(&target_id);
    })
    .await?;

    Ok(())
}

async fn scope_for_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<ModerationScope, AuraError> {
    if let Some(channel_id) = parse_channel_id_from_message_id(message_id) {
        return resolve_scope_by_channel_id(app_core, Some(channel_id)).await;
    }
    resolve_scope_by_channel_id(app_core, None).await
}

/// Pin a message in the current home.
pub async fn pin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let scope = scope_for_message(app_core, message_id).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with pin capability can pin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomePinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        runtime
            .current_time_ms()
            .await
            .map_err(|e| super::error::runtime_call("read timestamp for pin", e))?,
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
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with pin capability can unpin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
    let fact = HomeUnpinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        runtime
            .current_time_ms()
            .await
            .map_err(|e| super::error::runtime_call("read timestamp for unpin", e))?,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[]).await?;

    Ok(())
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
    use aura_core::{crypto::hash::hash, types::identifiers::AuthorityId};

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

    #[tokio::test]
    async fn resolve_target_authority_supports_contact_lookup() {
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
                is_member: false,
                last_interaction: None,
                is_online: true,
                read_receipt_policy: Default::default(),
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
    async fn resolve_scope_uses_named_channel_context_before_fallback_home() {
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
        runtime.set_authoritative_channel_name_matches("slash-lab", vec![channel_home_id]);
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
    async fn resolve_scope_by_channel_id_rejects_unknown_channel_scope() {
        let config = AppConfig::default();
        let runtime = Arc::new(OfflineRuntimeBridge::new(AuthorityId::new_from_entropy(
            [9u8; 32],
        )));
        let app_core = Arc::new(RwLock::new(AppCore::with_runtime(config, runtime).unwrap()));
        {
            let core = app_core.read().await;
            register_app_signals(core.reactive()).await.unwrap();
        }
        let unknown = ChannelId::from_bytes(hash(b"moderation-unknown-scope"));

        let error = resolve_scope_by_channel_id(&app_core, Some(unknown))
            .await
            .expect_err("unknown channel scope must fail");
        assert!(!error.to_string().is_empty());
    }
}
