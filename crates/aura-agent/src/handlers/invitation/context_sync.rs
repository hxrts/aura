//! Membership-checked relational-context sync for home, channel and DM
//! contexts (docs/111 §11.4).
//!
//! A member sends a peer the digests of the context-sync facts it holds for a
//! context; the peer, if it counts the requester as a member of that context,
//! answers with the facts the requester lacks. Commit-time sends to peers only
//! cut latency: this pull, run by the runtime-owned periodic sync, is what
//! makes a lost send converge.

use super::*;
use aura_journal::fact::RelationalFact;
use aura_protocol::amp::AmpJournalEffects;
use std::collections::BTreeSet;

/// Context sync request content type.
pub(super) const CONTEXT_SYNC_CONTENT_TYPE: &str = "application/aura-context-sync";

/// A member's digest of the context-sync facts it holds for `context_id`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct ContextSyncRequest {
    pub(super) context_id: ContextId,
    pub(super) held: BTreeSet<Hash32>,
}

pub(super) struct InvitationContextSync<'a> {
    handler: &'a InvitationHandler,
}

/// Whether a home-context fact is governance state a joining member needs
/// to evaluate moderation and access: creation (the creator's moderator
/// designation), moderator grants/revokes, access
/// overrides, the capability config, and membership episodes (joins and
/// leaves), so every member learns of members who joined after it and a
/// joiner learns of the members before it.
pub(super) fn is_home_governance_envelope(
    envelope: &aura_core::types::facts::FactEnvelope,
) -> bool {
    use aura_social::moderation::facts::{
        HOME_GRANT_MODERATOR_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    };
    match envelope.type_id.as_str() {
        HOME_GRANT_MODERATOR_FACT_TYPE_ID | HOME_REVOKE_MODERATOR_FACT_TYPE_ID => true,
        aura_social::SOCIAL_FACT_TYPE_ID => matches!(
            aura_social::SocialFact::from_envelope(envelope),
            Some(
                aura_social::SocialFact::HomeCreated { .. }
                    | aura_social::SocialFact::AccessOverrideSet { .. }
                    | aura_social::SocialFact::AccessLevelCapabilitiesConfigured { .. }
                    | aura_social::SocialFact::MemberJoined { .. }
                    | aura_social::SocialFact::MemberLeft { .. }
            )
        ),
        _ => false,
    }
}

fn is_channel_membership_envelope(envelope: &aura_core::types::facts::FactEnvelope) -> bool {
    envelope.type_id.as_str() == aura_amp::CHANNEL_MEMBERSHIP_FACT_TYPE_ID
}

/// Whether a relational fact takes part in context sync: home governance,
/// moderation actions, chat facts (channels, messages, delivery and read
/// state) and AMP channel membership.
fn is_context_sync_envelope(envelope: &aura_core::types::facts::FactEnvelope) -> bool {
    is_home_governance_envelope(envelope)
        || aura_social::moderation::facts::claimed_moderation_actor(envelope).is_some()
        || envelope.type_id.as_str() == CHAT_FACT_TYPE_ID
        || is_channel_membership_envelope(envelope)
}

/// Whether `own_authority` may serve `envelope` to a syncing member. A
/// receiver requires a moderation fact's actor to be its sender, so a
/// moderation fact is served only by its author.
fn may_serve_context_fact(
    envelope: &aura_core::types::facts::FactEnvelope,
    own_authority: AuthorityId,
) -> bool {
    if let Some(actor) = aura_social::moderation::facts::claimed_moderation_actor(envelope) {
        return actor == own_authority;
    }
    is_context_sync_envelope(envelope)
}

fn context_fact_digest(fact: &RelationalFact) -> AgentResult<Hash32> {
    let bytes = aura_core::util::serialization::to_vec(fact)
        .map_err(|error| AgentError::internal(error.to_string()))?;
    Ok(Hash32::from_bytes(&bytes))
}

impl<'a> InvitationContextSync<'a> {
    pub(super) fn new(handler: &'a InvitationHandler) -> Self {
        Self { handler }
    }

    fn own_authority(&self) -> AuthorityId {
        self.handler.context.authority.authority_id()
    }

    /// This authority's committed generic relational facts, by context.
    async fn committed_generic_facts(
        &self,
        effects: &AuraEffectSystem,
    ) -> AgentResult<Vec<RelationalFact>> {
        let facts = effects
            .load_committed_facts(self.own_authority())
            .await
            .map_err(|error| AgentError::effects(error.to_string()))?;
        Ok(facts
            .into_iter()
            .filter_map(|fact| match fact.content {
                aura_journal::fact::FactContent::Relational(
                    relational @ RelationalFact::Generic { .. },
                ) => Some(relational),
                _ => None,
            })
            .collect())
    }

