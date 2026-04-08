//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

#[allow(unused_imports)]
use crate::thresholds::{default_channel_threshold, normalize_channel_threshold};
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, ChannelFactKey, OperationId,
    OperationInstanceId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase,
};
use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::chat_commands::normalize_channel_name;
#[allow(unused_imports)]
use crate::workflows::context::{authority_default_relational_context, current_home_context};
use crate::workflows::harness_determinism;
use crate::workflows::observed_projection::{
    reduce_chat_fact_observed, update_chat_projection_observed,
};
use crate::workflows::observed_snapshot::{observed_chat_snapshot, observed_contacts_snapshot};
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, ensure_runtime_peer_connectivity,
    execute_with_runtime_retry_budget, execute_with_runtime_timeout_budget, require_runtime,
    timeout_runtime_call, warn_workflow_timeout, workflow_best_effort, workflow_retry_policy,
    workflow_timeout_budget,
};
use crate::workflows::runtime_error_classification::{
    classify_amp_channel_error, classify_invitation_accept_error, AmpChannelErrorClass,
    InvitationAcceptErrorClass,
};
#[allow(unused_imports)]
use crate::workflows::semantic_facts::{
    authoritative_semantic_facts_snapshot, issue_channel_invitation_created_proof,
    issue_message_committed_proof, prove_channel_membership_ready,
    publish_authoritative_semantic_fact, semantic_readiness_publication_capability,
    update_authoritative_semantic_facts, SemanticWorkflowOwner,
};
use crate::workflows::signals::read_signal;
use crate::workflows::stage_tracker::{
    new_workflow_stage_tracker, update_workflow_stage, WorkflowStageTracker,
};
use crate::{
    core::IntentError,
    runtime_bridge::{InvitationBridgeType, InvitationInfo, RuntimeBridge},
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    views::chat::{
        is_note_to_self_channel_name, note_to_self_channel_id, note_to_self_context_id, Channel,
        ChannelType, ChatState, Message, MessageDeliveryStatus, NOTE_TO_SELF_CHANNEL_NAME,
        NOTE_TO_SELF_CHANNEL_TOPIC,
    },
    AppCore,
};
use async_lock::RwLock;
use aura_chat::{ChatFact, ChatMessageDeliveryStatus};
#[allow(unused_imports)]
use aura_core::effects::amp::{ChannelCloseParams, ChannelLeaveParams};
use aura_core::{
    crypto::hash::hash,
    effects::amp::{ChannelCreateParams, ChannelJoinParams, ChannelSendParams},
    types::{AuthorityId, ChannelId, ContextId},
    AuraError, InvitationId, OperationContext, RetryRunError, TimeoutBudget, TimeoutBudgetError,
    TimeoutRunError, TraceContext,
};
use aura_journal::fact::{FactOptions, RelationalFact};
use aura_journal::DomainFact;
use aura_protocol::amp::{serialize_amp_message, AmpMessage};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

const CHAT_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const CHAT_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
pub(crate) const AMP_SEND_RETRY_ATTEMPTS: usize = 6;
pub(crate) const AMP_SEND_RETRY_BACKOFF_MS: u64 = 75;
const CHANNEL_CONTEXT_RETRY_ATTEMPTS: usize = 12;
const CHANNEL_CONTEXT_RETRY_BACKOFF_MS: u64 = 100;
const REMOTE_DELIVERY_RETRY_ATTEMPTS: usize = 24;
const REMOTE_DELIVERY_RETRY_BACKOFF_MS: u64 = 250;
const INVITE_USER_STAGE_TIMEOUT_MS: u64 = 20_000;
const INVITE_USER_OPERATION_TIMEOUT_MS: u64 = 15_000;
const MESSAGING_RUNTIME_QUERY_TIMEOUT: Duration = Duration::from_millis(5_000);
const MESSAGING_RUNTIME_OPERATION_TIMEOUT: Duration = Duration::from_millis(30_000);

#[cfg(feature = "instrumented")]
macro_rules! messaging_warn {
    ($($arg:tt)*) => {
        tracing::warn!($($arg)*)
    };
}

#[cfg(not(feature = "instrumented"))]
macro_rules! messaging_warn {
    ($($arg:tt)*) => {};
}

mod channel_refs;
mod channels;
mod followups;
mod invites;
mod readiness;
mod routing;
mod send;
mod validation;

#[cfg(test)]
use channel_refs::ensure_channel_visible_after_join;
pub(in crate::workflows) use channel_refs::{
    apply_authoritative_membership_projection, authoritative_join_member_count_if_joined,
    authoritative_recipient_peers_for_channel, context_id_for_channel,
    runtime_channel_state_exists, wait_for_runtime_channel_state,
};
pub use channel_refs::{
    authoritative_channel_ref, current_home_channel_id, current_home_channel_ref,
    materialize_authoritative_channel_binding_observed,
    require_authoritative_context_id_for_channel, resolve_authoritative_context_id_for_channel,
    AuthoritativeChannelRef, CreatedChannel,
};
#[allow(unused_imports)]
use channel_refs::{
    canonical_channel_name_hint_for_invite, next_observed_projection_timestamp_ms,
    require_authoritative_channel_ref, resolve_authoritative_channel_binding_from_input,
};
use channels::JoinChannelError;
pub use channels::{
    close_channel, close_channel_by_name, create_channel,
    create_channel_with_authoritative_binding,
    join_authoritative_channel_binding_with_terminal_status, join_channel, join_channel_by_name,
    join_channel_by_name_with_binding_terminal_status, join_channel_by_name_with_instance,
    join_channel_by_name_with_terminal_status, leave_channel, leave_channel_by_name, set_topic,
    set_topic_by_name,
};
pub(in crate::workflows) use followups::post_terminal_join_followups;
pub use followups::run_post_channel_invite_followups;
use followups::warm_channel_connectivity;
pub use invites::{
    invite_authority_to_channel, invite_authority_to_channel_with_context_terminal_status,
    invite_user_to_channel, invite_user_to_channel_with_context,
    invite_user_to_channel_with_context_terminal_status,
};
#[cfg(test)]
use readiness::{
    authoritative_send_readiness_for_channel, channel_id_from_pending_channel_invitation,
    select_pending_channel_invitation, ChannelReadinessCoordinator, ChannelReadinessState,
};
#[allow(unused_imports)]
use readiness::{
    bootstrap_required_for_recipients, clear_authoritative_channel_readiness_facts,
    publish_message_committed_fact, publish_send_message_failure,
    refresh_authoritative_delivery_readiness_for_channel, require_send_message_readiness,
    try_join_via_pending_channel_invitation,
};
pub(in crate::workflows) use readiness::{
    ensure_runtime_note_to_self_channel, publish_authoritative_channel_membership_ready,
    refresh_authoritative_channel_membership_readiness,
    refresh_authoritative_recipient_resolution_readiness,
};
#[cfg(test)]
use send::mark_message_delivery_failed;
use send::SendMessageError;
pub use send::{
    retry_message_by_name_with_terminal_status, retry_message_with_terminal_status, send_action,
    send_action_by_name, send_message, send_message_by_name,
    send_message_by_name_now_with_instance, send_message_by_name_now_with_terminal_status,
    send_message_by_name_with_instance, send_message_by_name_with_terminal_status,
    send_message_now, send_message_now_with_instance, send_message_now_with_terminal_status,
    send_message_ref, send_message_ref_with_instance, send_message_with_instance,
    send_message_with_terminal_status, start_direct_chat, start_direct_chat_with_authority,
};

async fn timeout_workflow_stage_with_deadline<T>(
    runtime: &Arc<dyn RuntimeBridge>,
    operation: &'static str,
    stage: &'static str,
    deadline: Option<TimeoutBudget>,
    future: impl Future<Output = Result<T, AuraError>>,
) -> Result<T, AuraError> {
    let requested = deadline
        .map(|deadline| {
            Duration::from_millis(deadline.timeout_ms())
                .min(Duration::from_millis(INVITE_USER_STAGE_TIMEOUT_MS))
        })
        .unwrap_or(Duration::from_millis(INVITE_USER_STAGE_TIMEOUT_MS));
    let budget = match workflow_timeout_budget(runtime, requested).await {
        Ok(budget) => budget,
        Err(TimeoutBudgetError::DeadlineExceeded { .. }) => {
            warn_workflow_timeout(operation, stage, 0);
            return Err(AuraError::from(super::error::WorkflowError::TimedOut {
                operation,
                stage,
                timeout_ms: 0,
            }));
        }
        Err(error) => return Err(error.into()),
    };
    match execute_with_runtime_timeout_budget(runtime, &budget, || future).await {
        Ok(value) => Ok(value),
        Err(TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. })) => {
            warn_workflow_timeout(operation, stage, budget.timeout_ms());
            Err(AuraError::from(super::error::WorkflowError::TimedOut {
                operation,
                stage,
                timeout_ms: budget.timeout_ms(),
            }))
        }
        Err(TimeoutRunError::Timeout(error)) => Err(error.into()),
        Err(TimeoutRunError::Operation(error)) => Err(error),
    }
}

/// Messaging backend policy (runtime-backed vs UI-local).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagingBackend {
    /// Use runtime bridge for AMP + persisted facts.
    Runtime,
    /// UI-local state only (no runtime calls).
    LocalOnly,
}

async fn messaging_backend(app_core: &Arc<RwLock<AppCore>>) -> MessagingBackend {
    let core = app_core.read().await;
    if core.runtime().is_some() {
        MessagingBackend::Runtime
    } else {
        MessagingBackend::LocalOnly
    }
}

