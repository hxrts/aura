use super::{AuraEffectSystem, DEFAULT_WINDOW};
use async_trait::async_trait;
use aura_core::effects::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
    RandomCoreEffects, RandomExtendedEffects,
};
use aura_core::hash::hash;
use aura_core::{AuraError, ChannelId, Hash32};
use aura_journal::DomainFact;
use aura_protocol::amp::{AmpJournalEffects, ChannelMembershipFact, ChannelParticipantEvent};
use aura_protocol::effects::TreeEffects;

#[async_trait]
impl AmpChannelEffects for AuraEffectSystem {
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> Result<ChannelId, AmpChannelError> {
        let channel = if let Some(id) = params.channel {
            id
        } else {
            let bytes = self.random_bytes(32).await;
            ChannelId::from_bytes(hash(&bytes))
        };

        let window = params.skip_window.unwrap_or(DEFAULT_WINDOW);

        let checkpoint = aura_journal::fact::ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Default::default(),
            skip_window_override: Some(window),
        };

        self.insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint),
        ))
        .await
        .map_err(map_amp_err)?;

        if params.topic.is_some() || params.skip_window.is_some() {
            let policy = aura_journal::fact::ChannelPolicy {
                context: params.context,
                channel,
                skip_window: params.skip_window.or(Some(window)),
            };
            self.insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
                aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
            ))
            .await
            .map_err(map_amp_err)?;
        }
        Ok(channel)
    }

    async fn close_channel(&self, params: ChannelCloseParams) -> Result<(), AmpChannelError> {
        let state = aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let bump_nonce = self.random_uuid().await.as_bytes().to_vec();
        let bump_id = Hash32(hash(&bump_nonce));
        let proposal = aura_journal::fact::ProposedChannelEpochBump {
            context: params.context,
            channel: params.channel,
            parent_epoch: state.chan_epoch,
            new_epoch: state.chan_epoch + 1,
            bump_id,
            reason: aura_journal::fact::ChannelBumpReason::Routine,
        };

        aura_protocol::amp::emit_proposed_bump(self, proposal.clone())
            .await
            .map_err(map_amp_err)?;

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::AmpEpochBump);
        if policy.allows_mode(aura_core::threshold::AgreementMode::ConsensusFinalized) {
            let tree_state = self.get_current_state().await.map_err(map_amp_err)?;
            let journal = self
                .fetch_context_journal(params.context)
                .await
                .map_err(map_amp_err)?;
            let mut hasher = aura_core::hash::hasher();
            hasher.update(b"RELATIONAL_CONTEXT_FACTS");
            hasher.update(params.context.as_bytes());
            for fact in journal.facts.iter() {
                let bytes = aura_core::util::serialization::to_vec(fact).map_err(|e| {
                    map_amp_err(AuraError::internal(format!(
                        "Failed to serialize context fact: {e}"
                    )))
                })?;
                hasher.update(&bytes);
            }
            let context_commitment = Hash32(hasher.finalize());
            let prestate = aura_core::Prestate::new(
                vec![(self.authority_id, Hash32(tree_state.root_commitment))],
                context_commitment,
            );
            let consensus_params =
                crate::runtime::consensus::build_consensus_params(self, self.authority_id, self)
                    .await
                    .map_err(map_amp_err)?;
            let transcript_ref = self
                .latest_dkg_transcript_commit(self.authority_id, params.context)
                .await
                .map_err(map_amp_err)?
                .and_then(|commit| commit.blob_ref.or(Some(commit.transcript_hash)));

            aura_protocol::amp::commit_bump_with_consensus(
                self,
                &prestate,
                &proposal,
                consensus_params.key_packages,
                consensus_params.group_public_key,
                transcript_ref,
            )
            .await
            .map_err(map_amp_err)?;
        }

        let policy = aura_journal::fact::ChannelPolicy {
            context: params.context,
            channel: params.channel,
            skip_window: Some(0),
        };

        self.insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelPolicy(policy),
        ))
        .await
        .map_err(map_amp_err)?;

        Ok(())
    }

    async fn join_channel(&self, params: ChannelJoinParams) -> Result<(), AmpChannelError> {
        aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let timestamp = ChannelMembershipFact::random_timestamp(self).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            timestamp,
        );
        self.insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_amp_err)?;

        tracing::debug!(
            "Participant {:?} joined channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );

        Ok(())
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;
        let timestamp = ChannelMembershipFact::random_timestamp(self).await;
        let membership = ChannelMembershipFact::new(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            timestamp,
        );
        self.insert_relational_fact(membership.to_generic())
            .await
            .map_err(map_amp_err)?;

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
    ) -> Result<AmpCiphertext, AmpChannelError> {
        let state = aura_protocol::amp::get_channel_state(self, params.context, params.channel)
            .await
            .map_err(map_amp_err)?;

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: state.chan_epoch,
            ratchet_gen: 0,
        };

        let cipher = AmpCiphertext {
            header,
            ciphertext: params.plaintext.clone(),
        };

        Ok(cipher)
    }
}

fn map_amp_err(e: aura_core::AuraError) -> AmpChannelError {
    AmpChannelError::Internal(e.to_string())
}
