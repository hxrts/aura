//! Admin maintenance commands (replacement, fork controls).

use anyhow::{anyhow, Result};
use aura_agent::EffectContext;
use aura_core::effects::JournalEffects;
use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
use aura_core::{FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use serde::Serialize;

use crate::AdminAction;

/// Handle admin-related maintenance commands.
pub async fn handle_admin(
    ctx: &EffectContext,
    effects: &aura_agent::AuraEffectSystem,
    device_id: DeviceId,
    action: &AdminAction,
) -> Result<()> {
    match action {
        AdminAction::Replace {
            account,
            new_admin,
            activation_epoch,
        } => {
            replace_admin(
                ctx,
                effects,
                device_id,
                account,
                new_admin,
                *activation_epoch,
            )
            .await
        }
    }
}

async fn replace_admin(
    ctx: &EffectContext,
    effects: &aura_agent::AuraEffectSystem,
    device_id: DeviceId,
    account: &str,
    new_admin: &str,
    activation_epoch: u64,
) -> Result<()> {
    let account_id: AccountId = account.parse().map_err(|e: uuid::Error| anyhow!(e))?;
    let new_admin_id: AuthorityId = new_admin.parse().map_err(|e: uuid::Error| anyhow!(e))?;

    println!(
        "Replacing admin for account {} with {} (activation epoch {})",
        account_id, new_admin_id, activation_epoch
    );

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(device_id.0);

    // Persist an admin replacement fact into the journal so downstream runtimes apply the change.
    #[derive(Serialize)]
    struct AdminReplacementFact {
        account_id: AccountId,
        requested_by: AuthorityId,
        new_admin: AuthorityId,
        activation_epoch: u64,
    }

    let fact_payload = AdminReplacementFact {
        account_id,
        requested_by: authority_id,
        new_admin: new_admin_id,
        activation_epoch,
    };

    let fact_content = FactContent::Relational(RelationalFact::Generic {
        context_id: ctx.context_id(),
        binding_type: "admin_replaced".to_string(),
        binding_data: serde_json::to_vec(&fact_payload)
            .map_err(|e| anyhow!("Failed to serialize admin replacement payload: {}", e))?,
    });

    let fact_value = serde_json::to_vec(&fact_content)
        .map(FactValue::Bytes)
        .map_err(|e| anyhow!("Failed to encode admin replacement fact: {}", e))?;

    let mut delta = Journal::new();
    let fact_key = format!("admin_replace:{}", account_id);
    delta.facts.insert(fact_key.clone(), fact_value);

    let current = effects
        .get_journal()
        .await
        .map_err(|e| anyhow!("Failed to load journal: {}", e))?;
    let merged = effects
        .merge_facts(&current, &delta)
        .await
        .map_err(|e| anyhow!("Failed to merge admin replacement fact: {}", e))?;
    effects
        .persist_journal(&merged)
        .await
        .map_err(|e| anyhow!("Failed to persist admin replacement fact: {}", e))?;

    println!(
        "Admin replacement recorded; new admin {} activates at epoch {}",
        new_admin_id, activation_epoch
    );
    Ok(())
}
