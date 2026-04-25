#![allow(missing_docs)]

use super::*;
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(super) enum SendMessageError {
    #[error("Failed to resolve channel {channel}: {detail}")]
    ChannelResolution { channel: String, detail: String },
    #[error("Missing authoritative context for channel {channel_id}")]
    MissingAuthoritativeContext { channel_id: ChannelId },
    #[error("Recipient peers are not resolved for channel {channel_id}")]
    RecipientResolutionNotReady { channel_id: ChannelId },
    #[error("Peer channel establishment is not complete for channel {channel_id}")]
    DeliveryNotReady { channel_id: ChannelId },
    #[error("Authoritative readiness facts are unavailable: {detail}")]
    ReadinessFactsUnavailable { detail: String },
    #[error(
        "AMP channel bootstrap is unavailable for channel {channel_id} in context {context_id}"
    )]
    ChannelBootstrapUnavailable {
        channel_id: ChannelId,
        context_id: ContextId,
    },
    #[error("Transport error while sending on channel {channel_id}: {detail}")]
    Transport {
        channel_id: ChannelId,
        detail: String,
    },
}

impl SendMessageError {
    pub(super) fn semantic_error(&self) -> SemanticOperationError {
        match self {
            Self::ChannelResolution { channel, detail } => SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!("channel={channel}; detail={detail}")),
            Self::MissingAuthoritativeContext { channel_id } => SemanticOperationError::new(
                SemanticFailureDomain::ChannelContext,
                SemanticFailureCode::MissingAuthoritativeContext,
            )
            .with_detail(format!("channel_id={channel_id}")),
            Self::RecipientResolutionNotReady { channel_id } => SemanticOperationError::new(
                SemanticFailureDomain::Delivery,
                SemanticFailureCode::DeliveryReadinessNotReached,
            )
            .with_detail(format!(
                "channel_id={channel_id}; reason=recipient_resolution_missing"
            )),
            Self::DeliveryNotReady { channel_id } => SemanticOperationError::new(
                SemanticFailureDomain::Delivery,
                SemanticFailureCode::PeerChannelNotEstablished,
            )
            .with_detail(format!("channel_id={channel_id}")),
            Self::ReadinessFactsUnavailable { detail } => SemanticOperationError::new(
                SemanticFailureDomain::Internal,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!("semantic_readiness_unavailable: {detail}")),
            Self::ChannelBootstrapUnavailable {
                channel_id,
                context_id,
            } => SemanticOperationError::new(
                SemanticFailureDomain::Transport,
                SemanticFailureCode::ChannelBootstrapUnavailable,
            )
            .with_detail(format!("channel_id={channel_id}; context_id={context_id}")),
            Self::Transport { channel_id, detail } => SemanticOperationError::new(
                SemanticFailureDomain::Internal,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!("channel_id={channel_id}; detail={detail}")),
        }
    }
}

impl From<SendMessageError> for AuraError {
    fn from(error: SendMessageError) -> Self {
        AuraError::agent(error.to_string())
    }
}

type PostTerminalDelivery = (
    Arc<dyn RuntimeBridge>,
    AuthoritativeChannelRef,
    RelationalFact,
    AuthorityId,
    ContextId,
    String,
);

async fn fail_send_message<T>(
    owner: &SemanticWorkflowOwner,
    error: SendMessageError,
) -> Result<T, AuraError> {
    publish_send_message_failure(owner, &error).await?;
    Err(error.into())
}

pub(super) async fn mark_message_delivery_failed(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    message_id: &str,
    actor_id: AuthorityId,
) -> Result<(), AuraError> {
    reduce_chat_fact_observed(
        app_core,
        &ChatFact::message_delivery_updated_ms(
            context_id,
            channel_id,
            message_id.to_string(),
            ChatMessageDeliveryStatus::Failed,
            next_observed_projection_timestamp_ms(app_core).await,
            actor_id,
        ),
    )
    .await?;

    #[cfg(feature = "instrumented")]
    tracing::warn!(
        message_id,
        "marked message delivery as failed after remote fanout exhaustion"
    );

    Ok(())
}

