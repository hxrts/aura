use super::{AuraEffectSystem, DEFAULT_WINDOW};
use async_trait::async_trait;
use aura_core::effects::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, ChannelCloseParams, ChannelCreateParams,
    ChannelJoinParams, ChannelLeaveParams, ChannelSendParams, RandomCoreEffects,
    RandomExtendedEffects,
};
use aura_core::hash::hash;
use aura_core::{AuraError, ChannelId, Hash32};
use aura_journal::DomainFact;
use aura_protocol::amp::{AmpJournalEffects, ChannelParticipantEvent};
use aura_protocol::effects::TreeEffects;

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
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

        match aura_protocol::amp::get_channel_state(self, params.context, channel).await {
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

        let checkpoint = aura_journal::fact::ChannelCheckpoint {
            context: params.context,
            channel,
            chan_epoch: 0,
            base_gen: 0,
            window,
            ck_commitment: Hash32::default(),
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
        let proposal = aura_journal::fact::ProposedChannelEpochBump::new(
            params.context,
            params.channel,
            state.chan_epoch,
            state.chan_epoch + 1,
            bump_id,
            aura_journal::fact::ChannelBumpReason::Routine,
        );

        aura_protocol::amp::emit_proposed_bump(self, proposal.clone())
            .await
            .map_err(map_amp_err)?;

        let policy =
            aura_core::threshold::policy_for(aura_core::threshold::CeremonyFlow::AmpEpochBump);
        let consensus_required =
            crate::runtime::consensus::consensus_required_for_authority(self, self.authority_id)
                .await;
        if policy.allows_mode(aura_core::threshold::AgreementMode::ConsensusFinalized)
            && consensus_required
        {
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
                    map_amp_err(AuraError::Serialization {
                        message: format!("Failed to serialize context fact: {e}"),
                        source: Some(std::sync::Arc::new(e)),
                    })
                })?;
                hasher.update(&bytes);
            }
            let context_commitment = Hash32(hasher.finalize());
            let prestate = aura_core::Prestate::new(
                vec![(self.authority_id, Hash32(tree_state.root_commitment))],
                context_commitment,
            )
            .map_err(|error| {
                map_amp_err(AuraError::Invalid {
                    message: format!("Invalid AMP prestate: {error}"),
                    source: Some(std::sync::Arc::new(error)),
                })
            })?;
            let consensus_params = crate::runtime::consensus::build_consensus_params(
                params.context,
                self,
                self.authority_id,
                self,
            )
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
        self.commit_channel_membership(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
            None,
        )
        .await?;
        tracing::debug!(
            "Participant {:?} joined channel {:?} in context {:?}",
            params.participant,
            params.channel,
            params.context
        );
        Ok(())
    }

    async fn leave_channel(&self, params: ChannelLeaveParams) -> Result<(), AmpChannelError> {
        self.commit_channel_membership(
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
            None,
        )
        .await?;
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
        let config = aura_protocol::amp::config::AmpRuntimeConfig::default();
        aura_protocol::amp::amp_send(
            self,
            params.context,
            params.channel,
            params.sender,
            params.plaintext,
            &config,
        )
        .await
        .map_err(map_amp_err)
    }
}

impl AuraEffectSystem {
    /// Record a channel membership event (a join naming `episode`, or a
    /// departure ending the observed episodes) in the context journal AMP
    /// state reads, and commit it for the chat projection.
    pub(crate) async fn commit_channel_membership(
        &self,
        context: aura_core::ContextId,
        channel: ChannelId,
        participant: aura_core::AuthorityId,
        event: ChannelParticipantEvent,
        episode: Option<String>,
    ) -> Result<aura_journal::fact::RelationalFact, AmpChannelError> {
        let membership = aura_protocol::amp::journal::channel_membership_event(
            self,
            context,
            channel,
            participant,
            event,
            episode,
        )
        .await
        .map_err(amp_membership_error)?
        .to_generic();
        self.insert_relational_fact(membership.clone())
            .await
            .map_err(map_amp_err)?;
        self.commit_relational_facts(vec![membership.clone()])
            .await
            .map_err(map_amp_err)?;
        Ok(membership)
    }
}

fn map_amp_err(e: aura_core::AuraError) -> AmpChannelError {
    AmpChannelError::Effect(e)
}

