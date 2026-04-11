use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct MessageSendReadiness {
    pub(super) recipient_resolution_ready: bool,
    pub(super) delivery_ready: bool,
}

#[derive(Debug, Clone)]
pub(super) struct ChannelReadinessState {
    channel_id: ChannelId,
    fact_key: ChannelFactKey,
    authoritative_channel: Option<AuthoritativeChannelRef>,
    pub(super) member_count: u32,
    pub(super) recipients: Vec<AuthorityId>,
    pub(super) delivery_supported: bool,
    had_membership_fact: bool,
}

impl ChannelReadinessState {
    pub(super) fn new(
        channel_id: ChannelId,
        fact_key: ChannelFactKey,
        member_count: u32,
        authoritative_channel: Option<AuthoritativeChannelRef>,
        recipients: Vec<AuthorityId>,
        had_membership_fact: bool,
    ) -> Self {
        let is_note_to_self = matches!(
            fact_key.name.as_deref(),
            Some(name) if name.eq_ignore_ascii_case("note to self")
        );
        let member_count = member_count.max((recipients.len() as u32).saturating_add(1));
        let delivery_supported = !is_note_to_self && !recipients.is_empty();
        Self {
            channel_id,
            fact_key,
            authoritative_channel,
            member_count,
            recipients,
            delivery_supported,
            had_membership_fact,
        }
    }

    fn membership_fact(&self) -> AuthoritativeSemanticFact {
        AuthoritativeSemanticFact::ChannelMembershipReady {
            channel: self.fact_key.clone(),
            member_count: self.member_count,
        }
    }

    fn recipient_resolution_fact(&self) -> Option<AuthoritativeSemanticFact> {
        self.delivery_supported
            .then(|| AuthoritativeSemanticFact::RecipientPeersResolved {
                channel: self.fact_key.clone(),
                member_count: self.member_count,
            })
    }

