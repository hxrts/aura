use super::scope::{current_moderation_scope, scope_for_message};
use super::support::{
    apply_local_home_projection, commit_and_fanout, moderation_timestamp, require_capability,
    resolve_channel_hint, resolve_target_id, ModerationCapability,
};
use crate::workflows::runtime::{require_runtime, timeout_runtime_call};
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    effects::amp::ChannelLeaveParams,
    types::identifiers::{AuthorityId, ChannelId},
    AuraError,
};
use aura_journal::DomainFact;
use aura_social::moderation::facts::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomePinFact, HomeUnbanFact, HomeUnmuteFact,
    HomeUnpinFact,
};
use std::sync::Arc;

/// Kick a user from the current home.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    channel: &str,
    target: &str,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_channel_hint(app_core, channel).await?;
    let target_id = resolve_target_id(app_core, target).await?;
    kick_user_resolved(app_core, channel_id, target_id, reason, kicked_at_ms).await
}

/// Kick a canonical authority from a canonical channel.
pub async fn kick_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    target_id: AuthorityId,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = current_moderation_scope(app_core).await?;
    if scope.home_id != channel_id {
        return Err(AuraError::invalid(
            "kick requires the selected home to match the target channel",
        ));
    }
    require_capability(&scope, ModerationCapability::Kick)?;

    let runtime = require_runtime(app_core).await?;
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
    timeout_runtime_call(
        &runtime,
        "kick_user_resolved",
        "amp_leave_channel",
        super::MODERATION_RUNTIME_TIMEOUT,
        || {
            runtime.amp_leave_channel(ChannelLeaveParams {
                context: scope.context_id,
                channel: channel_id,
                participant: target_id,
            })
        },
    )
    .await
    .map_err(|e| super::super::error::runtime_call("enforce kick membership leave", e))?
    .map_err(|e| super::super::error::runtime_call("enforce kick membership leave", e))?;

    Ok(())
}

/// Ban a user from the current home.
pub async fn ban_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    banned_at_ms: u64,
) -> Result<(), AuraError> {
    let target_id = resolve_target_id(app_core, target).await?;
    ban_user_resolved(app_core, target_id, reason, banned_at_ms).await
}

/// Ban a canonical authority in the currently selected home.
pub async fn ban_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    target_id: AuthorityId,
    reason: Option<&str>,
    banned_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = current_moderation_scope(app_core).await?;
    require_capability(&scope, ModerationCapability::Ban)?;

    let runtime = require_runtime(app_core).await?;
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
pub async fn unban_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let target_id = resolve_target_id(app_core, target).await?;
    unban_user_resolved(app_core, target_id).await
}

/// Unban a canonical authority in the currently selected home.
pub async fn unban_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    let scope = current_moderation_scope(app_core).await?;
    require_capability(&scope, ModerationCapability::Ban)?;

    let runtime = require_runtime(app_core).await?;
    let now_ms =
        moderation_timestamp(&runtime, "unban_user_resolved", "read timestamp for unban").await?;
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
    target: &str,
    duration_secs: Option<u64>,
    muted_at_ms: u64,
) -> Result<(), AuraError> {
    let target_id = resolve_target_id(app_core, target).await?;
    mute_user_resolved(app_core, target_id, duration_secs, muted_at_ms).await
}

/// Mute a canonical authority in the currently selected home.
pub async fn mute_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    target_id: AuthorityId,
    duration_secs: Option<u64>,
    muted_at_ms: u64,
) -> Result<(), AuraError> {
    let scope = current_moderation_scope(app_core).await?;
    require_capability(&scope, ModerationCapability::Mute)?;

    let runtime = require_runtime(app_core).await?;
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
pub async fn unmute_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let target_id = resolve_target_id(app_core, target).await?;
    unmute_user_resolved(app_core, target_id).await
}

/// Unmute a canonical authority in the currently selected home.
pub async fn unmute_user_resolved(
    app_core: &Arc<RwLock<AppCore>>,
    target_id: AuthorityId,
) -> Result<(), AuraError> {
    let scope = current_moderation_scope(app_core).await?;
    require_capability(&scope, ModerationCapability::Mute)?;

    let runtime = require_runtime(app_core).await?;
    let now_ms = moderation_timestamp(
        &runtime,
        "unmute_user_resolved",
        "read timestamp for unmute",
    )
    .await?;
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

/// Pin a message in the current home.
pub async fn pin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let scope = scope_for_message(app_core, message_id).await?;
    require_capability(&scope, ModerationCapability::Pin)?;

    let runtime = require_runtime(app_core).await?;
    let fact = HomePinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        moderation_timestamp(&runtime, "pin_message", "read timestamp for pin").await?,
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
    require_capability(&scope, ModerationCapability::Pin)?;

    let runtime = require_runtime(app_core).await?;
    let fact = HomeUnpinFact::new_ms(
        scope.context_id,
        scope.home_id,
        message_id.to_string(),
        runtime.authority_id(),
        moderation_timestamp(&runtime, "unpin_message", "read timestamp for unpin").await?,
    )
    .to_generic();
    commit_and_fanout(&runtime, &scope, fact, &[]).await?;

    Ok(())
}
