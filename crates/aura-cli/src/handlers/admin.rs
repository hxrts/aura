//! Admin maintenance commands (replacement, fork controls).

use anyhow::{anyhow, Result};
use aura_agent::{runtime::EffectSystemBuilder, AuraAgent};
use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
use aura_protocol::effect_traits::ConsoleEffects;

use crate::AdminAction;

/// Handle admin-related maintenance commands.
pub async fn handle_admin(device_id: DeviceId, action: &AdminAction) -> Result<()> {
    match action {
        AdminAction::Replace {
            account,
            new_admin,
            activation_epoch,
        } => replace_admin(device_id, account, new_admin, *activation_epoch).await,
    }
}

async fn replace_admin(
    device_id: DeviceId,
    account: &str,
    new_admin: &str,
    activation_epoch: u64,
) -> Result<()> {
    let account_id: AccountId = account.parse().map_err(|e: uuid::Error| anyhow!(e))?;
    let new_admin_id: AuthorityId = new_admin.parse().map_err(|e: uuid::Error| anyhow!(e))?;

    // Create effect system for this operation
    let effects = EffectSystemBuilder::new()
        .with_device_id(device_id)
        .build_sync()?;

    let _ = effects
        .log_info(&format!(
            "Replacing admin for account {} with {} (activation epoch {})",
            account_id, new_admin_id, activation_epoch
        ))
        .await;

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(device_id.0);

    let agent = AuraAgent::new(effects, authority_id);
    agent
        .replace_admin(account_id, new_admin_id, activation_epoch)
        .await
        .map_err(|e| anyhow!("admin replacement failed: {}", e))?;
    println!(
        "Admin replacement recorded for account {}. New admin {} becomes active at epoch {}.",
        account_id, new_admin_id, activation_epoch
    );
    Ok(())
}
