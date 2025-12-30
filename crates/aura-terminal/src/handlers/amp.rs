//! AMP CLI handlers with actual functionality.
//!
//! Returns structured `CliOutput` for testability.

use crate::cli::amp::AmpAction;
use crate::error::{TerminalError, TerminalResult};
use crate::handlers::{CliOutput, HandlerContext};
use aura_core::identifiers::{ChannelId, ContextId};
use aura_app::ui::workflows::amp;
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
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {e}")))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {e}")))?;

    let state = amp::inspect_channel(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("{e}")))?;

    output.section(format!("Channel State for {context_str}:{channel_str}"));
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
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {e}")))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {e}")))?;

    let proposal = amp::propose_bump(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("{e}")))?;

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
        .map_err(|e| TerminalError::Input(format!("Invalid context id: {e}")))?;
    let channel = ChannelId::from_str(channel_str)
        .map_err(|e| TerminalError::Input(format!("Invalid channel id: {e}")))?;

    let checkpoint = amp::create_checkpoint(ctx.effects(), context, channel)
        .await
        .map_err(|e| TerminalError::Operation(format!("{e}")))?;

    output.section(format!(
        "Checkpoint created for {context_str}:{channel_str}"
    ));
    output.kv("Epoch", checkpoint.chan_epoch.to_string());
    output.kv("Base Generation", checkpoint.base_gen.to_string());
    output.kv("Window Size", checkpoint.window.to_string());

    Ok(output)
}