    pub(super) fn delivery_facts(
        &self,
        context_id: ContextId,
        ready_peers: &[AuthorityId],
    ) -> (
        Vec<AuthoritativeSemanticFact>,
        Option<AuthoritativeSemanticFact>,
    ) {
        let peer_facts = ready_peers
            .iter()
            .map(|peer| AuthoritativeSemanticFact::PeerChannelReady {
                channel: self.fact_key.clone(),
                peer_authority_id: peer.to_string(),
                context_id: Some(context_id.to_string()),
            })
            .collect::<Vec<_>>();
        let delivery_fact = (self.delivery_supported && ready_peers.len() == self.recipients.len())
            .then(|| AuthoritativeSemanticFact::MessageDeliveryReady {
                channel: self.fact_key.clone(),
                member_count: self.member_count,
            });
        (peer_facts, delivery_fact)
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct ChannelReadinessCoordinator {
    states: Vec<ChannelReadinessState>,
}

#[derive(Debug, Clone)]
struct ChannelReadinessSeed {
    fact_key: ChannelFactKey,
    member_count: u32,
    authoritative_context: Option<ContextId>,
    had_membership_fact: bool,
}

impl ChannelReadinessSeed {
    fn from_fact(channel: ChannelFactKey, member_count: u32) -> Self {
        Self {
            fact_key: channel,
            member_count,
            authoritative_context: None,
            had_membership_fact: true,
        }
    }

    fn merge_fact(&mut self, channel: ChannelFactKey, member_count: u32) {
        if self
            .fact_key
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
        {
            self.fact_key.name = channel.name;
        }
        self.member_count = self.member_count.max(member_count);
    }

    fn merge_observed_channel(&mut self, channel: &Channel) {
        if self
            .fact_key
            .name
            .as_deref()
            .is_none_or(|name| name.trim().is_empty())
            && !channel.name.trim().is_empty()
        {
            self.fact_key.name = Some(channel.name.clone());
        }
        self.member_count = self.member_count.max(
            channel
                .member_count
                .max(channel.member_ids.len() as u32 + 1),
        );
        self.authoritative_context = self.authoritative_context.or(channel.context_id);
    }
}

impl ChannelReadinessCoordinator {
    pub(super) async fn load(
        app_core: &Arc<RwLock<AppCore>>,
        resolve_recipients: bool,
    ) -> Result<Self, AuraError> {
        // OWNERSHIP: fact-backed - authoritative semantic facts drive readiness;
        // observed chat only enriches already-materialized channel metadata.
        let facts = authoritative_semantic_facts_snapshot(app_core).await?;
        let observed_chat = observed_chat_snapshot(app_core).await;
        let (runtime, self_authority) = {
            let core = app_core.read().await;
            (core.runtime().cloned(), core.authority().copied())
        };
        let self_authority =
            self_authority.or_else(|| runtime.as_ref().map(|runtime| runtime.authority_id()));
        let mut seeds = BTreeMap::<ChannelId, ChannelReadinessSeed>::new();
        for fact in facts {
            let AuthoritativeSemanticFact::ChannelMembershipReady {
                channel,
                member_count,
            } = fact
            else {
                continue;
            };
            let channel_id = channel
                .id
                .as_deref()
                .ok_or_else(|| {
                    AuraError::invalid(
                        "ChannelMembershipReady facts must carry a canonical channel id",
                    )
                })?
                .parse::<ChannelId>()
                .map_err(|error| {
                    AuraError::invalid(format!(
                        "ChannelMembershipReady fact carried an invalid channel id: {error}"
                    ))
                })?;
            match seeds.get_mut(&channel_id) {
                Some(seed) => seed.merge_fact(channel, member_count),
                None => {
                    seeds.insert(
                        channel_id,
                        ChannelReadinessSeed::from_fact(channel, member_count),
                    );
                }
            }
        }

        for channel in observed_chat.all_channels() {
            let Some(context_id) = channel.context_id else {
                continue;
            };
            let seed = seeds
                .entry(channel.id)
                .or_insert_with(|| ChannelReadinessSeed {
                    fact_key: ChannelFactKey {
                        id: Some(channel.id.to_string()),
                        name: Some(channel.name.clone()),
                    },
                    member_count: channel
                        .member_count
                        .max(channel.member_ids.len() as u32 + 1),
                    authoritative_context: Some(context_id),
                    had_membership_fact: false,
                });
            seed.merge_observed_channel(channel);
        }

        let mut states = Vec::new();
        for (channel_id, seed) in seeds {
            let authoritative_channel = if runtime.is_some() {
                resolve_authoritative_context_id_for_channel(app_core, channel_id)
                    .await
                    .or(seed.authoritative_context)
                    .map(|context_id| AuthoritativeChannelRef::new(channel_id, context_id))
            } else {
                seed.authoritative_context
                    .map(|context_id| AuthoritativeChannelRef::new(channel_id, context_id))
            };
            let recipients = match (
                resolve_recipients,
                &runtime,
                self_authority,
                authoritative_channel,
            ) {
                (true, Some(runtime), Some(authority_id), Some(authoritative_channel)) => {
                    authoritative_recipient_peers_for_channel(
                        runtime,
                        authoritative_channel,
                        authority_id,
                    )
                    .await?
                }
                _ => Vec::new(),
            };
            states.push(ChannelReadinessState::new(
                channel_id,
                seed.fact_key,
                seed.member_count,
                authoritative_channel,
                recipients,
                seed.had_membership_fact,
            ));
        }

        Ok(Self { states })
    }

    fn states(&self) -> &[ChannelReadinessState] {
        &self.states
    }

    pub(super) fn state_for_channel(
        &self,
        channel_id: ChannelId,
    ) -> Option<&ChannelReadinessState> {
        self.states
            .iter()
            .find(|state| state.channel_id == channel_id)
    }
}

fn channel_membership_fact(
    channel_id: ChannelId,
    channel_name: Option<&str>,
    member_count: u32,
) -> AuthoritativeSemanticFact {
    AuthoritativeSemanticFact::ChannelMembershipReady {
        channel: ChannelFactKey {
            id: Some(channel_id.to_string()),
            name: channel_name.map(ToOwned::to_owned),
        },
        member_count,
    }
}

#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "semantic_readiness",
    family = "authorizer"
)]
pub(in crate::workflows) async fn publish_authoritative_channel_membership_ready(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    channel_name: Option<&str>,
    member_count: u32,
) -> Result<(), AuraError> {
    publish_authoritative_semantic_fact(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            channel_membership_fact(channel_id, channel_name, member_count),
        ),
    )
    .await
}

pub(crate) async fn publish_message_committed_fact(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    channel_name: &str,
    content: &str,
) -> Result<(), AuraError> {
    let message_committed = AuthoritativeSemanticFact::MessageCommitted {
        channel: ChannelFactKey {
            id: Some(channel_id.to_string()),
            name: Some(channel_name.to_string()),
        },
        content: content.to_string(),
    };
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|existing| existing.key() != message_committed.key());
        facts.push(message_committed);
    })
    .await
}

