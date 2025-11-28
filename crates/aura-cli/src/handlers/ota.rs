#![allow(clippy::disallowed_methods)]
//! OTA Upgrade CLI Commands
//!
//! Commands for managing over-the-air upgrades using the proper effect system architecture.


use anyhow::{Context, Result};
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::effects::{ConsoleEffects, StorageEffects};
use aura_core::{AccountId, Hash32, SemanticVersion};
use aura_sync::maintenance::{IdentityEpochFence, UpgradeProposal};
use aura_sync::protocols::ota::UpgradeKind;
use blake3::Hasher;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::OtaAction;

/// Handle OTA commands through effects
pub async fn handle_ota(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    action: &OtaAction,
) -> Result<()> {
    match action {
        OtaAction::Propose {
            from_version,
            to_version,
            upgrade_type,
            download_url,
            description,
        } => {
            propose_upgrade(
                _ctx,
                effects,
                from_version,
                to_version,
                upgrade_type,
                download_url,
                description,
            )
            .await
        }
        OtaAction::Policy { policy } => set_policy(_ctx, effects, policy).await,
        OtaAction::Status => get_status(_ctx, effects).await,
        OtaAction::OptIn { proposal_id } => opt_in(_ctx, effects, proposal_id).await,
        OtaAction::List => list_proposals(_ctx, effects).await,
        OtaAction::Stats => get_stats(_ctx, effects).await,
    }
}

async fn propose_upgrade(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    _from_version: &str,
    to_version: &str,
    upgrade_type: &str,
    download_url: &str,
    description: &str,
) -> Result<()> {
    println!(
        "Proposing {} upgrade to version {}: {}",
        upgrade_type, to_version, description
    );

    let kind = match upgrade_type {
        "soft" => UpgradeKind::SoftFork,
        "hard" => UpgradeKind::HardFork,
        _ => {
            return Err(anyhow::anyhow!(
                "Invalid upgrade type: {}. Use 'soft' or 'hard'",
                upgrade_type
            ))
        }
    };

    // Parse version string (e.g., "1.2.3")
    let parts: Vec<&str> = to_version.split('.').collect();
    if parts.len() != 3 {
        return Err(anyhow::anyhow!(
            "Invalid semantic version format. Expected: major.minor.patch"
        ));
    }
    let major: u16 = parts[0].parse().context("Invalid major version")?;
    let minor: u16 = parts[1].parse().context("Invalid minor version")?;
    let patch: u16 = parts[2].parse().context("Invalid patch version")?;
    let version = SemanticVersion::new(major, minor, patch);

    // Compute artifact hash from local file if available, otherwise hash the URL string
    let artifact_hash = compute_artifact_hash(download_url)?;

    let proposal = UpgradeProposal {
        package_id: Uuid::new_v4(),
        version,
        artifact_hash,
        artifact_uri: Some(download_url.to_string()),
        kind,
        activation_fence: match kind {
            UpgradeKind::HardFork => Some(IdentityEpochFence::new(
                AccountId::from_uuid(_ctx.authority_id().uuid()),
                0,
            )),
            _ => None,
        },
    };

    proposal.validate().context("Invalid upgrade proposal")?;

    let key = format!("ota:proposal:{}", proposal.package_id);
    effects
        .store(&key, serde_json::to_vec(&proposal)?)
        .await
        .map_err(anyhow::Error::from)?;

    ConsoleEffects::log_info(
        effects,
        &format!(
            "Created upgrade proposal {} (version {}, kind {:?})",
            proposal.package_id, proposal.version, proposal.kind
        ),
    )
    .await?;

    Ok(())
}

async fn set_policy(_ctx: &EffectContext, effects: &AuraEffectSystem, policy: &str) -> Result<()> {
    let key = "ota:policy";
    effects
        .store(key, policy.as_bytes().to_vec())
        .await
        .map_err(anyhow::Error::from)?;
    ConsoleEffects::log_info(effects, &format!("OTA policy set to: {}", policy)).await?;
    Ok(())
}

async fn get_status(_ctx: &EffectContext, effects: &AuraEffectSystem) -> Result<()> {
    let proposals = list_saved_proposals(effects).await?;
    if proposals.is_empty() {
        ConsoleEffects::log_info(effects, "OTA Status: No active upgrades").await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        effects,
        &format!("OTA Status: {} proposal(s) tracked", proposals.len()),
    )
    .await?;

    for proposal in proposals {
        let opt_in_key = format!("ota:optin:{}", proposal.package_id);
        let opted_in = effects
            .retrieve(&opt_in_key)
            .await
            .map(|v| v.is_some())
            .unwrap_or(false);
        ConsoleEffects::log_info(
            effects,
            &format!(
                "  • {} ({}, kind {:?}) opted_in={}",
                proposal.package_id, proposal.version, proposal.kind, opted_in
            ),
        )
        .await?;
    }
    Ok(())
}

async fn opt_in(_ctx: &EffectContext, effects: &AuraEffectSystem, proposal_id: &str) -> Result<()> {
    let proposal_uuid = Uuid::parse_str(proposal_id).context("proposal_id must be a UUID")?;

    let key = format!("ota:optin:{}", proposal_uuid);
    effects
        .store(&key, b"opted-in".to_vec())
        .await
        .map_err(anyhow::Error::from)?;

    ConsoleEffects::log_info(effects, &format!("Opted into proposal: {}", proposal_uuid)).await?;
    Ok(())
}

async fn list_proposals(_ctx: &EffectContext, effects: &AuraEffectSystem) -> Result<()> {
    let proposals = list_saved_proposals(effects).await?;
    if proposals.is_empty() {
        ConsoleEffects::log_info(effects, "No upgrade proposals found").await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        effects,
        &format!("Listing {} proposal(s):", proposals.len()),
    )
    .await?;

    for proposal in proposals {
        ConsoleEffects::log_info(
            effects,
            &format!(
                "  • {} version {} kind {:?}",
                proposal.package_id, proposal.version, proposal.kind
            ),
        )
        .await?;
    }
    Ok(())
}

async fn get_stats(_ctx: &EffectContext, effects: &AuraEffectSystem) -> Result<()> {
    let proposals = list_saved_proposals(effects).await?;
    let opt_ins = effects
        .list_keys(Some("ota:optin:"))
        .await
        .map(|list| list.len())
        .unwrap_or(0);

    ConsoleEffects::log_info(effects, "OTA Statistics:").await?;
    ConsoleEffects::log_info(effects, &format!("  Total proposals: {}", proposals.len())).await?;
    ConsoleEffects::log_info(effects, &format!("  Opt-ins: {}", opt_ins)).await?;
    Ok(())
}

fn compute_artifact_hash(download_url: &str) -> Result<Hash32> {
    let mut hasher = Hasher::new();
    let path = Path::new(download_url);
    if path.exists() {
        let data = fs::read(path)?;
        hasher.update(&data);
    } else {
        hasher.update(download_url.as_bytes());
    }
    let digest = hasher.finalize();
    Ok(Hash32(*digest.as_bytes()))
}

async fn list_saved_proposals(effects: &AuraEffectSystem) -> Result<Vec<UpgradeProposal>> {
    let mut proposals = Vec::new();
    let keys = effects
        .list_keys(Some("ota:proposal:"))
        .await
        .unwrap_or_default();
    for key in keys {
        if let Ok(Some(raw)) = effects.retrieve(&key).await {
            if let Ok(proposal) = serde_json::from_slice::<UpgradeProposal>(&raw) {
                proposals.push(proposal);
            }
        }
    }
    Ok(proposals)
}
