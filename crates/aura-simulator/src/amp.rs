//! Simulator implementation of AmpChannelEffects.
//!
//! Uses the same AMP facts/reduction path via AmpJournalEffects, but relies on
//! simulator-controlled time/random and applies a deterministic XOR mask over
//! plaintext using channel header + sender to avoid plaintext transport while
//! keeping the implementation side-effect free for simulation.

use async_trait::async_trait;
use aura_amp::journal::channel_membership_event;
use aura_amp::{get_channel_state, AmpJournalEffects, ChannelParticipantEvent};
use aura_core::effects::amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::{RandomCoreEffects, RandomExtendedEffects};
use aura_core::hash::hash;
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_core::Hash32;
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ChannelPolicy, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::DomainFact;

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

        match get_channel_state(&self.effects, params.context, channel).await {
            Ok(_) => {
                return Err(AmpChannelError::AlreadyExists {
                    context: params.context,
                    channel,
                })
            }
            Err(error)
                if aura_protocol::amp::ChannelStateUnavailable::find(&error).is_some_and(
                    |absence| absence.context() == params.context && absence.channel() == channel,
                ) => {}
            Err(error) => return Err(AmpChannelError::Effect(error)),
        }
        let window = params.skip_window.unwrap_or(DEFAULT_WINDOW);
        let checkpoint = ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Hash32::default(),
            skip_window_override: Some(window),
        };

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
            ))
            .await
            .map_err(map_err)?;

        if params.topic.is_some() || params.skip_window.is_some() {
            let policy = ChannelPolicy {
                context: params.context,
                channel,
                skip_window: params.skip_window.or(Some(window)),
            };
            self.effects
                .insert_relational_fact(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
                ))
                .await
                .map_err(map_err)?;
        }

        Ok(channel)
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        let state = get_channel_state(&self.effects, params.context, params.channel)
            .await
            .map_err(map_err)?;

        let policy = policy_for(CeremonyFlow::AmpEpochBump);
        if !policy.allows_mode(AgreementMode::Provisional) {
            return Err(AmpChannelError::InvalidState(
                "AMP epoch bump policy does not allow provisional mode".to_string(),
            ));
        }

        let bump_nonce = self.effects.random_uuid().await.as_bytes().to_vec();
        let bump_id = aura_core::Hash32(hash(&bump_nonce));
        let proposal = ProposedChannelEpochBump::new(
            params.context,
            params.channel,
            state.chan_epoch,
            state.chan_epoch + 1,
            bump_id,
            ChannelBumpReason::Routine,
        );

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(proposal),
            ))
            .await
            .map_err(map_err)?;

        let policy = ChannelPolicy {
            context: params.context,
            channel: params.channel,
            skip_window: Some(0),
        };

        self.effects
            .insert_relational_fact(RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
            ))
            .await
            .map_err(map_err)?;

        Ok(())
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        // The canonical AMP membership event verifies the channel exists.
        let membership = channel_membership_event(
            &self.effects,
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            None,
        )
        .await
        .map_err(map_err)?;
        self.effects
            .insert_relational_fact(membership.to_generic())
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
        // The canonical AMP membership event verifies the channel exists.
        let membership = channel_membership_event(
            &self.effects,
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            None,
        )
        .await
        .map_err(map_err)?;
        self.effects
            .insert_relational_fact(membership.to_generic())
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
        if !aura_amp::core::sender_allowed_by_epoch_state(&state, params.sender) {
            return Err(AmpChannelError::Unauthorized);
        }
        let send_ratchet = aura_amp::core::send_ratchet_from_epoch_state(&state);

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: send_ratchet.chan_epoch,
            ratchet_gen: state.current_gen,
        };

        // Compute ciphertext before moving header into AmpCiphertext
        let ciphertext = mask_ciphertext(&header, &params.sender, &params.plaintext);

        Ok(AmpCiphertext { header, ciphertext })
    }
}

fn map_err(error: aura_core::AuraError) -> AmpChannelError {
    AmpChannelError::Effect(error)
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