pub(crate) async fn clear_authoritative_channel_readiness_facts(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|fact| !fact_matches_channel(fact, channel_id));
    })
    .await
}

pub(crate) fn authoritative_send_readiness_for_channel(
    facts: &[AuthoritativeSemanticFact],
    channel: AuthoritativeChannelRef,
) -> MessageSendReadiness {
    let channel_id = channel.channel_id().to_string();
    let mut readiness = MessageSendReadiness::default();
    for fact in facts {
        match fact {
            AuthoritativeSemanticFact::RecipientPeersResolved { channel, .. }
                if channel.id.as_deref() == Some(channel_id.as_str()) =>
            {
                readiness.recipient_resolution_ready = true;
            }
            AuthoritativeSemanticFact::MessageDeliveryReady { channel, .. }
                if channel.id.as_deref() == Some(channel_id.as_str()) =>
            {
                readiness.delivery_ready = true;
            }
            _ => {}
        }
    }
    readiness
}

pub(crate) async fn publish_send_message_failure(
    owner: &SemanticWorkflowOwner,
    error: &SendMessageError,
) -> Result<(), AuraError> {
    owner.publish_failure(error.semantic_error()).await
}

pub(crate) async fn require_send_message_readiness(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
) -> Result<MessageSendReadiness, SendMessageError> {
    let facts = authoritative_semantic_facts_snapshot(app_core)
        .await
        .map_err(|error| SendMessageError::ReadinessFactsUnavailable {
            detail: error.to_string(),
        })?;
    let channel_id = channel.channel_id();
    let readiness = authoritative_send_readiness_for_channel(&facts, channel);
    if !readiness.recipient_resolution_ready {
        return Err(SendMessageError::RecipientResolutionNotReady { channel_id });
    }
    if !readiness.delivery_ready {
        return Err(SendMessageError::DeliveryNotReady { channel_id });
    }
    Ok(readiness)
}

pub(crate) fn bootstrap_required_for_recipients(recipient_count: usize) -> bool {
    recipient_count > 0
}

pub(crate) async fn ensure_runtime_note_to_self_channel(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    authority_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    let context_id = note_to_self_context_id(authority_id);
    let channel_id = note_to_self_channel_id(authority_id);

    let create_result = timeout_runtime_call(
        runtime,
        "ensure_runtime_note_to_self_channel",
        "amp_create_channel",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.amp_create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: Some(NOTE_TO_SELF_CHANNEL_TOPIC.to_string()),
            })
        },
    )
    .await;

    let created_now = match create_result {
        Ok(_) => true,
        Err(error) if classify_amp_channel_error(&error) == AmpChannelErrorClass::AlreadyExists => {
            false
        }
        Err(error) => {
            return Err(
                super::super::error::runtime_call("create note-to-self channel", error).into(),
            );
        }
    };

    if let Err(error) = timeout_runtime_call(
        runtime,
        "ensure_runtime_note_to_self_channel",
        "amp_join_channel",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: authority_id,
            })
        },
    )
    .await
    {
        if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
            return Err(
                super::super::error::runtime_call("join note-to-self channel", error).into(),
            );
        }
    }

    if created_now {
        let fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            NOTE_TO_SELF_CHANNEL_NAME.to_string(),
            Some(NOTE_TO_SELF_CHANNEL_TOPIC.to_string()),
            false,
            timestamp_ms,
            authority_id,
        )
        .to_generic();

        timeout_runtime_call(
            runtime,
            "ensure_runtime_note_to_self_channel",
            "commit_relational_facts",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
        )
        .await
        .map_err(|e| super::super::error::runtime_call("persist note-to-self channel", e))?
        .map_err(|e| super::super::error::runtime_call("persist note-to-self channel", e))?;
    }

    Ok(channel_id)
}

