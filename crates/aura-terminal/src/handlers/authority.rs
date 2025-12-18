//! Authority inspection CLI commands
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::authority::AuthorityCommands;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::effects::{PhysicalTimeEffects, StorageEffects};
use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

/// Handle authority inspection commands
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_authority(
    ctx: &HandlerContext<'_>,
    command: &AuthorityCommands,
) -> TerminalResult<CliOutput> {
    match command {
        AuthorityCommands::Create { threshold } => {
            create_authority(ctx, threshold.unwrap_or(1) as u32).await
        }
        AuthorityCommands::Status { authority_id } => show_authority(ctx, authority_id).await,
        AuthorityCommands::List => list_authorities(ctx).await,
        AuthorityCommands::AddDevice {
            authority_id,
            public_key,
        } => add_device(ctx, authority_id, public_key).await,
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct AuthorityRecord {
    authority_id: AuthorityId,
    threshold: u32,
    devices: Vec<String>,
    created_ms: u64,
}

async fn create_authority(ctx: &HandlerContext<'_>, threshold: u32) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let effects = ctx.effects();
    let effect_ctx = ctx.effect_context();

    let now = effects
        .physical_time()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get time: {}", e)))?
        .ts_ms;
    let authority_id =
        crate::ids::authority_id(&format!("authority:{}:{}", effect_ctx.authority_id(), now));

    let record = AuthorityRecord {
        authority_id,
        threshold,
        devices: vec![],
        created_ms: now,
    };

    let key = format!("authority:{}", authority_id);
    effects
        .store(
            &key,
            serde_json::to_vec(&record).map_err(|e| {
                TerminalError::Operation(format!("Failed to serialize authority record: {}", e))
            })?,
        )
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to persist authority: {}", e)))?;

    output.kv("Created authority", authority_id.to_string());
    output.kv("Threshold", threshold.to_string());

    Ok(output)
}

async fn show_authority(
    ctx: &HandlerContext<'_>,
    authority_id: &AuthorityId,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let effects = ctx.effects();

    let key = format!("authority:{}", authority_id);
    if let Some(bytes) = effects
        .retrieve(&key)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to read authority: {}", e)))?
    {
        let record: AuthorityRecord = serde_json::from_slice(&bytes).map_err(|e| {
            TerminalError::Config(format!("Failed to parse authority record: {}", e))
        })?;
        output.kv("Authority", record.authority_id.to_string());
        output.kv("Threshold", record.threshold.to_string());
        output.kv(
            "Devices",
            if record.devices.is_empty() {
                "(none)".to_string()
            } else {
                record.devices.join(", ")
            },
        );
    } else {
        output.eprintln(format!("Authority {} not found in storage", authority_id));
    }

    Ok(output)
}

async fn list_authorities(ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let effects = ctx.effects();

    let keys = effects
        .list_keys(Some("authority:"))
        .await
        .unwrap_or_default();
    if keys.is_empty() {
        output.println("No authorities stored yet");
    } else {
        output.section(format!("Stored authorities ({})", keys.len()));
        for key in keys {
            output.println(format!("  - {}", key));
        }
    }

    Ok(output)
}

async fn add_device(
    ctx: &HandlerContext<'_>,
    authority_id: &AuthorityId,
    public_key: &str,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let effects = ctx.effects();

    let key = format!("authority:{}", authority_id);
    let mut record: AuthorityRecord = if let Some(bytes) = effects
        .retrieve(&key)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to read authority: {}", e)))?
    {
        serde_json::from_slice(&bytes).map_err(|e| {
            TerminalError::Config(format!("Failed to parse authority record: {}", e))
        })?
    } else {
        return Err(TerminalError::NotFound(format!(
            "Authority {} not found; create it first",
            authority_id
        )));
    };

    record.devices.push(public_key.to_string());

    effects
        .store(
            &key,
            serde_json::to_vec(&record).map_err(|e| {
                TerminalError::Operation(format!("Failed to serialize authority record: {}", e))
            })?,
        )
        .await
        .map_err(|e| {
            TerminalError::Operation(format!(
                "Failed to update authority {}: {}",
                authority_id, e
            ))
        })?;

    output.kv("Added device to authority", authority_id.to_string());
    output.kv("Total devices", record.devices.len().to_string());

    Ok(output)
}
