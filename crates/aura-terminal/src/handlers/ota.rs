#![allow(clippy::disallowed_methods)]
//! OTA Upgrade CLI Commands
//!
//! Commands for managing over-the-air upgrades using the proper effect system architecture.

use crate::handlers::HandlerContext;
use anyhow::{Context, Result};
use aura_core::effects::{ConsoleEffects, StorageEffects};
use aura_core::{hash, AccountId, Hash32, SemanticVersion};
use aura_sync::maintenance::{IdentityEpochFence, UpgradeProposal};
use aura_sync::protocols::ota::UpgradeKind;
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::{ids, OtaAction};

/// Handle OTA commands through effects
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_ota(ctx: &HandlerContext<'_>, action: &OtaAction) -> Result<()> {
    match action {
        OtaAction::Propose {
            from_version,
            to_version,
            upgrade_type,
            download_url,
            description,
        } => {
            propose_upgrade(
                ctx,
                from_version,
                to_version,
                upgrade_type,
                download_url,
                description,
            )
            .await
        }
        OtaAction::Policy { policy } => set_policy(ctx, policy).await,
        OtaAction::Status => get_status(ctx).await,
        OtaAction::OptIn { proposal_id } => opt_in(ctx, proposal_id).await,
        OtaAction::List => list_proposals(ctx).await,
        OtaAction::Stats => get_stats(ctx).await,
    }
}

async fn propose_upgrade(
    ctx: &HandlerContext<'_>,
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
        package_id: ids::uuid(&format!(
            "ota:{}:{}:{}:{}",
            major, minor, patch, download_url
        )),
        version,
        artifact_hash,
        artifact_uri: Some(download_url.to_string()),
        kind,
        activation_fence: match kind {
            UpgradeKind::HardFork => Some(IdentityEpochFence::new(
                AccountId::from_uuid(ctx.effect_context().authority_id().uuid()),
                0,
            )),
            _ => None,
        },
    };

    proposal.validate().context("Invalid upgrade proposal")?;

    let key = format!("ota:proposal:{}", proposal.package_id);
    ctx.effects()
        .store(&key, serde_json::to_vec(&proposal)?)
        .await
        .map_err(anyhow::Error::from)?;

    ConsoleEffects::log_info(
        ctx.effects(),
        &format!(
            "Created upgrade proposal {} (version {}, kind {:?})",
            proposal.package_id, proposal.version, proposal.kind
        ),
    )
    .await?;

    Ok(())
}

async fn set_policy(ctx: &HandlerContext<'_>, policy: &str) -> Result<()> {
    let key = "ota:policy";
    ctx.effects()
        .store(key, policy.as_bytes().to_vec())
        .await
        .map_err(anyhow::Error::from)?;
    ConsoleEffects::log_info(ctx.effects(), &format!("OTA policy set to: {}", policy)).await?;
    Ok(())
}

async fn get_status(ctx: &HandlerContext<'_>) -> Result<()> {
    let proposals = list_saved_proposals(ctx).await?;
    if proposals.is_empty() {
        ConsoleEffects::log_info(ctx.effects(), "OTA Status: No active upgrades").await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        ctx.effects(),
        &format!("OTA Status: {} proposal(s) tracked", proposals.len()),
    )
    .await?;

    for proposal in proposals {
        let opt_in_key = format!("ota:optin:{}", proposal.package_id);
        let opted_in = ctx
            .effects()
            .retrieve(&opt_in_key)
            .await
            .map(|v| v.is_some())
            .unwrap_or(false);
        ConsoleEffects::log_info(
            ctx.effects(),
            &format!(
                "  • {} ({}, kind {:?}) opted_in={}",
                proposal.package_id, proposal.version, proposal.kind, opted_in
            ),
        )
        .await?;
    }
    Ok(())
}

async fn opt_in(ctx: &HandlerContext<'_>, proposal_id: &str) -> Result<()> {
    let proposal_uuid = Uuid::parse_str(proposal_id).context("proposal_id must be a UUID")?;

    let key = format!("ota:optin:{}", proposal_uuid);
    ctx.effects()
        .store(&key, b"opted-in".to_vec())
        .await
        .map_err(anyhow::Error::from)?;

    ConsoleEffects::log_info(
        ctx.effects(),
        &format!("Opted into proposal: {}", proposal_uuid),
    )
    .await?;
    Ok(())
}

async fn list_proposals(ctx: &HandlerContext<'_>) -> Result<()> {
    let proposals = list_saved_proposals(ctx).await?;
    if proposals.is_empty() {
        ConsoleEffects::log_info(ctx.effects(), "No upgrade proposals found").await?;
        return Ok(());
    }

    ConsoleEffects::log_info(
        ctx.effects(),
        &format!("Listing {} proposal(s):", proposals.len()),
    )
    .await?;

    for proposal in proposals {
        ConsoleEffects::log_info(
            ctx.effects(),
            &format!(
                "  • {} version {} kind {:?}",
                proposal.package_id, proposal.version, proposal.kind
            ),
        )
        .await?;
    }
    Ok(())
}

async fn get_stats(ctx: &HandlerContext<'_>) -> Result<()> {
    let proposals = list_saved_proposals(ctx).await?;
    let opt_ins = ctx
        .effects()
        .list_keys(Some("ota:optin:"))
        .await
        .map(|list| list.len())
        .unwrap_or(0);

    ConsoleEffects::log_info(ctx.effects(), "OTA Statistics:").await?;
    ConsoleEffects::log_info(
        ctx.effects(),
        &format!("  Total proposals: {}", proposals.len()),
    )
    .await?;
    ConsoleEffects::log_info(ctx.effects(), &format!("  Opt-ins: {}", opt_ins)).await?;
    Ok(())
}

fn compute_artifact_hash(download_url: &str) -> Result<Hash32> {
    let mut hasher = hash::hasher();
    let path = Path::new(download_url);
    if path.exists() {
        let data = fs::read(path)?;
        hasher.update(&data);
    } else {
        hasher.update(download_url.as_bytes());
    }
    let digest = hasher.finalize();
    Ok(Hash32::new(digest))
}

async fn list_saved_proposals(ctx: &HandlerContext<'_>) -> Result<Vec<UpgradeProposal>> {
    let mut proposals = Vec::new();
    let keys = ctx
        .effects()
        .list_keys(Some("ota:proposal:"))
        .await
        .unwrap_or_default();
    for key in keys {
        if let Ok(Some(raw)) = ctx.effects().retrieve(&key).await {
            if let Ok(proposal) = serde_json::from_slice::<UpgradeProposal>(&raw) {
                proposals.push(proposal);
            }
        }
    }
    Ok(proposals)
}
