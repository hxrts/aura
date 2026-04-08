use super::*;
use crate::workflows::error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum JoinChannelError {
    #[error("JoinChannel requires an authoritative context for channel {channel_id}")]
    MissingAuthoritativeContext { channel_id: ChannelId },
    #[error("JoinChannel failed for channel {channel_id}: {detail}")]
    Transport {
        channel_id: ChannelId,
        detail: String,
    },
}

impl JoinChannelError {
    pub(super) fn into_aura_error(self) -> AuraError {
        match self {
            Self::MissingAuthoritativeContext { channel_id } => {
                AuraError::not_found(channel_id.to_string())
            }
            Self::Transport {
                channel_id: _,
                detail,
            } => error::runtime_call("join channel transport", detail).into(),
        }
    }

    fn semantic_error(&self) -> SemanticOperationError {
        SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(self.to_string())
    }
}

/// Create a group channel in chat state.
///
/// **What it does**: Creates a chat channel and selects it
/// **Returns**: ChannelId (typed) - use this directly in send_message, not a string!
/// **Signal pattern**: Updates `CHAT_SIGNAL` directly
///
/// **Note**: This is currently UI-local state only; persistence will be provided by
/// runtime-backed AMP/Chat facts when fully wired.
///
/// # Type Safety
/// Returns `ChannelId` to ensure callers use the exact channel identity.
/// Do NOT convert to string and back - use the returned `ChannelId` directly
/// with `send_message` and other channel operations.
pub async fn create_channel(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    topic: Option<String>,
    members: &[String],
    threshold_k: u8,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    Ok(create_channel_with_authoritative_binding(
        app_core,
        name,
        topic,
        members,
        threshold_k,
        timestamp_ms,
    )
    .await?
    .channel_id)
}

