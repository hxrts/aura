//! Moderation Workflow - Portable Business Logic
//!
//! This module contains home moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.

use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    identifiers::{ChannelId, ContextId},
    AuraError,
};
use std::sync::Arc;

async fn current_home_context(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(ContextId, ChannelId, bool), AuraError> {
    let core = app_core.read().await;
    let homes = core.views().get_homes();
    let home_state = homes
        .current_home()
        .ok_or_else(|| AuraError::not_found("No current home selected"))?;

    let ctx_id = home_state
        .context_id
        .ok_or_else(|| AuraError::not_found("Home has no context ID"))?;
    Ok((ctx_id, home_state.id, home_state.is_admin()))
}

/// Kick a user from the current home.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    _kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can kick residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    runtime
        .moderation_kick(
            context_id,
            channel_id,
            target_id,
            reason.map(|s| s.to_string()),
        )
        .await
        .map_err(|e| AuraError::agent(format!("Failed to kick user: {e}")))?;

    Ok(())
}

/// Ban a user from the current home.
pub async fn ban_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    _banned_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can ban residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    runtime
        .moderation_ban(
            context_id,
            channel_id,
            target_id,
            reason.map(|s| s.to_string()),
        )
        .await
        .map_err(|e| AuraError::agent(format!("Failed to ban user: {e}")))?;

    Ok(())
}

/// Unban a user from the current home.
pub async fn unban_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unban residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    runtime
        .moderation_unban(context_id, channel_id, target_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unban user: {e}")))?;

    Ok(())
}

/// Mute a user in the current home.
pub async fn mute_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    duration_secs: Option<u64>,
    _muted_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can mute residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    runtime
        .moderation_mute(context_id, channel_id, target_id, duration_secs)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to mute user: {e}")))?;

    Ok(())
}

/// Unmute a user in the current home.
pub async fn unmute_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unmute residents",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    let target_id = parse_authority_id(target)?;
    runtime
        .moderation_unmute(context_id, channel_id, target_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unmute user: {e}")))?;

    Ok(())
}

/// Pin a message in the current home.
pub async fn pin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can pin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    runtime
        .moderation_pin(context_id, channel_id, message_id.to_string())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to pin message: {e}")))?;

    Ok(())
}

/// Unpin a message in the current home.
pub async fn unpin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_home_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unpin messages",
        ));
    }

    let runtime = { require_runtime(app_core).await? };

    runtime
        .moderation_unpin(context_id, channel_id, message_id.to_string())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unpin message: {e}")))?;

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
            "authority-00000000-0000-0000-0000-000000000000",
            None,
            0
        )
        .await
        .is_err());
        assert!(kick_user(
            &app_core,
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
}
