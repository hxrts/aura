//! AMP CLI handlers with actual functionality.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::amp::AmpAction;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{hash, Hash32};
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ProposedChannelEpochBump, RelationalFact,
};
use aura_protocol::amp::{get_channel_state, AmpJournalEffects};
use std::str::FromStr;

/// Handle AMP commands with effect system integration.
///
/// Returns `CliOutput` instead of printing directly.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_amp(ctx: &HandlerContext<'_>, action: &AmpAction) -> TerminalResult<CliOutput> {
    match action {
        AmpAction::Inspect { context, channel } => handle_amp_inspect(ctx, context, channel).await,
        AmpAction::Bump {
            context,
            channel,
            reason,
        } => handle_amp_bump(ctx, context, channel, reason).await,
        AmpAction::Checkpoint { context, channel } => {
            handle_amp_checkpoint(ctx, context, channel).await
        }
    }
}

/// Handle AMP channel state inspection
async fn handle_amp_inspect(
    ctx: &HandlerContext<'_>,
    context_str: &str,
    channel_str: &str,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let context = ContextId::from_str(context_str)
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {}", e)))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {}", e)))?;

    let state = get_channel_state(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get channel state: {}", e)))?;

    output.section(&format!(
        "Channel State for {}:{}",
        context_str, channel_str
    ));
    output.kv("Current Epoch", state.chan_epoch.to_string());
    output.kv("Current Generation", state.current_gen.to_string());
    output.kv("Last Checkpoint Gen", state.last_checkpoint_gen.to_string());
    output.kv("Skip Window", state.skip_window.to_string());

    if let Some(pending) = &state.pending_bump {
        output.kv(
            "Pending Bump",
            format!("{} -> {}", pending.parent_epoch, pending.new_epoch),
        );
        output.kv("Bump ID", pending.bump_id.to_string());
    } else {
        output.println("No pending bumps");
    }

    Ok(output)
}

/// Handle AMP channel epoch bump proposal
async fn handle_amp_bump(
    ctx: &HandlerContext<'_>,
    context_str: &str,
    channel_str: &str,
    reason: &str,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let context = ContextId::from_str(context_str)
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {}", e)))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {}", e)))?;

    let state = get_channel_state(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get channel state: {}", e)))?;

    if state.pending_bump.is_some() {
        output.eprintln("Error: Channel already has a pending bump");
        return Ok(output);
    }

    let proposal = ProposedChannelEpochBump {
        context,
        channel,
        parent_epoch: state.chan_epoch,
        new_epoch: state.chan_epoch + 1,
        reason: ChannelBumpReason::Routine,
        bump_id: Hash32::new(hash::hash(
            format!("amp-bump:{}:{}", context, channel).as_bytes(),
        )),
    };

    ctx.effects()
        .insert_relational_fact(RelationalFact::AmpProposedChannelEpochBump(
            proposal.clone(),
        ))
        .await?;

    output.println(format!(
        "Proposed epoch bump: {} -> {} (reason: {})",
        proposal.parent_epoch, proposal.new_epoch, reason
    ));
    output.kv("Bump ID", proposal.bump_id.to_string());
    output.println("Note: Consensus finalization is handled automatically by the protocol layer.");

    Ok(output)
}

/// Handle AMP channel checkpoint creation
async fn handle_amp_checkpoint(
    ctx: &HandlerContext<'_>,
    context_str: &str,
    channel_str: &str,
) -> TerminalResult<CliOutput> {
    let mut output = CliOutput::new();

    let context = ContextId::from_str(context_str)
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {}", e)))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {}", e)))?;

    let state = get_channel_state(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to get channel state: {}", e)))?;

    let checkpoint = ChannelCheckpoint {
        context,
        channel,
        chan_epoch: state.chan_epoch,
        base_gen: state.current_gen,
        window: 32, // Standard window size
        ck_commitment: Hash32::new(aura_core::hash::hash(
            serde_json::to_vec(&(state.chan_epoch, state.current_gen))
                .unwrap_or_default()
                .as_slice(),
        )),
        skip_window_override: None,
    };

    ctx.effects()
        .insert_relational_fact(RelationalFact::AmpChannelCheckpoint(checkpoint.clone()))
        .await
        .map_err(|e| TerminalError::Operation(format!("Failed to create checkpoint: {}", e)))?;

    output.section(&format!(
        "Checkpoint created for {}:{}",
        context_str, channel_str
    ));
    output.kv("Epoch", checkpoint.chan_epoch.to_string());
    output.kv("Base Generation", checkpoint.base_gen.to_string());
    output.kv("Window Size", checkpoint.window.to_string());

    Ok(output)
}
