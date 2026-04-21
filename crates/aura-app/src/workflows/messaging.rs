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
use crate::workflows::context::{
    authority_default_relational_context, current_group_context, current_home_context,
};
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
        let receiver = request.receiver;
        let channel_id = request.channel_id;
        let context_id = request.context_id;
        let outcome = super::invite_authority_to_channel_with_context_terminal_status(
            app_core,
            receiver,
            channel_id,
            context_id,
            request.channel_name_hint,
            request.operation_instance_id,
            request.message,
            request.ttl_ms,
        )
        .await;
        if outcome.result.is_ok() {
            if let Some(context_id) = context_id {
                let authoritative_channel =
                    super::authoritative_channel_ref(channel_id, context_id);
                super::run_post_channel_invite_followups(app_core, receiver, authoritative_channel)
                    .await;
                let _ = crate::workflows::system::refresh_account(app_core).await;
            }
        }
        outcome
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
    include!("messaging/tests.rs");
}
