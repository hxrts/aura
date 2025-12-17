//! Snapshot maintenance command handler.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::effects::JournalEffects;
use aura_core::identifiers::AuthorityId;
use aura_core::{FactValue, Journal};
use aura_journal::fact::{FactContent, RelationalFact};
use serde::Serialize;

use crate::SnapshotAction;

/// Dispatch snapshot-related CLI actions.
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_snapshot(
    ctx: &HandlerContext<'_>,
    action: &SnapshotAction,
) -> TerminalResult<CliOutput> {
    match action {
        SnapshotAction::Propose => propose_snapshot(ctx).await,
    }
}

async fn propose_snapshot(ctx: &HandlerContext<'_>) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    output.println("Starting snapshot proposal...");

    // Convert DeviceId to AuthorityId (1:1 mapping for single-device authorities)
    let authority_id = AuthorityId(ctx.device_id().0);

    #[derive(Serialize)]
    struct SnapshotProposal {
        proposer: AuthorityId,
        context: aura_core::identifiers::ContextId,
    }

    let proposal = SnapshotProposal {
        proposer: authority_id,
        context: ctx.effect_context().context_id(),
    };

    let fact_content = FactContent::Relational(RelationalFact::Generic {
        context_id: ctx.effect_context().context_id(),
        binding_type: "snapshot_proposed".to_string(),
        binding_data: serde_json::to_vec(&proposal)
            .map_err(|e| TerminalError::Operation(format!("Failed to serialize snapshot proposal: {}", e)))?,
    });

    let fact_value = serde_json::to_vec(&fact_content)
        .map(FactValue::Bytes)
        .map_err(|e| TerminalError::Operation(format!("Failed to encode snapshot proposal fact: {}", e)))?;

    let mut delta = Journal::new();
    let fact_key = format!("snapshot_proposed:{}", ctx.effect_context().context_id());
    delta.facts.insert(fact_key.clone(), fact_value);

    let current = ctx
        .effects()
        .get_journal()
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to load journal: {}", e)))?;
    let merged = ctx
        .effects()
        .merge_facts(&current, &delta)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to merge snapshot fact: {}", e)))?;
    ctx.effects()
        .persist_journal(&merged)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to persist snapshot fact: {}", e)))?;

    output.kv("Snapshot proposal recorded with key", fact_key);

    Ok(output)
}