async fn deliver_message_fact_remotely(
    _app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
    sender_id: AuthorityId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let context_id = channel.context_id();
    let channel_id = channel.channel_id();
    let mut delivered_remote = false;
    let mut recipients =
        authoritative_recipient_peers_for_channel(runtime, channel, sender_id).await?;
    let mut failed_fanout = Vec::new();
    let mut attempted_fanout_total = 0usize;
    let mut last_connectivity_error: Option<String> = None;
    let retry_policy = workflow_retry_policy(
        REMOTE_DELIVERY_RETRY_ATTEMPTS as u32,
        Duration::from_millis(REMOTE_DELIVERY_RETRY_BACKOFF_MS),
        Duration::from_millis(REMOTE_DELIVERY_RETRY_BACKOFF_MS),
    )?;
    let mut attempts = retry_policy.attempt_budget();
    loop {
        let attempt = attempts.record_attempt()?;
        if recipients.is_empty() {
            converge_runtime(runtime).await;
            if attempts.can_attempt() {
                runtime
                    .sleep_ms(retry_policy.delay_for_attempt(attempt).as_millis() as u64)
                    .await;
                recipients =
                    authoritative_recipient_peers_for_channel(runtime, channel, sender_id).await?;
                continue;
            }
            break;
        }

        let mut channel_setup_errors = Vec::new();
        for peer in recipients.iter().copied() {
            if let Err(error) = timeout_runtime_call(
                runtime,
                "deliver_message_fact_remotely",
                "ensure_peer_channel",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.ensure_peer_channel(context_id, peer),
            )
            .await
            {
                channel_setup_errors.push(format!("{peer}: {error}"));
            }
        }

        if let Err(error) = ensure_runtime_peer_connectivity(runtime, "send_message_ref").await {
            let mut detail = error.to_string();
            if !channel_setup_errors.is_empty() {
                detail.push_str("; channel_setup=");
                detail.push_str(&channel_setup_errors.join(", "));
            }
            last_connectivity_error = Some(detail);
        }

        failed_fanout.clear();
        let mut attempted_fanout = 0usize;
        for peer in recipients.iter().copied() {
            attempted_fanout = attempted_fanout.saturating_add(1);
            attempted_fanout_total = attempted_fanout_total.saturating_add(1);
            if let Err(error) = send_chat_fact_with_retry(runtime, peer, context_id, fact).await {
                failed_fanout.push(format!("{peer}: {error}"));
            }
        }

        if attempted_fanout > 0 && failed_fanout.is_empty() {
            delivered_remote = true;
            break;
        }

        if attempts.can_attempt() {
            converge_runtime(runtime).await;
            runtime
                .sleep_ms(retry_policy.delay_for_attempt(attempt).as_millis() as u64)
                .await;
            recipients =
                authoritative_recipient_peers_for_channel(runtime, channel, sender_id).await?;
        } else {
            break;
        }
    }

    if !delivered_remote {
        if recipients.is_empty() {
            return Err(super::super::error::WorkflowError::DeliveryFailed {
                peer: channel_id.to_string(),
                attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                source: AuraError::agent("no recipient peers resolved after extended retries"),
            }
            .into());
        }
        if attempted_fanout_total == 0 {
            return Err(
                super::super::error::WorkflowError::DeliveryPrerequisitesNeverConverged {
                    peer: channel_id.to_string(),
                    attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                    detail: last_connectivity_error
                        .unwrap_or_else(|| "no recipient fanout attempt executed".to_string()),
                }
                .into(),
            );
        }
        if !failed_fanout.is_empty() {
            return Err(
                super::super::error::WorkflowError::DeliveryFanoutUnavailable {
                    peer: channel_id.to_string(),
                    attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                    recipients: failed_fanout,
                }
                .into(),
            );
        }
    }

    converge_runtime(runtime).await;
    if let Err(_error) = ensure_runtime_peer_connectivity(runtime, "send_message_ref").await {
        #[cfg(feature = "instrumented")]
        tracing::warn!(
            error = %_error,
            channel_id = %channel_id,
            "message send completed without reachable peers — remote delivery may not have converged"
        );
    }

    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_post_terminal_message_followups<F>(spawner: &aura_core::OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawner.spawn(Box::pin(fut));
}

#[cfg(target_arch = "wasm32")]
fn spawn_post_terminal_message_followups<F>(spawner: &aura_core::OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + 'static,
{
    spawner.spawn_local(Box::pin(fut));
}

pub async fn send_message(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    send_message_ref(app_core, ChannelRef::Id(channel_id), content, timestamp_ms).await
}

pub async fn send_message_now(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message(app_core, channel_id, content, timestamp_ms).await
}

pub async fn send_message_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_ref = ChannelRef::Name(channel_name.to_string());
    send_message_ref(app_core, channel_ref, content, timestamp_ms).await
}

pub async fn send_message_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    timestamp_ms: u64,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    send_message_ref_with_instance(
        app_core,
        ChannelRef::Id(channel_id),
        content,
        timestamp_ms,
        instance_id,
    )
    .await
}

