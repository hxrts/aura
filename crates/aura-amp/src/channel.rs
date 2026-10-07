//! AMP channel lifecycle coordinator (Layer 4)
//!
//! Provides an implementation of `aura_core::effects::AmpChannelEffects` that
//! persists AMP channel facts to the context journal via `AmpJournalEffects`.
//! Production message encryption is handled by the high-level
//! `protocol::orchestration::amp_send` path, which derives ratchet keys and
//! seals payloads with AEAD before transport.

use aura_core::effects::amp::{
    AmpChannelEffects, AmpChannelError, AmpCiphertext, AmpHeader, ChannelCloseParams,
    ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams, ChannelSendParams,
};
use aura_core::effects::random::RandomExtendedEffects;
use aura_core::hash::hash;
use aura_core::threshold::{policy_for, AgreementMode, CeremonyFlow};
use aura_core::time::TimeStamp;
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::Hash32;
use aura_journal::fact::{
    ChannelBumpReason, ChannelCheckpoint, ChannelPolicy, ProposedChannelEpochBump, RelationalFact,
};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::{DomainFact, FactReducer};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use crate::{config::AmpRuntimeConfig, get_channel_state, AmpJournalEffects};

/// Simple coordinator that writes AMP channel facts into the context journal.
pub struct AmpChannelCoordinator<E> {
    effects: E,
}