/// Create a channel and return its authoritative identity bundle.
///
/// This is the single-step surface for UI layers that must preserve the
/// channel/context binding without performing a second lookup after creation.
// OWNERSHIP: fact-backed
pub async fn create_channel_with_authoritative_binding(
    app_core: &Arc<RwLock<AppCore>>,
    name: &str,
    topic: Option<String>,
    members: &[String],
    threshold_k: u8,
    timestamp_ms: u64,
) -> Result<CreatedChannel, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::create_channel(),
        None,
        SemanticOperationKind::CreateChannel,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;

    let result: Result<CreatedChannel, AuraError> = async {
        let backend = messaging_backend(app_core).await;
        let member_ids: Vec<AuthorityId> = members
            .iter()
            .map(|member| parse_authority_id(member))
            .collect::<Result<Vec<_>, AuraError>>()?;
        let mut channel_id =
            ChannelId::from_bytes(hash(format!("local:{timestamp_ms}").as_bytes()));
        let mut channel_context: Option<ContextId> = None;
        if backend == MessagingBackend::Runtime {
            if let Err(_error) = crate::workflows::system::refresh_account(app_core).await {
                messaging_warn!(
                    "best-effort refresh_account before create_channel failed: {}",
                    _error
                );
            }
            let runtime = require_runtime(app_core).await?;
            let context_id = current_home_context(app_core).await?;
            channel_context = Some(context_id);
            let channel_hint = (!name.trim().is_empty())
                .then(|| channel_id_from_input(name))
                .transpose()?;
            let params = ChannelCreateParams {
                context: context_id,
                channel: channel_hint,
                skip_window: None,
                topic: topic.clone(),
            };

            channel_id = timeout_runtime_call(
                &runtime,
                "create_channel_with_authoritative_binding",
                "amp_create_channel",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.amp_create_channel(params),
            )
            .await
            .map_err(|e| error::runtime_call("create channel", e))?
            .map_err(|e| error::runtime_call("create channel", e))?;

            timeout_runtime_call(
                &runtime,
                "create_channel_with_authoritative_binding",
                "amp_join_channel_self",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || {
                    runtime.amp_join_channel(ChannelJoinParams {
                        context: context_id,
                        channel: channel_id,
                        participant: runtime.authority_id(),
                    })
                },
            )
            .await
            .map_err(|e| error::runtime_call("join channel", e))?
            .map_err(|e| error::runtime_call("join channel", e))?;

            let fact = ChatFact::channel_created_ms(
                context_id,
                channel_id,
                name.to_string(),
                topic.clone(),
                false,
                timestamp_ms,
                runtime.authority_id(),
            )
            .to_generic();

            timeout_runtime_call(
                &runtime,
                "create_channel_with_authoritative_binding",
                "commit_relational_facts",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
            )
            .await
            .map_err(|e| error::runtime_call("persist channel", e))?
            .map_err(|e| error::runtime_call("persist channel", e))?;

            let mut attempted_fanout = 0usize;
            let mut failed_fanout = Vec::new();
            for peer in member_ids.iter().copied() {
                if peer == runtime.authority_id() {
                    continue;
                }
                attempted_fanout = attempted_fanout.saturating_add(1);
                if let Err(error) = timeout_runtime_call(
                    &runtime,
                    "create_channel_with_authoritative_binding",
                    "send_chat_fact",
                    MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                    || runtime.send_chat_fact(peer, context_id, &fact),
                )
                .await
                {
                    failed_fanout.push(format!("{peer}: {error}"));
                }
            }
            if attempted_fanout > 0 && failed_fanout.len() == attempted_fanout {
                messaging_warn!(
                    "Channel create fanout unavailable for all recipients on {channel_id}: {}",
                    failed_fanout.join("; ")
                );
            }
        } else if !name.trim().is_empty() {
            channel_id = channel_id_from_input(name)?;
        }

        if backend != MessagingBackend::Runtime {
            update_chat_projection_observed(app_core, |chat_state| {
                let channel = Channel {
                    id: channel_id,
                    context_id: channel_context,
                    name: name.to_string(),
                    topic,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: member_ids.clone(),
                    member_count: (member_ids.len() as u32).saturating_add(1),
                    last_message: None,
                    last_message_time: None,
                    last_activity: timestamp_ms,
                    last_finalized_epoch: 0,
                };

                chat_state.upsert_channel(channel);
            })
            .await?;
        }

        if backend == MessagingBackend::Runtime {
            let runtime = require_runtime(app_core).await?;
            let context_id = channel_context.ok_or_else(|| {
                AuraError::internal("Missing channel context after runtime channel creation")
            })?;
            let channel_ref = AuthoritativeChannelRef::new(channel_id, context_id);
            apply_authoritative_membership_projection(
                app_core,
                channel_id,
                context_id,
                true,
                Some(name),
            )
            .await?;
            wait_for_runtime_channel_state(app_core, &runtime, channel_ref).await?;
        }

        publish_authoritative_channel_membership_ready(
            app_core,
            channel_id,
            Some(name),
            (member_ids.len() as u32).saturating_add(1),
        )
        .await?;

        if backend == MessagingBackend::Runtime && !member_ids.is_empty() {
            let runtime = require_runtime(app_core).await?;
            let context_id = channel_context.ok_or_else(|| {
                AuraError::internal("Missing channel context after runtime channel creation")
            })?;

            let mut invitation_ids = Vec::new();
            let total_n = (member_ids.len() + 1) as u8;
            let threshold_k = if threshold_k == 0 {
                default_channel_threshold(total_n)
            } else {
                normalize_channel_threshold(threshold_k, total_n)
            };
            let invitation_message = Some(format!(
                "Group threshold: {threshold_k}-of-{total_n} (keys rotate after everyone accepts)"
            ));

            let bootstrap = if bootstrap_required_for_recipients(member_ids.len()) {
                Some(
                    timeout_runtime_call(
                        &runtime,
                        "create_channel_with_authoritative_binding",
                        "amp_create_channel_bootstrap",
                        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                        || {
                            runtime.amp_create_channel_bootstrap(
                                context_id,
                                channel_id,
                                member_ids.clone(),
                            )
                        },
                    )
                    .await
                    .map_err(|e| error::runtime_call("bootstrap channel", e))?
                    .map_err(|e| error::runtime_call("bootstrap channel", e))?,
                )
            } else {
                None
            };

            for receiver in &member_ids {
                let invitation = match crate::workflows::invitation::create_channel_invitation(
                    app_core,
                    *receiver,
                    channel_id.to_string(),
                    Some(context_id),
                    Some(name.to_string()),
                    bootstrap.clone(),
                    None,
                    None,
                    None,
                    invitation_message.clone(),
                    None,
                )
                .await
                {
                    Ok(invitation) => invitation,
                    Err(error) if is_invitation_capability_missing(&error) => {
                        if let Err(_join_error) = timeout_runtime_call(
                            &runtime,
                            "create_channel_with_authoritative_binding",
                            "amp_join_channel_receiver_fallback",
                            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                            || {
                                runtime.amp_join_channel(ChannelJoinParams {
                                    context: context_id,
                                    channel: channel_id,
                                    participant: *receiver,
                                })
                            },
                        )
                        .await
                        {
                            messaging_warn!(
                                "Channel invitation capability fallback failed for {} on {}: {}",
                                receiver,
                                channel_id,
                                _join_error
                            );
                        }
                        continue;
                    }
                    Err(_error) => {
                        messaging_warn!(
                            "Channel invitation failed for {} on {} ({}); attempting direct join fallback",
                            receiver,
                            channel_id,
                            _error
                        );
                        if let Err(_join_error) = timeout_runtime_call(
                            &runtime,
                            "create_channel_with_authoritative_binding",
                            "amp_join_channel_receiver_fallback",
                            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                            || {
                                runtime.amp_join_channel(ChannelJoinParams {
                                    context: context_id,
                                    channel: channel_id,
                                    participant: *receiver,
                                })
                            },
                        )
                        .await
                        {
                            messaging_warn!(
                                "Channel invitation join fallback failed for {} on {} ({}): {}",
                                receiver,
                                channel_id,
                                _error,
                                _join_error
                            );
                        }
                        continue;
                    }
                };
                invitation_ids.push(invitation.invitation_id().as_str().to_string());
            }

            if !invitation_ids.is_empty() {
                timeout_runtime_call(
                    &runtime,
                    "create_channel_with_authoritative_binding",
                    "start_channel_invitation_monitor",
                    MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                    || runtime.start_channel_invitation_monitor(invitation_ids, context_id, channel_id),
                )
                .await
                .map_err(|e| error::runtime_call("start channel invitation monitor", e))?
                .map_err(|e| error::runtime_call("start channel invitation monitor", e))?;
            }
        }

        Ok(CreatedChannel {
            channel_id,
            context_id: channel_context,
        })
    }
    .await;

    match &result {
        Ok(created) => {
            owner
                .publish_success_with(
                    prove_channel_membership_ready(app_core, created.channel_id).await?,
                )
                .await?;
        }
        Err(error) => {
            owner
                .publish_failure(
                    SemanticOperationError::new(
                        SemanticFailureDomain::Internal,
                        SemanticFailureCode::InternalError,
                    )
                    .with_detail(error.to_string()),
                )
                .await?;
        }
    }

    result
}