/// AMP error for a failed membership event: the rejoin refusal it carries, or
/// the effect failure.
pub(crate) fn amp_membership_error(error: AuraError) -> AmpChannelError {
    match std::error::Error::source(&error)
        .and_then(|source| source.downcast_ref::<AmpChannelError>())
    {
        Some(rejoin @ AmpChannelError::RejoinRequiresMembershipEvidence { .. }) => rejoin.clone(),
        _ => map_amp_err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[tokio::test]
    async fn canonical_checkpoint_admission_emits_typed_absence_and_duplicate() {
        let config = crate::core::AgentConfig::default();
        let effects = AuraEffectSystem::simulation_for_named_test(
            &config,
            "amp-canonical-checkpoint-admission",
        )
        .unwrap();
        let context = aura_core::ContextId::new_from_entropy([0x41; 32]);
        let channel = aura_core::ChannelId::from_bytes([0x42; 32]);
        let participant = effects.authority_id;
        let missing = effects
            .join_channel(ChannelJoinParams {
                context,
                channel,
                participant,
            })
            .await
            .unwrap_err();
        let cause = missing
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<aura_protocol::amp::ChannelStateUnavailable>()
            .unwrap();
        assert_eq!((cause.context(), cause.channel()), (context, channel));
        let create = ChannelCreateParams {
            context,
            channel: Some(channel),
            skip_window: None,
            topic: None,
        };
        assert_eq!(
            effects.create_channel(create.clone()).await.unwrap(),
            channel
        );
        let original = aura_protocol::amp::get_channel_state(&effects, context, channel)
            .await
            .unwrap();
        let duplicate = effects.create_channel(create).await.unwrap_err();
        assert!(matches!(duplicate, AmpChannelError::AlreadyExists {
            context: actual_context, channel: actual_channel,
        } if actual_context == context && actual_channel == channel));
        let retained = aura_protocol::amp::get_channel_state(&effects, context, channel)
            .await
            .unwrap();
        assert_eq!(retained, original);
    }
    #[tokio::test]
    async fn partial_amp_facts_do_not_materialize_checkpoint_or_suppress_creation() {
        use aura_core::time::PhysicalTime;
        use aura_journal::fact::{
            ChannelBootstrap, ChannelBumpReason, ChannelPolicy, ProposedChannelEpochBump,
        };
        let config = crate::core::AgentConfig::default();
        let effects = AuraEffectSystem::simulation_for_named_test(
            &config,
            "amp-partial-facts-before-canonical-checkpoint",
        )
        .unwrap();
        let replay_effects = AuraEffectSystem::simulation_for_named_test(
            &config,
            "amp-canonical-checkpoint-before-partial-replay",
        )
        .unwrap();
        let context = aura_core::ContextId::new_from_entropy([0x71; 32]);
        for (index, channel) in [
            ChannelId::from_bytes([0x72; 32]),
            ChannelId::from_bytes([0x73; 32]),
            ChannelId::from_bytes([0x74; 32]),
        ]
        .into_iter()
        .enumerate()
        {
            let partial = match index {
                0 => aura_journal::ProtocolRelationalFact::AmpChannelPolicy(ChannelPolicy {
                    context,
                    channel,
                    skip_window: Some(17),
                }),
                1 => aura_journal::ProtocolRelationalFact::AmpChannelBootstrap(ChannelBootstrap {
                    context,
                    channel,
                    bootstrap_id: Hash32::default(),
                    dealer: effects.authority_id,
                    recipients: vec![effects.authority_id],
                    created_at: PhysicalTime::exact(1),
                    expires_at: None,
                }),
                _ => aura_journal::ProtocolRelationalFact::AmpProposedChannelEpochBump(
                    ProposedChannelEpochBump::new(
                        context,
                        channel,
                        0,
                        1,
                        Hash32::default(),
                        ChannelBumpReason::Routine,
                    ),
                ),
            };
            let partial_fact = aura_journal::fact::RelationalFact::Protocol(partial);
            effects
                .insert_relational_fact(partial_fact.clone())
                .await
                .unwrap();
            let staged = aura_protocol::amp::get_reduced_channel_state(&effects, context, channel)
                .await
                .unwrap();
            assert!(staged.canonical_checkpoint.is_none());
            let missing = aura_protocol::amp::get_channel_state(&effects, context, channel)
                .await
                .unwrap_err();
            let absence = aura_protocol::amp::ChannelStateUnavailable::find(&missing).unwrap();
            assert_eq!((absence.context(), absence.channel()), (context, channel));
            assert_eq!(
                effects
                    .create_channel(ChannelCreateParams {
                        context,
                        channel: Some(channel),
                        skip_window: None,
                        topic: None,
                    })
                    .await
                    .unwrap(),
                channel
            );
            let canonical = aura_protocol::amp::get_channel_state(&effects, context, channel)
                .await
                .unwrap();
            let checkpoint = canonical.canonical_checkpoint.as_ref().unwrap();
            assert_eq!((checkpoint.context, checkpoint.channel), (context, channel));
            let duplicate = effects
                .create_channel(ChannelCreateParams {
                    context,
                    channel: Some(channel),
                    skip_window: None,
                    topic: None,
                })
                .await
                .unwrap_err();
            assert!(matches!(duplicate, AmpChannelError::AlreadyExists { .. }));
            assert_eq!(
                aura_protocol::amp::get_channel_state(&effects, context, channel)
                    .await
                    .unwrap(),
                canonical
            );

            replay_effects
                .insert_relational_fact(aura_journal::fact::RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(checkpoint.clone()),
                ))
                .await
                .unwrap();
            replay_effects
                .insert_relational_fact(partial_fact)
                .await
                .unwrap();
            let replay = aura_protocol::amp::get_channel_state(&replay_effects, context, channel)
                .await
                .unwrap();
            assert_eq!(
                replay, canonical,
                "checkpoint-first replay must retain the exact canonical state"
            );
        }
    }
}