pub async fn send_message_now_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message_with_instance(app_core, channel_id, content, timestamp_ms, instance_id).await
}

pub async fn send_message_now_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await;
    match timestamp_ms {
        Ok(timestamp_ms) => {
            send_message_with_terminal_status(
                app_core,
                ChannelRef::Id(channel_id),
                content,
                timestamp_ms,
                instance_id,
            )
            .await
        }
        Err(error) => crate::ui_contract::WorkflowTerminalOutcome {
            result: Err(error.into()),
            terminal: None,
        },
    }
}

pub async fn send_message_by_name_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    timestamp_ms: u64,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let channel_ref = ChannelRef::Name(channel_name.to_string());
    send_message_ref_with_instance(app_core, channel_ref, content, timestamp_ms, instance_id).await
}

pub async fn send_message_by_name_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    timestamp_ms: u64,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    send_message_with_terminal_status(
        app_core,
        ChannelRef::Name(channel_name.to_string()),
        content,
        timestamp_ms,
        instance_id,
    )
    .await
}

pub async fn send_message_by_name_now_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message_by_name_with_instance(app_core, channel_name, content, timestamp_ms, instance_id)
        .await
}

pub async fn send_message_by_name_now_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await;
    match timestamp_ms {
        Ok(timestamp_ms) => {
            send_message_by_name_with_terminal_status(
                app_core,
                channel_name,
                content,
                timestamp_ms,
                instance_id,
            )
            .await
        }
        Err(error) => crate::ui_contract::WorkflowTerminalOutcome {
            result: Err(error.into()),
            terminal: None,
        },
    }
}

pub async fn send_message_ref_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::send_message(),
        instance_id,
        SemanticOperationKind::SendChatMessage,
    );
    send_message_ref_owned(app_core, channel, content, timestamp_ms, &owner, None).await
}

pub async fn send_message_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::send_message(),
        instance_id,
        SemanticOperationKind::SendChatMessage,
    );
    let result =
        send_message_ref_owned(app_core, channel, content, timestamp_ms, &owner, None).await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

pub async fn send_message_ref(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::send_message(),
        None,
        SemanticOperationKind::SendChatMessage,
    );
    send_message_ref_owned(app_core, channel, content, timestamp_ms, &owner, None).await
}