/// Join an existing channel using a typed ChannelId.
// OWNERSHIP: authoritative-ref-only
pub async fn join_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
) -> Result<(), AuraError> {
    join_channel_with_name_hint(app_core, channel, None).await
}

async fn join_channel_with_name_hint(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };
    let channel_id = channel.channel_id();
    let context_id = channel.context_id();

    if let Some(member_count) =
        authoritative_join_member_count_if_joined(&runtime, channel, runtime.authority_id()).await?
    {
        apply_authoritative_membership_projection(
            app_core, channel_id, context_id, true, name_hint,
        )
        .await?;
        publish_authoritative_channel_membership_ready(
            app_core,
            channel_id,
            name_hint,
            member_count,
        )
        .await?;
        return Ok(());
    }

    enforce_home_join_allowed(app_core, context_id, channel_id, runtime.authority_id()).await?;

    let canonical_state_exists = runtime_channel_state_exists(&runtime, channel)
        .await
        .unwrap_or(false);
    if !canonical_state_exists {
        if let Err(error) = timeout_runtime_call(
            &runtime,
            "join_channel_with_name_hint",
            "amp_join_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || {
                runtime.amp_join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant: runtime.authority_id(),
                })
            },
        )
        .await
        .map_err(|error| AuraError::internal(error.to_string()))?
        {
            let canonical_state_exists = runtime_channel_state_exists(&runtime, channel)
                .await
                .unwrap_or(false);
            if intent_error_is_not_found(&error) && !canonical_state_exists {
                return Err(
                    JoinChannelError::MissingAuthoritativeContext { channel_id }.into_aura_error()
                );
            }
            if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists
                && !canonical_state_exists
            {
                return Err(JoinChannelError::Transport {
                    channel_id,
                    detail: error.to_string(),
                }
                .into_aura_error());
            }
        }
    }

    apply_authoritative_membership_projection(app_core, channel_id, context_id, true, name_hint)
        .await?;
    wait_for_runtime_channel_state(app_core, &runtime, channel).await?;
    publish_authoritative_channel_membership_ready(app_core, channel_id, name_hint, 1).await?;

    Ok(())
}

