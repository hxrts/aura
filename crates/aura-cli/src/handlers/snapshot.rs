//! Snapshot maintenance command handler.

use anyhow::{anyhow, Result};
use aura_agent::AuraAgent;
use aura_protocol::effects::{AuraEffectSystem, ConsoleEffects};

use crate::SnapshotAction;

/// Dispatch snapshot-related CLI actions.
pub async fn handle_snapshot(effects: AuraEffectSystem, action: &SnapshotAction) -> Result<()> {
    match action {
        SnapshotAction::Propose => propose_snapshot(effects).await,
    }
}

async fn propose_snapshot(effects: AuraEffectSystem) -> Result<()> {
    let device_id = effects.device_id();
    effects.log_info("Starting snapshot proposal…", &[]);
    // Move the effect system into the agent runtime so maintenance wiring is reused.
    let agent = AuraAgent::new(effects, device_id);
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