pub(in crate::workflows) async fn refresh_authoritative_channel_membership_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core, false).await?;
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let mut replacements = Vec::new();
    for state in coordinator.states() {
        let membership_ready = if let Some(runtime) = runtime.as_ref() {
            if let Some(channel) = state.authoritative_channel {
                match runtime_channel_state_exists(runtime, channel).await {
                    Ok(true) => true,
                    Ok(false) if state.had_membership_fact => {
                        messaging_warn!(
                            "Retaining ChannelMembershipReady for {} after transient runtime-state miss; authoritative leave/close owns revocation",
                            state.channel_id
                        );
                        true
                    }
                    Ok(false) => false,
                    Err(_error) if state.had_membership_fact => {
                        messaging_warn!(
                            "Retaining ChannelMembershipReady for {} after runtime-state probe error; authoritative leave/close owns revocation",
                            state.channel_id
                        );
                        true
                    }
                    Err(error) => return Err(error),
                }
            } else {
                messaging_warn!(
                    "Retaining ChannelMembershipReady for {} without a re-resolved authoritative context; see docs/122_ownership_model.md",
                    state.channel_id
                );
                true
            }
        } else {
            true
        };
        if membership_ready {
            replacements.push(state.membership_fact());
        }
    }
    let kind = AuthoritativeSemanticFactKind::ChannelMembershipReady;
    let mut merged = BTreeMap::new();
    for replacement in replacements {
        merged.insert(replacement.key(), replacement);
    }
    update_authoritative_semantic_facts(app_core, move |facts| {
        for existing in facts.iter().filter(|existing| existing.kind() == kind) {
            merged
                .entry(existing.key())
                .or_insert_with(|| existing.clone());
        }
        facts.retain(|existing| existing.kind() != kind);
        facts.extend(merged.values().cloned());
    })
    .await
}

pub(in crate::workflows) async fn refresh_authoritative_recipient_resolution_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core, true).await?;
    let replacements = coordinator
        .states()
        .iter()
        .filter_map(ChannelReadinessState::recipient_resolution_fact)
        .collect::<Vec<_>>();
    let kind = AuthoritativeSemanticFactKind::RecipientPeersResolved;
    let mut merged = BTreeMap::new();
    for replacement in replacements {
        merged.insert(replacement.key(), replacement);
    }
    update_authoritative_semantic_facts(app_core, move |facts| {
        for existing in facts.iter().filter(|existing| existing.kind() == kind) {
            merged
                .entry(existing.key())
                .or_insert_with(|| existing.clone());
        }
        facts.retain(|existing| existing.kind() != kind);
        facts.extend(merged.values().cloned());
    })
    .await
}

pub(crate) fn fact_matches_channel(
    fact: &AuthoritativeSemanticFact,
    channel_id: ChannelId,
) -> bool {
    let channel_id = channel_id.to_string();
    match fact {
        AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
        | AuthoritativeSemanticFact::RecipientPeersResolved { channel, .. }
        | AuthoritativeSemanticFact::MessageCommitted { channel, .. }
        | AuthoritativeSemanticFact::MessageDeliveryReady { channel, .. } => {
            channel.id.as_deref() == Some(channel_id.as_str())
        }
        AuthoritativeSemanticFact::PeerChannelReady { channel, .. } => {
            channel.id.as_deref() == Some(channel_id.as_str())
        }
        AuthoritativeSemanticFact::OperationStatus { .. }
        | AuthoritativeSemanticFact::InvitationAccepted { .. }
        | AuthoritativeSemanticFact::ContactLinkReady { .. }
        | AuthoritativeSemanticFact::PendingHomeInvitationReady => false,
    }
}

pub(crate) async fn refresh_authoritative_delivery_readiness_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core, true).await?;
    let channel_id = channel.channel_id();
    let Some(channel_state) = coordinator.state_for_channel(channel_id).cloned() else {
        return update_authoritative_semantic_facts(app_core, |facts| {
            facts.retain(|fact| {
                !matches!(
                    fact,
                    AuthoritativeSemanticFact::PeerChannelReady { .. }
                        | AuthoritativeSemanticFact::MessageDeliveryReady { .. }
                ) || !fact_matches_channel(fact, channel_id)
            });
        })
        .await;
    };

    let mut ready_peers = Vec::new();
    if channel_state.delivery_supported {
        for peer in channel_state.recipients.iter().copied() {
            if timeout_runtime_call(
                runtime,
                "refresh_authoritative_delivery_readiness_for_channel",
                "ensure_peer_channel",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.ensure_peer_channel(channel.context_id(), peer),
            )
            .await
            .is_ok()
            {
                ready_peers.push(peer);
            }
        }
    }
    let (peer_facts, delivery_fact) =
        channel_state.delivery_facts(channel.context_id(), &ready_peers);

    update_authoritative_semantic_facts(app_core, |facts| {
        facts.retain(|fact| {
            !matches!(
                fact,
                AuthoritativeSemanticFact::PeerChannelReady { .. }
                    | AuthoritativeSemanticFact::MessageDeliveryReady { .. }
            ) || !fact_matches_channel(fact, channel_id)
        });
        facts.extend(peer_facts);
        if let Some(delivery_fact) = delivery_fact {
            facts.push(delivery_fact);
        }
    })
    .await
}

