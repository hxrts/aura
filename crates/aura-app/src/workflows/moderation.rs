//! Moderation Workflow - Portable Business Logic
//!
//! This module contains block moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! These operations delegate to the RuntimeBridge to commit moderation facts.
//! UI state is updated by reactive views driven from the journal.

use crate::AppCore;
use async_lock::RwLock;
use aura_core::{
    identifiers::{AuthorityId, ChannelId, ContextId},
    AuraError,
};
use std::sync::Arc;

fn parse_authority(target: &str) -> Result<AuthorityId, AuraError> {
    target
        .parse::<AuthorityId>()
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID: {}", target)))
}

fn parse_context_id(context_id: &str) -> Result<ContextId, AuraError> {
    let trimmed = context_id.trim();
    if trimmed.is_empty() {
        return Err(AuraError::not_found("Block context not available"));
    }

    trimmed
        .parse::<ContextId>()
        .map_err(|_| AuraError::invalid(format!("Invalid context ID: {}", trimmed)))
}

async fn current_block_context(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(ContextId, ChannelId, bool), AuraError> {
    let core = app_core.read().await;
    let blocks = core.views().get_blocks();
    let block = blocks
        .current_block()
        .ok_or_else(|| AuraError::not_found("No current block selected"))?;

    let context_id = parse_context_id(&block.context_id)?;
    Ok((context_id, block.id, block.is_admin()))
}

/// Kick a user from the current block.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    _kicked_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can kick residents",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target_id = parse_authority(target)?;
    runtime
        .moderation_kick(
            context_id,
            channel_id,
            target_id,
            reason.map(|s| s.to_string()),
        )
        .await
        .map_err(|e| AuraError::agent(format!("Failed to kick user: {}", e)))?;

    Ok(())
}

/// Ban a user from the current block.
pub async fn ban_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    _banned_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can ban residents",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target_id = parse_authority(target)?;
    runtime
        .moderation_ban(
            context_id,
            channel_id,
            target_id,
            reason.map(|s| s.to_string()),
        )
        .await
        .map_err(|e| AuraError::agent(format!("Failed to ban user: {}", e)))?;

    Ok(())
}

/// Unban a user from the current block.
pub async fn unban_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unban residents",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target_id = parse_authority(target)?;
    runtime
        .moderation_unban(context_id, channel_id, target_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unban user: {}", e)))?;

    Ok(())
}

/// Mute a user in the current block.
pub async fn mute_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    duration_secs: Option<u64>,
    _muted_at_ms: u64,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can mute residents",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target_id = parse_authority(target)?;
    runtime
        .moderation_mute(context_id, channel_id, target_id, duration_secs)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to mute user: {}", e)))?;

    Ok(())
}

/// Unmute a user in the current block.
pub async fn unmute_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unmute residents",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    let target_id = parse_authority(target)?;
    runtime
        .moderation_unmute(context_id, channel_id, target_id)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unmute user: {}", e)))?;

    Ok(())
}

/// Pin a message in the current block.
pub async fn pin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can pin messages",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .moderation_pin(context_id, channel_id, message_id.to_string())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to pin message: {}", e)))?;

    Ok(())
}

/// Unpin a message in the current block.
pub async fn unpin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let (context_id, channel_id, is_admin) = current_block_context(app_core).await?;
    if !is_admin {
        return Err(AuraError::permission_denied(
            "Only stewards can unpin messages",
        ));
    }

    let runtime = {
        let core = app_core.read().await;
        core.runtime()
            .ok_or_else(|| AuraError::agent("Runtime bridge not available"))?
            .clone()
    };

    runtime
        .moderation_unpin(context_id, channel_id, message_id.to_string())
        .await
        .map_err(|e| AuraError::agent(format!("Failed to unpin message: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn moderation_requires_block() {
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
