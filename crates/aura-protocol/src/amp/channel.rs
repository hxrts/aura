//! AMP channel lifecycle coordinator (Layer 4)
//!
//! Provides an implementation of `aura_core::effects::AmpChannelEffects` that
//! persists AMP channel facts to the context journal via `AmpJournalEffects`.
//! Encryption uses a deterministic stream mask derived from channel header +
//! sender to keep Layer 4 transport non-plaintext without introducing a
//! ratchet/AEAD dependency here. This is sufficient for simulation/demo;
//! production AEAD can replace the mask while preserving the interface.

use aura_core::effects::amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::hash::hash;
use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::time::{OrderTime, TimeStamp};
use aura_journal::extensibility::DomainFact;
use aura_journal::fact::{
    ChannelCheckpoint, ChannelPolicy, CommittedChannelEpochBump, RelationalFact,
};
use serde::{Deserialize, Serialize};

use super::{get_channel_state, AmpJournalEffects};

const DEFAULT_WINDOW: u32 = 1024;

/// Simple coordinator that writes AMP channel facts into the context journal.
pub struct AmpChannelCoordinator<E> {
    effects: E,
}

impl<E> AmpChannelCoordinator<E> {
    pub fn new(effects: E) -> Self {
        Self { effects }
    }
}

#[async_trait::async_trait]
impl<E> AmpChannelEffects for AmpChannelCoordinator<E>
where
    E: AmpJournalEffects + Send + Sync,
{
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> std::result::Result<ChannelId, AmpChannelError> {
        let channel = if let Some(id) = params.channel {
            id
        } else {
            // Derive a ChannelId from random bytes
            let bytes = self.effects.random_bytes(32).await;
            aura_core::identifiers::ChannelId::from_bytes(hash(&bytes))
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

    async fn close_channel(
        &self,
        params: ChannelCloseParams,
    ) -> std::result::Result<(), AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        // Bump epoch to mark closure and set a restrictive policy
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

    async fn join_channel(
        &self,
        params: ChannelJoinParams,
    ) -> std::result::Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let timestamp = ChannelMembershipFact::random_timestamp(&self.effects).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            timestamp,
        );
        self.effects
            .insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "Participant {:?} joined channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn leave_channel(
        &self,
        params: ChannelLeaveParams,
    ) -> std::result::Result<(), AmpChannelError> {
        // Verify the channel exists by getting its state
        let _state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let timestamp = ChannelMembershipFact::random_timestamp(&self.effects).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            timestamp,
        );
        self.effects
            .insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_err)?;

        tracing::debug!(
            "Participant {:?} left channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn send_message(
        &self,
        params: ChannelSendParams,
    ) -> std::result::Result<AmpCiphertext, AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: state.chan_epoch,
            ratchet_gen: state.current_gen,
        };

        let ciphertext = mask_ciphertext(&header, &params.sender, &params.plaintext);

        Ok(AmpCiphertext { header, ciphertext })
    }
}

/// Event types for channel membership facts.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChannelParticipantEvent {
    /// Participant joined the channel.
    Joined,
    /// Participant left the channel.
    Left,
}

/// Domain fact that records AMP channel membership events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMembershipFact {
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
    event: ChannelParticipantEvent,
    timestamp: TimeStamp,
}

impl ChannelMembershipFact {
    pub fn new(
        context: ContextId,
        channel: ChannelId,
        participant: AuthorityId,
        event: ChannelParticipantEvent,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            context,
            channel,
            participant,
            event,
            timestamp,
        }
    }

    pub async fn random_timestamp<A: AmpJournalEffects>(effects: &A) -> TimeStamp {
        let bytes = effects.random_bytes(16).await;
        TimeStamp::OrderClock(OrderTime(hash(&bytes)))
    }
}

impl DomainFact for ChannelMembershipFact {
    fn type_id(&self) -> &'static str {
        "amp-channel-membership"
    }

    fn context_id(&self) -> ContextId {
        self.context
    }

    fn to_bytes(&self) -> Vec<u8> {
        match serde_json::to_vec(self) {
            Ok(bytes) => bytes,
            Err(err) => {
                tracing::error!("channel membership serialization failed: {err}");
                Vec::new()
            }
        }
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

fn map_err(e: aura_core::AuraError) -> AmpChannelError {
    match e {
        aura_core::AuraError::NotFound { message: _ } => AmpChannelError::NotFound,
        aura_core::AuraError::Storage { message } => AmpChannelError::Storage(message),
        aura_core::AuraError::PermissionDenied { message: _ } => AmpChannelError::Unauthorized,
        aura_core::AuraError::Crypto { message } => AmpChannelError::Crypto(message),
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