    /// Committed context-sync facts of `context_id`.
    async fn context_facts(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> AgentResult<Vec<RelationalFact>> {
        Ok(self
            .committed_generic_facts(effects)
            .await?
            .into_iter()
            .filter(|fact| {
                matches!(
                    fact,
                    RelationalFact::Generic { context_id: fact_context, envelope }
                        if *fact_context == context_id && is_context_sync_envelope(envelope)
                )
            })
            .collect())
    }

    /// Channels of `context_id` this authority knows: those with bootstrap
    /// key material plus those whose creation fact it holds.
    async fn context_channels(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> BTreeSet<ChannelId> {
        let mut channels: BTreeSet<ChannelId> = aura_amp::list_channel_bootstraps(effects)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|(context, _, _)| *context == context_id)
            .map(|(_, channel, _)| channel)
            .collect();
        for (context, channel) in self.created_channels(effects).await {
            if context == context_id {
                channels.insert(channel);
            }
        }
        channels
    }

    async fn created_channels(&self, effects: &AuraEffectSystem) -> Vec<(ContextId, ChannelId)> {
        self.committed_generic_facts(effects)
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|fact| {
                let RelationalFact::Generic { envelope, .. } = fact else {
                    return None;
                };
                if envelope.type_id.as_str() != CHAT_FACT_TYPE_ID {
                    return None;
                }
                match ChatFact::from_envelope(envelope)? {
                    ChatFact::ChannelCreated {
                        context_id,
                        channel_id,
                        ..
                    } => Some((context_id, channel_id)),
                    _ => None,
                }
            })
            .collect()
    }

    /// Contexts this authority syncs: its homes plus every context with a
    /// channel it holds keys for or saw created.
    pub(super) async fn sync_contexts(&self, effects: &AuraEffectSystem) -> BTreeSet<ContextId> {
        use aura_core::effects::reactive::ReactiveEffects;
        let mut contexts = BTreeSet::new();
        if let Ok(homes) = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::HOMES_SIGNAL)
            .await
        {
            contexts.extend(homes.all_homes().filter_map(|home| home.context_id));
        }
        contexts.extend(
            aura_amp::list_channel_bootstraps(effects)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|(context, _, _)| context),
        );
        contexts.extend(
            self.created_channels(effects)
                .await
                .into_iter()
                .map(|(context, _)| context),
        );
        contexts
    }

    /// Members of `context_id` as this authority knows them: the home roster
    /// (members and moderation targets) of a home with that context, and the
    /// authoritative participants of each channel in it. These are both the
    /// peers this authority syncs with and the requesters it serves.
    pub(super) async fn context_peers(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> BTreeSet<AuthorityId> {
        use aura_core::effects::reactive::ReactiveEffects;
        let mut peers = BTreeSet::new();
        if let Ok(homes) = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::HOMES_SIGNAL)
            .await
        {
            for home in homes.all_homes() {
                if home.context_id != Some(context_id) {
                    continue;
                }
                peers.extend(home.members.iter().map(|member| member.id));
                peers.extend(home.ban_list.keys().copied());
                peers.extend(home.mute_list.keys().copied());
                peers.extend(home.access_overrides.keys().copied());
            }
        }
        for channel in self.context_channels(effects, context_id).await {
            if let Ok(participants) = self
                .handler
                .channel_participants(effects, context_id, channel)
                .await
            {
                peers.extend(participants);
            }
        }
        peers
    }

    /// Ask `peer` for the context-sync facts of `context_id` this authority
    /// lacks (pull side).
    pub(super) async fn request(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<()> {
        let mut held = BTreeSet::new();
        for fact in self.context_facts(effects, context_id).await? {
            held.insert(context_fact_digest(&fact)?);
        }
        let payload =
            aura_core::util::serialization::to_vec(&ContextSyncRequest { context_id, held })
                .map_err(|error| AgentError::internal(error.to_string()))?;
        self.send(
            effects,
            peer,
            payload,
            CONTEXT_SYNC_CONTENT_TYPE,
            context_id,
            "context sync request send failed",
        )
        .await
    }

    /// Serve a verified request: the requester must be a member of the
    /// context here; it receives the facts it lacks that this authority may
    /// serve. Delivery is best-effort per fact; the next round repairs gaps.
    pub(super) async fn serve(
        &self,
        effects: &AuraEffectSystem,
        requester: AuthorityId,
        request: ContextSyncRequest,
    ) -> AgentResult<()> {
        if !self
            .context_peers(effects, request.context_id)
            .await
            .contains(&requester)
        {
            tracing::debug!(
                requester = %requester,
                context = %request.context_id,
                "Ignored context sync request from a non-member"
            );
            return Ok(());
        }
        let own = self.own_authority();
        let served = self
            .send_context_facts(effects, request.context_id, requester, |fact, envelope| {
                may_serve_context_fact(envelope, own)
                    && context_fact_digest(fact).is_ok_and(|digest| !request.held.contains(&digest))
            })
            .await?;
        tracing::debug!(
            requester = %requester,
            context = %request.context_id,
            held = request.held.len(),
            served,
            "Served context sync request"
        );
        Ok(())
    }

    /// Send a newly joined home member the home's governance facts this
    /// authority holds (docs/115 §3.2; latency only, the newcomer's own
    /// context sync is what guarantees delivery).
    pub(super) async fn send_home_governance_facts(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        newcomer: AuthorityId,
    ) -> AgentResult<()> {
        self.send_context_facts(effects, context_id, newcomer, |_, envelope| {
            is_home_governance_envelope(envelope)
        })
        .await
        .map(|_| ())
    }

    /// Whether `fact` is a context-sync fact already committed here, so a
    /// fact delivered again by sync (or by a send and a sync) commits once.
    pub(super) async fn already_holds(
        &self,
        effects: &AuraEffectSystem,
        fact: &RelationalFact,
    ) -> AgentResult<bool> {
        let RelationalFact::Generic { envelope, .. } = fact else {
            return Ok(false);
        };
        if !is_context_sync_envelope(envelope) {
            return Ok(false);
        }
        Ok(self
            .committed_generic_facts(effects)
            .await?
            .iter()
            .any(|held| held == fact))
    }

    /// A synced AMP channel membership fact also enters the AMP context
    /// journal, which the authoritative participant set reduces.
    pub(super) async fn admit_membership_fact(
        effects: &AuraEffectSystem,
        fact: &RelationalFact,
    ) -> AgentResult<()> {
        let RelationalFact::Generic { envelope, .. } = fact else {
            return Ok(());
        };
        if !is_channel_membership_envelope(envelope) {
            return Ok(());
        }
        effects
            .insert_relational_fact(fact.clone())
            .await
            .map_err(|error| AgentError::effects(error.to_string()))
    }

    async fn send_context_facts(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
        include: impl Fn(&RelationalFact, &aura_core::types::facts::FactEnvelope) -> bool,
    ) -> AgentResult<usize> {
        let mut sent = 0usize;
        for fact in self.context_facts(effects, context_id).await? {
            let RelationalFact::Generic { envelope, .. } = &fact else {
                continue;
            };
            if !include(&fact, envelope) {
                continue;
            }
            let payload = aura_core::util::serialization::to_vec(&fact)
                .map_err(|error| AgentError::internal(error.to_string()))?;
            if let Err(error) = self
                .send(
                    effects,
                    peer,
                    payload,
                    CHAT_FACT_CONTENT_TYPE,
                    context_id,
                    "context sync fact send failed",
                )
                .await
            {
                tracing::debug!(error = %error, peer = %peer, "context sync fact not delivered");
            } else {
                sent += 1;
            }
        }
        Ok(sent)
    }

    async fn send(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        payload: Vec<u8>,
        content_type: &'static str,
        context_id: ContextId,
        failure: &'static str,
    ) -> AgentResult<()> {
        let delivery_context = default_context_id_for_authority(peer);
        let mut envelope = TransportEnvelope {
            destination: peer,
            source: self.own_authority(),
            context: delivery_context,
            payload,
            metadata: crate::handlers::shared::build_transport_metadata(
                content_type,
                [("sync-context", context_id.to_string())],
            ),
            receipt: super::execute_charge_flow_budget(
                FlowCost::new(1),
                delivery_context,
                peer,
                effects,
            )
            .await?
            .map(transport_receipt_from_flow),
        };
        super::attach_invitation_test_receipt_if_needed(effects, &mut envelope);
        super::execution::attempt_network_send_envelope(effects, failure, envelope).await
    }
}

