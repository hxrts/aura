//! Admin maintenance commands (replacement, fork controls).

use anyhow::{anyhow, Result};
use aura_agent::AuraAgent;
use aura_core::identifiers::{AccountId, DeviceId};
use aura_protocol::effects::{AuraEffectSystem, ConsoleEffects};

use crate::AdminAction;

/// Handle admin-related maintenance commands.
pub async fn handle_admin(effects: AuraEffectSystem, action: &AdminAction) -> Result<()> {
    match action {
        AdminAction::Replace {
            account,
            new_admin,
            activation_epoch,
        } => replace_admin(effects, account, new_admin, *activation_epoch).await,
    }
}

async fn replace_admin(
    mut effects: AuraEffectSystem,
    account: &str,
    new_admin: &str,
    activation_epoch: u64,
) -> Result<()> {
    let account_id: AccountId = account.parse().map_err(|e: uuid::Error| anyhow!(e))?;
    let new_admin_id: DeviceId = new_admin.parse().map_err(|e: uuid::Error| anyhow!(e))?;

    let device_id = effects.device_id();
    effects.log_info(&format!(
        "Replacing admin for account {} with {} (activation epoch {})",
        account_id, new_admin_id, activation_epoch
    ));

    let agent = AuraAgent::new(effects, device_id);
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