pub(crate) fn is_amp_channel_state_unavailable(error: &impl std::fmt::Display) -> bool {
    classify_amp_channel_error(error) == AmpChannelErrorClass::ChannelStateUnavailable
}

/// Create a deterministic ChannelId from a DM channel descriptor string
fn dm_channel_id(target: &str) -> ChannelId {
    let descriptor = format!("dm:{target}");
    ChannelId::from_bytes(hash(descriptor.as_bytes()))
}

fn pair_dm_channel_id(left: AuthorityId, right: AuthorityId) -> ChannelId {
    let mut participants = [left.to_string(), right.to_string()];
    participants.sort();
    let descriptor = format!("dm:{}:{}", participants[0], participants[1]);
    ChannelId::from_bytes(hash(descriptor.as_bytes()))
}

fn pair_dm_context_id(left: AuthorityId, right: AuthorityId) -> ContextId {
    let mut participants = [left.to_string(), right.to_string()];
    participants.sort();
    let descriptor = format!("dm-context:{}:{}", participants[0], participants[1]);
    ContextId::new_from_entropy(hash(descriptor.as_bytes()))
}

fn channel_id_from_input(channel: &str) -> Result<ChannelId, AuraError> {
    routing::channel_id_from_input(channel)
}

async fn resolve_chat_channel_id_from_state_or_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<ChannelId, AuraError> {
    routing::resolve_chat_channel_id_from_state_or_input(app_core, channel_input).await
}

async fn resolve_local_chat_channel_id_from_observed_state_or_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<ChannelId, AuraError> {
    routing::resolve_local_chat_channel_id_from_observed_state_or_input(app_core, channel_input)
        .await
}

async fn matching_local_chat_channel_ids_from_observed_state(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<Vec<ChannelId>, AuraError> {
    routing::matching_local_chat_channel_ids_from_observed_state(app_core, channel_input).await
}

fn hex_prefix(bytes: &[u8], byte_len: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(byte_len * 2);
    for byte in bytes.iter().take(byte_len) {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn next_message_id(
    channel_id: ChannelId,
    sender_id: AuthorityId,
    timestamp_ms: u64,
    content: &str,
) -> String {
    let channel_string = channel_id.to_string();
    let sender_string = sender_id.to_string();
    let timestamp_string = timestamp_ms.to_string();
    let local_nonce = harness_determinism::parity_generated_nonce(
        "message-id",
        &[&channel_string, &sender_string, &timestamp_string, content],
    );
    let digest =
        hash(format!("{channel_id}:{sender_id}:{timestamp_ms}:{local_nonce}:{content}").as_bytes());
    let suffix = hex_prefix(&digest, 8);
    format!("msg-{channel_id}-{timestamp_ms}-{suffix}")
}

fn is_invitation_capability_missing(error: &AuraError) -> bool {
    validation::is_invitation_capability_missing(error)
}

async fn send_chat_fact_with_retry(
    runtime: &Arc<dyn RuntimeBridge>,
    peer: AuthorityId,
    context: ContextId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let retry_policy = workflow_retry_policy(
        CHAT_FACT_SEND_MAX_ATTEMPTS as u32,
        Duration::from_millis(1),
        Duration::from_millis(1),
    )?;
    let mut attempts = retry_policy.attempt_budget();
    let last_error = loop {
        let _attempt = attempts.record_attempt()?;
        match timeout_runtime_call(
            runtime,
            "send_chat_fact_with_retry",
            "send_chat_fact",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.send_chat_fact(peer, context, fact),
        )
        .await
        {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(error)) => {
                if attempts.can_attempt() {
                    converge_runtime(runtime).await;
                    for _ in 0..CHAT_FACT_SEND_YIELDS_PER_RETRY {
                        cooperative_yield().await;
                    }
                    continue;
                }
                break error.to_string();
            }
            Err(error) => {
                if attempts.can_attempt() {
                    converge_runtime(runtime).await;
                    for _ in 0..CHAT_FACT_SEND_YIELDS_PER_RETRY {
                        cooperative_yield().await;
                    }
                    continue;
                }
                break error.to_string();
            }
        }
    };

    Err(super::error::WorkflowError::DeliveryFailed {
        peer: peer.to_string(),
        attempts: CHAT_FACT_SEND_MAX_ATTEMPTS,
        source: AuraError::agent(last_error),
    }
    .into())
}

async fn resolve_target_authority_for_invite(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
) -> Result<AuthorityId, AuraError> {
    routing::resolve_target_authority_for_invite(app_core, target_user_id).await
}

#[cfg(test)]
fn join_error_is_not_found(error: &AuraError) -> bool {
    if matches!(error, AuraError::NotFound { .. }) {
        return true;
    }

    let lowered = error.to_string().to_ascii_lowercase();
    lowered.contains("not found")
        || lowered.contains("unknown channel")
        || lowered.contains("no such channel")
}

fn intent_error_is_not_found(error: &IntentError) -> bool {
    validation::intent_error_is_not_found(error)
}

async fn enforce_home_moderation_for_sender(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    sender_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    validation::enforce_home_moderation_for_sender(
        app_core,
        context_id,
        channel_id,
        sender_id,
        timestamp_ms,
    )
    .await
}

async fn enforce_home_join_allowed(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_id: ChannelId,
    authority_id: AuthorityId,
) -> Result<(), AuraError> {
    validation::enforce_home_join_allowed(app_core, context_id, channel_id, authority_id).await
}

async fn warm_invited_peer_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    context_id: ContextId,
    receiver: AuthorityId,
) -> bool {
    let authority_context = authority_default_relational_context(receiver);
    for _ in 0..8 {
        let receiver_peer_id = receiver.to_string();
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "trigger_discovery",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_discovery(),
        )
        .await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "ensure_authority_peer_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.ensure_peer_channel(authority_context, receiver),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "ensure_peer_channel",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.ensure_peer_channel(context_id, receiver),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "sync_with_peer",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.sync_with_peer(&receiver_peer_id),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "process_ceremony_messages",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.process_ceremony_messages(),
        )
        .await;
        let _ = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "trigger_sync",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.trigger_sync(),
        )
        .await;
        converge_runtime(runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        let _ = crate::workflows::network::refresh_discovered_peers(app_core).await;
        let peer_online = timeout_runtime_call(
            runtime,
            "invite_authority_to_channel",
            "is_peer_online",
            MESSAGING_RUNTIME_OPERATION_TIMEOUT,
            || runtime.is_peer_online(receiver),
        )
        .await
        .unwrap_or(false);
        if peer_online {
            return true;
        }
    }

    false
}

async fn ensure_invited_peer_channel(
    runtime: &Arc<dyn RuntimeBridge>,
    context_id: ContextId,
    receiver: AuthorityId,
) -> Result<(), AuraError> {
    timeout_runtime_call(
        runtime,
        "invite_authority_to_channel",
        "ensure_invited_peer_channel",
        MESSAGING_RUNTIME_OPERATION_TIMEOUT,
        || runtime.ensure_peer_channel(context_id, receiver),
    )
    .await
    .map_err(|error| {
        AuraError::from(crate::workflows::error::runtime_call(
            "ensure invited peer channel",
            error,
        ))
    })?
    .map_err(|error| {
        AuraError::from(crate::workflows::error::runtime_call(
            "ensure invited peer channel",
            error,
        ))
    })?;
    converge_runtime(runtime).await;
    Ok(())
}

/// Send a direct message to a contact
///
/// **What it does**: Sends a message in a DM channel with the contact
/// **Returns**: DM channel ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// This operation:
/// 1. Creates DM channel if it doesn't exist
/// 2. Adds message to chat state
/// 3. ViewState update auto-forwards to CHAT_SIGNAL for UI updates
///
/// **Note**: Full implementation would use Intent::SendMessage for persistence.
/// Currently updates chat state locally for UI responsiveness.
///
/// # Arguments
/// * `app_core` - The application core
/// * `target` - Target contact ID
/// * `content` - Message content
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn send_direct_message(
    app_core: &Arc<RwLock<AppCore>>,
    target: &str,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let contact = crate::workflows::query::resolve_contact(app_core, target).await?;
    let channel_id =
        send_direct_message_to_authority(app_core, contact.id, content, timestamp_ms).await?;
    Ok(channel_id.to_string())
}

/// Send a direct message to a canonical authority ID.
pub async fn send_direct_message_to_authority(
    app_core: &Arc<RwLock<AppCore>>,
    target: AuthorityId,
    content: &str,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    if let Ok(runtime) = require_runtime(app_core).await {
        if target == runtime.authority_id() {
            return Err(AuraError::invalid("Cannot send direct message to yourself"));
        }
    }

    let channel_id = start_direct_chat_with_authority(app_core, target, timestamp_ms).await?;
    send_message(app_core, channel_id, content, timestamp_ms).await?;
    Ok(channel_id)
}

/// Typed frontend handoff facades for messaging workflows.
pub mod handoff {
    use super::*;

    /// Strongest available target for a chat-send handoff.
    #[derive(Debug, Clone)]
    pub enum SendChatTarget {
        /// Canonical channel id.
        ChannelId(ChannelId),
        /// Canonical channel name.
        ChannelName(String),
    }

