//! Moderation Workflow - Portable Business Logic
//!
//! This module contains home moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.

use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{converge_runtime, cooperative_yield, require_runtime};
use crate::workflows::{channel_ref::ChannelSelector, snapshot_policy::chat_snapshot};
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

const MODERATION_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const MODERATION_FACT_SEND_YIELDS_PER_RETRY: usize = 4;

#[derive(Debug, Clone)]
struct ModerationScope {
    context_id: ContextId,
    home_id: ChannelId,
    can_moderate: bool,
    peers: Vec<aura_core::identifiers::AuthorityId>,
}

fn parse_channel_hint(channel: &str) -> Result<ChannelSelector, AuraError> {
    ChannelSelector::parse(channel)
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
    let parsed = parse_channel_hint(channel)?;
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    match parsed {
        ChannelSelector::Id(channel_id) => {
            if chat.channel(&channel_id).is_some() || homes.home_state(&channel_id).is_some() {
                Ok(channel_id)
            } else {
                Err(AuraError::not_found(format!(
                    "Unknown channel scope: {channel_id}"
                )))
            }
        }
        ChannelSelector::Name(name) => chat
            .all_channels()
            .find(|entry| entry.name.eq_ignore_ascii_case(&name))
            .map(|entry| entry.id)
            .or_else(|| {
                homes
                    .iter()
                    .find(|(_, home)| home.name.eq_ignore_ascii_case(&name))
                    .map(|(home_id, _)| *home_id)
            })
            .ok_or_else(|| AuraError::not_found(format!("Unknown channel scope: {channel}"))),
    }
}

#[cfg(test)]
async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModerationScope, AuraError> {
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let hinted_channel = if let Some(hint) = channel_hint {
        Some(resolve_channel_id(app_core, hint).await?)
    } else {
        None
    };

    let context_from_channel = hinted_channel.and_then(|channel_id| {
        chat.channel(&channel_id)
            .and_then(|channel| channel.context_id)
    });

    let home_from_channel = hinted_channel.and_then(|channel_id| {
        if let Some(home) = homes.home_state(&channel_id) {
            if let Some(context_id) = home.context_id {
                if !home.can_moderate() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.can_moderate() {
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

    let (context_id, home_id, can_moderate, peers) =
        if let Some((home_id, home_state)) = home_from_channel {
            let peers = home_state
                .members
                .iter()
                .map(|member| member.id)
                .collect::<Vec<_>>();
            (
                home_state
                    .context_id
                    .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
                home_id,
                home_state.can_moderate(),
                peers,
            )
        } else if let Some(context_id) = context_from_channel {
            let home_id = hinted_channel
                .ok_or_else(|| AuraError::not_found("Channel hint missing for moderation scope"))?;
            let peers = homes
                .current_home()
                .map(|home| home.members.iter().map(|member| member.id).collect())
                .unwrap_or_default();
            let can_moderate = homes
                .current_home()
                .map(|home| home.can_moderate())
                .unwrap_or(false);
            (context_id, home_id, can_moderate, peers)
        } else if let Some(fallback) = homes.current_home() {
            (
                fallback
                    .context_id
                    .ok_or_else(|| AuraError::not_found("Home has no context ID"))?,
                fallback.id,
                fallback.can_moderate(),
                fallback.members.iter().map(|member| member.id).collect(),
            )
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
    let chat = chat_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };

    let from_home = |home_id: ChannelId, home: &crate::views::home::HomeState| {
        let context_id = home
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = home.members.iter().map(|member| member.id).collect();
        Ok(ModerationScope {
            context_id,
            home_id,
            can_moderate: home.can_moderate(),
            peers,
        })
    };

    if let Some(channel_id) = channel_hint {
        if let Some(home) = homes.home_state(&channel_id) {
            if let Some(context_id) = home.context_id {
                if !home.can_moderate() {
                    if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
                        if best_home.can_moderate() {
                            return from_home(best_id, &best_home);
                        }
                    }
                }
            }
            return from_home(channel_id, home);
        }

        let context_id = chat
            .channel(&channel_id)
            .and_then(|channel| channel.context_id)
            .ok_or_else(|| AuraError::not_found(format!("Unknown channel scope: {channel_id}")))?;

        if let Some((best_id, best_home)) = best_home_for_context(&homes, context_id) {
            return from_home(best_id, &best_home);
        }

        return Err(AuraError::permission_denied(format!(
            "Moderation requires a moderator home for context {context_id}"
        )));
    }

    if let Some(current_home) = homes.current_home() {
        return from_home(current_home.id, current_home);
    }

    Err(AuraError::permission_denied(
        "Moderation requires a valid home context and moderator privileges",
    ))
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
        send_moderation_fact_with_retry(runtime, peer, scope.context_id, &fact).await?;
    }

    Ok(())
}

async fn send_moderation_fact_with_retry(
    runtime: &Arc<dyn crate::runtime_bridge::RuntimeBridge>,
    peer: aura_core::identifiers::AuthorityId,
    context_id: ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let mut last_error: Option<String> = None;

    for attempt in 0..MODERATION_FACT_SEND_MAX_ATTEMPTS {
        match runtime.send_chat_fact(peer, context_id, fact).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error.to_string()),
        }

        if attempt + 1 < MODERATION_FACT_SEND_MAX_ATTEMPTS {
            converge_runtime(runtime).await;
            for _ in 0..MODERATION_FACT_SEND_YIELDS_PER_RETRY {
                cooperative_yield().await;
            }
        }
    }

    let message = last_error.unwrap_or_else(|| "unknown transport error".to_string());
    Err(AuraError::agent(format!(
        "Failed to deliver moderation fact to {peer} after {MODERATION_FACT_SEND_MAX_ATTEMPTS} attempts: {message}"
    )))
}

async fn apply_local_home_projection<F>(
    app_core: &Arc<RwLock<AppCore>>,
    scope: &ModerationScope,
    actor: aura_core::identifiers::AuthorityId,
    timestamp_ms: u64,
    update: F,
) -> Result<(), AuraError>
where
    F: FnOnce(&mut crate::views::home::HomeState),
{
    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
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
        return Err(AuraError::not_found(format!(
            "Moderation scope home {} not found",
            scope.home_id
        )));
    };
    update(home);
    core.views_mut().set_homes(homes);
    Ok(())
}