#[aura_macros::semantic_owner(
    owner = "send_message_ref_with_instance",
    wrapper = "send_message_ref_with_instance",
    terminal = "publish_success_with",
    postcondition = "message_committed",
    proof = crate::workflows::semantic_facts::MessageCommittedProof,
    authoritative_inputs = "runtime,authoritative_source",
    depends_on = "channel_target_resolved,authoritative_context_materialized,delivery_ready",
    child_ops = "",
    category = "move_owned"
)]
async fn send_message_ref_owned(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<String, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;

    let backend = messaging_backend(app_core).await;
    let mut post_terminal_delivery: Option<PostTerminalDelivery> = None;
    let (channel_id, channel_label) = match &channel {
        ChannelRef::Id(id) => (*id, id.to_string()),
        ChannelRef::Name(name) => {
            let resolution = if backend == MessagingBackend::LocalOnly {
                resolve_local_chat_channel_id_from_observed_state_or_input(app_core, name).await
            } else {
                resolve_chat_channel_id_from_state_or_input(app_core, name).await
            };
            match resolution {
                Ok(channel_id) => (channel_id, name.clone()),
                Err(error) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::ChannelResolution {
                            channel: name.clone(),
                            detail: error.to_string(),
                        },
                    )
                    .await;
                }
            }
        }
    };

    let mut channel_context: Option<ContextId> = None;
    let mut epoch_hint: Option<u32> = None;
    let (sender_id, message_id) = if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let sender_id = runtime.authority_id();
        let is_note_to_self = channel_id == note_to_self_channel_id(sender_id)
            || matches!(&channel, ChannelRef::Name(name) if is_note_to_self_channel_name(name));
        if is_note_to_self {
            let context_id = note_to_self_context_id(sender_id);
            let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
            channel_context = Some(context_id);
            let fact = ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_id.clone(),
                sender_id,
                "You".to_string(),
                content.as_bytes().to_vec(),
                timestamp_ms,
                None,
                None,
            )
            .to_generic();
            timeout_runtime_call(
                &runtime,
                "send_message_ref_owned",
                "commit_relational_facts",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
            )
            .await
            .map_err(|e| super::super::error::runtime_call("persist note-to-self message", e))?
            .map_err(|e| super::super::error::runtime_call("persist note-to-self message", e))?;
            (sender_id, message_id)
        } else {
            let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
            let authoritative_channel =
                match require_authoritative_context_id_for_channel(app_core, channel_id)
                    .await
                    .map(|context_id| authoritative_channel_ref(channel_id, context_id))
                {
                    Ok(channel) => channel,
                    Err(_) => {
                        return fail_send_message(
                            owner,
                            SendMessageError::MissingAuthoritativeContext { channel_id },
                        )
                        .await;
                    }
                };
            let context_id = authoritative_channel.context_id();
            owner
                .publish_phase(SemanticOperationPhase::AuthoritativeContextReady)
                .await?;
            if channel_id != note_to_self_channel_id(sender_id) {
                let readiness = match require_send_message_readiness(
                    app_core,
                    authoritative_channel,
                )
                .await
                {
                    Ok(readiness) => readiness,
                    Err(
                        SendMessageError::RecipientResolutionNotReady { .. }
                        | SendMessageError::DeliveryNotReady { .. },
                    ) => {
                        if let Err(error) =
                            refresh_authoritative_recipient_resolution_readiness(app_core).await
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::ReadinessFactsUnavailable {
                                    detail: format!(
                                        "recipient resolution refresh failed for {channel_id}: {error}"
                                    ),
                                },
                            )
                            .await;
                        }
                        if let Err(error) = refresh_authoritative_delivery_readiness_for_channel(
                            app_core,
                            &runtime,
                            authoritative_channel,
                        )
                        .await
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::ReadinessFactsUnavailable {
                                    detail: format!(
                                        "delivery readiness refresh failed for {channel_id}: {error}"
                                    ),
                                },
                            )
                            .await;
                        }
                        if !warm_channel_connectivity(app_core, &runtime, authoritative_channel)
                            .await
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::ReadinessFactsUnavailable {
                                    detail: format!(
                                        "channel connectivity warmup failed for {channel_id}"
                                    ),
                                },
                            )
                            .await;
                        }
                        match require_send_message_readiness(app_core, authoritative_channel).await
                        {
                            Ok(readiness) => readiness,
                            Err(error) => return fail_send_message(owner, error).await,
                        }
                    }
                    Err(error) => return fail_send_message(owner, error).await,
                };
                if readiness.recipient_resolution_ready {
                    owner
                        .publish_phase(SemanticOperationPhase::RecipientResolutionReady)
                        .await?;
                }
                if readiness.delivery_ready {
                    owner
                        .publish_phase(SemanticOperationPhase::DeliveryReady)
                        .await?;
                }
            }
            enforce_home_moderation_for_sender(
                app_core,
                context_id,
                channel_id,
                sender_id,
                timestamp_ms,
            )
            .await?;
            channel_context = Some(context_id);

            let send_params = ChannelSendParams {
                context: context_id,
                channel: channel_id,
                sender: sender_id,
                plaintext: content.as_bytes().to_vec(),
                reply_to: None,
            };
            let mut maybe_cipher = match timeout_runtime_call(
                &runtime,
                "send_message_ref_owned",
                "amp_send_message",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.amp_send_message(send_params.clone()),
            )
            .await
            .map_err(|error| AuraError::internal(error.to_string()))?
            {
                Ok(cipher) => Some(cipher),
                Err(error) => {
                    if is_amp_channel_state_unavailable(&error) {
                        None
                    } else {
                        return fail_send_message(
                            owner,
                            SendMessageError::Transport {
                                channel_id,
                                detail: format!(
                                    "context_id={context_id}; amp_send_message failed: {error}"
                                ),
                            },
                        )
                        .await;
                    }
                }
            };
            if maybe_cipher.is_none() {
                #[derive(Debug, thiserror::Error)]
                enum AmpSendRetryError {
                    #[error("canonical AMP channel state required")]
                    ChannelStateUnavailable,
                    #[error("{0}")]
                    Transport(String),
                }

                let retry_policy = workflow_retry_policy(
                    AMP_SEND_RETRY_ATTEMPTS as u32,
                    Duration::from_millis(AMP_SEND_RETRY_BACKOFF_MS),
                    Duration::from_millis(
                        AMP_SEND_RETRY_BACKOFF_MS * AMP_SEND_RETRY_ATTEMPTS as u64,
                    ),
                )?;
                match execute_with_runtime_retry_budget(&runtime, &retry_policy, |attempt| {
                    let runtime = Arc::clone(&runtime);
                    let send_params = send_params.clone();
                    async move {
                        if attempt > 0 {
                            converge_runtime(&runtime).await;
                        }
                        match timeout_runtime_call(
                            &runtime,
                            "send_message_ref_owned",
                            "amp_send_message_retry",
                            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                            || runtime.amp_send_message(send_params),
                        )
                        .await
                        .map_err(|error| AmpSendRetryError::Transport(error.to_string()))?
                        {
                            Ok(cipher) => Ok(cipher),
                            Err(error) if is_amp_channel_state_unavailable(&error) => {
                                Err(AmpSendRetryError::ChannelStateUnavailable)
                            }
                            Err(error) => Err(AmpSendRetryError::Transport(error.to_string())),
                        }
                    }
                })
                .await
                {
                    Ok(cipher) => maybe_cipher = Some(cipher),
                    Err(RetryRunError::Timeout(timeout_error)) => {
                        return fail_send_message(
                            owner,
                            SendMessageError::Transport {
                                channel_id,
                                detail: format!(
                                    "context_id={context_id}; amp_send_message retry timed out: {timeout_error}"
                                ),
                            },
                        )
                        .await;
                    }
                    Err(RetryRunError::AttemptsExhausted {
                        last_error: AmpSendRetryError::ChannelStateUnavailable,
                        ..
                    }) => {
                        maybe_cipher = None;
                    }
                    Err(RetryRunError::AttemptsExhausted { last_error, .. }) => {
                        return fail_send_message(
                            owner,
                            SendMessageError::Transport {
                                channel_id,
                                detail: format!(
                                    "context_id={context_id}; amp_send_message retry failed: {last_error}"
                                ),
                            },
                        )
                        .await;
                    }
                }
            }

            let maybe_fact = if let Some(cipher) = maybe_cipher {
                let wire = AmpMessage::new(cipher.header, cipher.ciphertext.clone());
                let sealed =
                    serialize_amp_message(&wire).map_err(super::super::error::fact_encoding)?;

                epoch_hint = Some(cipher.header.chan_epoch as u32);

                Some(
                    ChatFact::message_sent_sealed_ms(
                        context_id,
                        channel_id,
                        message_id.clone(),
                        sender_id,
                        "You".to_string(),
                        sealed,
                        timestamp_ms,
                        None,
                        epoch_hint,
                    )
                    .to_generic(),
                )
            } else {
                messaging_warn!(
                    "AMP send unavailable for context {} channel {} after {} retries; falling back to optimistic local send",
                    context_id,
                    channel_id,
                    AMP_SEND_RETRY_ATTEMPTS
                );
                None
            };

            if let Some(fact) = maybe_fact {
                timeout_runtime_call(
                    &runtime,
                    "send_message_ref_owned",
                    "commit_relational_facts_with_options",
                    MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                    || {
                        runtime.commit_relational_facts_with_options(
                            std::slice::from_ref(&fact),
                            FactOptions::default().with_ack_tracking(),
                        )
                    },
                )
                .await
                .map_err(|e| super::super::error::runtime_call("persist message", e))?
                .map_err(|e| super::super::error::runtime_call("persist message", e))?;
                post_terminal_delivery = Some((
                    runtime.clone(),
                    authoritative_channel,
                    fact,
                    sender_id,
                    context_id,
                    message_id.clone(),
                ));
            } else {
                return fail_send_message(
                    owner,
                    SendMessageError::ChannelBootstrapUnavailable {
                        channel_id,
                        context_id,
                    },
                )
                .await;
            }

            (sender_id, message_id)
        }
    } else {
        let sender_id = AuthorityId::new_from_entropy([1u8; 32]);
        let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
        (sender_id, message_id)
    };

    if let Some(context_id) = channel_context {
        reduce_chat_fact_observed(
            app_core,
            &ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_id.clone(),
                sender_id,
                "You".to_string(),
                content.as_bytes().to_vec(),
                timestamp_ms,
                None,
                epoch_hint,
            ),
        )
        .await?;
    } else {
        update_chat_projection_observed(app_core, |chat_state| {
            if !chat_state.has_channel(&channel_id) {
                chat_state.add_channel(Channel {
                    id: channel_id,
                    context_id: channel_context,
                    name: channel_label.clone(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: timestamp_ms,
                    last_finalized_epoch: 0,
                });
            }

            chat_state.apply_message(
                channel_id,
                Message {
                    id: message_id.clone(),
                    channel_id,
                    sender_id,
                    sender_name: "You".to_string(),
                    content: content.to_string(),
                    timestamp: timestamp_ms,
                    reply_to: None,
                    is_own: true,
                    is_read: true,
                    delivery_status: MessageDeliveryStatus::Sent,
                    epoch_hint,
                    is_finalized: false,
                },
            );
        })
        .await?;
    }

    if backend == MessagingBackend::Runtime {
        publish_message_committed_fact(app_core, channel_id, &channel_label, content).await?;
    }

    owner
        .publish_success_with(issue_message_committed_proof(message_id.clone()))
        .await?;

    if let Some((
        runtime,
        authoritative_channel,
        fact,
        sender_id,
        context_id,
        followup_message_id,
    )) = post_terminal_delivery
    {
        let spawner = runtime.task_spawner();
        let app_core = app_core.clone();
        spawn_post_terminal_message_followups(&spawner, async move {
            let mut best_effort = workflow_best_effort();
            let _ = best_effort
                .capture(async {
                    if let Err(error) = deliver_message_fact_remotely(
                        &app_core,
                        &runtime,
                        authoritative_channel,
                        sender_id,
                        &fact,
                    )
                    .await
                    {
                        messaging_warn!(
                            error = %error,
                            channel_id = %channel_id,
                            message_id = %followup_message_id,
                            "post-terminal remote message delivery failed"
                        );
                        if let Err(_mark_error) = mark_message_delivery_failed(
                            &app_core,
                            context_id,
                            channel_id,
                            &followup_message_id,
                            sender_id,
                        )
                        .await
                        {
                            messaging_warn!(
                                delivery_error = %error,
                                mark_error = %_mark_error,
                                message_id = %followup_message_id,
                                "post-terminal remote message delivery failed and mark-failed also failed"
                            );
                        }
                        return Err(error);
                    }
                    Ok(())
                })
                .await;
            let _ = best_effort.finish();
        });
    }

    Ok(message_id)
}