    /// Inputs for sending a chat message through the handoff path.
    #[derive(Debug, Clone)]
    pub struct SendChatMessageRequest {
        /// Strongest available channel target.
        pub target: SendChatTarget,
        /// Message content.
        pub content: String,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for joining a channel by name through the handoff path.
    #[derive(Debug, Clone)]
    pub struct JoinChannelByNameRequest {
        /// Canonical channel name as entered by the frontend.
        pub channel_name: String,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for retrying a previously failed chat message.
    #[derive(Debug, Clone)]
    pub struct RetryChatMessageRequest {
        /// Strongest available channel target.
        pub target: SendChatTarget,
        /// Message content to retry.
        pub content: String,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
    }

    /// Inputs for inviting an authoritative actor to an authoritative channel.
    #[derive(Debug, Clone)]
    pub struct InviteAuthorityToChannelRequest {
        /// Receiver authority id.
        pub receiver: AuthorityId,
        /// Canonical channel id.
        pub channel_id: ChannelId,
        /// Optional authoritative context id.
        pub context_id: Option<ContextId>,
        /// Optional canonical channel-name hint.
        pub channel_name_hint: Option<String>,
        /// Optional frontend-owned semantic instance id.
        pub operation_instance_id: Option<OperationInstanceId>,
        /// Optional invitation message.
        pub message: Option<String>,
        /// Optional invitation TTL in milliseconds.
        pub ttl_ms: Option<u64>,
    }

    /// Send a chat message through one typed handoff workflow.
    pub async fn send_chat_message(
        app_core: &Arc<RwLock<AppCore>>,
        request: SendChatMessageRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        match request.target {
            SendChatTarget::ChannelId(channel_id) => {
                super::send_message_now_with_terminal_status(
                    app_core,
                    channel_id,
                    &request.content,
                    request.operation_instance_id,
                )
                .await
            }
            SendChatTarget::ChannelName(channel_name) => {
                super::send_message_by_name_now_with_terminal_status(
                    app_core,
                    &channel_name,
                    &request.content,
                    request.operation_instance_id,
                )
                .await
            }
        }
    }

    /// Join a channel by name as one typed handoff workflow.
    pub async fn join_channel_by_name(
        app_core: &Arc<RwLock<AppCore>>,
        request: JoinChannelByNameRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        super::join_channel_by_name_with_terminal_status(
            app_core,
            &request.channel_name,
            request.operation_instance_id,
        )
        .await
    }

    /// Retry a failed chat message through one typed handoff workflow.
    pub async fn retry_chat_message(
        app_core: &Arc<RwLock<AppCore>>,
        request: RetryChatMessageRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<String> {
        match request.target {
            SendChatTarget::ChannelId(channel_id) => {
                super::retry_message_with_terminal_status(
                    app_core,
                    channel_id,
                    &request.content,
                    request.operation_instance_id,
                )
                .await
            }
            SendChatTarget::ChannelName(channel_name) => {
                super::retry_message_by_name_with_terminal_status(
                    app_core,
                    &channel_name,
                    &request.content,
                    request.operation_instance_id,
                )
                .await
            }
        }
    }

    /// Invite an authoritative receiver to an authoritative channel.
    pub async fn invite_authority_to_channel(
        app_core: &Arc<RwLock<AppCore>>,
        request: InviteAuthorityToChannelRequest,
    ) -> crate::ui_contract::WorkflowTerminalOutcome<InvitationId> {
        super::invite_authority_to_channel_with_context_terminal_status(
            app_core,
            request.receiver,
            request.channel_id,
            request.context_id,
            request.channel_name_hint,
            request.operation_instance_id,
            request.message,
            request.ttl_ms,
        )
        .await
    }
}

/// Start a direct chat with a contact
///
/// **What it does**: Creates a DM channel and selects it
/// **Returns**: DM channel ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// This operation:
/// 1. Gets contact name from ViewState
/// 2. Creates DM channel if it doesn't exist
/// 3. Selects the channel for active conversation
/// 4. ViewState update auto-forwards to CHAT_SIGNAL for UI updates
///
/// # Arguments
/// * `app_core` - The application core
/// * `contact_id` - Contact ID to start chat with
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
///
/// Get current chat state
///
/// **What it does**: Reads chat state from ViewState
/// **Returns**: Current chat state with channels and messages
/// **Signal pattern**: Read-only operation (no emission)
// OWNERSHIP: observed
pub async fn get_chat_state(app_core: &Arc<RwLock<AppCore>>) -> Result<ChatState, AuraError> {
    Ok(observed_chat_snapshot(app_core).await)
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime_bridge::InvitationBridgeStatus;
    use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
    use crate::ui_contract::{SemanticOperationStatus, WorkflowTerminalStatus};
    use crate::views::contacts::{Contact, ContactsState};
    use crate::views::home::{BanRecord, HomeRole, HomeState, HomesState, MuteRecord};
    use crate::workflows::semantic_facts::{
        assert_succeeded_with_postcondition, assert_terminal_failure_or_cancelled,
    };
    use crate::workflows::signals::read_signal_or_default;
    use crate::AppConfig;

    async fn register_signals_only(app_core: &Arc<RwLock<AppCore>>) {
        let core = app_core.read().await;
        crate::signal_defs::register_app_signals(&*core)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_get_chat_state_default() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let state = get_chat_state(&app_core).await.unwrap();
        assert!(state.is_empty());
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_ignores_observed_channels_without_authoritative_membership_fact(
    ) {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready"));
        let peer = AuthorityId::new_from_entropy([55u8; 32]);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(!facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_recreates_fact_from_authoritative_channel_binding(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([63u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"recreated-membership-ready"));
        let context_id = ContextId::new_from_entropy([64u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, Vec::new());
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && channel.name.as_deref() == Some("shared-parity-lab")
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_does_not_require_participant_lookup(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([65u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready-no-participants"));
        let context_id = ContextId::new_from_entropy([66u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && channel.name.as_deref() == Some("shared-parity-lab")
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_revalidates_existing_membership_facts(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([111u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready-authoritative"));
        let context_id = ContextId::new_from_entropy([112u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, Vec::new());
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            1,
        )
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && channel.name.as_deref() == Some("shared-parity-lab")
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_retains_existing_fact_on_transient_runtime_miss(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([121u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready-transient-miss"));
        let context_id = ContextId::new_from_entropy([122u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_state_exists(context_id, channel_id, false);

        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            1,
        )
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && channel.name.as_deref() == Some("shared-parity-lab")
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_channel_membership_readiness_preserves_existing_fact_when_refresh_snapshot_is_empty(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([123u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready-empty-refresh"));
        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            1,
        )
        .await
        .unwrap();

        refresh_authoritative_channel_membership_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_refresh_authoritative_recipient_resolution_readiness_preserves_existing_fact_when_refresh_snapshot_is_empty(
    ) {
        let config = AppConfig::default();
        let local = AuthorityId::new_from_entropy([124u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime;
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"recipient-ready-empty-refresh"));
        publish_authoritative_semantic_fact(
            &app_core,
            aura_core::AuthorizedReadinessPublication::authorize(
                semantic_readiness_publication_capability(),
                AuthoritativeSemanticFact::RecipientPeersResolved {
                    channel: ChannelFactKey {
                        id: Some(channel_id.to_string()),
                        name: Some("shared-parity-lab".to_string()),
                    },
                    member_count: 2,
                },
            ),
        )
        .await
        .unwrap();

        refresh_authoritative_recipient_resolution_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
    }

    #[tokio::test]
    async fn test_create_channel_publishes_authoritative_membership_readiness() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        register_signals_only(&app_core).await;

        let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42)
            .await
            .unwrap();
        let channel_id_string = channel_id.to_string();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && channel.name.as_deref() == Some("shared-parity-lab")
                        && *member_count == 1
            )
        }));
    }

    #[tokio::test]
    async fn test_create_channel_publishes_authoritative_operation_status() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let _channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42)
            .await
            .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::OperationStatus {
                    operation_id,
                    status,
                    ..
                } if *operation_id == OperationId::create_channel()
                    && status.kind == SemanticOperationKind::CreateChannel
                    && status.phase == SemanticOperationPhase::Succeeded
            )
        }));
    }

    #[tokio::test]
    async fn test_join_channel_terminal_status_success_implies_membership_ready_postcondition() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        join_channel_by_name_with_instance(
            &app_core,
            "shared-parity-lab",
            Some(OperationInstanceId("join-postcondition-1".to_string())),
        )
        .await
        .unwrap();

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert_succeeded_with_postcondition(
            &facts,
            &OperationId::join_channel(),
            &OperationInstanceId("join-postcondition-1".to_string()),
            SemanticOperationKind::JoinChannel,
            |facts| {
                facts.iter().any(|fact| {
                    matches!(
                        fact,
                        AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                            if channel.name.as_deref() == Some("shared-parity-lab")
                    )
                })
            },
        );
    }

    #[tokio::test]
    async fn authoritative_context_uses_runtime_context_over_stale_projection() {
        let config = AppConfig::default();
        let owner = AuthorityId::new_from_entropy([91u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        let channel_id = ChannelId::from_bytes(hash(b"home-context-preferred"));
        let authoritative_context = ContextId::new_from_entropy([92u8; 32]);
        let stale_chat_context = ContextId::new_from_entropy([93u8; 32]);
        runtime.set_amp_channel_context(channel_id, authoritative_context);

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_chat(ChatState::from_channels(vec![Channel {
                    id: channel_id,
                    context_id: Some(stale_chat_context),
                    name: "shared-parity-lab".to_string(),
                    topic: None,
                    channel_type: ChannelType::Home,
                    unread_count: 0,
                    is_dm: false,
                    member_ids: Vec::new(),
                    member_count: 1,
                    last_message: None,
                    last_message_time: None,
                    last_activity: 0,
                    last_finalized_epoch: 0,
                }]));
        }

        assert_eq!(
            resolve_authoritative_context_id_for_channel(&app_core, channel_id).await,
            Some(authoritative_context)
        );
    }

    #[tokio::test]
    async fn authoritative_context_id_for_channel_does_not_fallback_to_pending_channel_invitation()
    {
        let config = AppConfig::default();
        let owner = AuthorityId::new_from_entropy([94u8; 32]);
        let sender = AuthorityId::new_from_entropy([95u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        let channel_id = ChannelId::from_bytes(hash(b"pending-home-context-fallback"));
        let invitation_context = ContextId::new_from_entropy([96u8; 32]);

        runtime.set_pending_invitations(vec![InvitationInfo {
            invitation_id: InvitationId::new("inv-pending-channel-context"),
            sender_id: sender,
            receiver_id: owner,
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(invitation_context),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 42,
            expires_at_ms: None,
            message: None,
        }]);

        assert_eq!(
            resolve_authoritative_context_id_for_channel(&app_core, channel_id).await,
            None
        );
    }

    #[tokio::test]
    async fn test_refresh_authoritative_recipient_resolution_readiness_tracks_resolved_peers() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let local = AuthorityId::new_from_entropy([56u8; 32]);
        let peer = AuthorityId::new_from_entropy([57u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"recipient-ready"));
        {
            let mut core = app_core.write().await;
            core.set_authority(local);
        }

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        refresh_authoritative_recipient_resolution_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(!facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, .. }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
            )
        }));
    }

    #[tokio::test]
    async fn test_authoritative_recipient_resolution_ignores_projection_members_without_runtime_participants(
    ) {
        let local = AuthorityId::new_from_entropy([101u8; 32]);
        let context_id = ContextId::new_from_entropy([103u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"authoritative-recipient-runtime-empty"));
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, Vec::new());
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let recipients = authoritative_recipient_peers_for_channel(
            &runtime_bridge,
            AuthoritativeChannelRef::new(channel_id, context_id),
            local,
        )
        .await
        .expect("authoritative recipient query should succeed");

        assert!(
            recipients.is_empty(),
            "projection members must not be treated as authoritative recipients"
        );
    }

    #[tokio::test]
    async fn test_refresh_authoritative_recipient_resolution_uses_observed_context_fallback() {
        let local = AuthorityId::new_from_entropy([113u8; 32]);
        let peer = AuthorityId::new_from_entropy([114u8; 32]);
        let context_id = ContextId::new_from_entropy([115u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"recipient-resolution-observed-context"));
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        runtime.set_amp_channel_participants(context_id, channel_id, vec![local, peer]);
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;
        {
            let mut core = app_core.write().await;
            core.set_authority(local);
        }

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();
        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            1,
        )
        .await
        .unwrap();

        refresh_authoritative_recipient_resolution_readiness(&app_core)
            .await
            .unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count >= 1
            )
        }));
    }

