//! AMP CLI handlers with actual functionality.
use crate::cli::amp::AmpAction;
use crate::handlers::HandlerContext;
use anyhow::Result;
use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{hash, Hash32};
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ProposedChannelEpochBump, RelationalFact,
};
use aura_protocol::amp::{get_channel_state, AmpJournalEffects};
use std::str::FromStr;

/// Handle AMP commands with effect system integration.
///
/// **Standardized Signature (Task 2.2)**: Uses `HandlerContext` for unified parameter passing.
pub async fn handle_amp(ctx: &HandlerContext<'_>, action: &AmpAction) -> Result<()> {
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
) -> Result<()> {
    let context = ContextId::from_str(context_str)?;
    let channel = ChannelId::from_str(channel_str)?;

    let state = get_channel_state(ctx.effects(), context, channel).await?;

    println!("Channel State for {}:{}", context_str, channel_str);
    println!("  Current Epoch: {}", state.chan_epoch);
    println!("  Current Generation: {}", state.current_gen);
    println!("  Last Checkpoint Gen: {}", state.last_checkpoint_gen);
    println!("  Skip Window: {}", state.skip_window);

    if let Some(pending) = &state.pending_bump {
        println!(
            "  Pending Bump: {} -> {}",
            pending.parent_epoch, pending.new_epoch
        );
        println!("  Bump ID: {}", pending.bump_id);
    } else {
        println!("  No pending bumps");
    }

    Ok(())
}

/// Handle AMP channel epoch bump proposal
async fn handle_amp_bump(
    ctx: &HandlerContext<'_>,
    context_str: &str,
    channel_str: &str,
    reason: &str,
) -> Result<()> {
    let context = ContextId::from_str(context_str)?;
    let channel = ChannelId::from_str(channel_str)?;

    let state = get_channel_state(ctx.effects(), context, channel).await?;

    if state.pending_bump.is_some() {
        println!("Error: Channel already has a pending bump");
        return Ok(());
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

    println!(
        "Proposed epoch bump: {} -> {} (reason: {})",
        proposal.parent_epoch, proposal.new_epoch, reason
    );
    println!("Bump ID: {}", proposal.bump_id);
    println!("Note: Consensus finalization is handled automatically by the protocol layer.");

    Ok(())
}

/// Handle AMP channel checkpoint creation
async fn handle_amp_checkpoint(
    ctx: &HandlerContext<'_>,
    context_str: &str,
    channel_str: &str,
) -> Result<()> {
    let context = ContextId::from_str(context_str)?;
    let channel = ChannelId::from_str(channel_str)?;

    let state = get_channel_state(ctx.effects(), context, channel).await?;

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
        .await?;

    println!("Checkpoint created for {}:{}", context_str, channel_str);
    println!("  Epoch: {}", checkpoint.chan_epoch);
    println!("  Base Generation: {}", checkpoint.base_gen);
    println!("  Window Size: {}", checkpoint.window);

    Ok(())
}