pub(super) fn channel_id_from_pending_channel_invitation(
    invitation: &InvitationInfo,
) -> Option<ChannelId> {
    match &invitation.invitation_type {
        InvitationBridgeType::Channel { home_id, .. } => home_id.parse().ok(),
        _ => None,
    }
}

pub(super) fn select_pending_channel_invitation(
    pending: &[InvitationInfo],
    local_authority: AuthorityId,
    requested_channel_id: ChannelId,
) -> Option<InvitationInfo> {
    let candidates: Vec<InvitationInfo> = pending
        .iter()
        .filter(|invitation| invitation.sender_id != local_authority)
        .filter(|invitation| channel_id_from_pending_channel_invitation(invitation).is_some())
        .cloned()
        .collect();

    if let Some(exact) = candidates.iter().find(|invitation| {
        channel_id_from_pending_channel_invitation(invitation) == Some(requested_channel_id)
    }) {
        return Some(exact.clone());
    }

    if candidates.len() == 1 {
        return candidates.first().cloned();
    }

    None
}

pub(crate) async fn try_join_via_pending_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    requested_channel_id: ChannelId,
) -> Result<bool, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let pending = timeout_runtime_call(
        &runtime,
        "try_join_via_pending_channel_invitation",
        "try_list_pending_invitations",
        MESSAGING_RUNTIME_QUERY_TIMEOUT,
        || runtime.try_list_pending_invitations(),
    )
    .await
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })?
    .map_err(|e| {
        AuraError::from(super::super::error::runtime_call(
            "list pending invitations",
            e,
        ))
    })?;
    let Some(invitation) =
        select_pending_channel_invitation(&pending, runtime.authority_id(), requested_channel_id)
    else {
        return Ok(false);
    };
    let invited_channel_id =
        channel_id_from_pending_channel_invitation(&invitation).ok_or_else(|| {
            AuraError::invalid("pending channel invitation missing invited channel id")
        })?;

    if let Err(error) = timeout_runtime_call(
        &runtime,
        "try_join_via_pending_channel_invitation",
        "accept_invitation",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || runtime.accept_invitation(invitation.invitation_id.as_str()),
    )
    .await
    .map_err(|error| {
        super::super::error::runtime_call("accept pending channel invitation", error)
    })? {
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return Err(super::super::error::runtime_call(
                "accept pending channel invitation",
                error,
            )
            .into());
        }
    }

    for _ in 0..4 {
        converge_runtime(&runtime).await;
        if ensure_runtime_peer_connectivity(&runtime, "accept_pending_channel_invitation")
            .await
            .is_ok()
        {
            break;
        }
    }

    if let Err(_e) = crate::workflows::system::refresh_account(app_core).await {
        #[cfg(feature = "instrumented")]
        tracing::debug!(error = %_e, "refresh_account after invitation accept failed");
    }

    let local_channel_id = invited_channel_id;
    let channel_name_hint = match &invitation.invitation_type {
        InvitationBridgeType::Channel {
            nickname_suggestion,
            ..
        } => nickname_suggestion.as_deref(),
        _ => None,
    };

    if let Ok(authoritative_channel) =
        require_authoritative_context_id_for_channel(app_core, local_channel_id)
            .await
            .map(|context_id| authoritative_channel_ref(local_channel_id, context_id))
    {
        let _ = apply_authoritative_membership_projection(
            app_core,
            local_channel_id,
            authoritative_channel.context_id(),
            true,
            channel_name_hint,
        )
        .await;
        if let Err(_e) = super::join_channel(app_core, authoritative_channel).await {
            #[cfg(feature = "instrumented")]
            tracing::debug!(
                error = %_e,
                channel_id = %local_channel_id,
                "best-effort join_channel after invitation accept failed"
            );
        }
        super::warm_channel_connectivity(app_core, &runtime, authoritative_channel).await;
    }
    Ok(true)
}
