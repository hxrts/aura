#![allow(missing_docs)]

use super::*;

/// Strong authoritative reference for parity-critical channel operations.
///
/// Parity-critical helpers must accept this typed reference instead of raw
/// `ChannelId` once authoritative context is known.
#[aura_macros::strong_reference(domain = "channel")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeChannelRef {
    channel_id: ChannelId,
    context_id: ContextId,
}

impl AuthoritativeChannelRef {
    #[must_use]
    pub(crate) fn new(channel_id: ChannelId, context_id: ContextId) -> Self {
        Self {
            channel_id,
            context_id,
        }
    }

    #[must_use]
    pub fn channel_id(self) -> ChannelId {
        self.channel_id
    }

    #[must_use]
    pub fn context_id(self) -> ContextId {
        self.context_id
    }
}

/// Authoritative channel identity returned by channel-creation workflows.
///
/// This bundle keeps the canonical `channel_id` and the authoritative
/// `context_id` together so frontend callers do not need to rediscover the
/// context immediately after create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatedChannel {
    pub channel_id: ChannelId,
    pub context_id: Option<ContextId>,
}

pub async fn current_home_channel_id(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<ChannelId, AuraError> {
    let homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
        .await
        .ok();

    if let Some(homes) = homes {
        if let Some(channel_id) = homes.current_home_id() {
            return Ok(*channel_id);
        }
    }

    channel_id_from_input("home")
}

pub async fn current_home_channel_ref(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<String, AuraError> {
    let channel_id = current_home_channel_id(app_core).await?;
    Ok(format!("home:{channel_id}"))
}

pub(crate) async fn context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    local_authority: Option<AuthorityId>,
) -> Result<ContextId, AuraError> {
    routing::context_id_for_channel(app_core, channel_id, local_authority).await
}

pub(crate) async fn next_observed_projection_timestamp_ms(app_core: &Arc<RwLock<AppCore>>) -> u64 {
    // OWNERSHIP: observed-display-update - this helper inspects observed chat
    // projections only to synthesize a monotone local timestamp for projection
    // repair; it does not authorize semantic decisions.
    let chat = observed_chat_snapshot(app_core).await;
    let channel_activity = chat
        .all_channels()
        .map(|channel| channel.last_activity)
        .max();
    let message_activity = chat
        .all_channels()
        .flat_map(|channel| chat.messages_for_channel(&channel.id).iter())
        .map(|message| message.timestamp)
        .max();

    channel_activity
        .into_iter()
        .chain(message_activity)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

pub(crate) async fn ensure_channel_visible_after_join(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: ContextId,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    // OWNERSHIP: observed-display-update - this routine repairs the observed
    // chat projection after an authoritative join succeeds, using observed
    // names only to preserve display continuity.
    let existing_name = observed_chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .map(|channel| channel.name.clone())
        .filter(|name| !name.trim().is_empty())
        .filter(|name| name != &channel_id.to_string());
    let normalized_name = name_hint
        .map(normalize_channel_name)
        .filter(|value| !value.is_empty())
        .or(existing_name)
        .ok_or_else(|| {
            AuraError::from(super::super::error::WorkflowError::Precondition(
                "authoritative join projection missing canonical channel name",
            ))
        })?;

    let placeholder_channel = {
        let chat = observed_chat_snapshot(app_core).await;
        let existing = chat
            .all_channels()
            .find(|channel| {
                channel.id != channel_id
                    && channel.name.eq_ignore_ascii_case(normalized_name.as_str())
            })
            .cloned();
        existing
    };
    if let Some(placeholder_channel) = placeholder_channel {
        let canonical_name = normalized_name.clone();
        update_chat_projection_observed(app_core, |chat| {
            let mut canonical = placeholder_channel.clone();
            canonical.id = channel_id;
            canonical.context_id = Some(context_id);
            canonical.name = canonical_name.clone();
            chat.rebind_channel_identity(&placeholder_channel.id, canonical);
        })
        .await?;
    }

    let updated_at_ms = next_observed_projection_timestamp_ms(app_core).await;
    reduce_chat_fact_observed(
        app_core,
        &ChatFact::channel_updated_ms(
            context_id,
            channel_id,
            Some(normalized_name),
            None,
            Some(1),
            None,
            updated_at_ms,
            AuthorityId::new_from_entropy([0u8; 32]),
        ),
    )
    .await
}

pub(in crate::workflows) async fn apply_authoritative_membership_projection(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: ContextId,
    joined: bool,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    // OWNERSHIP: observed-display-update - this helper mutates only observed
    // chat projection state after authoritative membership outcomes are known.
    if joined {
        ensure_channel_visible_after_join(app_core, channel_id, context_id, name_hint).await?;
        let chat = observed_chat_snapshot(app_core).await;
        if chat.channel(&channel_id).is_none() {
            return Err(super::super::error::WorkflowError::Precondition(
                "join projection missing canonical channel",
            )
            .into());
        }
        return Ok(());
    }

    let existing_name = observed_chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .map(|channel| channel.name.clone())
        .filter(|name| !name.trim().is_empty())
        .filter(|name| name != &channel_id.to_string());
    let canonical_name = name_hint
        .map(normalize_channel_name)
        .filter(|value| !value.is_empty())
        .or(existing_name)
        .ok_or_else(|| {
            AuraError::from(super::super::error::WorkflowError::Precondition(
                "authoritative membership projection missing canonical channel name",
            ))
        })?;
    let updated_at_ms = next_observed_projection_timestamp_ms(app_core).await;
    reduce_chat_fact_observed(
        app_core,
        &ChatFact::channel_updated_ms(
            context_id,
            channel_id,
            Some(canonical_name),
            None,
            Some(0),
            None,
            updated_at_ms,
            AuthorityId::new_from_entropy([0u8; 32]),
        ),
    )
    .await
}

pub async fn resolve_authoritative_context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Option<ContextId> {
    // OWNERSHIP: authoritative-source - prefer runtime authority; observed chat
    // is a bounded fallback for pre-existing canonical context materialization.
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    if let Some(runtime) = runtime {
        if let Ok(Ok(Some(context_id))) = timeout_runtime_call(
            &runtime,
            "resolve_authoritative_context_id_for_channel",
            "resolve_amp_channel_context",
            MESSAGING_RUNTIME_QUERY_TIMEOUT,
            || runtime.resolve_amp_channel_context(channel_id),
        )
        .await
        {
            return Some(context_id);
        }
    }
    observed_chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .and_then(|channel| channel.context_id)
}

#[must_use]
pub fn authoritative_channel_ref(
    channel_id: ChannelId,
    context_id: ContextId,
) -> AuthoritativeChannelRef {
    AuthoritativeChannelRef::new(channel_id, context_id)
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub async fn require_authoritative_context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<ContextId, AuraError> {
    resolve_authoritative_context_id_for_channel(app_core, channel_id)
        .await
        .ok_or_else(|| {
            JoinChannelError::MissingAuthoritativeContext { channel_id }.into_aura_error()
        })
}

pub(crate) async fn canonical_channel_name_hint_for_invite(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    channel_name_or_id: &str,
) -> Result<String, AuraError> {
    // OWNERSHIP: observed - canonical naming here is a UI hint derivation path;
    // it may consult observed chat labels but does not authorize the invite.
    let existing_name = observed_chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .map(|channel| channel.name.clone())
        .filter(|name| !name.trim().is_empty())
        .filter(|name| name != &channel_id.to_string());
    if let Some(name) = existing_name {
        return Ok(name);
    }

    let parsed_input = routing::parse_channel_ref(channel_name_or_id)?;
    if matches!(
        parsed_input,
        crate::workflows::channel_ref::ChannelSelector::Id(_)
    ) {
        return Err(super::super::error::WorkflowError::Precondition(
            "channel invitation creation requires canonical channel metadata, not a raw channel id hint",
        )
        .into());
    }

    let normalized_name = normalize_channel_name(channel_name_or_id);
    if normalized_name.is_empty() {
        return Err(AuraError::invalid("Channel name cannot be empty"));
    }
    Ok(normalized_name)
}

#[aura_macros::authoritative_source(kind = "runtime")]
pub(crate) async fn resolve_authoritative_channel_binding_from_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<crate::runtime_bridge::AuthoritativeChannelBinding, AuraError> {
    // OWNERSHIP: authoritative-source - observed chat can disambiguate an
    // already materialized binding, but runtime remains the authoritative
    // source when the projection is ambiguous or incomplete.
    match routing::parse_channel_ref(channel_input)? {
        crate::workflows::channel_ref::ChannelSelector::Id(channel_id) => {
            let context_id =
                require_authoritative_context_id_for_channel(app_core, channel_id).await?;
            Ok(crate::runtime_bridge::AuthoritativeChannelBinding {
                channel_id,
                context_id,
            })
        }
        _ => {
            let normalized_name = normalize_channel_name(channel_input);
            let observed_chat = observed_chat_snapshot(app_core).await;
            let mut observed_matches = observed_chat
                .all_channels()
                .filter(|channel| channel.name.eq_ignore_ascii_case(&normalized_name))
                .filter_map(|channel| {
                    channel.context_id.map(|context_id| {
                        crate::runtime_bridge::AuthoritativeChannelBinding {
                            channel_id: channel.id,
                            context_id,
                        }
                    })
                });
            if let Some(binding) = observed_matches.next() {
                if observed_matches.next().is_none() {
                    return Ok(binding);
                }
            }

            let runtime = require_runtime(app_core).await?;
            timeout_runtime_call(
                &runtime,
                "resolve_authoritative_channel_binding_from_input",
                "identify_materialized_channel_bindings_by_name",
                MESSAGING_RUNTIME_QUERY_TIMEOUT,
                || runtime.identify_materialized_channel_bindings_by_name(&normalized_name),
            )
            .await
            .map_err(|error| {
                AuraError::from(super::super::error::runtime_call(
                    "identify materialized channel bindings by name",
                    error,
                ))
            })?
            .map_err(|error| {
                AuraError::from(super::super::error::runtime_call(
                    "identify materialized channel bindings by name",
                    error,
                ))
            })?
            .into_iter()
            .next()
            .ok_or_else(|| AuraError::not_found(normalized_name.clone()))
        }
    }
}

pub(crate) async fn require_authoritative_channel_ref(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
    _operation: &str,
) -> Result<AuthoritativeChannelRef, AuraError> {
    let policy = workflow_retry_policy(
        CHANNEL_CONTEXT_RETRY_ATTEMPTS as u32,
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
    )?;
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        if let Ok(context_id) =
            require_authoritative_context_id_for_channel(app_core, channel_id).await
        {
            return Ok(authoritative_channel_ref(channel_id, context_id));
        }
        converge_runtime(runtime).await;
        Err(AuraError::from(
            super::super::error::WorkflowError::Precondition(
                "authoritative context required for channel",
            ),
        ))
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { .. } => {
            AuraError::from(super::super::error::WorkflowError::Precondition(
                "authoritative context required for channel",
            ))
        }
    })
}

pub(in crate::workflows) async fn runtime_channel_state_exists(
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
) -> Result<bool, AuraError> {
    timeout_runtime_call(
        runtime,
        "runtime_channel_state_exists",
        "amp_channel_state_exists",
        MESSAGING_RUNTIME_QUERY_TIMEOUT,
        || runtime.amp_channel_state_exists(channel.context_id(), channel.channel_id()),
    )
    .await
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "inspect channel state",
            error,
        ))
    })?
    .map_err(|error| {
        AuraError::from(super::super::error::runtime_call(
            "inspect channel state",
            error,
        ))
    })
}

