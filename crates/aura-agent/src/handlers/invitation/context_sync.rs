//! Membership-checked relational-context sync for home, channel and DM
//! contexts (docs/111 §11.4).
//!
//! A member pulls the context-sync facts it lacks from a peer that counts it
//! as a member of the context. Requests stay bounded however long the
//! context's history is: the requester sends a fixed-size summary (a hash of
//! its fact digests in each of [`DIGEST_BUCKETS`] buckets, split by digest
//! prefix); the peer names the buckets whose hashes differ from its own; the
//! requester then sends its digests in those buckets only, in ranged pages of
//! at most [`MAX_PAGE_DIGESTS`]; the peer answers each page with the facts in
//! its range the requester lacks. Equal histories cost one summary message.
//! Commit-time sends to peers only cut latency: this pull, run by the
//! runtime-owned periodic sync, is what makes a lost send (or a lost sync
//! message) converge on a later round.

use super::*;
use aura_journal::fact::RelationalFact;
use aura_protocol::amp::AmpJournalEffects;
use std::collections::BTreeSet;

/// Context sync message content type.
pub(super) const CONTEXT_SYNC_CONTENT_TYPE: &str = "application/aura-context-sync";

/// Number of digest buckets a summary covers (the digest's top four bits).
const DIGEST_BUCKETS: usize = 16;

/// Hard cap on the fact digests one page carries; a page above it is refused.
pub(super) const MAX_PAGE_DIGESTS: usize = 256;

/// One context-sync message between members of `context_id`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) enum ContextSyncMessage {
    /// Requester to peer: the hash of the requester's digests per bucket.
    Summary {
        context_id: ContextId,
        bucket_hashes: Vec<Hash32>,
    },
    /// Peer to requester: the buckets whose hashes differ from the peer's.
    Differing {
        context_id: ContextId,
        buckets: BTreeSet<u8>,
    },
    /// Requester to peer: the requester's digests in one range of a bucket.
    Page {
        context_id: ContextId,
        page: DigestPage,
    },
}

impl ContextSyncMessage {
    fn context_id(&self) -> ContextId {
        match self {
            Self::Summary { context_id, .. }
            | Self::Differing { context_id, .. }
            | Self::Page { context_id, .. } => *context_id,
        }
    }
}

/// The requester's digests in the range `(after, upto]` of `bucket` (an
/// absent bound is the bucket's edge). The pages of a bucket cover it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct DigestPage {
    bucket: u8,
    after: Option<Hash32>,
    upto: Option<Hash32>,
    held: BTreeSet<Hash32>,
}

impl DigestPage {
    /// Whether a fact with `digest` is in this page's range and missing
    /// from the requester.
    fn wants(&self, digest: &Hash32) -> bool {
        digest_bucket(digest) == self.bucket
            && self.after.map_or(true, |after| *digest > after)
            && self.upto.map_or(true, |upto| *digest <= upto)
            && !self.held.contains(digest)
    }

    fn within_cap(&self) -> bool {
        usize::from(self.bucket) < DIGEST_BUCKETS && self.held.len() <= MAX_PAGE_DIGESTS
    }
}

fn digest_bucket(digest: &Hash32) -> u8 {
    digest.as_bytes()[0] >> 4
}

/// Hash of the sorted digests in each bucket.
fn bucket_hashes(digests: &BTreeSet<Hash32>) -> Vec<Hash32> {
    let mut buckets = vec![Vec::new(); DIGEST_BUCKETS];
    for digest in digests {
        buckets[usize::from(digest_bucket(digest))].extend_from_slice(digest.as_bytes());
    }
    buckets
        .iter()
        .map(|bytes| Hash32::from_bytes(bytes))
        .collect()
}

/// Buckets whose hash in `remote` differs from this side's (none for a
/// malformed summary).
fn differing_buckets(local: &BTreeSet<Hash32>, remote: &[Hash32]) -> BTreeSet<u8> {
    if remote.len() != DIGEST_BUCKETS {
        return BTreeSet::new();
    }
    bucket_hashes(local)
        .iter()
        .zip(remote)
        .zip(0u8..)
        .filter(|((local, remote), _)| local != remote)
        .map(|(_, bucket)| bucket)
        .collect()
}

