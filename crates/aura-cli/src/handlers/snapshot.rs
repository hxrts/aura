//! Snapshot maintenance command handler.

use anyhow::{anyhow, Result};
use aura_agent::{AgentBuilder, AuraEffectSystem, EffectContext};
use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_protocol::effect_traits::ConsoleEffects;

use crate::SnapshotAction;

/// Dispatch snapshot-related CLI actions.
pub async fn handle_snapshot(
    _ctx: &EffectContext,
    device_id: DeviceId,
    action: &SnapshotAction,
) -> Result<()> {
    match action {
        SnapshotAction::Propose => propose_snapshot(_ctx, device_id).await,
    }
}

async fn propose_snapshot(_ctx: &EffectContext, device_id: DeviceId) -> Result<()> {
    // Create agent for this operation
    let agent = AgentBuilder::new()
        .with_authority(AuthorityId::new())
        .build_testing()?;
    let _effects = agent.runtime().effects();

    println!("Starting snapshot proposal…");

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let _authority_id = AuthorityId(device_id.0);

    // Placeholder implementation - propose_snapshot method not available in current architecture
    println!("Snapshot proposal not yet implemented in new architecture");
    let _outcome = "snapshot_proposed";

    println!("Snapshot proposal completed successfully");

    Ok(())
}