async fn resolve_target_authority(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
) -> Result<aura_core::identifiers::AuthorityId, AuraError> {
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
    target_id: aura_core::identifiers::AuthorityId,
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
    target_id: aura_core::identifiers::AuthorityId,
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
        reason.unwrap_or_default().to_string(),
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
                reason: reason.unwrap_or_default().to_string(),
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
    target_id: aura_core::identifiers::AuthorityId,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with ban capability can unban members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
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
    target_id: aura_core::identifiers::AuthorityId,
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
    target_id: aura_core::identifiers::AuthorityId,
) -> Result<(), AuraError> {
    let scope = resolve_scope_by_channel_id(app_core, channel_hint).await?;
    if !scope.can_moderate {
        return Err(AuraError::permission_denied(
            "Only moderators with mute capability can unmute members",
        ));
    }

    let runtime = { require_runtime(app_core).await? };
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
        runtime.current_time_ms().await.map_err(|e| {
            AuraError::agent(format!("Failed to read timestamp for unpin operation: {e}"))
        })?,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[]).await?;

    Ok(())
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
    use aura_core::{crypto::hash::hash, identifiers::AuthorityId};

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
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        let fallback_context = ContextId::new_from_entropy([21u8; 32]);
        let channel_context = ContextId::new_from_entropy([22u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let fallback_home_id = ChannelId::from_bytes(hash(b"moderation-fallback-home"));
        let channel_home_id = ChannelId::from_bytes(hash(b"moderation-channel-home"));

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
                member_ids: vec![owner],
                member_count: 1,
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
    async fn resolve_scope_by_channel_id_rejects_unknown_channel_scope() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));
        let unknown = ChannelId::from_bytes(hash(b"moderation-unknown-scope"));

        let error = resolve_scope_by_channel_id(&app_core, Some(unknown))
            .await
            .expect_err("unknown channel scope must fail");
        assert!(error.to_string().contains("Unknown channel scope"));
    }
}
