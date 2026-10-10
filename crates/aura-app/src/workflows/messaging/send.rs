#![allow(missing_docs)]

use super::*;
use std::future::Future;

#[derive(Debug, Clone, Error)]
pub(super) enum SendMessageError {
    #[error("Authoritative moderation denied message: {source}")]
    ModerationDenied {
        #[source]
        source: AuraError,
    },
    #[error("Failed to resolve channel {channel}: {detail}")]
    ChannelResolution { channel: String, detail: String },
    #[error("Recipient peers are not resolved for channel {channel_id}")]
    RecipientResolutionNotReady { channel_id: ChannelId },
    #[error("Peer channel establishment is not complete for channel {channel_id}")]
    DeliveryNotReady { channel_id: ChannelId },
    #[error("Authoritative readiness facts are unavailable: {detail}")]
    ReadinessFactsUnavailable {
        detail: String,
        #[source]
        source: AuraError,
    },
    #[error(
        "AMP channel bootstrap is unavailable for channel {channel_id} in context {context_id}"
    )]
    ChannelBootstrapUnavailable {
        channel_id: ChannelId,
        context_id: ContextId,
        #[source]
        source: AuraError,
    },
    #[error("Transport error while sending on channel {channel_id}: {detail}")]
    Transport {
        channel_id: ChannelId,
        detail: String,
        #[source]
        source: AuraError,
    },
}

impl SendMessageError {
    pub(super) fn semantic_error(&self) -> SemanticOperationError {
        match self {
            Self::ModerationDenied { source } => SemanticOperationError::new(
                SemanticFailureDomain::Command,
                send_transport_failure_code(source),
            )
            .with_detail(source.to_string()),
            Self::ChannelResolution { channel, detail } => SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::InternalError,
            )
            .with_detail(format!("channel={channel}; detail={detail}")),
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
            Self::ReadinessFactsUnavailable { detail, source } => SemanticOperationError::new(
                SemanticFailureDomain::Internal,
                send_transport_failure_code(source),
            )
            .with_detail(format!("semantic_readiness_unavailable: {detail}")),
            Self::ChannelBootstrapUnavailable {
                channel_id,
                context_id,
                ..
            } => SemanticOperationError::new(
                SemanticFailureDomain::Transport,
                SemanticFailureCode::ChannelBootstrapUnavailable,
            )
            .with_detail(format!("channel_id={channel_id}; context_id={context_id}")),
            Self::Transport {
                channel_id,
                detail,
                source,
            } => SemanticOperationError::new(
                SemanticFailureDomain::Transport,
                send_transport_failure_code(source),
            )
            .with_detail(format!("channel_id={channel_id}; detail={detail}")),
        }
    }
}