async fn fail_join_channel<T>(
    owner: &SemanticWorkflowOwner,
    error: JoinChannelError,
) -> Result<T, AuraError> {
    owner.publish_failure(error.semantic_error()).await?;
    Err(error.into_aura_error())
}

async fn join_channel_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    binding: crate::runtime_bridge::AuthoritativeChannelBinding,
    owner: &SemanticWorkflowOwner,
    channel_name: &str,
) -> Result<String, AuraError> {
    let channel_id = binding.channel_id;
    let authoritative_channel = authoritative_channel_ref(binding.channel_id, binding.context_id);
    let existing_membership_proof = prove_channel_membership_ready(app_core, channel_id)
        .await
        .ok();
    if let Some(proof) = existing_membership_proof {
        owner.publish_success_with(proof).await?;
        let mut best_effort = workflow_best_effort();
        let _ = best_effort
            .capture(post_terminal_join_followups(
                app_core,
                authoritative_channel,
                Some(channel_name),
            ))
            .await;
        let _ = best_effort.finish();
        return Ok(channel_id.to_string());
    }

    let runtime = require_runtime(app_core).await?;
    let already_joined_authoritatively = authoritative_join_member_count_if_joined(
        &runtime,
        authoritative_channel,
        runtime.authority_id(),
    )
    .await?
    .is_some();
    if !already_joined_authoritatively
        && try_join_via_pending_channel_invitation(app_core, channel_id).await?
    {
        owner
            .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
            .await?;
        let mut best_effort = workflow_best_effort();
        let _ = best_effort
            .capture(post_terminal_join_followups(
                app_core,
                authoritative_channel,
                Some(channel_name),
            ))
            .await;
        let _ = best_effort.finish();
        return Ok(channel_id.to_string());
    }

    if let Err(error) =
        join_channel_with_name_hint(app_core, authoritative_channel, Some(channel_name)).await
    {
        let join_error = match error {
            aura_core::AuraError::NotFound { .. } => {
                JoinChannelError::MissingAuthoritativeContext { channel_id }
            }
            _ => JoinChannelError::Transport {
                channel_id,
                detail: error.to_string(),
            },
        };
        return fail_join_channel(owner, join_error).await;
    }
    owner
        .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
        .await?;
    let mut best_effort = workflow_best_effort();
    let _ = best_effort
        .capture(post_terminal_join_followups(
            app_core,
            authoritative_channel,
            Some(channel_name),
        ))
        .await;
    let _ = best_effort.finish();
    Ok(channel_id.to_string())
}

