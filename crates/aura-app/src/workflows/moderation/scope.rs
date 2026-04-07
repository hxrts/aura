#[cfg(test)]
use crate::workflows::home_scope::{best_home_for_context_by, identify_materialized_channel_hint};
use crate::workflows::observed_projection::homes_signal_snapshot;
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    types::identifiers::{ChannelId, ContextId},
    AuraError,
};
use std::sync::Arc;

#[aura_macros::strong_reference(domain = "home_scope")]
#[derive(Debug, Clone)]
pub(crate) struct ModerationScope {
    pub(crate) context_id: ContextId,
    pub(crate) home_id: ChannelId,
    pub(crate) can_moderate: bool,
    pub(crate) peers: Vec<aura_core::types::identifiers::AuthorityId>,
}

#[cfg(test)]
pub(crate) fn best_home_for_context(
    homes: &crate::views::home::HomesState,
    context_id: ContextId,
) -> Option<(ChannelId, crate::views::home::HomeState)> {
    best_home_for_context_by(homes, context_id, |home| {
        (
            u8::from(home.can_moderate()),
            u8::from(!home.members.is_empty()),
            home.member_count as usize,
        )
    })
}

#[cfg(test)]
pub(crate) async fn resolve_scope(
    app_core: &Arc<RwLock<AppCore>>,
    channel_hint: Option<&str>,
) -> Result<ModerationScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_signal_snapshot(app_core).await?;

    let hinted_channel = if let Some(hint) = channel_hint {
        Some(
            identify_materialized_channel_hint(
                app_core,
                hint,
                "identify_materialized_channel_hint",
                "resolve moderation channel",
                super::MODERATION_RUNTIME_TIMEOUT,
            )
            .await?,
        )
    } else {
        None
    };

    let home_from_channel = if let Some(hinted_channel) = hinted_channel {
        let channel_id = hinted_channel.channel_id;
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
        } else if let Some(context_id) = match hinted_channel.context_id {
            Some(context_id) => Some(context_id),
            None => timeout_runtime_call(
                &runtime,
                "resolve_scope",
                "resolve_amp_channel_context",
                super::MODERATION_RUNTIME_TIMEOUT,
                || runtime.resolve_amp_channel_context(channel_id),
            )
            .await
            .map_err(|e| super::super::error::runtime_call("resolve moderation scope context", e))?
            .map_err(|e| {
                super::super::error::runtime_call("resolve moderation scope context", e)
            })?,
        } {
            best_home_for_context(&homes, context_id)
        } else {
            None
        }
    } else {
        None
    };

    let (context_id, home_id, can_moderate, peers) = if let Some((home_id, home_state)) =
        home_from_channel
    {
        let home_context_id = home_state
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = timeout_runtime_call(
            &runtime,
            "resolve_scope",
            "amp_list_channel_participants",
            super::MODERATION_RUNTIME_TIMEOUT,
            || runtime.amp_list_channel_participants(home_context_id, home_id),
        )
        .await
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?;
        (home_context_id, home_id, home_state.can_moderate(), peers)
    } else if hinted_channel.is_some() {
        return Err(AuraError::permission_denied(
            "Moderation requires an authoritative home scope for the requested channel",
        ));
    } else if let Some(fallback) = homes.current_home() {
        let context_id = fallback
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = timeout_runtime_call(
            &runtime,
            "resolve_scope",
            "amp_list_channel_participants",
            super::MODERATION_RUNTIME_TIMEOUT,
            || runtime.amp_list_channel_participants(context_id, fallback.id),
        )
        .await
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?;
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

pub(crate) async fn current_moderation_scope(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ModerationScope, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let homes = homes_signal_snapshot(app_core).await?;

    if let Some(current_home) = homes.current_home() {
        let context_id = current_home
            .context_id
            .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
        let peers = timeout_runtime_call(
            &runtime,
            "current_moderation_scope",
            "amp_list_channel_participants",
            super::MODERATION_RUNTIME_TIMEOUT,
            || runtime.amp_list_channel_participants(context_id, current_home.id),
        )
        .await
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?
        .map_err(|e| super::super::error::runtime_call("list moderation scope participants", e))?;
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

pub(crate) async fn scope_for_message(
    app_core: &Arc<RwLock<AppCore>>,
    _message_id: &str,
) -> Result<ModerationScope, AuraError> {
    current_moderation_scope(app_core).await
}