impl<E> AmpChannelCoordinator<E> {
    pub fn new(effects: E) -> Self {
        Self { effects }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl<E> AmpChannelEffects for AmpChannelCoordinator<E>
where
    E: AmpJournalEffects + RandomExtendedEffects + Send + Sync,
{
    async fn create_channel(
        &self,
        params: ChannelCreateParams,
    ) -> std::result::Result<ChannelId, AmpChannelError> {
        let policy = policy_for(CeremonyFlow::AmpBootstrap);
        if !policy.allows_mode(AgreementMode::Provisional) {
            return Err(AmpChannelError::InvalidState(
                "AMP bootstrap policy does not allow provisional channels".to_string(),
            ));
        }
        let channel = if let Some(id) = params.channel {
            id
        } else {
            let order = self.effects.order_time().await.map_err(|error| {
                AmpChannelError::Effect(aura_core::AuraError::Internal {
                    message: error.to_string(),
                    source: Some(std::sync::Arc::new(error)),
                })
            })?;
            aura_core::types::identifiers::ChannelId::from_bytes(order.0)
        };

        match get_channel_state(&self.effects, params.context, channel).await {
            Ok(_) => {
                return Err(AmpChannelError::AlreadyExists {
                    context: params.context,
                    channel,
                })
            }
            Err(error)
                if crate::journal::ChannelStateUnavailable::find(&error).is_some_and(
                    |absence| absence.context() == params.context && absence.channel() == channel,
                ) => {}
            Err(error) => return Err(AmpChannelError::Effect(error)),
        }

        let config = AmpRuntimeConfig::default();
        let window = params
            .skip_window
            .unwrap_or_else(|| config.default_skip_window.get());

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

    async fn close_channel(
        &self,
        params: ChannelCloseParams,
    ) -> std::result::Result<(), AmpChannelError> {
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

    async fn join_channel(
        &self,
        params: ChannelJoinParams,
    ) -> std::result::Result<(), AmpChannelError> {
        persist_channel_membership_event(
            &self.effects,
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Joined,
        )
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
        persist_channel_membership_event(
            &self.effects,
            params.context,
            params.channel,
            params.participant,
            ChannelParticipantEvent::Left,
        )
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
        if !crate::core::sender_allowed_by_epoch_state(&state, params.sender) {
            return Err(AmpChannelError::Unauthorized);
        }
        if !crate::journal::sender_allowed_by_channel_membership(
            &self.effects,
            params.context,
            params.channel,
            params.sender,
        )
        .await
        .map_err(map_err)?
        {
            return Err(AmpChannelError::Unauthorized);
        }
        let send_epoch = state
            .pending_bump
            .as_ref()
            .filter(|_| {
                state.transition.as_ref().is_some_and(|transition| {
                    transition.status
                        == aura_journal::reduction::AmpTransitionReductionStatus::A2Live
                })
            })
            .map(|pending| pending.new_epoch)
            .unwrap_or(state.chan_epoch);

        let header = AmpHeader {
            context: params.context,
            channel: params.channel,
            chan_epoch: send_epoch,
            ratchet_gen: state.current_gen,
        };

        let _ = header;
        Err(AmpChannelError::Crypto(
            "legacy AMP channel coordinator cannot emit ciphertext; use amp_send AEAD path"
                .to_string(),
        ))
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
///
/// Membership is a set of episodes per participant. A join starts the
/// episode its `episode` names (the accepted invitation id; `None` is the
/// unnamed schema-one episode). A departure ends the unnamed episode and the
/// named episodes in `ends`, which are those its writer observed. A
/// participant is a member while one of its episodes is live, so a fresh
/// invitation re-admits a departed member while a replayed one cannot.
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "amp-channel-membership",
    schema_version = 2,
    context = "context"
)]
pub struct ChannelMembershipFact {
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
    event: ChannelParticipantEvent,
    timestamp: TimeStamp,
    /// Episode a join starts (schema v2); `None` is the unnamed episode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    episode: Option<String>,
    /// Named episodes a departure ends (schema v2).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    ends: BTreeSet<String>,
}

impl ChannelMembershipFact {
    /// Membership event in the unnamed episode (a join that names no
    /// invitation, or a departure that ends no named episode).
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
            episode: None,
            ends: BTreeSet::new(),
        }
    }

    /// Join that starts the episode named by an accepted invitation.
    pub fn joined_episode(
        context: ContextId,
        channel: ChannelId,
        participant: AuthorityId,
        episode: String,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            episode: Some(episode),
            ..Self::new(
                context,
                channel,
                participant,
                ChannelParticipantEvent::Joined,
                timestamp,
            )
        }
    }

    /// Departure that ends the participant's episodes in `observed`.
    pub fn departure(
        observed: &ChannelMembershipObservations,
        participant: AuthorityId,
        timestamp: TimeStamp,
    ) -> Self {
        Self {
            ends: observed.named_episodes(participant),
            ..Self::new(
                observed.context,
                observed.channel,
                participant,
                ChannelParticipantEvent::Left,
                timestamp,
            )
        }
    }

    pub async fn random_timestamp<A: AmpJournalEffects>(
        effects: &A,
    ) -> aura_core::Result<TimeStamp> {
        effects
            .order_time()
            .await
            .map(TimeStamp::OrderClock)
            .map_err(|source| aura_core::AuraError::Internal {
                message: "read original AMP membership order token".into(),
                source: Some(std::sync::Arc::new(source)),
            })
    }

    /// Context that scopes this membership event.
    #[must_use]
    pub fn context(&self) -> ContextId {
        self.context
    }

    /// Channel this membership event applies to.
    #[must_use]
    pub fn channel(&self) -> ChannelId {
        self.channel
    }

    /// Participant affected by this membership event.
    #[must_use]
    pub fn participant(&self) -> AuthorityId {
        self.participant
    }

    /// Membership event kind (joined or left).
    #[must_use]
    pub fn event(&self) -> ChannelParticipantEvent {
        self.event
    }

    /// Logical timestamp captured for this event.
    #[must_use]
    pub fn timestamp(&self) -> TimeStamp {
        self.timestamp.clone()
    }

    /// Episode a join starts (`None`: the unnamed episode).
    #[must_use]
    pub fn episode(&self) -> Option<&str> {
        self.episode.as_deref()
    }
}

/// Type identifier for AMP channel membership facts.
pub const CHANNEL_MEMBERSHIP_FACT_TYPE_ID: &str = "amp-channel-membership";

/// Registry reducer for [`ChannelMembershipFact`]. It indexes each event as a
/// content-addressed binding; membership itself is reduced only by
/// [`ChannelMembershipObservations`] over the whole fact set.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChannelMembershipFactReducer;

impl FactReducer for ChannelMembershipFactReducer {
    fn handles_type(&self) -> &'static str {
        CHANNEL_MEMBERSHIP_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        let fact = ChannelMembershipFact::from_envelope(envelope)?;
        if fact.context != context_id {
            return None;
        }
        let sub_type = match fact.event {
            ChannelParticipantEvent::Joined => "amp-channel-joined",
            ChannelParticipantEvent::Left => "amp-channel-left",
        };
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type.to_string()),
            context_id,
            data: hash(&envelope.payload).to_vec(),
        })
    }
}