/// Join an existing channel by name for callers that only carry channel names.
pub async fn join_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<String, AuraError> {
    join_channel_by_name_with_instance(app_core, channel_name, None).await
}

/// Join an existing channel by name and attribute the semantic operation to a
/// specific UI instance when one exists.
pub async fn join_channel_by_name_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::join_channel(),
        instance_id.clone(),
        SemanticOperationKind::JoinChannel,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    join_channel_by_name_owned(app_core, channel_name, &owner, None).await
}

/// Join an existing channel by name and return the directly-settled terminal
/// status for frontend handoff consumers.
pub async fn join_channel_by_name_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::join_channel(),
        instance_id,
        SemanticOperationKind::JoinChannel,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        join_channel_by_name_owned(app_core, channel_name, &owner, None).await
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

async fn join_channel_binding_witness(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: &str,
) -> Result<crate::ui_contract::ChannelBindingWitness, AuraError> {
    let channel_id = channel_id.parse::<ChannelId>().map_err(|error| {
        AuraError::invalid(format!(
            "join workflow returned invalid canonical channel id '{channel_id}': {error}"
        ))
    })?;
    let context_id = resolve_authoritative_context_id_for_channel(app_core, channel_id)
        .await
        .map(|context_id| context_id.to_string());
    Ok(crate::ui_contract::ChannelBindingWitness::new(
        channel_id.to_string(),
        context_id,
    ))
}

fn authoritative_binding_from_witness(
    binding: &crate::ui_contract::ChannelBindingWitness,
) -> Result<crate::runtime_bridge::AuthoritativeChannelBinding, AuraError> {
    let channel_id = binding.channel_id.parse::<ChannelId>().map_err(|error| {
        AuraError::invalid(format!(
            "join fallback carried invalid canonical channel id '{}': {error}",
            binding.channel_id
        ))
    })?;
    let context_id = binding
        .context_id
        .as_deref()
        .ok_or_else(|| AuraError::invalid("join fallback requires an authoritative context id"))?
        .parse::<ContextId>()
        .map_err(|error| {
            AuraError::invalid(format!(
                "join fallback carried invalid authoritative context id: {error}"
            ))
        })?;
    Ok(crate::runtime_bridge::AuthoritativeChannelBinding {
        channel_id,
        context_id,
    })
}

/// Join an existing channel by name and return the channel selection/binding
/// witness settled by the workflow.
pub async fn join_channel_by_name_with_binding_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<crate::ui_contract::ChannelBindingWitness> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::join_channel(),
        instance_id,
        SemanticOperationKind::JoinChannel,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
            let channel_id =
                join_channel_by_name_owned(app_core, channel_name, &owner, None).await?;
            return join_channel_binding_witness(app_core, &channel_id).await;
        }

        let authoritative_binding =
            resolve_authoritative_channel_binding_from_input(app_core, channel_name).await?;
        let _channel_id =
            join_channel_authoritative(app_core, authoritative_binding, &owner, channel_name)
                .await?;
        Ok(crate::ui_contract::ChannelBindingWitness::new(
            authoritative_binding.channel_id.to_string(),
            Some(authoritative_binding.context_id.to_string()),
        ))
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Join an existing channel when the caller already holds an authoritative
/// channel binding and needs the normal runtime join flow plus terminal status.
pub async fn join_authoritative_channel_binding_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    binding: &crate::ui_contract::ChannelBindingWitness,
    channel_name: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<crate::ui_contract::ChannelBindingWitness> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::join_channel(),
        instance_id,
        SemanticOperationKind::JoinChannel,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        let authoritative_binding = authoritative_binding_from_witness(binding)?;
        let _channel_id = join_channel_authoritative(
            app_core,
            authoritative_binding.clone(),
            &owner,
            channel_name,
        )
        .await?;
        Ok(crate::ui_contract::ChannelBindingWitness::new(
            authoritative_binding.channel_id.to_string(),
            Some(authoritative_binding.context_id.to_string()),
        ))
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

