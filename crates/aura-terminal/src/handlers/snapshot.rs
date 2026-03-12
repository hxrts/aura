//! Snapshot maintenance command handler.
//! Returns structured `CliOutput` for testability.

use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use crate::SnapshotAction;
use aura_app::ui::workflows::snapshot;

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

    let authority_id = ctx.authority_id();

    // Use the aura-app workflow for snapshot proposal
    let fact_key = snapshot::propose_snapshot(ctx.effects(), authority_id)
        .await
        .map_err(|e| TerminalError::Operation(format!("{e}")))?;

    output.kv("Snapshot proposal recorded with key", fact_key);

    Ok(output)
}