/// Membership episodes observed in one context and channel. This is neither
/// authorization nor canonical channel creation evidence. Episode ends are
/// permanent; opaque order tokens never order a join after a departure, only
/// a join naming a fresh episode re-admits a departed participant.
#[derive(Debug, Clone)]
pub struct ChannelMembershipObservations {
    context: ContextId,
    channel: ChannelId,
    started: BTreeMap<AuthorityId, BTreeSet<Option<String>>>,
    ended: BTreeMap<AuthorityId, BTreeSet<Option<String>>>,
}
impl ChannelMembershipObservations {
    /// Begin empty observations for an exact context and channel.
    pub fn new(context: ContextId, channel: ChannelId) -> Self {
        Self {
            context,
            channel,
            started: BTreeMap::new(),
            ended: BTreeMap::new(),
        }
    }
    /// Observe an exact-scope fact; foreign context/channel facts are rejected.
    /// Returns whether the fact belongs to this observation scope.
    pub fn observe(&mut self, fact: &ChannelMembershipFact) -> bool {
        if fact.context() != self.context || fact.channel() != self.channel {
            return false;
        }
        match fact.event() {
            ChannelParticipantEvent::Joined => {
                self.started
                    .entry(fact.participant())
                    .or_default()
                    .insert(fact.episode.clone());
            }
            ChannelParticipantEvent::Left => {
                let ended = self.ended.entry(fact.participant()).or_default();
                ended.insert(None);
                ended.extend(fact.ends.iter().cloned().map(Some));
            }
        }
        true
    }
    /// Sorted participants with a live episode.
    pub fn participants(&self) -> impl Iterator<Item = AuthorityId> + '_ {
        self.started
            .keys()
            .copied()
            .filter(|participant| self.contains(*participant))
    }
    /// Whether the participant departed and has no live episode.
    pub fn departed(&self, participant: AuthorityId) -> bool {
        self.ended.contains_key(&participant) && !self.contains(participant)
    }
    /// Whether the participant's `episode` was ended by an observed departure.
    pub fn episode_ended(&self, participant: AuthorityId, episode: Option<&str>) -> bool {
        self.ended
            .get(&participant)
            .is_some_and(|ended| ended.iter().any(|ended| ended.as_deref() == episode))
    }
    /// Named episodes started for the participant.
    fn named_episodes(&self, participant: AuthorityId) -> BTreeSet<String> {
        self.started
            .get(&participant)
            .into_iter()
            .flatten()
            .flatten()
            .cloned()
            .collect()
    }
    /// Whether any join or departure evidence has been observed.
    pub fn has_observations(&self) -> bool {
        !self.started.is_empty() || !self.ended.is_empty()
    }
    /// Whether the participant has a live episode.
    pub fn contains(&self, participant: AuthorityId) -> bool {
        self.started.get(&participant).is_some_and(|started| {
            started
                .iter()
                .any(|episode| !self.episode_ended(participant, episode.as_deref()))
        })
    }
}

fn map_err(error: aura_core::AuraError) -> AmpChannelError {
    AmpChannelError::Effect(error)
}

async fn persist_channel_membership_event<E: AmpJournalEffects>(
    effects: &E,
    context: ContextId,
    channel: ChannelId,
    participant: AuthorityId,
    event: ChannelParticipantEvent,
) -> aura_core::Result<()> {
    let membership = crate::journal::channel_membership_event(
        effects,
        context,
        channel,
        participant,
        event,
        None,
    )
    .await?;
    effects
        .insert_relational_fact(membership.to_generic())
        .await
}

