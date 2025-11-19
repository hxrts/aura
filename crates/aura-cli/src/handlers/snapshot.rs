//! Snapshot maintenance command handler.

use anyhow::{anyhow, Result};
use aura_agent::{runtime::EffectSystemBuilder, AuraAgent};
use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_protocol::effect_traits::ConsoleEffects;

use crate::SnapshotAction;

/// Dispatch snapshot-related CLI actions.
pub async fn handle_snapshot(device_id: DeviceId, action: &SnapshotAction) -> Result<()> {
    match action {
        SnapshotAction::Propose => propose_snapshot(device_id).await,
    }
}

async fn propose_snapshot(device_id: DeviceId) -> Result<()> {
    // Create effect system for this operation
    let effects = EffectSystemBuilder::new()
        .with_device_id(device_id)
        .build_sync()?;

    let _ = effects.log_info("Starting snapshot proposal…").await;

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(device_id.0);

    // Move the effect system into the agent runtime so maintenance wiring is reused.
    let agent = AuraAgent::new(effects, authority_id);
    let outcome = agent
        .propose_snapshot()
        .await
        .map_err(|e| anyhow!("snapshot workflow failed: {e}"))?;

    let digest_hex = hex::encode(outcome.state_digest);
    println!(
        "Snapshot {} committed @ epoch {} (digest {})",
        outcome.proposal_id, outcome.snapshot.epoch, digest_hex
    );

    Ok(())
}