impl From<SendMessageError> for AuraError {
    fn from(error: SendMessageError) -> Self {
        AuraError::Internal {
            message: error.to_string(),
            source: Some(Arc::new(error)),
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum AmpSendRetryError {
    #[error("canonical AMP channel state required: {0}")]
    ChannelStateUnavailable(#[source] crate::runtime_bridge::RuntimeBridgeError),
    #[error("{0}")]
    Transport(#[source] AuraError),
}

fn send_transport_failure_code(error: &AuraError) -> SemanticFailureCode {
    if let Some(denial) = crate::workflows::moderation::denial_from_error(error) {
        return denial.semantic_code();
    }
    use crate::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind as K};
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        if let Some(native) = cause.downcast_ref::<RuntimeBridgeError>() {
            return match native.kind() {
                K::BudgetExceeded => SemanticFailureCode::BudgetExceeded,
                K::Unauthorized => SemanticFailureCode::PermissionDenied,
                K::Validation => SemanticFailureCode::InvalidArgument,
                K::NotFound | K::ContextNotFound => SemanticFailureCode::NotFound,
                K::NoAgent | K::Service => SemanticFailureCode::Unavailable,
                K::TimedOut => SemanticFailureCode::OperationTimedOut,
                K::Crypto => SemanticFailureCode::CryptoFailure,
                K::Serialization => SemanticFailureCode::SerializationFailure,
                K::Storage => SemanticFailureCode::StorageFailure,
                K::Journal => SemanticFailureCode::JournalFailure,
                K::Reactive => SemanticFailureCode::ReactiveFailure,
                K::Network => SemanticFailureCode::CommandFailed,
                K::Internal => SemanticFailureCode::InternalError,
            };
        }
        if let Some(budget) = cause.downcast_ref::<aura_core::TimeoutBudgetError>() {
            return crate::workflows::runtime_error_classification::timeout_budget_failure_code(
                budget,
            );
        }
        if matches!(
            cause.downcast_ref::<crate::workflows::error::WorkflowError>(),
            Some(crate::workflows::error::WorkflowError::TimedOut { .. })
        ) {
            return SemanticFailureCode::OperationTimedOut;
        }
        current = cause.source();
    }
    SemanticFailureCode::InternalError
}

type PostTerminalDelivery = (
    Arc<dyn RuntimeBridge>,
    AuthoritativeChannelRef,
    RelationalFact,
    AuthorityId,
    ContextId,
    String,
);

#[derive(Debug, Error)]
#[error("Message operation failed: {cause}; failure publication also failed: {publication}")]
struct SendMessageFailurePublication {
    #[source]
    cause: SendMessageError,
    publication: AuraError,
}

async fn fail_send_message<T>(
    owner: &SemanticWorkflowOwner,
    error: SendMessageError,
) -> Result<T, AuraError> {
    if let Err(publication) = publish_send_message_failure(owner, &error).await {
        let retained = SendMessageFailurePublication {
            cause: error,
            publication,
        };
        return Err(
            match crate::workflows::runtime_error_classification::native_runtime_error_kind(
                &retained,
            ) {
                Some(kind) => kind.wrap_source(retained.to_string(), retained),
                None => AuraError::Internal {
                    message: retained.to_string(),
                    source: Some(Arc::new(retained)),
                },
            },
        );
    }
    Err(error.into())
}

#[cfg(test)]
pub(super) async fn mark_message_delivery_failed(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    message_id: &str,
    actor_id: AuthorityId,
) -> Result<(), AuraError> {
    let runtime = { app_core.read().await.runtime().cloned() };
    let budget = match runtime.as_ref() {
        Some(runtime) => {
            Some(workflow_timeout_budget(runtime, MESSAGING_RUNTIME_OPERATION_TIMEOUT).await?)
        }
        None => None,
    };
    mark_message_delivery_failed_with_budget(
        app_core,
        context_id,
        channel_id,
        message_id,
        actor_id,
        budget.as_ref(),
    )
    .await
}

async fn mark_message_delivery_failed_with_budget(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    message_id: &str,
    actor_id: AuthorityId,
    parent: Option<&TimeoutBudget>,
) -> Result<(), AuraError> {
    let failed = ChatFact::message_delivery_updated_ms(
        context_id,
        channel_id,
        message_id.to_string(),
        ChatMessageDeliveryStatus::Failed,
        next_observed_projection_timestamp_ms(app_core).await,
        actor_id,
    );
    // Commit so the runtime chat view keeps the failure; an observed-only
    // update would be replaced on its next emission.
    if let Ok(runtime) = require_runtime(app_core).await {
        let generic = failed.to_generic();
        let committed = timeout_runtime_call_with_budget(
            &runtime,
            parent.ok_or_else(|| {
                AuraError::invalid("runtime delivery persistence requires original timeout budget")
            })?,
            "mark_message_delivery_failed",
            "commit_relational_facts",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.commit_relational_facts(std::slice::from_ref(&generic)),
        )
        .await?;
        committed.map_err(|error| {
            super::super::error::runtime_call("persist failed delivery status", error)
        })?;
    }
    reduce_chat_fact_observed(app_core, &failed).await?;

    tracing::warn!(
        message_id,
        "marked message delivery as failed after remote fanout exhaustion"
    );

    Ok(())
}

/// Typed diagnostic records for a failed remote delivery: one per
/// unreachable recipient, or one channel-level record otherwise.
fn outbound_delivery_failures(
    error: &AuraError,
    context_id: ContextId,
    channel_id: ChannelId,
    message_id: &str,
) -> Vec<crate::runtime_bridge::OutboundMessageDeliveryFailure> {
    use super::super::error::WorkflowError as W;
    use crate::runtime_bridge::{
        OutboundDeliveryFailureCause as Cause, OutboundMessageDeliveryFailure,
    };
    let record = |recipient, cause| OutboundMessageDeliveryFailure {
        context_id,
        channel_id,
        message_id: message_id.to_string(),
        recipient,
        cause,
    };
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        match current.downcast_ref::<W>() {
            Some(W::DeliveryFanoutUnavailable { recipients, .. }) if !recipients.is_empty() => {
                return recipients
                    .iter()
                    .map(|(peer, detail)| {
                        record(
                            Some(*peer),
                            Cause::RecipientUnreachable {
                                detail: detail.clone(),
                            },
                        )
                    })
                    .collect();
            }
            Some(W::DeliveryPrerequisitesNeverConverged { detail, .. }) => {
                return vec![record(
                    None,
                    Cause::PrerequisitesNeverConverged {
                        detail: detail.clone(),
                    },
                )];
            }
            Some(W::DeliveryRecipientsUnresolved { .. }) => {
                return vec![record(None, Cause::NoRecipients)]
            }
            _ => source = current.source(),
        }
    }
    vec![record(
        None,
        Cause::Other {
            detail: error.to_string(),
        },
    )]
}

/// One send of a committed message fact to each recipient. The send only cuts
/// latency: a recipient it misses receives the message through
/// relational-context sync (docs/111 §11.4), and the recipient's delivery receipt,
/// not this send, advances the message to Delivered. A failed send marks the
/// message Failed until such a receipt arrives.
async fn deliver_message_fact_remotely(
    runtime: &Arc<dyn RuntimeBridge>,
    channel: AuthoritativeChannelRef,
    sender_id: AuthorityId,
    fact: &RelationalFact,
    parent: &TimeoutBudget,
) -> Result<(), AuraError> {
    timeout_runtime_call_with_budget(
        runtime,
        parent,
        "deliver_message_fact_remotely",
        "delivery_followup",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || async {
            let context_id = channel.context_id();
            let channel_id = channel.channel_id();
            let recipients =
                authoritative_recipient_peers_for_channel(runtime, channel, sender_id, parent)
                    .await?;
            if recipients.is_empty() {
                return Err(
                    super::super::error::WorkflowError::DeliveryRecipientsUnresolved {
                        channel: channel_id.to_string(),
                        attempts: 1,
                    }
                    .into(),
                );
            }
            for peer in recipients.iter().copied() {
                // Route warmup; a failure surfaces as the send's own error.
                let _ = timeout_runtime_call_with_budget(
                    runtime,
                    parent,
                    "deliver_message_fact_remotely",
                    "ensure_peer_channel",
                    MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                    || runtime.ensure_peer_channel(context_id, peer),
                )
                .await?;
            }
            let mut failed_fanout = Vec::new();
            for peer in recipients {
                if let Err(error) = send_committed_fact_with_budget(
                    runtime,
                    parent,
                    "deliver_message_fact_remotely",
                    peer,
                    context_id,
                    fact,
                )
                .await
                {
                    failed_fanout.push((peer, error.to_string()));
                }
            }
            if failed_fanout.is_empty() {
                Ok(())
            } else {
                Err(
                    super::super::error::WorkflowError::DeliveryFanoutUnavailable {
                        peer: channel_id.to_string(),
                        attempts: 1,
                        recipients: failed_fanout,
                    }
                    .into(),
                )
            }
        },
    )
    .await?
}

/// Record a remote delivery that failed or never started: typed per-recipient
/// diagnostics plus a committed `Failed` status on the message, so a
/// committed send never stays silently at "sent".
async fn record_remote_delivery_failure(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    error: &AuraError,
    context_id: ContextId,
    channel_id: ChannelId,
    message_id: &str,
    sender_id: AuthorityId,
    parent: &TimeoutBudget,
) {
    tracing::warn!(
        error = %error,
        channel_id = %channel_id,
        message_id,
        "post-terminal remote message delivery failed"
    );
    for failure in outbound_delivery_failures(error, context_id, channel_id, message_id) {
        runtime.record_outbound_message_delivery_failure(failure);
    }
    if let Err(mark_error) = mark_message_delivery_failed_with_budget(
        app_core,
        context_id,
        channel_id,
        message_id,
        sender_id,
        Some(parent),
    )
    .await
    {
        tracing::warn!(
            delivery_error = %error,
            mark_error = %mark_error,
            message_id,
            "post-terminal remote message delivery failed and mark-failed also failed"
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_post_terminal_message_followups<F>(
    spawner: &aura_core::OwnedTaskSpawner,
    fut: F,
) -> Result<(), AuraError>
where
    F: Future<Output = Result<(), AuraError>> + Send + 'static,
{
    spawner.spawn_fallible_cancellable("messaging.remote_delivery", Box::pin(fut))
}

#[cfg(target_arch = "wasm32")]
fn spawn_post_terminal_message_followups<F>(
    spawner: &aura_core::OwnedTaskSpawner,
    fut: F,
) -> Result<(), AuraError>
where
    F: Future<Output = Result<(), AuraError>> + 'static,
{
    spawner.spawn_local_fallible_cancellable("messaging.remote_delivery", Box::pin(fut))
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
    let owner_runtime = { app_core.read().await.runtime().cloned() };
    let owner_budget = match owner_runtime.as_ref() {
        Some(runtime) => {
            Some(workflow_timeout_budget(runtime, MESSAGING_RUNTIME_OPERATION_TIMEOUT).await?)
        }
        None => None,
    };
    let mut post_terminal_delivery: Option<PostTerminalDelivery> = None;
    let (channel_id, channel_label) = match &channel {
        ChannelRef::Id(id) => (*id, id.to_string()),
        ChannelRef::Name(name) => {
            let resolution = if backend == MessagingBackend::LocalOnly {
                resolve_local_chat_channel_id_from_observed_state_or_input(app_core, name).await
            } else {
                resolve_chat_channel_id_from_state_or_input(
                    app_core,
                    name,
                    owner_budget.as_ref().ok_or_else(|| {
                        AuraError::invalid(
                            "runtime channel lookup requires original timeout budget",
                        )
                    })?,
                )
                .await
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

    let mut epoch_hint: Option<u32> = None;
    let (sender_id, message_id) = if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let parent = owner_budget
            .as_ref()
            .ok_or_else(|| AuraError::invalid("runtime send requires original timeout budget"))?;
        let sender_id = runtime.authority_id();
        let is_note_to_self = channel_id == note_to_self_channel_id(sender_id)
            || matches!(&channel, ChannelRef::Name(name) if is_note_to_self_channel_name(name));
        if is_note_to_self {
            let context_id = note_to_self_context_id(sender_id);
            let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
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
            timeout_runtime_call_with_budget(
                &runtime,
                parent,
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
            let authoritative_channel = match require_authoritative_channel_ref(
                app_core,
                &runtime,
                channel_id,
                "send message context",
                parent,
            )
            .await
            {
                Ok(channel) => channel,
                Err(error) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::Transport {
                            channel_id,
                            detail: error.to_string(),
                            source: error,
                        },
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
                        if let Err(error) = refresh_authoritative_channel_readiness_for_channel(
                            app_core,
                            authoritative_channel,
                            parent,
                        )
                        .await
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::ReadinessFactsUnavailable {
                                    detail: format!(
                                        "recipient resolution refresh failed for {channel_id}: {error}"
                                    ),
                                    source: error,
                                },
                            )
                            .await;
                        }
                        if let Err(error) = refresh_authoritative_delivery_readiness_for_channel(
                            app_core,
                            &runtime,
                            authoritative_channel,
                            parent,
                        )
                        .await
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::ReadinessFactsUnavailable {
                                    detail: format!(
                                        "delivery readiness refresh failed for {channel_id}: {error}"
                                    ),
                                    source: error,
                                },
                            )
                            .await;
                        }
                        if !warm_channel_connectivity(
                            app_core,
                            &runtime,
                            authoritative_channel,
                            parent,
                        )
                        .await?
                        {
                            return fail_send_message(
                                owner,
                                SendMessageError::DeliveryNotReady { channel_id },
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
            if let Err(error) = enforce_home_moderation_for_sender(
                app_core,
                context_id,
                channel_id,
                sender_id,
                timestamp_ms,
                parent,
            )
            .await
            {
                return fail_send_message(
                    owner,
                    SendMessageError::ModerationDenied { source: error },
                )
                .await;
            }
            // Readiness facts are retained across refreshes, so recheck the
            // authoritative AMP membership: success is reported before
            // delivery, and a shared-channel send with no recipient would
            // otherwise succeed with nothing to deliver.
            match authoritative_recipient_peers_for_channel(
                &runtime,
                authoritative_channel,
                sender_id,
                parent,
            )
            .await
            {
                Ok(recipients) if !recipients.is_empty() => {}
                Ok(_) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::RecipientResolutionNotReady { channel_id },
                    )
                    .await;
                }
                Err(error) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::ReadinessFactsUnavailable {
                            detail: format!(
                                "recipient resolution failed for {channel_id}: {error}"
                            ),
                            source: error,
                        },
                    )
                    .await;
                }
            }

            let send_params = ChannelSendParams {
                context: context_id,
                channel: channel_id,
                sender: sender_id,
                plaintext: content.as_bytes().to_vec(),
                reply_to: None,
            };
            let initial = match timeout_runtime_call_with_budget(
                &runtime,
                parent,
                "send_message_ref_owned",
                "amp_send_message",
                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                || runtime.amp_send_message(send_params.clone()),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::Transport {
                            channel_id,
                            detail: error.to_string(),
                            source: error,
                        },
                    )
                    .await
                }
            };
            let mut cipher_result: Result<_, AuraError> = match initial {
                Ok(cipher) => Ok(cipher),
                Err(error) if is_amp_channel_state_unavailable(&error, context_id, channel_id) => {
                    Err(super::super::error::runtime_call("AMP channel state", error).into())
                }
                Err(error) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::Transport {
                            channel_id,
                            detail: error.to_string(),
                            source: super::super::error::runtime_call("AMP send", error).into(),
                        },
                    )
                    .await
                }
            };
            if cipher_result.is_err() {
                let retry_policy = workflow_retry_policy(
                    AMP_SEND_RETRY_ATTEMPTS as u32,
                    Duration::from_millis(AMP_SEND_RETRY_BACKOFF_MS),
                    Duration::from_millis(
                        AMP_SEND_RETRY_BACKOFF_MS * AMP_SEND_RETRY_ATTEMPTS as u64,
                    ),
                )?;
                match execute_with_runtime_retry_budget(
                    &runtime,
                    parent,
                    &retry_policy,
                    |attempt, child| {
                        let runtime = Arc::clone(&runtime);
                        let send_params = send_params.clone();
                        async move {
                            if attempt > 0 {
                                converge_runtime(&runtime, &child)
                                    .await
                                    .map_err(AmpSendRetryError::Transport)?;
                            }
                            match timeout_runtime_call_with_budget(
                                &runtime,
                                &child,
                                "send_message_ref_owned",
                                "amp_send_message_retry",
                                MESSAGING_RUNTIME_OPERATION_TIMEOUT,
                                || runtime.amp_send_message(send_params),
                            )
                            .await
                            .map_err(AmpSendRetryError::Transport)?
                            {
                                Ok(cipher) => Ok(cipher),
                                Err(error)
                                    if is_amp_channel_state_unavailable(
                                        &error, context_id, channel_id,
                                    ) =>
                                {
                                    Err(AmpSendRetryError::ChannelStateUnavailable(error))
                                }
                                Err(error) => Err(AmpSendRetryError::Transport(
                                    super::super::error::runtime_call("AMP send retry", error)
                                        .into(),
                                )),
                            }
                        }
                    },
                )
                .await
                {
                    Ok(cipher) => cipher_result = Ok(cipher),
                    Err(RetryRunError::Timeout(timeout_error)) => {
                        return fail_send_message(
                            owner,
                            SendMessageError::Transport {
                                channel_id,
                                detail: timeout_error.to_string(),
                                source: timeout_error.into(),
                            },
                        )
                        .await
                    }
                    Err(RetryRunError::AttemptsExhausted {
                        last_error: AmpSendRetryError::ChannelStateUnavailable(error),
                        ..
                    }) => {
                        cipher_result = Err(super::super::error::runtime_call(
                            "AMP channel state after retries",
                            error,
                        )
                        .into());
                    }
                    Err(RetryRunError::AttemptsExhausted { last_error, .. }) => {
                        return fail_send_message(
                            owner,
                            SendMessageError::Transport {
                                channel_id,
                                detail: last_error.to_string(),
                                source: super::super::error::runtime_call(
                                    "AMP send retry",
                                    last_error,
                                )
                                .into(),
                            },
                        )
                        .await
                    }
                }
            }

            let cipher = match cipher_result {
                Ok(cipher) => cipher,
                Err(source) => {
                    return fail_send_message(
                        owner,
                        SendMessageError::ChannelBootstrapUnavailable {
                            channel_id,
                            context_id,
                            source,
                        },
                    )
                    .await
                }
            };
            let fact = {
                let wire = AmpMessage::new(cipher.header, cipher.ciphertext.clone());
                let sealed =
                    serialize_amp_message(&wire).map_err(super::super::error::fact_encoding)?;

                epoch_hint = Some(cipher.header.chan_epoch as u32);

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
                .to_generic()
            };

            {
                timeout_runtime_call_with_budget(
                    &runtime,
                    parent,
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
            }

            (sender_id, message_id)
        }
    } else {
        let sender_id = AuthorityId::new_from_entropy([1u8; 32]);
        let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
        (sender_id, message_id)
    };

