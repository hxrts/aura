//! AMP workflows (channel state inspection and maintenance).

use aura_core::identifiers::{ChannelId, ContextId};
use aura_core::{hash, AuraError, Hash32};
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::ChannelEpochState;
use aura_protocol::amp::{get_channel_state, AmpJournalEffects};

/// Fetch current AMP channel state.
pub async fn inspect_channel<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
) -> Result<ChannelEpochState, AuraError> {
    get_channel_state(effects, context, channel)
        .await
        .map_err(|e| AuraError::agent(format!("Failed to get channel state: {e}")))
}

/// Propose an AMP channel epoch bump.
pub async fn propose_bump<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
) -> Result<ProposedChannelEpochBump, AuraError> {
    let state = inspect_channel(effects, context, channel).await?;
    if state.pending_bump.is_some() {
        return Err(AuraError::agent("Channel already has a pending bump"));
    }

    let proposal = ProposedChannelEpochBump {
        context,
        channel,
        parent_epoch: state.chan_epoch,
        new_epoch: state.chan_epoch + 1,
        reason: ChannelBumpReason::Routine,
        bump_id: Hash32::new(hash::hash(
            format!("amp-bump:{context}:{channel}").as_bytes(),
        )),
    };

    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal.clone()),
        ))
        .await
        .map_err(|e| AuraError::agent(format!("Failed to insert bump proposal: {e}")))?;

    Ok(proposal)
}

/// Create an AMP channel checkpoint fact.
pub async fn create_checkpoint<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
) -> Result<ChannelCheckpoint, AuraError> {
    let state = inspect_channel(effects, context, channel).await?;
    let checkpoint = ChannelCheckpoint {
        context,
        channel,
        chan_epoch: state.chan_epoch,
        base_gen: state.current_gen,
        window: 32,
        ck_commitment: Hash32::new(hash::hash(
            serde_json::to_vec(&(state.chan_epoch, state.current_gen))
                .unwrap_or_default()
                .as_slice(),
        )),
        skip_window_override: None,
    };

    effects
        .insert_relational_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint.clone()),
        ))
        .await
        .map_err(|e| AuraError::agent(format!("Failed to create checkpoint: {e}")))?;

    Ok(checkpoint)
}