#[aura_macros::semantic_owner(
    owner = "join_channel_by_name_with_instance",
    wrapper = "join_channel_by_name_with_instance",
    terminal = "publish_success_with",
    postcondition = "channel_membership_ready",
    proof = crate::workflows::semantic_facts::ChannelMembershipReadyProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "channel_target_resolved,authoritative_context_materialized",
    child_ops = "",
    category = "move_owned"
)]
async fn join_channel_by_name_owned(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<String, AuraError> {
    let channel_name = channel_name.trim();
    if channel_name.is_empty() {
        let error = AuraError::invalid("Channel name cannot be empty");
        owner
            .publish_failure(
                SemanticOperationError::new(
                    SemanticFailureDomain::Command,
                    SemanticFailureCode::InternalError,
                )
                .with_detail(error.to_string()),
            )
            .await?;
        return Err(error);
    }

    if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
        // OWNERSHIP: observed - the local-only branch seeds a purely local chat
        // channel from observed projections and contacts; no runtime authority
        // exists on this path.
        let normalized_channel_name = normalize_channel_name(channel_name);
        let existing_local_channel = {
            let chat = observed_chat_snapshot(app_core).await;
            let existing = chat
                .all_channels()
                .find(|channel| {
                    channel
                        .name
                        .eq_ignore_ascii_case(normalized_channel_name.as_str())
                })
                .cloned();
            existing
        };
        let known_members: Vec<String> = observed_contacts_snapshot(app_core)
            .await
            .contact_ids()
            .map(ToString::to_string)
            .collect();
        let channel_id = if let Some(channel) = existing_local_channel {
            publish_authoritative_channel_membership_ready(
                app_core,
                channel.id,
                Some(channel.name.as_str()),
                channel
                    .member_count
                    .max(channel.member_ids.len() as u32 + 1),
            )
            .await?;
            channel.id
        } else {
            create_channel(
                app_core,
                normalized_channel_name.as_str(),
                None,
                &known_members,
                0,
                0,
            )
            .await?
        };
        owner
            .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
            .await?;
        return Ok(channel_id.to_string());
    }

    let binding =
        match resolve_authoritative_channel_binding_from_input(app_core, channel_name).await {
            Ok(binding) => binding,
            Err(error) => {
                owner
                    .publish_failure(
                        SemanticOperationError::new(
                            SemanticFailureDomain::Command,
                            SemanticFailureCode::InternalError,
                        )
                        .with_detail(error.to_string()),
                    )
                    .await?;
                return Err(error);
            }
        };
    join_channel_authoritative(app_core, binding, owner, channel_name).await
}

async fn leave_channel_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "leave_channel",
        "amp_leave_channel",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.amp_leave_channel(ChannelLeaveParams {
                context: channel.context_id(),
                channel: channel.channel_id(),
                participant: runtime.authority_id(),
            })
        },
    )
    .await
    .map_err(|e| error::runtime_call("leave channel", e))?
    .map_err(|e| error::runtime_call("leave channel", e))?;

    apply_authoritative_membership_projection(
        app_core,
        channel.channel_id(),
        channel.context_id(),
        false,
        name_hint,
    )
    .await?;
    clear_authoritative_channel_readiness_facts(app_core, channel.channel_id()).await?;

    Ok(())
}

/// Leave a channel using a typed ChannelId.
// OWNERSHIP: observed-display-update
// Local-only display/runtime shim until leave facts exist locally.
pub async fn leave_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
        update_chat_projection_observed(app_core, |chat| {
            let _ = chat.remove_channel(&channel_id);
        })
        .await?;
        clear_authoritative_channel_readiness_facts(app_core, channel_id).await?;
        return Ok(());
    }

    let runtime = require_runtime(app_core).await?;
    let channel =
        require_authoritative_channel_ref(app_core, &runtime, channel_id, "leave_channel").await?;
    leave_channel_authoritative(app_core, channel, None).await
}

