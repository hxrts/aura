#![allow(missing_docs)]

use super::*;
use crate::workflows::parse::parse_context_id;

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
    _name_hint: Option<&str>,
) -> Result<(), AuraError> {
    // A join and a display-name hint are not channel creation evidence. If
    // signal delivery trails the join, recover only the committed creation
    // fact bound to the authoritative channel and context.
    // OWNERSHIP: observed. Read back the projection after the runtime supplied
    // canonical creation evidence; this check cannot materialize a channel.
    if observed_chat_snapshot(app_core)
        .await
        .has_canonical_channel(&channel_id, context_id)
    {
        return Ok(());
    }

    let runtime = { app_core.read().await.runtime().cloned() };
    if let Some(runtime) = runtime {
        let binding = crate::runtime_bridge::AuthoritativeChannelBinding {
            channel_id,
            context_id,
        };
        let creation = timeout_runtime_call(
            &runtime,
            "ensure_channel_visible_after_join",
            "canonical_channel_creation",
            MESSAGING_RUNTIME_QUERY_TIMEOUT,
            || runtime.canonical_channel_creation(binding),
        )
        .await
        .map_err(|error| {
            AuraError::from(super::super::error::runtime_call(
                "load canonical channel creation",
                error,
            ))
        })?
        .map_err(|error| {
            AuraError::from(super::super::error::runtime_call(
                "load canonical channel creation",
                error,
            ))
        })?;
        if let Some(creation) = creation {
            update_chat_projection_observed(app_core, |chat| {
                chat.materialize_canonical_channel(creation, None);
            })
            .await?;
        }
    }
    if observed_chat_snapshot(app_core)
        .await
        .has_canonical_channel(&channel_id, context_id)
    {
        Ok(())
    } else {
        Err(super::super::error::WorkflowError::Precondition(
            "join projection missing canonical channel creation fact",
        )
        .into())
    }
}

pub async fn materialize_authoritative_channel_binding_observed(
    app_core: &Arc<RwLock<AppCore>>,
    binding: &crate::ui_contract::ChannelBindingWitness,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let channel_id = binding.channel_id.parse::<ChannelId>().map_err(|error| {
        AuraError::invalid(format!(
            "accepted channel binding carried invalid canonical channel id '{}': {error}",
            binding.channel_id
        ))
    })?;
    let context_id = match binding.context_id.as_deref() {
        Some(context_id) => parse_context_id(context_id)?,
        None => require_authoritative_context_id_for_channel(app_core, channel_id).await?,
    };
    ensure_channel_visible_after_join(app_core, channel_id, context_id, name_hint).await
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

    // Leaving removes the channel from this client, matching the runtime view
    // (which drops it on our own Left membership event).
    let _ = (context_id, name_hint);
    update_chat_projection_observed(app_core, |chat| {
        let _ = chat.remove_channel(&channel_id);
    })
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

pub(in crate::workflows) async fn runtime_amp_duplicate_is_reconciled(
    runtime: &Arc<dyn RuntimeBridge>,
    error: &(impl std::error::Error + 'static),
    context: ContextId,
    channel: ChannelId,
) -> Result<bool, AuraError> {
    if super::super::runtime_error_classification::classify_amp_channel_error(
        error, context, channel,
    ) != super::super::runtime_error_classification::AmpChannelErrorClass::AlreadyExists
    {
        return Ok(false);
    }
    timeout_runtime_call(
        runtime,
        "reconcile AMP duplicate",
        "amp_channel_state_exists",
        MESSAGING_RUNTIME_QUERY_TIMEOUT,
        || runtime.amp_channel_state_exists(context, channel),
    )
    .await
    .map_err(|error| super::super::error::runtime_call("reconcile AMP duplicate", error))?
    .map_err(|error| super::super::error::runtime_call("reconcile AMP duplicate", error).into())
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

pub(super) async fn authoritative_channel_participants(
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
            source: AuraError::Internal {
                message: "required authoritative AMP membership read".into(),
                source: Some(Arc::new(error)),
            },
        },
    )?
    .map_err(
        |error| super::super::error::WorkflowError::AuthoritativeParticipantsLookup {
            channel: channel_id.to_string(),
            context: context_id.to_string(),
            source: AuraError::Internal {
                message: "required authoritative AMP membership read".into(),
                source: Some(Arc::new(error)),
            },
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
                source: AuraError::Internal {
                    message: "required authoritative AMP membership read".into(),
                    source: Some(Arc::new(error)),
                },
            }
        })?
        .map_err(|error| {
            super::super::error::WorkflowError::AuthoritativeParticipantsLookupAfterConvergence {
                channel: channel_id.to_string(),
                context: context_id.to_string(),
                source: AuraError::Internal {
                    message: "required authoritative AMP membership read".into(),
                    source: Some(Arc::new(error)),
                },
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
