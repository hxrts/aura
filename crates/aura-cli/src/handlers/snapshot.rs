//! Snapshot maintenance command handler.

use anyhow::Result;
use aura_agent::{AuraEffectSystem, EffectContext};
use aura_core::effects::JournalEffects;
use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_core::{FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use serde::Serialize;

use crate::SnapshotAction;

/// Dispatch snapshot-related CLI actions.
pub async fn handle_snapshot(
    _ctx: &EffectContext,
    effects: &AuraEffectSystem,
    device_id: DeviceId,
    action: &SnapshotAction,
) -> Result<()> {
    match action {
        SnapshotAction::Propose => propose_snapshot(_ctx, effects, device_id).await,
    }
}

async fn propose_snapshot(
    ctx: &EffectContext,
    effects: &AuraEffectSystem,
    device_id: DeviceId,
) -> Result<()> {
    println!("Starting snapshot proposal…");

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(device_id.0);

    #[derive(Serialize)]
    struct SnapshotProposal {
        proposer: AuthorityId,
        context: aura_core::identifiers::ContextId,
    }

    let proposal = SnapshotProposal {
        proposer: authority_id,
        context: ctx.context_id(),
    };

    let fact_content = FactContent::Relational(RelationalFact::Generic {
        context_id: ctx.context_id(),
        binding_type: "snapshot_proposed".to_string(),
        binding_data: serde_json::to_vec(&proposal)
            .map_err(|e| anyhow::anyhow!("Failed to serialize snapshot proposal: {}", e))?,
    });

    let fact_value = serde_json::to_vec(&fact_content)
        .map(FactValue::Bytes)
        .map_err(|e| anyhow::anyhow!("Failed to encode snapshot proposal fact: {}", e))?;

    let mut delta = Journal::new();
    let fact_key = format!("snapshot_proposed:{}", ctx.context_id());
    delta.facts.insert(fact_key.clone(), fact_value);

    let current = effects
        .get_journal()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load journal: {}", e))?;
    let merged = effects
        .merge_facts(&current, &delta)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to merge snapshot fact: {}", e))?;
    effects
        .persist_journal(&merged)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to persist snapshot fact: {}", e))?;

    println!("Snapshot proposal recorded with key {}", fact_key);

    Ok(())
}