    #[test]
    fn test_delivery_readiness_facts_emit_peer_and_channel_facts() {
        let peer = AuthorityId::new_from_entropy([59u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"delivery-ready"));
        let context_id = ContextId::new_from_entropy([61u8; 32]);
        let state = ChannelReadinessState::new(
            channel_id,
            ChannelFactKey {
                id: Some(channel_id.to_string()),
                name: Some("shared-parity-lab".to_string()),
            },
            2,
            Some(AuthoritativeChannelRef::new(channel_id, context_id)),
            vec![peer],
            false,
        );

        let (peer_facts, delivery_fact) = state.delivery_facts(context_id, &[peer]);

        let channel_id_string = channel_id.to_string();
        let peer_id_string = peer.to_string();
        assert!(peer_facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::PeerChannelReady {
                    channel,
                    peer_authority_id,
                    context_id: Some(fact_context),
                } if channel.id.as_deref() == Some(channel_id_string.as_str())
                    && peer_authority_id == &peer_id_string
                    && fact_context == &context_id.to_string()
            )
        }));
        assert!(matches!(
            delivery_fact,
            Some(AuthoritativeSemanticFact::MessageDeliveryReady { channel, member_count })
                if channel.id.as_deref() == Some(channel_id_string.as_str())
                    && member_count == 2
        ));
    }

    #[tokio::test]
    async fn test_channel_readiness_coordinator_tracks_authoritative_membership_facts() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        register_signals_only(&app_core).await;

        let local = AuthorityId::new_from_entropy([62u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"channel-readiness-coordinator"));
        {
            let mut core = app_core.write().await;
            core.set_authority(local);
        }

        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            2,
        )
        .await
        .unwrap();

        let coordinator = ChannelReadinessCoordinator::load(&app_core, false)
            .await
            .expect("channel readiness should load");
        let state = coordinator
            .state_for_channel(channel_id)
            .unwrap_or_else(|| panic!("expected channel readiness state for {channel_id}"));

        assert_eq!(state.member_count, 2);
        assert!(state.recipients.is_empty());
        assert!(!state.delivery_supported);
    }

    #[tokio::test]
    async fn test_leave_channel_clears_authoritative_membership_readiness_locally() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        register_signals_only(&app_core).await;

        let channel_id = create_channel(&app_core, "shared-parity-lab", None, &[], 0, 42)
            .await
            .unwrap();

        leave_channel(&app_core, channel_id).await.unwrap();

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(!facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
            )
        }));
    }

    #[test]
    fn test_authoritative_send_readiness_for_channel_requires_matching_facts() {
        let channel_id = ChannelId::from_bytes(hash(b"send-ready"));
        let other_channel_id = ChannelId::from_bytes(hash(b"other-send-ready"));
        let context_id = ContextId::new_from_entropy([7u8; 32]);
        let facts = vec![
            AuthoritativeSemanticFact::RecipientPeersResolved {
                channel: ChannelFactKey {
                    id: Some(channel_id.to_string()),
                    name: Some("shared-parity-lab".to_string()),
                },
                member_count: 2,
            },
            AuthoritativeSemanticFact::MessageDeliveryReady {
                channel: ChannelFactKey {
                    id: Some(other_channel_id.to_string()),
                    name: Some("other".to_string()),
                },
                member_count: 2,
            },
        ];

        let readiness = authoritative_send_readiness_for_channel(
            &facts,
            AuthoritativeChannelRef::new(channel_id, context_id),
        );
        assert!(readiness.recipient_resolution_ready);
        assert!(!readiness.delivery_ready);
    }

    #[test]
    fn test_send_message_error_maps_to_typed_semantic_failure() {
        let channel_id = ChannelId::from_bytes(hash(b"send-error"));
        let error = SendMessageError::DeliveryNotReady { channel_id };
        let semantic_error = error.semantic_error();
        assert_eq!(semantic_error.domain, SemanticFailureDomain::Delivery);
        assert_eq!(
            semantic_error.code,
            SemanticFailureCode::PeerChannelNotEstablished
        );
        assert!(semantic_error
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains(&channel_id.to_string())));
    }

    #[test]
    fn test_join_channel_error_missing_context_maps_to_not_found() {
        let channel_id = ChannelId::from_bytes(hash(b"join-missing-context"));
        let error = JoinChannelError::MissingAuthoritativeContext { channel_id }.into_aura_error();
        match error {
            AuraError::NotFound { message } => {
                assert!(message.contains(&channel_id.to_string()));
            }
            other => panic!("expected not_found error, got {other:?}"),
        }
    }

    #[test]
    fn test_pair_dm_context_id_commutative() {
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);
        assert_eq!(pair_dm_context_id(a, b), pair_dm_context_id(b, a));
    }

    #[test]
    fn test_next_message_id_changes_for_same_timestamp() {
        let channel_id = ChannelId::from_bytes(hash(b"channel:next-message-id-test"));
        let sender_id = AuthorityId::new_from_entropy([7u8; 32]);
        let ts = 1_701_000_000_000u64;

        let first = next_message_id(channel_id, sender_id, ts, "same-content");
        let second = next_message_id(channel_id, sender_id, ts, "same-content");

        assert_ne!(first, second);
        assert!(first.starts_with(&format!("msg-{channel_id}-{ts}-")));
        assert!(second.starts_with(&format!("msg-{channel_id}-{ts}-")));
    }

    #[test]
    fn test_bootstrap_required_for_recipients() {
        assert!(!bootstrap_required_for_recipients(0));
        assert!(bootstrap_required_for_recipients(1));
        assert!(bootstrap_required_for_recipients(2));
    }

    fn pending_channel_invitation(
        invitation_suffix: &str,
        sender: AuthorityId,
        channel_id: ChannelId,
    ) -> InvitationInfo {
        InvitationInfo {
            invitation_id: InvitationId::new(format!("inv-{invitation_suffix}")),
            sender_id: sender,
            receiver_id: AuthorityId::new_from_entropy([99u8; 32]),
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: None,
                nickname_suggestion: None,
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }
    }

    #[test]
    fn test_select_pending_channel_invitation_prefers_requested_channel_id() {
        let local = AuthorityId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([2u8; 32]);
        let requested = ChannelId::from_bytes(hash(b"join-target"));
        let other = ChannelId::from_bytes(hash(b"join-other"));

        let pending = vec![
            pending_channel_invitation("other", sender, other),
            pending_channel_invitation("target", sender, requested),
        ];

        let selected = select_pending_channel_invitation(&pending, local, requested)
            .expect("expected a matching invitation");
        assert_eq!(
            channel_id_from_pending_channel_invitation(&selected),
            Some(requested)
        );
        assert_eq!(selected.invitation_id.as_str(), "inv-target");
    }

    #[test]
    fn test_select_pending_channel_invitation_uses_single_candidate_fallback() {
        let local = AuthorityId::new_from_entropy([3u8; 32]);
        let sender = AuthorityId::new_from_entropy([4u8; 32]);
        let requested = ChannelId::from_bytes(hash(b"requested-channel"));
        let invited = ChannelId::from_bytes(hash(b"invited-channel"));

        let pending = vec![pending_channel_invitation("single", sender, invited)];
        let selected = select_pending_channel_invitation(&pending, local, requested)
            .expect("expected single candidate fallback");
        assert_eq!(
            channel_id_from_pending_channel_invitation(&selected),
            Some(invited)
        );
        assert_eq!(selected.invitation_id.as_str(), "inv-single");
    }

    #[tokio::test]
    async fn test_join_channel_by_name_local_creates_channel() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        join_channel_by_name(&app_core, "porch")
            .await
            .expect("local join should create channel");

        let state = get_chat_state(&app_core).await.unwrap();
        let found = state
            .all_channels()
            .any(|channel| channel.name.eq_ignore_ascii_case("porch"));
        assert!(found, "expected porch channel to exist after /join");
    }

    #[tokio::test]
    async fn test_join_channel_by_name_local_is_idempotent() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        join_channel_by_name(&app_core, "porch")
            .await
            .expect("first local join should create channel");
        join_channel_by_name(&app_core, "porch")
            .await
            .expect("second local join should be a no-op");

        let state = get_chat_state(&app_core).await.unwrap();
        let count = state
            .all_channels()
            .filter(|channel| channel.name.eq_ignore_ascii_case("porch"))
            .count();
        assert_eq!(count, 1, "join should not duplicate channels");
    }

    #[tokio::test]
    async fn test_join_channel_by_name_local_reuses_existing_channel_id() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let existing_id = ChannelId::from_bytes(hash(b"join-existing-id"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: existing_id,
                context_id: None,
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let joined_channel_id = join_channel_by_name(&app_core, "#slash-lab")
            .await
            .expect("join should reuse existing channel");

        let state = get_chat_state(&app_core).await.unwrap();
        let count = state
            .all_channels()
            .filter(|channel| channel.name.eq_ignore_ascii_case("slash-lab"))
            .count();
        assert_eq!(count, 1, "join should not duplicate named channels");
        assert!(state.channel(&existing_id).is_some());
        assert_eq!(joined_channel_id, existing_id.to_string());
    }

    #[tokio::test]
    async fn test_join_channel_by_name_reuses_observed_authoritative_binding() {
        let local = AuthorityId::new_from_entropy([110u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime;
        let config = AppConfig::default();
        let core = AppCore::with_runtime(config, runtime_bridge).expect("runtime-backed app core");
        let app_core = Arc::new(RwLock::new(core));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let channel_id = ChannelId::from_bytes(hash(b"observed-authoritative-join"));
        let context_id = ContextId::new_from_entropy([111u8; 32]);
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();
        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            2,
        )
        .await
        .expect("membership readiness should publish");

        let outcome = join_channel_by_name_with_binding_terminal_status(
            &app_core,
            "shared-parity-lab",
            Some(OperationInstanceId(
                "join-channel-observed-authoritative-binding".to_string(),
            )),
        )
        .await;

        let binding = outcome
            .result
            .expect("join should reuse observed authoritative binding");
        let context_id_string = context_id.to_string();
        assert_eq!(binding.channel_id, channel_id.to_string());
        assert_eq!(
            binding.context_id.as_deref(),
            Some(context_id_string.as_str())
        );
        assert!(matches!(
            outcome.terminal,
            Some(WorkflowTerminalStatus {
                status: SemanticOperationStatus {
                    kind: SemanticOperationKind::JoinChannel,
                    phase: SemanticOperationPhase::Succeeded,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn test_join_channel_by_name_with_instance_publishes_terminal_success() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let instance_id = OperationInstanceId("join-channel-1".to_string());
        join_channel_by_name_with_instance(&app_core, "porch", Some(instance_id.clone()))
            .await
            .expect("local join should succeed");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                instance_id: Some(observed_instance_id),
                status,
                ..
            } if *operation_id == OperationId::join_channel()
                && observed_instance_id == &instance_id
                && status.kind == SemanticOperationKind::JoinChannel
                && status.phase == SemanticOperationPhase::Succeeded
        )));
    }

    #[tokio::test]
    async fn test_join_channel_by_name_with_instance_publishes_terminal_failure() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let instance_id = OperationInstanceId("join-channel-2".to_string());
        let result =
            join_channel_by_name_with_instance(&app_core, "   ", Some(instance_id.clone())).await;
        assert!(result.is_err());

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                instance_id: Some(observed_instance_id),
                status,
                ..
            } if *operation_id == OperationId::join_channel()
                && observed_instance_id == &instance_id
                && status.kind == SemanticOperationKind::JoinChannel
                && status.phase == SemanticOperationPhase::Failed
        )));
    }

    #[tokio::test]
    async fn test_join_channel_by_name_with_terminal_status_returns_direct_terminal_status() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let outcome = join_channel_by_name_with_terminal_status(
            &app_core,
            "porch",
            Some(OperationInstanceId("join-channel-direct-1".to_string())),
        )
        .await;

        assert!(outcome.result.is_ok());
        assert!(matches!(
            outcome.terminal,
            Some(WorkflowTerminalStatus {
                status: SemanticOperationStatus {
                    kind: SemanticOperationKind::JoinChannel,
                    phase: SemanticOperationPhase::Succeeded,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn join_channel_revalidates_membership_and_recipient_readiness_when_membership_was_already_visible(
    ) {
        let owner = AuthorityId::new_from_entropy([116u8; 32]);
        let peer = AuthorityId::new_from_entropy([117u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"join-channel-revalidate-membership"));
        let context_id = ContextId::new_from_entropy([118u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, vec![owner, peer]);
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();
        publish_authoritative_channel_membership_ready(
            &app_core,
            channel_id,
            Some("shared-parity-lab"),
            1,
        )
        .await
        .unwrap();

        let outcome = join_channel_by_name_with_binding_terminal_status(
            &app_core,
            "shared-parity-lab",
            Some(OperationInstanceId(
                "join-channel-revalidate-membership-1".to_string(),
            )),
        )
        .await;

        let binding = outcome
            .result
            .expect("join should succeed with a materialized authoritative binding");
        assert_eq!(binding.channel_id, channel_id.to_string());
        assert_eq!(
            binding.context_id.as_deref(),
            Some(context_id.to_string().as_str())
        );

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
    }

    #[tokio::test]
    async fn join_channel_succeeds_when_runtime_already_has_self_membership() {
        let owner = AuthorityId::new_from_entropy([130u8; 32]);
        let peer = AuthorityId::new_from_entropy([131u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"join-channel-existing-runtime-membership"));
        let context_id = ContextId::new_from_entropy([132u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, vec![owner, peer]);
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let outcome = join_channel_by_name_with_binding_terminal_status(
            &app_core,
            "shared-parity-lab",
            Some(OperationInstanceId(
                "join-channel-existing-runtime-membership-1".to_string(),
            )),
        )
        .await;

        let binding = outcome
            .result
            .expect("join should succeed without reissuing runtime join when self is already authoritative participant");
        assert_eq!(binding.channel_id, channel_id.to_string());
        assert_eq!(
            binding.context_id.as_deref(),
            Some(context_id.to_string().as_str())
        );

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
        assert!(matches!(
            outcome.terminal,
            Some(WorkflowTerminalStatus {
                status: SemanticOperationStatus {
                    kind: SemanticOperationKind::JoinChannel,
                    phase: SemanticOperationPhase::Succeeded,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn join_channel_via_pending_invitation_stabilizes_membership_and_recipient_readiness() {
        let owner = AuthorityId::new_from_entropy([140u8; 32]);
        let peer = AuthorityId::new_from_entropy([141u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(AppConfig::default(), runtime_bridge).unwrap(),
        ));
        register_signals_only(&app_core).await;

        let channel_id = ChannelId::from_bytes(hash(b"join-channel-pending-invitation"));
        let context_id = ContextId::new_from_entropy([142u8; 32]);
        runtime.set_amp_channel_context(channel_id, context_id);
        runtime.set_amp_channel_participants(context_id, channel_id, vec![owner, peer]);
        runtime.set_amp_channel_state_exists(context_id, channel_id, true);
        runtime.set_pending_invitations(vec![crate::runtime_bridge::InvitationInfo {
            invitation_id: InvitationId::new("inv-pending-join-stabilization"),
            sender_id: peer,
            receiver_id: owner,
            invitation_type: crate::runtime_bridge::InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: InvitationBridgeStatus::Pending,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        }]);
        runtime.set_accept_invitation_result(Ok(
            crate::runtime_bridge::InvitationMutationOutcome {
                invitation_id: InvitationId::new("inv-pending-join-stabilization"),
                new_status: InvitationBridgeStatus::Accepted,
            },
        ));

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: vec![peer],
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let outcome = join_channel_by_name_with_binding_terminal_status(
            &app_core,
            "shared-parity-lab",
            Some(OperationInstanceId(
                "join-channel-pending-invitation-1".to_string(),
            )),
        )
        .await;

        let binding = outcome
            .result
            .expect("pending invitation join should produce a canonical binding");
        assert_eq!(binding.channel_id, channel_id.to_string());
        assert_eq!(
            binding.context_id.as_deref(),
            Some(context_id.to_string().as_str())
        );

        let channel_id_string = channel_id.to_string();
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
    }

    #[tokio::test]
    async fn test_join_channel_by_name_with_binding_terminal_status_returns_binding_witness() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let outcome = join_channel_by_name_with_binding_terminal_status(
            &app_core,
            "porch",
            Some(OperationInstanceId(
                "join-channel-direct-binding".to_string(),
            )),
        )
        .await;

        let binding = outcome
            .result
            .expect("joined channel should return a binding witness");
        assert!(!binding.channel_id.is_empty());
        match binding.semantic_value() {
            crate::scenario_contract::SemanticCommandValue::ChannelSelection { channel_id }
            | crate::scenario_contract::SemanticCommandValue::AuthoritativeChannelBinding {
                channel_id,
                ..
            } => assert_eq!(channel_id, binding.channel_id),
            other => panic!("unexpected semantic value for channel binding: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_join_channel_success_implies_membership_ready_postcondition() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let outcome = join_channel_by_name_with_terminal_status(
            &app_core,
            "porch",
            Some(OperationInstanceId(
                "join-channel-direct-postcondition".to_string(),
            )),
        )
        .await;

        assert!(outcome.result.is_ok());

        let channel_id = get_chat_state(&app_core)
            .await
            .expect("chat state")
            .all_channels()
            .find(|channel| channel.name == "porch")
            .map(|channel| channel.id)
            .expect("joined channel must exist");
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        let channel_id_string = channel_id.to_string();
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                if channel.id.as_deref() == Some(channel_id_string.as_str())
        )));
    }

    #[tokio::test]
    async fn test_join_channel_stale_projection_fails_explicitly_without_projection_fallback() {
        let local = AuthorityId::new_from_entropy([106u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime;
        let config = AppConfig::default();
        let core = AppCore::with_runtime(config, runtime_bridge).expect("runtime-backed app core");
        let app_core = Arc::new(RwLock::new(core));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let stale_channel_id = ChannelId::from_bytes(hash(b"stale-projection-join"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: stale_channel_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let outcome = tokio::time::timeout(
            Duration::from_millis(250),
            join_channel_by_name_with_terminal_status(
                &app_core,
                "shared-parity-lab",
                Some(OperationInstanceId(
                    "join-channel-stale-projection".to_string(),
                )),
            ),
        )
        .await
        .expect("stale projection join must fail without spinning");

        assert!(outcome.result.is_err());
        assert!(matches!(
            outcome.terminal,
            Some(WorkflowTerminalStatus {
                status: SemanticOperationStatus {
                    kind: SemanticOperationKind::JoinChannel,
                    phase: SemanticOperationPhase::Failed,
                    ..
                },
                ..
            })
        ));
    }

    #[tokio::test]
    async fn test_send_message_stale_projection_fails_explicitly_without_projection_fallback() {
        let local = AuthorityId::new_from_entropy([108u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(local));
        let runtime_bridge: Arc<dyn RuntimeBridge> = runtime;
        let config = AppConfig::default();
        let core = AppCore::with_runtime(config, runtime_bridge).expect("runtime-backed app core");
        let app_core = Arc::new(RwLock::new(core));
        {
            let core = app_core.read().await;
            crate::signal_defs::register_app_signals(&*core)
                .await
                .unwrap();
        }

        let stale_channel_id = ChannelId::from_bytes(hash(b"stale-projection-send"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: stale_channel_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let instance_id = OperationInstanceId("send-stale-projection".to_string());
        let result = tokio::time::timeout(
            Duration::from_millis(250),
            send_message_by_name_with_instance(
                &app_core,
                "shared-parity-lab",
                "hello",
                1_701_000_000_123,
                Some(instance_id.clone()),
            ),
        )
        .await
        .expect("stale projection send must fail without spinning");

        assert!(result.is_err());
        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert_terminal_failure_or_cancelled(
            &facts,
            &OperationId::send_message(),
            &instance_id,
            SemanticOperationKind::SendChatMessage,
        );
    }

    #[tokio::test]
    async fn test_send_message_with_instance_publishes_terminal_success() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let instance_id = OperationInstanceId("send-message-1".to_string());
        let channel_id = ChannelId::from_bytes(hash(b"send-message-success"));
        send_message_with_instance(
            &app_core,
            channel_id,
            "hello shared owner",
            1_701_000_000_000,
            Some(instance_id.clone()),
        )
        .await
        .expect("local send should succeed");

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                instance_id: Some(observed_instance_id),
                status,
                ..
            } if *operation_id == OperationId::send_message()
                && observed_instance_id == &instance_id
                && status.kind == SemanticOperationKind::SendChatMessage
                && status.phase == SemanticOperationPhase::Succeeded
        )));
    }

    #[tokio::test]
    async fn test_send_message_by_name_with_instance_publishes_terminal_failure() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let instance_id = OperationInstanceId("send-message-2".to_string());
        let result = send_message_by_name_with_instance(
            &app_core,
            "",
            "hello shared owner",
            1_701_000_000_001,
            Some(instance_id.clone()),
        )
        .await;
        assert!(result.is_err());

        let facts = read_signal_or_default(&app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                instance_id: Some(observed_instance_id),
                status,
                ..
            } if *operation_id == OperationId::send_message()
                && observed_instance_id == &instance_id
                && status.kind == SemanticOperationKind::SendChatMessage
                && status.phase == SemanticOperationPhase::Failed
        )));
    }

    #[tokio::test]
    async fn test_mark_message_delivery_failed_reduces_delivery_status() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([91u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"delivery-failed-reduction"));
        let sender_id = AuthorityId::new_from_entropy([92u8; 32]);
        let message_id = "delivery-failed-message".to_string();

        reduce_chat_fact_observed(
            &app_core,
            &ChatFact::channel_created_ms(
                context_id,
                channel_id,
                "delivery-failed".to_string(),
                None,
                false,
                1_701_000_000_010,
                sender_id,
            ),
        )
        .await
        .unwrap();
        reduce_chat_fact_observed(
            &app_core,
            &ChatFact::message_sent_sealed_ms(
                context_id,
                channel_id,
                message_id.clone(),
                sender_id,
                "Alice".to_string(),
                b"hello".to_vec(),
                1_701_000_000_011,
                None,
                None,
            ),
        )
        .await
        .unwrap();

        mark_message_delivery_failed(&app_core, context_id, channel_id, &message_id, sender_id)
            .await
            .unwrap();

        let chat = observed_chat_snapshot(&app_core).await;
        let message = chat
            .channel(&channel_id)
            .and_then(|_| {
                chat.messages_for_channel(&channel_id)
                    .iter()
                    .find(|message| message.id == message_id)
            })
            .expect("message should still exist after failure reduction");
        assert_eq!(message.delivery_status, MessageDeliveryStatus::Failed);
    }

    #[tokio::test]
    async fn test_ensure_channel_visible_after_join_inserts_missing_channel() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([12u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"join-visible-missing"));
        ensure_channel_visible_after_join(&app_core, channel_id, context_id, Some("slash-lab"))
            .await
            .expect("join visibility should succeed");

        let chat = observed_chat_snapshot(&app_core).await;
        let channel = chat
            .channel(&channel_id)
            .expect("channel should be inserted");
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "slash-lab");
    }

    #[tokio::test]
    async fn test_ensure_channel_visible_after_join_updates_existing_name_with_hint() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([13u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"join-visible-existing"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: channel_id.to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        ensure_channel_visible_after_join(&app_core, channel_id, context_id, Some("#slash-lab"))
            .await
            .expect("join visibility should succeed");

        let chat = observed_chat_snapshot(&app_core).await;
        let channel = chat.channel(&channel_id).expect("channel should exist");
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "slash-lab");
    }

    #[tokio::test]
    async fn test_ensure_channel_visible_after_join_preserves_existing_name_without_hint() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([14u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"join-visible-preserve-name"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        ensure_channel_visible_after_join(&app_core, channel_id, context_id, None)
            .await
            .expect("join visibility should preserve existing authoritative name");

        let chat = observed_chat_snapshot(&app_core).await;
        let channel = chat.channel(&channel_id).expect("channel should exist");
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "shared-parity-lab");
    }

    #[tokio::test]
    async fn test_canonical_channel_name_hint_for_invite_uses_existing_channel_name_for_id_input() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"invite-channel-name-existing"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(ContextId::new_from_entropy([41u8; 32])),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let hint =
            canonical_channel_name_hint_for_invite(&app_core, channel_id, &channel_id.to_string())
                .await
                .expect("existing canonical name should be used");

        assert_eq!(hint, "shared-parity-lab");
    }

    #[tokio::test]
    async fn test_canonical_channel_name_hint_for_invite_rejects_raw_id_without_name() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"invite-channel-name-missing"));

        let error =
            canonical_channel_name_hint_for_invite(&app_core, channel_id, &channel_id.to_string())
                .await
                .expect_err("raw id input without canonical metadata must fail");

        assert!(matches!(error, AuraError::Internal { .. }));
        assert!(
            error
                .to_string()
                .contains("requires canonical channel metadata"),
            "expected explicit raw-id rejection, got: {error}"
        );
    }

    #[tokio::test]
    async fn test_ensure_channel_visible_after_join_rebinds_same_name_placeholder_channel() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let stale_id = ChannelId::from_bytes(hash(b"join-visible-stale"));
        let canonical_id = ChannelId::from_bytes(hash(b"join-visible-canonical"));
        let context_id = ContextId::new_from_entropy([22u8; 32]);

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: stale_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
            chat.apply_message(
                stale_id,
                Message {
                    id: "pre-canonical".to_string(),
                    channel_id: stale_id,
                    sender_id: AuthorityId::new_from_entropy([3u8; 32]),
                    sender_name: "Alice".to_string(),
                    content: "hello-from-tu1".to_string(),
                    timestamp: 1,
                    reply_to: None,
                    is_own: false,
                    is_read: false,
                    delivery_status: MessageDeliveryStatus::Delivered,
                    epoch_hint: None,
                    is_finalized: false,
                },
            );
        })
        .await
        .unwrap();

        ensure_channel_visible_after_join(
            &app_core,
            canonical_id,
            context_id,
            Some("shared-parity-lab"),
        )
        .await
        .expect("join visibility should rebind placeholder channel");

        let chat = observed_chat_snapshot(&app_core).await;
        assert!(
            chat.channel(&stale_id).is_none(),
            "stale placeholder must be removed"
        );
        let channel = chat
            .channel(&canonical_id)
            .expect("canonical channel should exist");
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "shared-parity-lab");
        let messages = chat.messages_for_channel(&canonical_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].channel_id, canonical_id);
        assert_eq!(messages[0].content, "hello-from-tu1");
    }

    #[tokio::test]
    async fn test_local_channel_resolution_matches_name_variants() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"resolve-name-variants"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let by_name =
            resolve_local_chat_channel_id_from_observed_state_or_input(&app_core, "slash-lab")
                .await
                .expect("name selector should resolve");
        let by_hash =
            resolve_local_chat_channel_id_from_observed_state_or_input(&app_core, "#slash-lab")
                .await
                .expect("#name selector should resolve");
        let by_spaced_hash =
            resolve_local_chat_channel_id_from_observed_state_or_input(&app_core, "# slash-lab")
                .await
                .expect("# spaced selector should resolve");

        assert_eq!(by_name, channel_id);
        assert_eq!(by_hash, channel_id);
        assert_eq!(by_spaced_hash, channel_id);
    }

    #[tokio::test]
    async fn test_local_channel_resolution_requires_observed_chat_match_when_chat_missing() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let home_id = ChannelId::from_bytes(hash(b"resolve-home-projection"));
        let owner = AuthorityId::new_from_entropy([17u8; 32]);
        let context_id = ContextId::new_from_entropy([18u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                home_id,
                Some("slash-lab".to_string()),
                owner,
                0,
                context_id,
            ));
            homes.select_home(Some(home_id));
            core.views_mut().set_homes(homes);
        }

        let error =
            resolve_local_chat_channel_id_from_observed_state_or_input(&app_core, "#slash-lab")
                .await
                .expect_err("missing observed chat channel must fail explicitly");
        assert!(matches!(error, AuraError::NotFound { .. }));
    }

    #[tokio::test]
    async fn test_local_channel_resolution_prefers_chat_match_over_home_name_collision() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let local_home_id = ChannelId::from_bytes(hash(b"resolve-local-home-preferred"));
        let foreign_chat_id = ChannelId::from_bytes(hash(b"resolve-foreign-chat-placeholder"));
        let owner = AuthorityId::new_from_entropy([24u8; 32]);
        let context_id = ContextId::new_from_entropy([25u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                local_home_id,
                Some("shared-parity-lab".to_string()),
                owner,
                0,
                context_id,
            ));
            core.views_mut().set_homes(homes);
        }

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: foreign_chat_id,
                context_id: None,
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let resolved = resolve_local_chat_channel_id_from_observed_state_or_input(
            &app_core,
            "shared-parity-lab",
        )
        .await
        .expect("chat channel should win over home collision");
        assert_eq!(resolved, foreign_chat_id);
    }

    #[tokio::test]
    async fn test_local_channel_resolution_prefers_context_backed_chat_over_home_name_match() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let home_id = ChannelId::from_bytes(hash(b"resolve-home-name-collision"));
        let chat_id = ChannelId::from_bytes(hash(b"resolve-chat-name-collision"));
        let owner = AuthorityId::new_from_entropy([26u8; 32]);
        let home_context_id = ContextId::new_from_entropy([27u8; 32]);
        let chat_context_id = ContextId::new_from_entropy([28u8; 32]);

        {
            let mut core = app_core.write().await;
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                home_id,
                Some("shared-parity-lab".to_string()),
                owner,
                0,
                home_context_id,
            ));
            core.views_mut().set_homes(homes);
        }

        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: chat_id,
                context_id: Some(chat_context_id),
                name: "shared-parity-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 10,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        let resolved = resolve_local_chat_channel_id_from_observed_state_or_input(
            &app_core,
            "shared-parity-lab",
        )
        .await
        .expect("context-backed chat should win over home name collision");
        assert_eq!(resolved, chat_id);
    }

    #[tokio::test]
    async fn resolve_authoritative_context_id_for_channel_ignores_pending_invitation_fallbacks() {
        let config = AppConfig::default();
        let owner = AuthorityId::new_from_entropy([21u8; 32]);
        let runtime = Arc::new(crate::runtime_bridge::OfflineRuntimeBridge::new(owner));
        let runtime_bridge: Arc<dyn crate::runtime_bridge::RuntimeBridge> = runtime.clone();
        let app_core = Arc::new(RwLock::new(
            AppCore::with_runtime(config, runtime_bridge).unwrap(),
        ));
        let channel_id = ChannelId::from_bytes(hash(b"canonical-accepted-home-id"));
        let context_id = ContextId::new_from_entropy([22u8; 32]);

        let invitation = InvitationInfo {
            invitation_id: InvitationId::new("inv-local-context".to_string()),
            sender_id: AuthorityId::new_from_entropy([23u8; 32]),
            receiver_id: owner,
            invitation_type: InvitationBridgeType::Channel {
                home_id: channel_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: InvitationBridgeStatus::Accepted,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        };
        runtime.set_pending_invitations(vec![invitation]);

        let resolved = resolve_authoritative_context_id_for_channel(&app_core, channel_id).await;

        assert_eq!(resolved, None);
    }

    #[tokio::test]
    async fn test_leave_then_join_name_reuses_canonical_channel_id() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([19u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"leave-join-canonical-reuse"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        apply_authoritative_membership_projection(&app_core, channel_id, context_id, false, None)
            .await
            .expect("leave projection should preserve canonical channel entry");

        let resolved =
            resolve_local_chat_channel_id_from_observed_state_or_input(&app_core, "slash-lab")
                .await
                .expect("name selector should resolve");
        assert_eq!(resolved, channel_id);

        let chat = observed_chat_snapshot(&app_core).await;
        let channel = chat
            .channel(&channel_id)
            .expect("channel entry should remain");
        assert_eq!(channel.member_count, 0);
    }

    #[tokio::test]
    async fn test_leave_channel_by_name_uses_state_resolution_locally() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"leave-by-name-local"));
        update_chat_projection_observed(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: None,
                name: "slash-lab".to_string(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 1,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await
        .unwrap();

        leave_channel_by_name(&app_core, "#slash-lab")
            .await
            .expect("leave should remove channel by name");

        let state = get_chat_state(&app_core).await.unwrap();
        assert!(state.channel(&channel_id).is_none());
    }

    #[tokio::test]
    async fn test_leave_channel_by_name_missing_target_is_a_noop() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        leave_channel_by_name(&app_core, "#missing-channel")
            .await
            .expect("missing leave target should settle without retries or fallback");
    }

    #[test]
    fn test_join_error_is_not_found_detects_variant() {
        let error = AuraError::not_found("channel missing");
        assert!(join_error_is_not_found(&error));
    }

    #[test]
    fn test_join_error_is_not_found_rejects_permission_denied() {
        let error = AuraError::permission_denied("no permission to join");
        assert!(!join_error_is_not_found(&error));
    }

    #[test]
    fn test_intent_error_is_not_found_detects_context_not_found() {
        let error = IntentError::ContextNotFound {
            context_id: "ctx-123".to_string(),
        };
        assert!(intent_error_is_not_found(&error));
    }

    #[test]
    fn test_intent_error_is_not_found_rejects_unauthorized() {
        let error = IntentError::Unauthorized {
            reason: "no token".to_string(),
        };
        assert!(!intent_error_is_not_found(&error));
    }

    #[tokio::test]
    async fn test_enforce_home_moderation_allows_when_member_list_is_empty() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([5u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([9u8; 32]);
        let home_id = ChannelId::from_bytes(hash(b"messaging-empty-members-home"));

        let mut home = HomeState::new(home_id, Some("shared".to_string()), owner, 0, context_id);
        home.my_role = HomeRole::Participant;
        home.members.clear();
        home.member_count = 0;
        home.online_count = 0;

        let mut homes = HomesState::new();
        let result = homes.add_home(home);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_moderation_for_sender(&app_core, context_id, home_id, sender, 1_000).await;
        let error = result.expect_err("authoritative moderation now requires runtime");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn test_enforce_home_moderation_blocks_muted_sender_with_empty_members() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([6u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([9u8; 32]);
        let actor = AuthorityId::new_from_entropy([2u8; 32]);
        let home_id = ChannelId::from_bytes(hash(b"messaging-muted-home"));

        let mut home = HomeState::new(home_id, Some("shared".to_string()), owner, 0, context_id);
        home.my_role = HomeRole::Participant;
        home.members.clear();
        home.member_count = 0;
        home.online_count = 0;
        home.add_mute(MuteRecord {
            authority_id: sender,
            duration_secs: Some(300),
            muted_at: 1_000,
            expires_at: Some(301_000),
            actor,
        });

        let mut homes = HomesState::new();
        let result = homes.add_home(home);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_moderation_for_sender(&app_core, context_id, home_id, sender, 2_000).await;
        let error = result.expect_err("authoritative moderation now requires runtime");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn test_enforce_home_join_blocks_banned_sender_when_context_mismatched() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let banned = AuthorityId::new_from_entropy([3u8; 32]);
        let actor = AuthorityId::new_from_entropy([4u8; 32]);
        let owner = AuthorityId::new_from_entropy([5u8; 32]);
        let home_context = ContextId::new_from_entropy([7u8; 32]);
        let mismatched_context = ContextId::new_from_entropy([8u8; 32]);
        let home_id = ChannelId::from_bytes(hash(b"join-ban-home"));

        let mut home = HomeState::new(
            home_id,
            Some("slash-lab".to_string()),
            owner,
            0,
            home_context,
        );
        home.add_ban(BanRecord {
            authority_id: banned,
            reason: "scenario-ban".to_string(),
            actor,
            banned_at: 1_000,
        });

        let mut homes = HomesState::new();
        let result = homes.add_home(home);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_join_allowed(&app_core, mismatched_context, home_id, banned).await;
        let error = result.expect_err("authoritative moderation now requires runtime");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn test_enforce_home_moderation_blocks_muted_sender_across_context_homes() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([14u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let sender = AuthorityId::new_from_entropy([9u8; 32]);
        let actor = AuthorityId::new_from_entropy([2u8; 32]);
        let channel_home_id = ChannelId::from_bytes(hash(b"messaging-context-primary-home"));
        let moderation_home_id = ChannelId::from_bytes(hash(b"messaging-context-moderation-home"));

        let primary_home = HomeState::new(
            channel_home_id,
            Some("primary".to_string()),
            owner,
            0,
            context_id,
        );

        let mut moderation_home = HomeState::new(
            moderation_home_id,
            Some("shared".to_string()),
            owner,
            0,
            context_id,
        );
        moderation_home.my_role = HomeRole::Participant;
        moderation_home.members.clear();
        moderation_home.member_count = 0;
        moderation_home.online_count = 0;
        moderation_home.add_mute(MuteRecord {
            authority_id: sender,
            duration_secs: Some(300),
            muted_at: 1_000,
            expires_at: Some(301_000),
            actor,
        });

        let mut homes = HomesState::new();
        let result = homes.add_home(primary_home);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
        homes.add_home(moderation_home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result = enforce_home_moderation_for_sender(
            &app_core,
            context_id,
            channel_home_id,
            sender,
            2_000,
        )
        .await;
        let error = result.expect_err("authoritative moderation now requires runtime");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn test_enforce_home_join_blocks_banned_sender_across_context_homes() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let banned = AuthorityId::new_from_entropy([31u8; 32]);
        let actor = AuthorityId::new_from_entropy([4u8; 32]);
        let owner = AuthorityId::new_from_entropy([5u8; 32]);
        let context_id = ContextId::new_from_entropy([17u8; 32]);
        let channel_home_id = ChannelId::from_bytes(hash(b"join-ban-context-primary-home"));
        let moderation_home_id = ChannelId::from_bytes(hash(b"join-ban-context-other-home"));

        let primary_home = HomeState::new(
            channel_home_id,
            Some("primary".to_string()),
            owner,
            0,
            context_id,
        );

        let mut moderation_home = HomeState::new(
            moderation_home_id,
            Some("secondary".to_string()),
            owner,
            0,
            context_id,
        );
        moderation_home.add_ban(BanRecord {
            authority_id: banned,
            reason: "scenario-ban".to_string(),
            actor,
            banned_at: 1_000,
        });

        let mut homes = HomesState::new();
        let result = homes.add_home(primary_home);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
        homes.add_home(moderation_home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_join_allowed(&app_core, context_id, channel_home_id, banned).await;
        let error = result.expect_err("authoritative moderation now requires runtime");
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }

    #[tokio::test]
    async fn test_resolve_target_authority_for_invite_uses_contact_resolution() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let bob_id = AuthorityId::new_from_entropy([11u8; 32]);
        let bob_contact = Contact {
            id: bob_id,
            nickname: "Bob".to_string(),
            nickname_suggestion: Some("Bobby".to_string()),
            is_guardian: false,
            is_member: false,
            last_interaction: None,
            is_online: true,
            read_receipt_policy: Default::default(),
            relationship_state: crate::views::contacts::ContactRelationshipState::Contact,
        };

        {
            let mut core = app_core.write().await;
            core.views_mut()
                .set_contacts(ContactsState::from_contacts(vec![bob_contact]));
        }

        let by_name = resolve_target_authority_for_invite(&app_core, "bob")
            .await
            .expect("expected contact nickname to resolve");
        assert_eq!(by_name, bob_id);

        let by_id = resolve_target_authority_for_invite(&app_core, &bob_id.to_string())
            .await
            .expect("expected contact id string to resolve");
        assert_eq!(by_id, bob_id);
    }

    #[tokio::test]
    async fn require_send_message_readiness_fails_when_authoritative_facts_are_unavailable() {
        let app_core = crate::testing::default_test_app_core();
        let channel = AuthoritativeChannelRef::new(
            ChannelId::from_bytes(hash(b"semantic-readiness-channel")),
            ContextId::new_from_entropy(hash(b"semantic-readiness-context")),
        );

        let error = require_send_message_readiness(&app_core, channel)
            .await
            .expect_err("readiness should fail when authoritative facts are unavailable");
        assert!(matches!(
            error,
            SendMessageError::RecipientResolutionNotReady { .. }
        ));
    }

    #[test]
    fn invite_authority_with_context_keeps_channel_name_hint_for_local_projection() {
        let source = include_str!("messaging/invites.rs");
        let start = source
            .find("pub(super) async fn invite_authority_to_channel_with_context(")
            .expect("invite_authority_to_channel_with_context definition");
        let end = source[start..]
            .find("Ok(invitation.invitation_id)")
            .map(|offset| start + offset)
            .expect("invite_authority_to_channel_with_context return");
        let body = &source[start..end];
        assert!(
            body.contains("local_projection_name_hint.as_deref()"),
            "invite_authority_to_channel_with_context must preserve canonical channel name hints through local projection"
        );
    }

    #[test]
    fn invite_authority_with_context_warms_receiver_before_create() {
        let invite_source = include_str!("messaging/invites.rs");
        let invite_start = invite_source
            .find("pub(super) async fn invite_authority_to_channel_with_context(")
            .expect("invite_authority_to_channel_with_context definition");
        let invite_end = invite_source[invite_start..]
            .find("update_workflow_stage(&stage_tracker, \"create_channel_invitation\");")
            .map(|offset| invite_start + offset)
            .expect("create_channel_invitation stage marker");
        let invite_body = &invite_source[invite_start..invite_end];
        let warm_source = include_str!("messaging.rs");
        let warm_start = warm_source
            .find("async fn warm_invited_peer_connectivity(")
            .expect("warm_invited_peer_connectivity definition");
        let warm_end = warm_source[warm_start..]
            .find("async fn ensure_invited_peer_channel(")
            .map(|offset| warm_start + offset)
            .expect("ensure_invited_peer_channel definition");
        let warm_body = &warm_source[warm_start..warm_end];
        assert!(
            invite_body.contains(
                "warm_invited_peer_connectivity(app_core, &runtime, context_id, receiver)"
            ) || invite_body.contains(
                    "warm_invited_peer_connectivity(app_core, &runtime, context_id, receiver).await;"
                ),
            "invite_authority_to_channel_with_context must warm the invite receiver before creating the channel invitation"
        );
        assert!(
            warm_body.contains("authority_default_relational_context(receiver)"),
            "invite_authority_to_channel_with_context must warm the receiver's authority-scoped peer path before delivery"
        );
    }

    #[test]
    fn invite_authority_terminal_wrapper_publishes_terminal_status() {
        let source = include_str!("messaging/invites.rs");
        let start = source
            .find("pub async fn invite_authority_to_channel_with_context_terminal_status(")
            .expect("invite_authority_to_channel_with_context_terminal_status definition");
        let end = source[start..]
            .find("crate::ui_contract::WorkflowTerminalOutcome {")
            .map(|offset| start + offset)
            .expect("terminal outcome marker");
        let body = &source[start..end];
        assert!(
            body.contains("owner.publish_failure(semantic_error).await"),
            "authoritative terminal wrapper must publish failure before returning terminal status"
        );
        assert!(
            body.contains("owner\n            .publish_success_with(issue_channel_invitation_created_proof(")
                || body.contains("owner\r\n            .publish_success_with(issue_channel_invitation_created_proof("),
            "authoritative terminal wrapper must publish success before returning terminal status"
        );
    }
}