pub(in crate::workflows) async fn wait_for_runtime_channel_state(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
) -> Result<(), AuraError> {
    let policy = workflow_retry_policy(
        CHANNEL_CONTEXT_RETRY_ATTEMPTS as u32,
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
    )?;
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        if runtime_channel_state_exists(runtime, channel).await? {
            return Ok(());
        }
        converge_runtime(runtime).await;
        Err(AuraError::from(
            super::super::error::WorkflowError::Precondition(
                "canonical AMP channel state required",
            ),
        ))
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { .. } => {
            AuraError::from(super::super::error::WorkflowError::Precondition(
                "canonical AMP channel state required",
            ))
        }
    })
}

pub(in crate::workflows) async fn authoritative_recipient_peers_for_channel(
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
    self_authority: AuthorityId,
) -> Result<Vec<AuthorityId>, AuraError> {
    let mut participants = authoritative_channel_participants(runtime, channel).await?;
    participants.retain(|authority| *authority != self_authority);
    Ok(participants)
}

async fn authoritative_channel_participants(
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
) -> Result<Vec<AuthorityId>, AuraError> {
    let context_id = channel.context_id();
    let channel_id = channel.channel_id();
    let mut last = timeout_runtime_call(
        runtime,
        "authoritative_channel_participants",
        "amp_list_channel_participants",
        MESSAGING_RUNTIME_QUERY_TIMEOUT,
        || runtime.amp_list_channel_participants(context_id, channel_id),
    )
    .await
    .map_err(
        |error| super::super::error::WorkflowError::AuthoritativeParticipantsLookup {
            channel: channel_id.to_string(),
            context: context_id.to_string(),
            source: AuraError::agent(error.to_string()),
        },
    )?
    .map_err(
        |error| super::super::error::WorkflowError::AuthoritativeParticipantsLookup {
            channel: channel_id.to_string(),
            context: context_id.to_string(),
            source: AuraError::agent(error.to_string()),
        },
    )?;

    for _ in 0..3 {
        let mut participants = last.clone();
        participants.sort_unstable();
        participants.dedup();
        if !participants.is_empty() {
            return Ok(participants);
        }

        let _ = timeout_runtime_call(
            runtime,
            "authoritative_channel_participants",
            "process_ceremony_messages",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await;
        converge_runtime(runtime).await;
        last = timeout_runtime_call(
            runtime,
            "authoritative_channel_participants",
            "amp_list_channel_participants_after_convergence",
            MESSAGING_RUNTIME_QUERY_TIMEOUT,
            || runtime.amp_list_channel_participants(context_id, channel_id),
        )
        .await
        .map_err(|error| {
            super::super::error::WorkflowError::AuthoritativeParticipantsLookupAfterConvergence {
                channel: channel_id.to_string(),
                context: context_id.to_string(),
                source: AuraError::agent(error.to_string()),
            }
        })?
        .map_err(|error| {
            super::super::error::WorkflowError::AuthoritativeParticipantsLookupAfterConvergence {
                channel: channel_id.to_string(),
                context: context_id.to_string(),
                source: AuraError::agent(error.to_string()),
            }
        })?;
    }

    last.sort_unstable();
    last.dedup();
    Ok(last)
}

pub(crate) async fn authoritative_join_member_count_if_joined(
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
    self_authority: AuthorityId,
) -> Result<Option<u32>, AuraError> {
    if !runtime_channel_state_exists(runtime, channel).await? {
        return Ok(None);
    }
    let participants = authoritative_channel_participants(runtime, channel).await?;
    if participants.contains(&self_authority) {
        return Ok(Some((participants.len() as u32).max(1)));
    }
    Ok(None)
}
