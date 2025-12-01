//! Authority management handler.

use crate::commands::authority::AuthorityCommands;
use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::effects::{ConsoleEffects, PhysicalTimeEffects, StorageEffects};
use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

/// Execute authority management commands.
pub async fn handle_authority(
    ctx: &EffectContext,
    effect_system: &AuraEffectSystem,
    command: &AuthorityCommands,
) -> Result<()> {
    match command {
        AuthorityCommands::Create { threshold } => {
            create_authority(ctx, effect_system, threshold.unwrap_or(1) as u32).await?;
        }
        AuthorityCommands::Status { authority_id } => {
            show_authority(effect_system, authority_id).await?;
        }
        AuthorityCommands::List => {
            list_authorities(effect_system).await?;
        }
        AuthorityCommands::AddDevice {
            authority_id,
            public_key,
        } => {
            add_device(effect_system, authority_id, public_key).await?;
        }
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthorityRecord {
    authority_id: AuthorityId,
    threshold: u32,
    devices: Vec<String>,
    created_ms: u64,
}

async fn create_authority(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    threshold: u32,
) -> Result<()> {
    let now = effects
        .physical_time()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get time: {}", e))?
        .ts_ms;
    let authority_id =
        crate::ids::authority_id(&format!("authority:{}:{}", ctx.authority_id(), now));

    let record = AuthorityRecord {
        authority_id,
        threshold,
        devices: vec![],
        created_ms: now,
    };

    let key = format!("authority:{}", authority_id);
    effects
        .store(&key, serde_json::to_vec(&record)?)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to persist authority: {}", e))?;

    ConsoleEffects::log_info(
        effects,
        &format!(
            "Created authority {} with threshold {}",
            authority_id, threshold
        ),
    )
    .await?;
    Ok(())
}

async fn show_authority(effects: &AuraEffectSystem, authority_id: &AuthorityId) -> Result<()> {
    let key = format!("authority:{}", authority_id);
    if let Some(bytes) = effects.retrieve(&key).await? {
        let record: AuthorityRecord = serde_json::from_slice(&bytes)?;
        ConsoleEffects::log_info(
            effects,
            &format!(
                "Authority {} — threshold {}, devices: {}",
                record.authority_id,
                record.threshold,
                record.devices.join(", ")
            ),
        )
        .await?;
    } else {
        ConsoleEffects::log_error(
            effects,
            &format!("Authority {} not found in storage", authority_id),
        )
        .await?;
    }
    Ok(())
}

async fn list_authorities(effects: &AuraEffectSystem) -> Result<()> {
    let keys = effects
        .list_keys(Some("authority:"))
        .await
        .unwrap_or_default();
    if keys.is_empty() {
        ConsoleEffects::log_info(effects, "No authorities stored yet").await?;
    } else {
        ConsoleEffects::log_info(effects, &format!("Stored authorities ({}):", keys.len())).await?;
        for key in keys {
            ConsoleEffects::log_info(effects, &format!("- {}", key)).await?;
        }
    }
    Ok(())
}

async fn add_device(
    effects: &AuraEffectSystem,
    authority_id: &AuthorityId,
    public_key: &str,
) -> Result<()> {
    let key = format!("authority:{}", authority_id);
    let mut record: AuthorityRecord = if let Some(bytes) = effects.retrieve(&key).await? {
        serde_json::from_slice(&bytes)?
    } else {
        return Err(anyhow::anyhow!(
            "Authority {} not found; create it first",
            authority_id
        ));
    };

    record.devices.push(public_key.to_string());

    effects
        .store(&key, serde_json::to_vec(&record)?)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update authority {}: {}", authority_id, e))?;

    ConsoleEffects::log_info(
        effects,
        &format!(
            "Added device key to authority {} ({} devices total)",
            authority_id,
            record.devices.len()
        ),
    )
    .await?;
    Ok(())
}