    // Local echo of our own send. The sealed-fact reducer discards the
    // payload (it renders `<sealed message>`, not ours), so the sender's
    // plaintext copy is applied directly; the runtime view's later copy of
    // the same message id does not replace readable content.
    {
        update_chat_projection_observed(app_core, |chat_state| {
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
        let delivery_budget = match workflow_timeout_budget(
            &runtime,
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        )
        .await
        {
            Ok(budget) => budget,
            Err(error) => {
                let error = AuraError::from(error);
                for failure in
                    outbound_delivery_failures(&error, context_id, channel_id, &followup_message_id)
                {
                    runtime.record_outbound_message_delivery_failure(failure);
                }
                tracing::warn!(error = %error, "postterminal delivery owner could not establish its clock window");
                return Ok(message_id);
            }
        };
        let spawner = runtime.task_spawner();
        let delivery = {
            let app_core = app_core.clone();
            let runtime = runtime.clone();
            let followup_message_id = followup_message_id.clone();
            let delivery_budget = delivery_budget.clone();
            async move {
                if let Err(error) = deliver_message_fact_remotely(
                    &runtime,
                    authoritative_channel,
                    sender_id,
                    &fact,
                    &delivery_budget,
                )
                .await
                {
                    record_remote_delivery_failure(
                        &app_core,
                        &runtime,
                        &error,
                        context_id,
                        channel_id,
                        &followup_message_id,
                        sender_id,
                        &delivery_budget,
                    )
                    .await;
                }
                // The failure is recorded on the message and in the runtime's
                // delivery diagnostics; it is not a supervised-task fault.
                Ok(())
            }
        };
        // A delivery task the runtime refuses to admit would otherwise leave
        // the committed message at "sent" with nothing delivering it.
        if let Err(error) = spawn_post_terminal_message_followups(&spawner, delivery) {
            record_remote_delivery_failure(
                app_core,
                &runtime,
                &error,
                context_id,
                channel_id,
                &followup_message_id,
                sender_id,
                &delivery_budget,
            )
            .await;
        }
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
        let budget = workflow_timeout_budget(&runtime, MESSAGING_RUNTIME_OPERATION_TIMEOUT).await?;
        let parent = &budget;
        let context_id = pair_dm_context_id(runtime.authority_id(), contact_authority);
        let channel_name = if contact_name.trim().is_empty() {
            format!("dm-{}", &contact_id[..8.min(contact_id.len())])
        } else {
            format!("DM: {contact_name}")
        };
        let channel_id = pair_dm_channel_id(runtime.authority_id(), contact_authority);

        let create_result = timeout_runtime_call_with_budget(
            &runtime,
            parent,
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
        .await?;
        if let Err(error) = create_result {
            if !runtime_amp_duplicate_is_reconciled(
                &runtime, &error, context_id, channel_id, parent,
            )
            .await?
            {
                return Err(
                    super::super::error::runtime_call("create direct channel", error).into(),
                );
            }
        }

        timeout_runtime_call_with_budget(
            &runtime,
            parent,
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

        timeout_runtime_call_with_budget(
            &runtime,
            parent,
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

        timeout_runtime_call_with_budget(
            &runtime,
            parent,
            "start_direct_chat_with_authority",
            "commit_relational_facts",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.commit_relational_facts(std::slice::from_ref(&fact)),
        )
        .await
        .map_err(|error| super::super::error::runtime_call("persist direct channel", error))?
        .map_err(|error| super::super::error::runtime_call("persist direct channel", error))?;

        reduce_chat_fact_observed(app_core, &chat_fact).await?;
        // Latency only; the contact receives the creation fact through
        // relational-context sync if this send is lost.
        let _ = send_committed_fact_with_budget(
            &runtime,
            parent,
            "start_direct_chat_with_authority",
            contact_authority,
            context_id,
            &fact,
        )
        .await;
        // Direct chats need epoch-0 key material on both sides; deliver it to the
        // contact as a channel invitation, which the receiving runtime installs
        // without a manual accept (it recognises the pair DM channel).
        let bootstrap = timeout_runtime_call_with_budget(
            &runtime,
            parent,
            "start_direct_chat_with_authority",
            "amp_create_channel_bootstrap",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || {
                runtime.amp_create_channel_bootstrap(
                    context_id,
                    channel_id,
                    vec![contact_authority],
                )
            },
        )
        .await
        .map_err(|error| super::super::error::runtime_call("bootstrap direct channel", error))?
        .map_err(|error| super::super::error::runtime_call("bootstrap direct channel", error))?;
        crate::workflows::invitation::create_channel_invitation(
            app_core,
            contact_authority,
            channel_id.to_string(),
            Some(context_id),
            Some(channel_name.clone()),
            bootstrap,
            None,
            Some(parent.clone()),
            None,
            Some("Direct message".to_string()),
            None,
        )
        .await?;
        wait_for_runtime_channel_state(
            app_core,
            &runtime,
            AuthoritativeChannelRef::new(channel_id, context_id),
            parent,
        )
        .await?;
        publish_authoritative_channel_membership_ready(
            app_core,
            channel_id,
            Some(channel_name.as_str()),
            2,
        )
        .await?;
        refresh_authoritative_channel_readiness_for_channel(
            app_core,
            AuthoritativeChannelRef::new(channel_id, context_id),
            parent,
        )
        .await?;
        refresh_authoritative_delivery_readiness_for_channel(
            app_core,
            &runtime,
            AuthoritativeChannelRef::new(channel_id, context_id),
            parent,
        )
        .await?;

        return Ok(channel_id);
    }

    let channel_id = dm_channel_id(&contact_id);
    let local_context =
        ContextId::new_from_entropy(hash(format!("local-dm-context:{channel_id}").as_bytes()));
    let local_owner = { app_core.read().await.authority().cloned() }.unwrap_or_else(|| {
        AuthorityId::new_from_entropy(hash(format!("local-dm-owner:{channel_id}").as_bytes()))
    });
    let name = if contact_name.trim().is_empty() {
        format!("dm-{}", &contact_id[..8.min(contact_id.len())])
    } else {
        format!("DM: {contact_name}")
    };
    reduce_chat_fact_observed(
        app_core,
        &ChatFact::channel_created_ms(
            local_context,
            channel_id,
            name,
            Some(format!("Direct messages with {contact_id}")),
            true,
            timestamp_ms,
            local_owner,
        ),
    )
    .await?;
    reduce_chat_fact_observed(
        app_core,
        &ChatFact::channel_updated_ms(
            local_context,
            channel_id,
            None,
            None,
            Some(2),
            Some(vec![contact_authority]),
            timestamp_ms,
            local_owner,
        ),
    )
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

#[cfg(test)]
mod source_tests {
    use super::*;
    use crate::runtime_bridge::{RuntimeBridgeError, RuntimeBridgeErrorKind as K};
    use std::error::Error as _;

    fn has_io(error: &(dyn std::error::Error + 'static)) -> bool {
        let mut current = Some(error);
        while let Some(cause) = current {
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                return io.kind() == std::io::ErrorKind::ConnectionReset;
            }
            current = cause.source();
        }
        false
    }

    #[tokio::test]
    async fn actual_send_failure_publisher_fault_retains_original_concrete_source() {
        let authority = AuthorityId::new_from_entropy([242; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(authority));
        let app = Arc::new(RwLock::new(
            AppCore::with_runtime(crate::AppConfig::default(), runtime).unwrap(),
        ));
        {
            let core = app.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .expect("actual reactive graph");
        }
        let owner = SemanticWorkflowOwner::new(
            &app,
            OperationId::send_message(),
            Some(OperationInstanceId("publisher-fault-exact-instance".into())),
            SemanticOperationKind::SendChatMessage,
        );
        owner
            .publish_failure(SemanticOperationError::new(
                SemanticFailureDomain::Command,
                SemanticFailureCode::InvalidState,
            ))
            .await
            .expect("first actual terminal publication");
        let result: Result<(), AuraError> = fail_send_message(
            &owner,
            SendMessageError::Transport {
                channel_id: ChannelId::from_bytes([243; 32]),
                detail: "actual transport failure".into(),
                source: AuraError::Network {
                    message: "transport reset".into(),
                    source: Some(Arc::new(std::io::Error::from(
                        std::io::ErrorKind::ConnectionReset,
                    ))),
                },
            },
        )
        .await;
        let error = result.expect_err("already-terminal owner rejects second publication");
        assert!(has_io(&error));
        let retained = error
            .source()
            .unwrap()
            .downcast_ref::<SendMessageFailurePublication>()
            .expect("both faults retained by real failure publisher");
        assert!(matches!(retained.publication, AuraError::Invalid { .. }));
    }

    #[test]
    fn failed_terminal_publication_retains_original_and_publication_faults() {
        let cause = SendMessageError::Transport {
            channel_id: ChannelId::from_bytes([241; 32]),
            detail: "display irrelevant".into(),
            source: AuraError::Network {
                message: "transport unavailable".into(),
                source: Some(Arc::new(std::io::Error::from(
                    std::io::ErrorKind::ConnectionReset,
                ))),
            },
        };
        let failure = SendMessageFailurePublication {
            cause,
            publication: AuraError::Storage {
                message: "publisher unavailable".into(),
                source: Some(Arc::new(std::io::Error::from(
                    std::io::ErrorKind::PermissionDenied,
                ))),
            },
        };
        assert!(has_io(&failure));
        assert_eq!(
            failure
                .publication
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    #[test]
    fn retry_and_send_conversion_preserve_original_io_after_clone() {
        let native = RuntimeBridgeError::with_source(
            IntentError::network_error("send failed"),
            std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "actual transport reset",
            ),
        );
        let retry = AmpSendRetryError::Transport(
            super::super::super::error::runtime_call("retry", native).into(),
        );
        assert!(has_io(&retry));
        let error = SendMessageError::Transport {
            channel_id: ChannelId::from_bytes([0x7a; 32]),
            detail: retry.to_string(),
            source: super::super::super::error::runtime_call("AMP retry exhausted", retry).into(),
        };
        assert_eq!(
            error.semantic_error().code,
            SemanticFailureCode::CommandFailed
        );
        let cloned: AuraError = error.clone().into();
        assert!(cloned
            .source()
            .expect("send error retains cause")
            .is::<SendMessageError>());
        assert!(has_io(&cloned));
        assert!(has_io(&error));
    }

    #[test]
    fn transport_codes_use_exhaustive_native_kinds_without_display_policy() {
        for (kind, expected) in [
            (K::Unauthorized, SemanticFailureCode::PermissionDenied),
            (K::Validation, SemanticFailureCode::InvalidArgument),
            (K::NotFound, SemanticFailureCode::NotFound),
            (K::ContextNotFound, SemanticFailureCode::NotFound),
            (K::NoAgent, SemanticFailureCode::Unavailable),
            (K::Service, SemanticFailureCode::Unavailable),
            (K::TimedOut, SemanticFailureCode::OperationTimedOut),
            (K::Crypto, SemanticFailureCode::CryptoFailure),
            (K::Serialization, SemanticFailureCode::SerializationFailure),
            (K::Journal, SemanticFailureCode::JournalFailure),
            (K::Reactive, SemanticFailureCode::ReactiveFailure),
            (K::Network, SemanticFailureCode::CommandFailed),
            (K::Storage, SemanticFailureCode::StorageFailure),
            (K::Internal, SemanticFailureCode::InternalError),
        ] {
            let native = RuntimeBridgeError::with_source(
                IntentError::internal_error("timeout not found permission denied"),
                std::io::Error::other("misleading timeout display"),
            )
            .with_kind(kind);
            let error: AuraError =
                super::super::super::error::runtime_call("transport", native).into();
            assert_eq!(send_transport_failure_code(&error), expected);
            assert_eq!(send_transport_failure_code(&error.clone()), expected);
        }
        assert_eq!(
            send_transport_failure_code(&AuraError::internal(
                "deadline exceeded permission denied"
            )),
            SemanticFailureCode::InternalError
        );
    }

    #[test]
    fn required_clock_failure_is_unavailable_and_real_deadline_is_timeout() {
        let clock = aura_core::TimeoutBudgetError::time_source_failure(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "clock failed",
        ));
        let clock_error: AuraError = clock.into();
        assert_eq!(
            send_transport_failure_code(&clock_error),
            SemanticFailureCode::Unavailable
        );
        assert!(has_io(&clock_error));
        let deadline: AuraError = aura_core::TimeoutBudgetError::DeadlineExceeded {
            deadline_at_ms: 2,
            observed_at_ms: 3,
        }
        .into();
        assert_eq!(
            send_transport_failure_code(&deadline),
            SemanticFailureCode::OperationTimedOut
        );
        for budget in [
            aura_core::TimeoutBudgetError::invalid_policy("invalid"),
            aura_core::TimeoutBudgetError::AttemptBudgetExhausted {
                max_attempts: 1,
                attempts_used: 1,
            },
        ] {
            assert_eq!(
                send_transport_failure_code(&budget.into()),
                SemanticFailureCode::InvalidArgument
            );
        }
    }

    // Task 125: failed remote deliveries become typed diagnostic records,
    // one per unreachable recipient, found through wrapped error chains.
    #[test]
    fn outbound_delivery_failures_are_typed_per_recipient() {
        use crate::runtime_bridge::OutboundDeliveryFailureCause as Cause;
        use crate::workflows::error::WorkflowError as W;
        let context = ContextId::new_from_entropy([241u8; 32]);
        let channel = ChannelId::from_bytes([242u8; 32]);
        let (a, b) = (
            AuthorityId::new_from_entropy([243u8; 32]),
            AuthorityId::new_from_entropy([244u8; 32]),
        );
        let fanout: AuraError = W::DeliveryFanoutUnavailable {
            peer: channel.to_string(),
            attempts: 3,
            recipients: vec![(a, "reset".to_string()), (b, "timeout".to_string())],
        }
        .into();
        let wrapped = AuraError::Internal {
            message: "followup".to_string(),
            source: Some(Arc::new(fanout)),
        };
        let records = outbound_delivery_failures(&wrapped, context, channel, "m1");
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].recipient, Some(a));
        assert_eq!(records[1].recipient, Some(b));
        assert_eq!(
            records[1].cause,
            Cause::RecipientUnreachable {
                detail: "timeout".to_string()
            }
        );
        assert!(records
            .iter()
            .all(|r| r.message_id == "m1" && r.channel_id == channel && r.context_id == context));

        let unresolved: AuraError = W::DeliveryRecipientsUnresolved {
            channel: channel.to_string(),
            attempts: 3,
        }
        .into();
        let records = outbound_delivery_failures(&unresolved, context, channel, "m2");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].recipient, None);
        assert_eq!(records[0].cause, Cause::NoRecipients);

        let never: AuraError = W::DeliveryPrerequisitesNeverConverged {
            peer: channel.to_string(),
            attempts: 3,
            detail: "no peer channel".to_string(),
        }
        .into();
        assert_eq!(
            outbound_delivery_failures(&never, context, channel, "m3")[0].cause,
            Cause::PrerequisitesNeverConverged {
                detail: "no peer channel".to_string()
            }
        );

        let other = AuraError::agent("journal unavailable");
        assert!(matches!(
            outbound_delivery_failures(&other, context, channel, "m4")[0].cause,
            Cause::Other { .. }
        ));
    }
}