/// Pages of at most [`MAX_PAGE_DIGESTS`] digests covering `bucket`.
fn bucket_pages(held: &BTreeSet<Hash32>, bucket: u8) -> Vec<DigestPage> {
    let in_bucket: Vec<Hash32> = held
        .iter()
        .filter(|digest| digest_bucket(digest) == bucket)
        .copied()
        .collect();
    if in_bucket.is_empty() {
        return vec![DigestPage {
            bucket,
            after: None,
            upto: None,
            held: BTreeSet::new(),
        }];
    }
    let chunks: Vec<&[Hash32]> = in_bucket.chunks(MAX_PAGE_DIGESTS).collect();
    let last = chunks.len() - 1;
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| DigestPage {
            bucket,
            after: index
                .checked_sub(1)
                .and_then(|prev| chunks[prev].last().copied()),
            upto: (index != last).then(|| chunk.last().copied()).flatten(),
            held: chunk.iter().copied().collect(),
        })
        .collect()
}

pub(super) struct InvitationContextSync<'a> {
    handler: &'a InvitationHandler,
}

/// Whether a home-context fact is governance state a joining member needs
/// to evaluate moderation and access: creation (the creator's moderator
/// designation), member admissions, moderator grants/revokes, access
/// overrides, the capability config, and membership episodes (joins and
/// leaves), so every member learns of members who joined after it and a
/// joiner learns of the members before it.
pub(super) fn is_home_governance_envelope(
    envelope: &aura_core::types::facts::FactEnvelope,
) -> bool {
    use aura_social::moderation::facts::{
        HOME_ADMIT_MEMBER_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID,
        HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    };
    match envelope.type_id.as_str() {
        HOME_ADMIT_MEMBER_FACT_TYPE_ID
        | HOME_GRANT_MODERATOR_FACT_TYPE_ID
        | HOME_REVOKE_MODERATOR_FACT_TYPE_ID => true,
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
        || is_channel_epoch_commit_envelope(envelope)
}

fn is_channel_epoch_commit_envelope(envelope: &aura_core::types::facts::FactEnvelope) -> bool {
    envelope.type_id.as_str() == aura_amp::CHANNEL_EPOCH_COMMIT_FACT_TYPE_ID
}

/// The author a peer fact claims, for kinds that name one: a moderation
/// fact's actor, or the writer of an AMP channel membership event. The
/// envelope receipt binds the sender, so a receiver requires the claimed
/// author to be the sender (and a member relaying another member's event, or
/// forging one in their name, is refused).
pub(super) fn claimed_fact_author(
    envelope: &aura_core::types::facts::FactEnvelope,
) -> Option<AuthorityId> {
    use aura_journal::DomainFact;
    aura_social::moderation::facts::claimed_moderation_actor(envelope).or_else(|| {
        is_channel_membership_envelope(envelope)
            .then(|| aura_amp::ChannelMembershipFact::from_envelope(envelope))
            .flatten()
            .map(|membership| membership.author())
    })
}

/// Why the author of an AMP membership event written for another participant
/// has no standing here, or `None` when it has (or the fact is not such an
/// event). A join must come from an admitted member (the inviter of an
/// accepted invitation, or the creator adding a direct-chat peer); a
/// departure (a kick) must come from a moderator of the context's home with
/// the kick capability. An event refused now is offered again by a later
/// sync round, so standing that arrives later still converges.
pub(super) async fn membership_standing_refusal(
    effects: &AuraEffectSystem,
    envelope: &aura_core::types::facts::FactEnvelope,
) -> AgentResult<Option<crate::reactive::MessageDropReason>> {
    use crate::reactive::MessageDropReason;
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_journal::DomainFact;
    if !is_channel_membership_envelope(envelope) {
        return Ok(None);
    }
    let Some(membership) = aura_amp::ChannelMembershipFact::from_envelope(envelope) else {
        return Ok(None);
    };
    let author = membership.author();
    if author == membership.participant() {
        return Ok(None);
    }
    let refusal = || MessageDropReason::MembershipAuthorWithoutStanding {
        author,
        participant: membership.participant(),
    };
    let observations = aura_amp::channel_membership_observations(
        effects,
        membership.context(),
        membership.channel(),
    )
    .await
    .map_err(|error| AgentError::effects(error.to_string()))?;
    if !observations.has_standing(author) {
        return Ok(Some(refusal()));
    }
    if matches!(membership.event(), aura_amp::ChannelParticipantEvent::Left) {
        let Ok(homes) = effects
            .reactive_handler()
            .read(&*aura_app::signal_defs::HOMES_SIGNAL)
            .await
        else {
            return Ok(Some(MessageDropReason::HomesUnavailable));
        };
        let moderates = crate::reactive::app_signal_projection::collect_moderation_homes(
            &homes,
            membership.context(),
            membership.channel(),
        )
        .iter()
        .any(|home| home.actor_may_moderate(&author, "moderate:kick"));
        if !moderates {
            return Ok(Some(refusal()));
        }
    }
    Ok(None)
}

/// Whether `own_authority` may serve `envelope` to a syncing member: a fact
/// claiming an author is served only by that author.
fn may_serve_context_fact(
    envelope: &aura_core::types::facts::FactEnvelope,
    own_authority: AuthorityId,
) -> bool {
    if let Some(author) = claimed_fact_author(envelope) {
        return author == own_authority;
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

    /// Digests of the context-sync facts of `context_id` held here.
    async fn held_digests(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
    ) -> AgentResult<BTreeSet<Hash32>> {
        self.context_facts(effects, context_id)
            .await?
            .iter()
            .map(context_fact_digest)
            .collect()
    }

    /// Ask `peer` for the context-sync facts of `context_id` this authority
    /// lacks (pull side): send the fixed-size bucket summary.
    pub(super) async fn request(
        &self,
        effects: &AuraEffectSystem,
        context_id: ContextId,
        peer: AuthorityId,
    ) -> AgentResult<()> {
        let held = self.held_digests(effects, context_id).await?;
        self.send_message(
            effects,
            peer,
            &ContextSyncMessage::Summary {
                context_id,
                bucket_hashes: bucket_hashes(&held),
            },
        )
        .await
    }

    /// Handle a verified context-sync message from `peer`, who must be a
    /// member of the context here. A summary is answered with the differing
    /// buckets, differing buckets with this side's digest pages, and a page
    /// with the facts in its range the peer lacks and this authority may
    /// serve. Delivery is best-effort; the next round repairs any gap.
    pub(super) async fn handle(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        message: ContextSyncMessage,
    ) -> AgentResult<()> {
        let context_id = message.context_id();
        if !self
            .context_peers(effects, context_id)
            .await
            .contains(&peer)
        {
            tracing::debug!(
                peer = %peer,
                context = %context_id,
                "Ignored context sync message from a non-member"
            );
            return Ok(());
        }
        match message {
            ContextSyncMessage::Summary { bucket_hashes, .. } => {
                let held = self.held_digests(effects, context_id).await?;
                let buckets = differing_buckets(&held, &bucket_hashes);
                if buckets.is_empty() {
                    return Ok(());
                }
                self.send_message(
                    effects,
                    peer,
                    &ContextSyncMessage::Differing {
                        context_id,
                        buckets,
                    },
                )
                .await
            }
            ContextSyncMessage::Differing { buckets, .. } => {
                let held = self.held_digests(effects, context_id).await?;
                for bucket in buckets
                    .into_iter()
                    .filter(|bucket| usize::from(*bucket) < DIGEST_BUCKETS)
                {
                    for page in bucket_pages(&held, bucket) {
                        self.send_message(
                            effects,
                            peer,
                            &ContextSyncMessage::Page { context_id, page },
                        )
                        .await?;
                    }
                }
                Ok(())
            }
            ContextSyncMessage::Page { page, .. } => {
                if !page.within_cap() {
                    tracing::debug!(
                        peer = %peer,
                        context = %context_id,
                        held = page.held.len(),
                        "Ignored context sync page over the digest cap"
                    );
                    return Ok(());
                }
                let own = self.own_authority();
                let served = self
                    .send_context_facts(effects, context_id, peer, |fact, envelope| {
                        may_serve_context_fact(envelope, own)
                            && context_fact_digest(fact).is_ok_and(|digest| page.wants(&digest))
                    })
                    .await?;
                tracing::debug!(
                    peer = %peer,
                    context = %context_id,
                    bucket = page.bucket,
                    held = page.held.len(),
                    served,
                    "Served context sync page"
                );
                Ok(())
            }
        }
    }

    async fn send_message(
        &self,
        effects: &AuraEffectSystem,
        peer: AuthorityId,
        message: &ContextSyncMessage,
    ) -> AgentResult<()> {
        let payload = aura_core::util::serialization::to_vec(message)
            .map_err(|error| AgentError::internal(error.to_string()))?;
        self.send(
            effects,
            peer,
            payload,
            CONTEXT_SYNC_CONTENT_TYPE,
            message.context_id(),
            "context sync message send failed",
        )
        .await
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
    /// journal, which the authoritative participant set reduces; a synced
    /// epoch commit enters it as the committed bump and its consensus
    /// evidence (it was verified on intake).
    pub(super) async fn admit_membership_fact(
        effects: &AuraEffectSystem,
        fact: &RelationalFact,
    ) -> AgentResult<()> {
        let RelationalFact::Generic { envelope, .. } = fact else {
            return Ok(());
        };
        if is_channel_membership_envelope(envelope) {
            return effects
                .insert_relational_fact(fact.clone())
                .await
                .map_err(|error| AgentError::effects(error.to_string()));
        }
        if is_channel_epoch_commit_envelope(envelope) {
            use aura_journal::DomainFact;
            let Some(commit) = aura_amp::ChannelEpochCommitFact::from_envelope(envelope) else {
                return Ok(());
            };
            effects
                .insert_relational_fact(commit.committed_bump_fact())
                .await
                .map_err(|error| AgentError::effects(error.to_string()))?;
        }
        Ok(())
    }

    /// Why a synced epoch commit is refused: it must verify against the
    /// new epoch's group key this member holds from its own key ceremony
    /// (a member not on the new roster holds none and refuses it).
    pub(super) async fn epoch_commit_refusal(
        effects: &AuraEffectSystem,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<String> {
        use aura_journal::DomainFact;
        if !is_channel_epoch_commit_envelope(envelope) {
            return None;
        }
        let Some(commit) = aura_amp::ChannelEpochCommitFact::from_envelope(envelope) else {
            return Some("undecodable channel epoch commit".to_string());
        };
        let scope = crate::runtime::context_dkg::ChannelKeyScope {
            context: commit.context(),
            channel: commit.channel(),
        };
        let trusted = match crate::runtime::context_dkg::load_roster(
            effects,
            scope,
            commit.committed().new_epoch,
        )
        .await
        {
            Ok((_, public)) => aura_core::crypto::tree_signing::PublicKeyPackage::from(public),
            Err(error) => return Some(format!("no key for the committed epoch: {error}")),
        };
        commit
            .verify_with(&trusted)
            .err()
            .map(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn digests(range: std::ops::Range<u32>) -> BTreeSet<Hash32> {
        range
            .map(|index| Hash32::from_bytes(&index.to_le_bytes()))
            .collect()
    }

    fn encoded_len(message: &ContextSyncMessage) -> usize {
        aura_core::util::serialization::to_vec(message)
            .expect("context sync message encodes")
            .len()
    }

    /// One pull round of `requester` against `server`, returning the facts
    /// served, each delivered unless `drop` says it is lost. Asserts every
    /// requester message stays within the per-message bound.
    fn pull_round(
        context_id: ContextId,
        requester: &BTreeSet<Hash32>,
        server: &BTreeSet<Hash32>,
        max_message_len: usize,
        drop: impl Fn(&Hash32) -> bool,
    ) -> (BTreeSet<Hash32>, usize) {
        let summary = ContextSyncMessage::Summary {
            context_id,
            bucket_hashes: bucket_hashes(requester),
        };
        assert!(encoded_len(&summary) <= max_message_len);
        let ContextSyncMessage::Summary { bucket_hashes, .. } = summary else {
            unreachable!()
        };
        let mut delivered = BTreeSet::new();
        let mut pages = 0;
        for bucket in differing_buckets(server, &bucket_hashes) {
            for page in bucket_pages(requester, bucket) {
                assert!(page.within_cap());
                let message = ContextSyncMessage::Page { context_id, page };
                assert!(encoded_len(&message) <= max_message_len);
                let ContextSyncMessage::Page { page, .. } = message else {
                    unreachable!()
                };
                pages += 1;
                delivered.extend(
                    server
                        .iter()
                        .filter(|digest| page.wants(digest) && !drop(digest))
                        .copied(),
                );
            }
        }
        (delivered, pages)
    }

    // Task 136: with a long history, every context-sync message stays under
    // a fixed bound, equal histories cost no pages, and a member missing
    // facts converges even when a round's deliveries are dropped.
    #[test]
    fn large_history_sync_messages_are_bounded_and_converge_after_drops() {
        let context_id = ContextId::new_from_entropy([42u8; 32]);
        let server = digests(0..20_000);
        // Bound: a page of MAX_PAGE_DIGESTS digests plus framing.
        let max_message_len = MAX_PAGE_DIGESTS * 70 + 512;
        // The summary is DIGEST_BUCKETS hashes whatever the history length.
        let summary_bound = DIGEST_BUCKETS * 70 + 128;
        for history in [digests(0..3), server.clone()] {
            let summary = ContextSyncMessage::Summary {
                context_id,
                bucket_hashes: bucket_hashes(&history),
            };
            assert!(encoded_len(&summary) <= summary_bound);
        }

        let (served, pages) = pull_round(context_id, &server, &server, max_message_len, |_| false);
        assert!(
            served.is_empty() && pages == 0,
            "equal histories exchange no pages"
        );

        // The requester lacks every 97th fact and holds a few the server
        // does not.
        let mut requester: BTreeSet<Hash32> = server
            .iter()
            .enumerate()
            .filter(|(index, _)| index % 97 != 0)
            .map(|(_, digest)| *digest)
            .collect();
        requester.extend(digests(50_000..50_010));
        let missing: BTreeSet<Hash32> = server.difference(&requester).copied().collect();
        assert!(!missing.is_empty());

        // First round: every other served fact is dropped in transit.
        let (served, _) = pull_round(context_id, &requester, &server, max_message_len, |digest| {
            digest.as_bytes()[31] % 2 == 0
        });
        assert!(served.is_subset(&missing));
        assert!(served.len() < missing.len(), "the drop lost some facts");
        requester.extend(served);

        // Later rounds repair the gap.
        let (served, _) = pull_round(context_id, &requester, &server, max_message_len, |_| false);
        requester.extend(served);
        assert!(server.is_subset(&requester), "the requester converged");
        let (served, _) = pull_round(context_id, &requester, &server, max_message_len, |_| false);
        assert!(served.is_empty());
    }

    #[test]
    fn pages_cover_a_bucket_and_respect_the_cap() {
        let held = digests(0..10_000);
        for bucket in 0..DIGEST_BUCKETS as u8 {
            let pages = bucket_pages(&held, bucket);
            assert!(pages.iter().all(DigestPage::within_cap));
            let covered: BTreeSet<Hash32> = pages
                .iter()
                .flat_map(|page| page.held.iter().copied())
                .collect();
            let in_bucket: BTreeSet<Hash32> = held
                .iter()
                .filter(|digest| digest_bucket(digest) == bucket)
                .copied()
                .collect();
            assert_eq!(covered, in_bucket);
            // Each digest of the bucket falls in exactly one page's range.
            for digest in digests(20_000..21_000)
                .iter()
                .filter(|digest| digest_bucket(digest) == bucket)
            {
                assert_eq!(pages.iter().filter(|page| page.wants(digest)).count(), 1);
            }
        }
        let oversized = DigestPage {
            bucket: 0,
            after: None,
            upto: None,
            held: digests(0..(MAX_PAGE_DIGESTS as u32 + 1)),
        };
        assert!(!oversized.within_cap());
    }
}