impl InvitationHandler {
    /// Authoritative participants of a channel: the AMP membership reduction,
    /// plus the other party of each accepted invitation to that channel.
    pub(crate) async fn channel_participants(
        &self,
        effects: &AuraEffectSystem,
        context: ContextId,
        channel: ChannelId,
    ) -> Result<BTreeSet<AuthorityId>, aura_core::AuraError> {
        let mut participants: BTreeSet<AuthorityId> =
            aura_protocol::amp::list_channel_participants(effects, context, channel)
                .await?
                .into_iter()
                .collect();
        let local_authority = self.context.authority.authority_id();
        for invitation in self
            .list_channel_invitations_with_storage_required(effects)
            .await?
        {
            if invitation.status != InvitationStatus::Accepted || invitation.context_id != context {
                continue;
            }
            let InvitationType::Channel { home_id, .. } = invitation.invitation_type else {
                continue;
            };
            if home_id != channel {
                continue;
            }
            if invitation.sender_id == local_authority {
                participants.insert(invitation.receiver_id);
            } else if invitation.receiver_id == local_authority {
                participants.insert(invitation.sender_id);
            }
        }
        Ok(participants)
    }

    /// Contexts this authority runs context sync for.
    pub(crate) async fn context_sync_contexts(
        &self,
        effects: &AuraEffectSystem,
    ) -> BTreeSet<ContextId> {
        InvitationContextSync::new(self)
            .sync_contexts(effects)
            .await
    }

    /// Members of `context_id` this authority syncs with.
    pub(crate) async fn context_sync_peers(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> BTreeSet<AuthorityId> {
        InvitationContextSync::new(self)
            .context_peers(effects, context_id)
            .await
    }

    /// Ask `peer` for the context-sync facts of `context_id` this authority
    /// lacks.
    pub(crate) async fn request_context_sync(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<()> {
        InvitationContextSync::new(self)
            .request(effects, context_id, peer)
            .await
    }
}
