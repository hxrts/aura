//! Simulator implementation of AmpChannelEffects.
//!
//! Uses the same AMP facts/reduction path via AmpJournalEffects, but relies on
//! simulator-controlled time/random and applies a deterministic XOR mask over
//! plaintext using channel header + sender to avoid plaintext transport while
//! keeping the implementation side-effect free for simulation.

use async_trait::async_trait;
use aura_core::effects::amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::RandomCoreEffects;
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId};
use aura_journal::fact::{
    ChannelCheckpoint, ChannelPolicy, CommittedChannelEpochBump, RelationalFact,
};
use aura_amp::{get_channel_state, AmpJournalEffects};
use serde::{Deserialize, Serialize};

/// Channel membership status for tracking participants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MembershipStatus {
    /// Participant has joined the channel
    Joined,
    /// Participant has left the channel
    Left,
}

/// Data structure for channel membership facts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMembershipData {
    /// The channel this membership relates to
    pub channel: ChannelId,
    /// The participant authority
    pub participant: AuthorityId,
    /// Current membership status
    pub status: MembershipStatus,
}

const DEFAULT_WINDOW: u32 = 1024;

pub struct SimAmpChannels<E> {
    effects: E,
}

impl<E> SimAmpChannels<E> {
    pub fn new(effects: E) -> Self {
        Self { effects }
    }
}

#[async_trait]
impl<E> AmpChannelEffects for SimAmpChannels<E>
where
    E: AmpJournalEffects + RandomCoreEffects + Send + Sync,
{
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError> {
        let channel = if let Some(id) = params.channel {
            id
        } else {
            let bytes = self.effects.random_bytes(32).await;
            ChannelId::from_bytes(hash(&bytes))
        };

        let window = params.skip_window.unwrap_or(DEFAULT_WINDOW);
        let checkpoint = ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Default::default(),
            skip_window_override: Some(window),
        };

        self.effects
            .insert_relational_fact(RelationalFact::AmpChannelCheckpoint(checkpoint))
            .await
            .map_err(map_err)?;

        if params.topic.is_some() || params.skip_window.is_some() {
            let policy = ChannelPolicy {
                context: params.context,
                channel,
                skip_window: params.skip_window.or(Some(window)),
            };
            self.effects
                .insert_relational_fact(RelationalFact::AmpChannelPolicy(policy))
                .await
                .map_err(map_err)?;
        }

        Ok(channel)
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let committed = CommittedChannelEpochBump {
            context: params.context,
            channel: params.channel,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            chosen_bump_id: Default::default(),
            consensus_id: Default::default(),
        };

        self.effects
            .insert_relational_fact(RelationalFact::AmpCommittedChannelEpochBump(committed))
            .await
            .map_err(map_err)?;

        let policy = ChannelPolicy {
            context: params.context,
            channel: params.channel,
            skip_window: Some(0),
        };

        self.effects
            .insert_relational_fact(RelationalFact::AmpChannelPolicy(policy))
            .await
            .map_err(map_err)?;

        Ok(())
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        // Record channel membership fact using Generic relational fact
        let membership_data = ChannelMembershipData {
            channel: params.channel,
            participant: params.participant,
            status: MembershipStatus::Joined,
        };

        let binding_data = serde_json::to_vec(&membership_data)
            .map_err(|e| AmpChannelError::Internal(format!("Serialization error: {}", e)))?;

        self.effects
            .insert_relational_fact(RelationalFact::Generic {
                context_id: params.context,
                binding_type: "channel_membership".to_string(),
                binding_data,
            })
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "[sim] Participant {:?} joined channel {:?}",
            params.participant,
            params.channel
        );

        Ok(())
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        // Record channel membership revocation fact using Generic relational fact
        let membership_data = ChannelMembershipData {
            channel: params.channel,
            participant: params.participant,
            status: MembershipStatus::Left,
        };

        let binding_data = serde_json::to_vec(&membership_data)
            .map_err(|e| AmpChannelError::Internal(format!("Serialization error: {}", e)))?;

        self.effects
            .insert_relational_fact(RelationalFact::Generic {
                context_id: params.context,
                binding_type: "channel_membership".to_string(),
                binding_data,
            })
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "[sim] Participant {:?} left channel {:?}",
            params.participant,
            params.channel
        );

        Ok(())
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> Result<AmpCiphertext, AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: state.chan_epoch,
            ratchet_gen: state.current_gen,
        };

        // Compute ciphertext before moving header into AmpCiphertext
        let ciphertext = mask_ciphertext(&header, &params.sender, &params.plaintext);

        Ok(AmpCiphertext { header, ciphertext })
    }
}

fn map_err(e: aura_core::AuraError) -> AmpChannelError {
    match e {
        aura_core::AuraError::NotFound { .. } => AmpChannelError::NotFound,
        aura_core::AuraError::PermissionDenied { .. } => AmpChannelError::Unauthorized,
        aura_core::AuraError::Storage { message } => AmpChannelError::Storage(message),
        aura_core::AuraError::Crypto { message } => AmpChannelError::Crypto(message),
        aura_core::AuraError::Invalid { message } => AmpChannelError::InvalidState(message),
        aura_core::AuraError::Internal { message } => AmpChannelError::Internal(message),
        other => AmpChannelError::Internal(other.to_string()),
    }
}

/// Derive a deterministic keystream from header + sender and XOR-mask the payload.
fn mask_ciphertext(header: &AmpHeader, sender: &AuthorityId, plaintext: &[u8]) -> Vec<u8> {
    let mut key_material = Vec::new();
    key_material.extend_from_slice(header.channel.as_bytes());
    key_material.extend_from_slice(&header.chan_epoch.to_le_bytes());
    key_material.extend_from_slice(&header.ratchet_gen.to_le_bytes());
    key_material.extend_from_slice(sender.0.as_bytes());

    let mut keystream = Vec::with_capacity(plaintext.len());
    let mut counter: u64 = 0;
    while keystream.len() < plaintext.len() {
        let mut block_input = key_material.clone();
        block_input.extend_from_slice(&counter.to_le_bytes());
        let block = hash(&block_input);
        keystream.extend_from_slice(&block);
        counter += 1;
    }

    plaintext
        .iter()
        .zip(keystream)
        .map(|(p, k)| p ^ k)
        .collect()
}
