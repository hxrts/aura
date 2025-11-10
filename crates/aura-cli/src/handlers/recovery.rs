//! Recovery CLI handlers.

use crate::RecoveryAction;
use anyhow::{anyhow, Context, Result};
use aura_agent::handlers::RecoveryOperations;
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{AccountId, DeviceId};
use aura_protocol::effects::{AuraEffectSystem, ConsoleEffects, TimeEffects};
use aura_recovery::guardian_recovery::{
    guardian_from_device, GuardianRecoveryCoordinator, GuardianRecoveryRequest, RecoveryPriority, DEFAULT_DISPUTE_WINDOW_SECS,
};
use aura_recovery::{GuardianSet};
use std::{fs, str::FromStr, sync::Arc};
use tokio::sync::RwLock;

const DEFAULT_JUSTIFICATION: &str = "CLI initiated guardian recovery";

/// Top-level dispatcher for recovery subcommands.
pub async fn handle_recovery(effects: &AuraEffectSystem, action: &RecoveryAction) -> Result<()> {
    match action {
        RecoveryAction::Start {
            account,
            guardians,
            threshold,
            priority,
            dispute_hours,
            justification,
        } => {
            let request = build_request(
                effects,
                account,
                guardians,
                *threshold as usize,
                priority,
                *dispute_hours,
                justification.as_deref(),
            )
            .await?;

            let coordinator = GuardianRecoveryCoordinator::new(effects.clone());
            let response = coordinator
                .execute_recovery(request)
                .await
                .context("guardian recovery failed")?;

            effects.log_info(
                &format!(
                    "Recovery completed with {} approvals (evidence recorded).",
                    response.guardian_approvals.len()
                ),
                &[],
            );
            Ok(())
        }
        RecoveryAction::Approve { request_file } => {
            let serialized = fs::read_to_string(request_file)
                .with_context(|| format!("unable to read request {:?}", request_file))?;
            let request: GuardianRecoveryRequest =
                serde_json::from_str(&serialized).context("invalid guardian recovery request")?;
            let operations = recovery_ops(effects);
            let share = operations
                .approve_guardian_recovery(request.clone())
                .await
                .map_err(|err| anyhow!("guardian approval failed: {}", err))?;

            let cooldown_until = share.issued_at + share.guardian.cooldown_secs;
            effects.log_info(
                &format!(
                    "Guardian {} approved recovery for account {} (cooldown until {}).",
                    share.guardian.guardian_id, request.account_id, cooldown_until
                ),
                &[],
            );
            Ok(())
        }
        RecoveryAction::Status => {
            let operations = recovery_ops(effects);
            let status = operations
                .recovery_status()
                .await
                .map_err(|err| anyhow!("failed to read recovery status: {}", err))?;

            let cooldown_summary = match (
                status.cooldown_remaining,
                status.cooldown_expires_at,
                status
                    .latest_evidence
                    .as_ref()
                    .map(|evidence| evidence.guardians.len()),
            ) {
                (Some(remaining), Some(expires_at), Some(guardian_count)) => format!(
                    "Cooldown active for {}s (until {}) with {} guardians recorded.",
                    remaining, expires_at, guardian_count
                ),
                (None, Some(expires_at), Some(guardian_count)) => format!(
                    "Cooldown expired at {} ({} guardians participated).",
                    expires_at, guardian_count
                ),
                _ => "No guardian recovery evidence recorded yet.".to_string(),
            };

            let dispute_summary = match (status.dispute_window_ends_at, status.disputed) {
                (Some(deadline), false) => format!("Dispute window open until {}.", deadline),
                (Some(deadline), true) => {
                    format!("Dispute filed; activation paused until {}.", deadline)
                }
                (None, _) => "No dispute window information available.".to_string(),
            };

            effects.log_info(
                &format!(
                    "Pending sessions: {}. {} {}",
                    status.pending_sessions, cooldown_summary, dispute_summary
                ),
                &[],
            );
            Ok(())
        }
        RecoveryAction::Dispute { evidence, reason } => {
            let operations = recovery_ops(effects);
            operations
                .dispute_guardian_recovery(evidence, reason)
                .await
                .map_err(|err| anyhow!("failed to dispute recovery: {}", err))?;

            effects.log_info(
                &format!(
                    "Filed dispute for recovery evidence {} (reason: {}).",
                    evidence, reason
                ),
                &[],
            );
            Ok(())
        }
    }
}

async fn build_request(
    effects: &AuraEffectSystem,
    account: &str,
    guardians: &str,
    threshold: usize,
    priority: &str,
    dispute_hours: u64,
    justification: Option<&str>,
) -> Result<GuardianRecoveryRequest> {
    let account_id = AccountId::from_str(account)
        .map_err(|err| anyhow!("invalid account id '{}': {}", account, err))?;
    let guardian_devices = parse_guardians(guardians)?;
    let guardian_profiles = guardian_devices
        .into_iter()
        .map(|device| guardian_from_device(device, "cli-guardian"))
        .collect();
    let guardian_set = GuardianSet::new(guardian_profiles);
    if guardian_set.is_empty() {
        return Err(anyhow!("at least one guardian device must be provided"));
    }

    let recovery_priority = parse_priority(priority)?;
    let timestamp = effects.current_timestamp().await;
    let recovery_context = RecoveryContext {
        operation_type: RecoveryOperationType::DeviceKeyRecovery,
        justification: justification.unwrap_or(DEFAULT_JUSTIFICATION).to_string(),
        is_emergency: matches!(recovery_priority, RecoveryPriority::Emergency),
        timestamp,
    };

    Ok(GuardianRecoveryRequest {
        requesting_device: effects.device_id(),
        account_id,
        recovery_context,
        required_threshold: threshold,
        available_guardians: guardian_set,
        priority: recovery_priority,
        dispute_window_secs: if dispute_hours == 0 {
            DEFAULT_DISPUTE_WINDOW_SECS
        } else {
            dispute_hours.saturating_mul(3600)
        },
    })
}

fn recovery_ops(effects: &AuraEffectSystem) -> RecoveryOperations {
    RecoveryOperations::new(Arc::new(RwLock::new(effects.clone())), effects.device_id())
}

fn parse_guardians(value: &str) -> Result<Vec<DeviceId>> {
    value
        .split(',')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            DeviceId::from_str(entry.trim())
                .map_err(|err| anyhow!("invalid guardian device id '{}': {}", entry, err))
        })
        .collect()
}

fn parse_priority(value: &str) -> Result<RecoveryPriority> {
    match value.to_ascii_lowercase().as_str() {
        "normal" => Ok(RecoveryPriority::Normal),
        "urgent" => Ok(RecoveryPriority::Urgent),
        "emergency" => Ok(RecoveryPriority::Emergency),
        other => Err(anyhow!(
            "unknown recovery priority '{}'. Expected normal|urgent|emergency.",
            other
        )),
    }
}