/// Leave a channel by name for callers that only carry channel names.
pub async fn leave_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<(), AuraError> {
    if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
        for channel_id in
            matching_local_chat_channel_ids_from_observed_state(app_core, channel_name).await?
        {
            leave_channel(app_core, channel_id).await?;
        }
        return Ok(());
    }

    let binding = resolve_authoritative_channel_binding_from_input(app_core, channel_name).await?;
    leave_channel_authoritative(
        app_core,
        authoritative_channel_ref(binding.channel_id, binding.context_id),
        Some(channel_name),
    )
    .await?;

    Ok(())
}

async fn close_channel_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "close_channel",
        "amp_close_channel",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.amp_close_channel(ChannelCloseParams {
                context: channel.context_id(),
                channel: channel.channel_id(),
            })
        },
    )
    .await
    .map_err(|e| error::runtime_call("close channel", e))?
    .map_err(|e| error::runtime_call("close channel", e))?;

    let fact = ChatFact::channel_closed_ms(
        channel.context_id(),
        channel.channel_id(),
        timestamp_ms,
        runtime.authority_id(),
    )
    .to_generic();

    let facts = vec![fact];
    timeout_runtime_call(
        &runtime,
        "close_channel",
        "commit_relational_facts",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || runtime.commit_relational_facts(&facts),
    )
    .await
    .map_err(|e| error::runtime_call("persist channel close", e))?
    .map_err(|e| error::runtime_call("persist channel close", e))?;

    Ok(())
}

/// Close/archive a channel using a typed ChannelId.
///
/// Today this is a UI-local operation that removes the channel from `CHAT_SIGNAL`.
/// A fully persisted implementation will commit a `ChatFact::ChannelClosed` fact.
pub async fn close_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let channel =
        require_authoritative_channel_ref(app_core, &runtime, channel_id, "close_channel").await?;
    close_channel_authoritative(app_core, channel, timestamp_ms).await
}

/// Close/archive a channel by name for callers that only carry channel names.
pub async fn close_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let binding = resolve_authoritative_channel_binding_from_input(app_core, channel_name).await?;
    close_channel_authoritative(
        app_core,
        authoritative_channel_ref(binding.channel_id, binding.context_id),
        timestamp_ms,
    )
    .await
}

async fn set_topic_authoritative(
    app_core: &Arc<RwLock<AppCore>>,
    channel: AuthoritativeChannelRef,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;

    timeout_runtime_call(
        &runtime,
        "set_topic",
        "channel_set_topic",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || {
            runtime.channel_set_topic(
                channel.context_id(),
                channel.channel_id(),
                text.to_string(),
                timestamp_ms,
            )
        },
    )
    .await
    .map_err(|e| error::runtime_call("set channel topic", e))?
    .map_err(|e| error::runtime_call("set channel topic", e))?;

    Ok(())
}

/// Set a channel topic using a typed ChannelId.
///
/// Today this is a UI-local operation that updates the channel entry in `CHAT_SIGNAL`.
/// A fully persisted implementation will commit a topic fact.
pub async fn set_topic(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let runtime = require_runtime(app_core).await?;
    let channel =
        require_authoritative_channel_ref(app_core, &runtime, channel_id, "set_topic").await?;
    set_topic_authoritative(app_core, channel, text, timestamp_ms).await
}

/// Set a channel topic by name for callers that only carry channel names.
pub async fn set_topic_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let binding = resolve_authoritative_channel_binding_from_input(app_core, channel_name).await?;
    set_topic_authoritative(
        app_core,
        authoritative_channel_ref(binding.channel_id, binding.context_id),
        text,
        timestamp_ms,
    )
    .await
}