#[cfg(test)]
mod membership_tests {
    use super::*;
    use aura_core::effects::TimeError;
    use aura_core::effects::{JournalEffects, OrderClockEffects};
    use aura_core::time::OrderTime;
    use aura_core::{FlowBudget, FlowCost, Journal};
    use aura_journal::fact::FactContent;

    struct FaultClockJournal {
        journal: tokio::sync::Mutex<Journal>,
    }
    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl JournalEffects for FaultClockJournal {
        async fn get_journal(&self) -> aura_core::Result<Journal> {
            Ok(self.journal.lock().await.clone())
        }
        async fn persist_journal(&self, journal: &Journal) -> aura_core::Result<()> {
            *self.journal.lock().await = journal.clone();
            Ok(())
        }
        async fn merge_facts(
            &self,
            mut target: Journal,
            delta: Journal,
        ) -> aura_core::Result<Journal> {
            target.merge_facts(delta.facts);
            Ok(target)
        }
        async fn refine_caps(&self, _: Journal, _: Journal) -> aura_core::Result<Journal> {
            panic!("membership does not refine capabilities")
        }
        async fn get_flow_budget(
            &self,
            _: &ContextId,
            _: &AuthorityId,
        ) -> aura_core::Result<FlowBudget> {
            panic!("membership does not read flow budgets")
        }
        async fn update_flow_budget(
            &self,
            _: &ContextId,
            _: &AuthorityId,
            _: &FlowBudget,
        ) -> aura_core::Result<FlowBudget> {
            panic!("membership does not update flow budgets")
        }
        async fn charge_flow_budget(
            &self,
            _: &ContextId,
            _: &AuthorityId,
            _: FlowCost,
        ) -> aura_core::Result<FlowBudget> {
            panic!("membership does not charge flow budgets")
        }
    }
    #[async_trait::async_trait]
    impl OrderClockEffects for FaultClockJournal {
        async fn order_time(&self) -> Result<OrderTime, TimeError> {
            Err(TimeError::ServiceUnavailable)
        }
    }
    fn scope() -> (ContextId, ChannelId, AuthorityId) {
        (
            ContextId::new_from_entropy(hash(
                b"aura-amp.membership-original-source-regression.context",
            )),
            ChannelId::from_bytes(hash(
                b"aura-amp.membership-original-source-regression.channel",
            )),
            AuthorityId::new_from_entropy(hash(
                b"aura-amp.membership-original-source-regression.participant",
            )),
        )
    }
    fn original_journal(
        context: ContextId,
        channel: ChannelId,
        participant: AuthorityId,
        departed: bool,
    ) -> Journal {
        let mut journal = Journal::new();
        let checkpoint = RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::AmpChannelCheckpoint(ChannelCheckpoint {
                context,
                channel,
                chan_epoch: 0,
                base_gen: 0,
                window: 1024,
                ck_commitment: Hash32::default(),
                skip_window_override: None,
            }),
        );
        let entries = if departed {
            vec![
                checkpoint,
                ChannelMembershipFact::new(
                    context,
                    channel,
                    participant,
                    ChannelParticipantEvent::Left,
                    TimeStamp::OrderClock(OrderTime([0; 32])),
                )
                .to_generic(),
            ]
        } else {
            vec![checkpoint]
        };
        for (index, content) in (0u8..).zip(entries) {
            let encoded = match serde_json::to_vec(&FactContent::Relational(content)) {
                Ok(bytes) => bytes,
                Err(source) => panic!("encode actual original fixture: {source}"),
            };
            if let Err(source) = journal.facts.insert(
                format!("relational:{context}:{}", hex::encode([index; 32])),
                aura_core::FactValue::Bytes(encoded),
            ) {
                panic!("retain original fixture: {source}");
            }
        }
        journal
    }
    #[tokio::test]
    async fn original_membership_clock_failure_retains_source_and_appends_nothing() {
        let (context, channel, participant) = scope();
        let initial = original_journal(context, channel, participant, false);
        let effects = FaultClockJournal {
            journal: tokio::sync::Mutex::new(initial.clone()),
        };
        let Err(error) = persist_channel_membership_event(
            &effects,
            context,
            channel,
            participant,
            ChannelParticipantEvent::Joined,
        )
        .await
        else {
            panic!("clock outage cannot publish membership success");
        };
        assert!(std::error::Error::source(&error).is_some_and(|source| source.is::<TimeError>()));
        assert_eq!(*effects.journal.lock().await, initial);
    }
    #[tokio::test]
    async fn original_departure_rejects_unversioned_rejoin_and_empty_sender_bypass() {
        let (context, channel, participant) = scope();
        let initial = original_journal(context, channel, participant, true);
        let effects = FaultClockJournal {
            journal: tokio::sync::Mutex::new(initial.clone()),
        };
        let Err(error) = persist_channel_membership_event(
            &effects,
            context,
            channel,
            participant,
            ChannelParticipantEvent::Joined,
        )
        .await
        else {
            panic!("opaque token cannot authorize rejoin");
        };
        assert!(matches!(
            std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<AmpChannelError>()),
            Some(AmpChannelError::RejoinRequiresMembershipEvidence { .. })
        ));
        let allowed = crate::journal::sender_allowed_by_channel_membership(
            &effects,
            context,
            channel,
            participant,
        )
        .await;
        assert!(
            matches!(allowed, Ok(false)),
            "all departed membership must not grant send authority"
        );
        assert_eq!(*effects.journal.lock().await, initial);
    }

    #[test]
    fn membership_reduction_is_independent_of_arrival_order() {
        let (context, channel, participant) = scope();
        let at = |byte| TimeStamp::OrderClock(aura_core::time::OrderTime([byte; 32]));
        let first = ChannelMembershipFact::joined_episode(
            context,
            channel,
            participant,
            "first".to_string(),
            at(1),
        );
        let mut observed = ChannelMembershipObservations::new(context, channel);
        observed.observe(&first);
        let departure = ChannelMembershipFact::departure(&observed, participant, at(2));
        let rejoin = ChannelMembershipFact::joined_episode(
            context,
            channel,
            participant,
            "second".to_string(),
            at(3),
        );
        let reduce = |facts: &[ChannelMembershipFact]| {
            let mut observations = ChannelMembershipObservations::new(context, channel);
            for fact in facts {
                observations.observe(fact);
            }
            (
                observations.participants().collect::<Vec<_>>(),
                observations.episode_ended(participant, Some("first")),
            )
        };
        let ended = aura_journal::causal_reduction::assert_permutation_invariant(
            &[first.clone(), departure.clone()],
            reduce,
        );
        assert_eq!(ended, (Vec::new(), true));
        let rejoined = aura_journal::causal_reduction::assert_permutation_invariant(
            &[first, departure, rejoin],
            reduce,
        );
        assert_eq!(rejoined, (vec![participant], true));
    }

    #[test]
    fn registered_membership_reducer_uses_the_envelope_version_and_context() {
        let (context, channel, participant) = scope();
        let fact = ChannelMembershipFact::new(
            context,
            channel,
            participant,
            ChannelParticipantEvent::Left,
            TimeStamp::OrderClock(aura_core::time::OrderTime([4; 32])),
        );
        assert_eq!(fact.type_id(), CHANNEL_MEMBERSHIP_FACT_TYPE_ID);
        let envelope = fact.to_envelope();
        assert_eq!(envelope.schema_version, 2);
        let binding = ChannelMembershipFactReducer
            .reduce_envelope(context, &envelope)
            .expect("registered reducer decodes its own fact");
        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic("amp-channel-left".to_string())
        );
        let foreign = ContextId::new_from_entropy([7; 32]);
        assert!(ChannelMembershipFactReducer
            .reduce_envelope(foreign, &envelope)
            .is_none());
    }
}