pub async fn start_direct_chat(
    app_core: &Arc<RwLock<AppCore>>,
    contact_id: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let contact_authority = parse_authority_id(contact_id)?;
    let channel_id =
        start_direct_chat_with_authority(app_core, contact_authority, timestamp_ms).await?;
    Ok(channel_id.to_string())
}

pub async fn start_direct_chat_with_authority(
    app_core: &Arc<RwLock<AppCore>>,
    contact_authority: AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    // OWNERSHIP: observed - contact projection data here is used only to derive
    // a display label for the DM path; it does not authorize the direct chat.
    let backend = messaging_backend(app_core).await;
    let contacts = observed_contacts_snapshot(app_core).await;
    let contact_id = contact_authority.to_string();

    let contact_name = contacts
        .contact(&contact_authority)
        .map(|c| {
            if !c.nickname.trim().is_empty() {
                c.nickname.clone()
            } else if let Some(suggestion) = c
                .nickname_suggestion
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                suggestion.clone()
            } else {
                format!("DM with {}", &contact_id[..8.min(contact_id.len())])
            }
        })
        .unwrap_or_else(|| format!("DM with {}", &contact_id[..8.min(contact_id.len())]));

    if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let context_id = pair_dm_context_id(runtime.authority_id(), contact_authority);
        let channel_name = if contact_name.trim().is_empty() {
            format!("dm-{}", &contact_id[..8.min(contact_id.len())])
        } else {
            format!("DM: {contact_name}")
        };
        let channel_id = pair_dm_channel_id(runtime.authority_id(), contact_authority);

        let create_result = timeout_runtime_call(
            &runtime,
            "start_direct_chat_with_authority",
            "amp_create_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || {
                runtime.amp_create_channel(ChannelCreateParams {
                    context: context_id,
                    channel: Some(channel_id),
                    skip_window: None,
                    topic: Some(format!("Direct messages with {contact_id}")),
                })
            },
        )
        .await;
        if let Err(error) = create_result {
            if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
                return Err(
                    super::super::error::runtime_call("create direct channel", error).into(),
                );
            }
        }

        timeout_runtime_call(
            &runtime,
            "start_direct_chat_with_authority",
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
        .map_err(|error| super::super::error::runtime_call("join direct channel", error))?
        .map_err(|error| super::super::error::runtime_call("join direct channel", error))?;

        timeout_runtime_call(
            &runtime,
            "start_direct_chat_with_authority",
            "amp_join_channel_contact",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || {
                runtime.amp_join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant: contact_authority,
                })
            },
        )
        .await
        .map_err(|error| super::super::error::runtime_call("add contact to direct channel", error))?
        .map_err(|error| {
            super::super::error::runtime_call("add contact to direct channel", error)
        })?;

        let chat_fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            channel_name.clone(),
            Some(format!("Direct messages with {contact_id}")),
            true,
            timestamp_ms,
            runtime.authority_id(),
        );
        let fact = chat_fact.to_generic();

        timeout_runtime_call(
            &runtime,
            "start_direct_chat_with_authority",
            "commit_relational_facts",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
        )
        .await
        .map_err(|error| super::super::error::runtime_call("persist direct channel", error))?
        .map_err(|error| super::super::error::runtime_call("persist direct channel", error))?;

        reduce_chat_fact_observed(app_core, &chat_fact).await?;
        send_chat_fact_with_retry(&runtime, contact_authority, context_id, &fact).await?;
        wait_for_runtime_channel_state(
            app_core,
            &runtime,
            AuthoritativeChannelRef::new(channel_id, context_id),
        )
        .await?;
        publish_authoritative_channel_membership_ready(
            app_core,
            channel_id,
            Some(channel_name.as_str()),
            2,
        )
        .await?;
        refresh_authoritative_channel_membership_readiness(app_core).await?;
        refresh_authoritative_recipient_resolution_readiness(app_core).await?;
        refresh_authoritative_delivery_readiness_for_channel(
            app_core,
            &runtime,
            AuthoritativeChannelRef::new(channel_id, context_id),
        )
        .await?;

        return Ok(channel_id);
    }

    let channel_id = dm_channel_id(&contact_id);
    let now = timestamp_ms;
    let dm_channel = Channel {
        id: channel_id,
        context_id: None,
        name: if contact_name.trim().is_empty() {
            format!("dm-{}", &contact_id[..8.min(contact_id.len())])
        } else {
            format!("DM: {contact_name}")
        },
        topic: Some(format!("Direct messages with {contact_id}")),
        channel_type: ChannelType::DirectMessage,
        unread_count: 0,
        is_dm: true,
        member_ids: vec![contact_authority],
        member_count: 2,
        last_message: None,
        last_message_time: None,
        last_activity: now,
        last_finalized_epoch: 0,
    };

    update_chat_projection_observed(app_core, |chat_state| {
        chat_state.add_channel(dm_channel);
    })
    .await?;

    Ok(channel_id)
}

pub async fn send_action(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    action: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let content = format!("* You {action}");
    send_message(app_core, channel_id, &content, timestamp_ms).await
}

pub async fn send_action_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    action: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let content = format!("* You {action}");
    send_message_by_name(app_core, channel_name, &content, timestamp_ms).await
}

/// Retry a failed chat message by canonical channel id and return the
/// directly-settled terminal status for frontend handoff consumers.
pub async fn retry_message_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::retry_message(),
        instance_id.clone(),
        SemanticOperationKind::RetryChatMessage,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        send_message_now_with_instance(app_core, channel_id, content, instance_id).await
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}

/// Retry a failed chat message by canonical channel name and return the
/// directly-settled terminal status for frontend handoff consumers.
pub async fn retry_message_by_name_with_terminal_status(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::retry_message(),
        instance_id.clone(),
        SemanticOperationKind::RetryChatMessage,
    );
    let result = async {
        owner
            .publish_phase(SemanticOperationPhase::WorkflowDispatched)
            .await?;
        send_message_by_name_now_with_instance(app_core, channel_name, content, instance_id).await
    }
    .await;
    crate::ui_contract::WorkflowTerminalOutcome {
        result,
        terminal: owner.terminal_status().await,
    }
}
