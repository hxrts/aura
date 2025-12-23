//! Moderation Workflow - Portable Business Logic
//!
//! This module contains block moderation operations (kick/ban/mute/pin) that are
//! portable across all frontends.
//!
//! Note: These operations currently update view state directly. In the full
//! architecture, moderation should commit facts through the guard chain.

use crate::{views::block::{BanRecord, KickRecord, MuteRecord}, AppCore};
use async_lock::RwLock;
use aura_core::{identifiers::AuthorityId, AuraError};
use std::sync::Arc;

fn actor_id_for_demo() -> AuthorityId {
    AuthorityId::default()
}

async fn with_current_block_mut<T>(
    app_core: &Arc<RwLock<AppCore>>,
    f: impl FnOnce(&mut crate::views::BlocksState) -> Result<T, AuraError>,
) -> Result<T, AuraError> {
    let mut core = app_core.write().await;
    let mut blocks = core.views().get_blocks().clone();

    let out = f(&mut blocks)?;

    if let Some(block) = blocks.current_block() {
        core.views_mut().set_block(block.clone());
    }
    core.views_mut().set_blocks(blocks);

    Ok(out)
}

fn parse_authority(target: &str) -> Result<AuthorityId, AuraError> {
    target
        .parse::<AuthorityId>()
        .map_err(|_| AuraError::invalid(format!("Invalid authority ID: {}", target)))
}

/// Kick a user from the current block.
pub async fn kick_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    kicked_at_ms: u64,
) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can kick residents",
            ));
        }

        let target_id = parse_authority(target)?;

        let removed = block
            .remove_resident(&target_id)
            .ok_or_else(|| AuraError::not_found(format!("Resident not found: {}", target)))?;

        let record = KickRecord {
            authority_id: removed.id,
            channel: block.id,
            reason: reason.unwrap_or("").to_string(),
            actor: actor_id_for_demo(),
            kicked_at: kicked_at_ms,
        };
        block.add_kick(record);

        Ok(())
    })
    .await
}

/// Ban a user from the current block.
pub async fn ban_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    reason: Option<&str>,
    banned_at_ms: u64,
) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can ban residents",
            ));
        }

        let target_id = parse_authority(target)?;

        let record = BanRecord {
            authority_id: target_id,
            reason: reason.unwrap_or("").to_string(),
            actor: actor_id_for_demo(),
            banned_at: banned_at_ms,
        };
        block.add_ban(record);

        let _ = block.remove_resident(&target_id);

        Ok(())
    })
    .await
}

/// Unban a user from the current block.
pub async fn unban_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can unban residents",
            ));
        }

        let target_id = parse_authority(target)?;

        if block.remove_ban(&target_id).is_none() {
            return Err(AuraError::not_found(format!("User is not banned: {}", target)));
        }

        Ok(())
    })
    .await
}

/// Mute a user in the current block.
pub async fn mute_user(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    duration_secs: Option<u64>,
    muted_at_ms: u64,
) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can mute residents",
            ));
        }

        let target_id = parse_authority(target)?;

        let expires_at = duration_secs.map(|s| muted_at_ms.saturating_add(s.saturating_mul(1000)));
        let record = MuteRecord {
            authority_id: target_id,
            duration_secs,
            muted_at: muted_at_ms,
            expires_at,
            actor: actor_id_for_demo(),
        };
        block.add_mute(record);

        Ok(())
    })
    .await
}

/// Unmute a user in the current block.
pub async fn unmute_user(app_core: &Arc<RwLock<AppCore>>, target: &str) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can unmute residents",
            ));
        }

        let target_id = parse_authority(target)?;

        if block.remove_mute(&target_id).is_none() {
            return Err(AuraError::not_found(format!("User is not muted: {}", target)));
        }

        Ok(())
    })
    .await
}

/// Pin a message in the current block.
pub async fn pin_message(app_core: &Arc<RwLock<AppCore>>, message_id: &str) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can pin messages",
            ));
        }

        block.pin_message(message_id.to_string());
        Ok(())
    })
    .await
}

/// Unpin a message in the current block.
pub async fn unpin_message(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    with_current_block_mut(app_core, |blocks| {
        let block = blocks
            .current_block_mut()
            .ok_or_else(|| AuraError::not_found("No current block selected"))?;

        if !block.is_admin() {
            return Err(AuraError::permission_denied(
                "Only stewards can unpin messages",
            ));
        }

        if !block.unpin_message(message_id) {
            return Err(AuraError::not_found(format!(
                "Message is not pinned: {}",
                message_id
            )));
        }

        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AppConfig;

    #[tokio::test]
    async fn moderation_requires_block() {
        let config = AppConfig::default();
        let app_core = Arc::new(RwLock::new(AppCore::new(config).unwrap()));

        assert!(ban_user(&app_core, "authority-00000000-0000-0000-0000-000000000000", None, 0)
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
