//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

use crate::signal_defs::AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL;
use crate::ui_contract::{
    AuthoritativeSemanticFact, AuthoritativeSemanticFactKind, ChannelFactKey, OperationId,
    OperationInstanceId, SemanticFailureCode, SemanticFailureDomain, SemanticOperationError,
    SemanticOperationKind, SemanticOperationPhase,
};
use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::chat_commands::normalize_channel_name;
use crate::workflows::context::current_home_context_or_authority_default;
use crate::workflows::harness_determinism;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, ensure_runtime_peer_connectivity,
    execute_with_runtime_retry_budget, execute_with_runtime_timeout_budget, require_runtime,
    workflow_retry_policy, workflow_timeout_budget,
};
use crate::workflows::runtime_error_classification::{
    classify_amp_channel_error, classify_invitation_accept_error, AmpChannelErrorClass,
    InvitationAcceptErrorClass,
};
use crate::workflows::semantic_facts::{
    issue_channel_invitation_created_proof, prove_channel_membership_ready,
    prove_message_committed,
    replace_authoritative_semantic_facts_of_kind, semantic_readiness_publication_capability,
    update_authoritative_semantic_facts, SemanticWorkflowOwner,
};
use crate::workflows::signals::{emit_signal, read_signal, read_signal_or_default};
use crate::workflows::snapshot_policy::{chat_snapshot, contacts_snapshot};
use crate::workflows::state_helpers::with_chat_state;
use crate::{
    core::IntentError,
    runtime_bridge::{InvitationBridgeType, InvitationInfo, RuntimeBridge},
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    thresholds::{default_channel_threshold, normalize_channel_threshold},
    views::{
        chat::{
            is_note_to_self_channel_name, note_to_self_channel_id, note_to_self_context_id,
            Channel, ChannelType, ChatState, Message, MessageDeliveryStatus,
            NOTE_TO_SELF_CHANNEL_NAME, NOTE_TO_SELF_CHANNEL_TOPIC,
        },
        contacts::ContactsState,
        home::{HomeMember, HomeRole, HomeState, HomesState},
    },
    AppCore,
};
use async_lock::RwLock;
use aura_chat::ChatFact;
use aura_core::{
    crypto::hash::hash,
    effects::amp::{
        ChannelCloseParams, ChannelCreateParams, ChannelJoinParams, ChannelLeaveParams,
        ChannelSendParams,
    },
    types::{AuthorityId, ChannelId, ContextId},
    AuraError, InvitationId, OperationContext, OwnedTaskSpawner, RetryRunError, TimeoutBudget,
    TimeoutBudgetError, TimeoutRunError, TraceContext,
};
use aura_journal::fact::{FactOptions, RelationalFact};
use aura_journal::DomainFact;
use aura_protocol::amp::{serialize_amp_message, AmpMessage};
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[allow(clippy::disallowed_types)]
type InviteStageTracker = Arc<std::sync::Mutex<&'static str>>;
const CHAT_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const CHAT_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
pub(crate) const AMP_SEND_RETRY_ATTEMPTS: usize = 6;
pub(crate) const AMP_SEND_RETRY_BACKOFF_MS: u64 = 75;
const CHANNEL_CONTEXT_RETRY_ATTEMPTS: usize = 12;
const CHANNEL_CONTEXT_RETRY_BACKOFF_MS: u64 = 100;
const REMOTE_DELIVERY_RETRY_ATTEMPTS: usize = 24;
const REMOTE_DELIVERY_RETRY_BACKOFF_MS: u64 = 250;
const INVITE_USER_STAGE_TIMEOUT_MS: u64 = 5_000;
const INVITE_USER_OPERATION_TIMEOUT_MS: u64 = 5_000;

mod routing;
mod validation;

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

#[allow(clippy::disallowed_types)]
fn new_invite_stage_tracker(stage: &'static str) -> InviteStageTracker {
    Arc::new(std::sync::Mutex::new(stage))
}

fn update_invite_stage(tracker: &Option<InviteStageTracker>, stage: &'static str) {
    if let Some(tracker) = tracker {
        if let Ok(mut guard) = tracker.lock() {
            *guard = stage;
        }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct MessageSendReadiness {
    recipient_resolution_ready: bool,
    delivery_ready: bool,
}

#[derive(Debug, Clone)]
struct ChannelReadinessState {
    channel: Channel,
    fact_key: ChannelFactKey,
    member_count: u32,
    recipients: Vec<AuthorityId>,
    delivery_supported: bool,
}

impl ChannelReadinessState {
    fn new(channel: Channel, recipients: Vec<AuthorityId>) -> Self {
        let member_count = channel
            .member_count
            .max(channel.member_ids.len() as u32 + 1);
        let delivery_supported = !channel.name.eq_ignore_ascii_case("note to self")
            && !channel.is_dm
            && !recipients.is_empty();
        Self {
            fact_key: ChannelFactKey {
                id: Some(channel.id.to_string()),
                name: Some(channel.name.clone()),
            },
            channel,
            member_count,
            recipients,
            delivery_supported,
        }
    }

    fn membership_fact(&self) -> AuthoritativeSemanticFact {
        AuthoritativeSemanticFact::ChannelMembershipReady {
            channel: self.fact_key.clone(),
            member_count: self.member_count,
        }
    }

    fn recipient_resolution_fact(&self) -> Option<AuthoritativeSemanticFact> {
        (self.delivery_supported).then(|| AuthoritativeSemanticFact::RecipientPeersResolved {
            channel: self.fact_key.clone(),
            member_count: self.member_count.max(self.recipients.len() as u32),
        })
    }

    fn delivery_facts(
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
struct ChannelReadinessCoordinator {
    states: Vec<ChannelReadinessState>,
}

impl ChannelReadinessCoordinator {
    async fn load(app_core: &Arc<RwLock<AppCore>>) -> Self {
        let chat = chat_snapshot(app_core).await;
        let contacts = contacts_snapshot(app_core).await;
        let (homes, runtime, self_authority) = {
            let core = app_core.read().await;
            (
                core.views().get_homes(),
                core.runtime().cloned(),
                core.authority().copied(),
            )
        };
        let self_authority =
            self_authority.or_else(|| runtime.as_ref().map(|runtime| runtime.authority_id()));
        let discovered = if let Some(runtime) = runtime {
            let authority_id = self_authority.unwrap_or_else(|| runtime.authority_id());
            match runtime.try_get_discovered_peers().await {
                Ok(peers) => peers
                    .into_iter()
                    .filter(|peer| *peer != authority_id)
                    .collect::<Vec<_>>(),
                Err(_error) => {
                    #[cfg(feature = "instrumented")]
                    tracing::debug!(
                        error = %_error,
                        "channel readiness skipped discovered peers because runtime read failed"
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let states = chat
            .all_channels()
            .cloned()
            .map(|channel| {
                let recipients = self_authority
                    .map(|authority_id| {
                        resolved_recipient_peers_for_channel_view(
                            &channel,
                            &homes,
                            &contacts,
                            &discovered,
                            authority_id,
                        )
                    })
                    .unwrap_or_default();
                ChannelReadinessState::new(channel, recipients)
            })
            .collect();

        Self { states }
    }

    fn states(&self) -> &[ChannelReadinessState] {
        &self.states
    }

    fn state_for_channel(&self, channel_id: ChannelId) -> Option<&ChannelReadinessState> {
        self.states
            .iter()
            .find(|state| state.channel.id == channel_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
enum SendMessageError {
    #[error("Failed to resolve channel {channel}: {detail}")]
    ChannelResolution { channel: String, detail: String },
    #[error("Missing authoritative context for channel {channel_id}")]
    MissingAuthoritativeContext { channel_id: ChannelId },
    #[error("Recipient peers are not resolved for channel {channel_id}")]
    RecipientResolutionNotReady { channel_id: ChannelId },
    #[error("Peer channel establishment is not complete for channel {channel_id}")]
    DeliveryNotReady { channel_id: ChannelId },
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
    fn semantic_error(&self) -> SemanticOperationError {
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
enum JoinChannelError {
    #[error("JoinChannel requires an authoritative context for channel {channel_id}")]
    MissingAuthoritativeContext { channel_id: ChannelId },
    #[error("JoinChannel failed for channel {channel_id}: {detail}")]
    Transport {
        channel_id: ChannelId,
        detail: String,
    },
}

impl JoinChannelError {
    fn into_aura_error(self) -> AuraError {
        match self {
            Self::MissingAuthoritativeContext { channel_id } => {
                AuraError::not_found(channel_id.to_string())
            }
            Self::Transport {
                channel_id: _,
                detail,
            } => super::error::runtime_call("join channel transport", detail).into(),
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

async fn messaging_backend(app_core: &Arc<RwLock<AppCore>>) -> MessagingBackend {
    let core = app_core.read().await;
    if core.runtime().is_some() {
        MessagingBackend::Runtime
    } else {
        MessagingBackend::LocalOnly
    }
}

fn send_operation_requires_delivery_readiness(channel: Option<&Channel>) -> bool {
    channel
        .is_some_and(|channel| !channel.is_dm && !channel.name.eq_ignore_ascii_case("note to self"))
}

fn authoritative_send_readiness_for_channel(
    facts: &[AuthoritativeSemanticFact],
    channel_id: ChannelId,
) -> MessageSendReadiness {
    let channel_id = channel_id.to_string();
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

async fn publish_send_message_failure(
    owner: &SemanticWorkflowOwner,
    error: &SendMessageError,
) -> Result<(), AuraError> {
    owner.publish_failure(error.semantic_error()).await
}

async fn require_send_message_readiness(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<MessageSendReadiness, SendMessageError> {
    let facts = read_signal_or_default(app_core, &*AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL).await;
    let readiness = authoritative_send_readiness_for_channel(&facts, channel_id);
    if !readiness.recipient_resolution_ready {
        return Err(SendMessageError::RecipientResolutionNotReady { channel_id });
    }
    if !readiness.delivery_ready {
        return Err(SendMessageError::DeliveryNotReady { channel_id });
    }
    Ok(readiness)
}

pub(crate) fn is_amp_channel_state_unavailable(error: &impl std::fmt::Display) -> bool {
    classify_amp_channel_error(error) == AmpChannelErrorClass::ChannelStateUnavailable
}

async fn fail_send_message<T>(
    owner: &SemanticWorkflowOwner,
    error: SendMessageError,
) -> Result<T, AuraError> {
    publish_send_message_failure(owner, &error).await?;
    Err(error.into())
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

async fn matching_chat_channel_ids(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Vec<ChannelId> {
    routing::matching_chat_channel_ids(app_core, channel_input).await
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
        match runtime.send_chat_fact(peer, context, fact).await {
            Ok(()) => return Ok(()),
            Err(error) => {
                if attempts.can_attempt() {
                    converge_runtime(runtime).await;
                    for _ in 0..CHAT_FACT_SEND_YIELDS_PER_RETRY {
                        cooperative_yield().await;
                    }
                    continue;
                }
                break error;
            }
        }
    };

    Err(super::error::WorkflowError::DeliveryFailed {
        peer: peer.to_string(),
        attempts: CHAT_FACT_SEND_MAX_ATTEMPTS,
        source: AuraError::agent(last_error.to_string()),
    }
    .into())
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_message_delivery_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    spawner.spawn_cancellable(Box::pin(fut));
}

#[cfg(target_arch = "wasm32")]
fn spawn_message_delivery_task<F>(spawner: &OwnedTaskSpawner, fut: F)
where
    F: Future<Output = ()> + 'static,
{
    spawner.spawn_local_cancellable(Box::pin(fut));
}

async fn mark_message_delivery_failed(
    app_core: &Arc<RwLock<AppCore>>,
    message_id: &str,
) -> Result<(), AuraError> {
    let updated =
        with_chat_state(app_core, |chat_state| chat_state.mark_failed(message_id)).await?;
    if !updated {
        return Ok(());
    }

    #[cfg(feature = "instrumented")]
    tracing::warn!(
        message_id,
        "marked message delivery as failed after remote fanout exhaustion"
    );

    Ok(())
}

async fn deliver_message_fact_remotely(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    context_id: ContextId,
    channel_id: ChannelId,
    sender_id: AuthorityId,
    fact: &RelationalFact,
) -> Result<(), AuraError> {
    let mut delivered_remote = false;
    let mut recipients = recipient_peers_for_channel(app_core, channel_id, sender_id).await;
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
                recipients = recipient_peers_for_channel(app_core, channel_id, sender_id).await;
                continue;
            }
            break;
        }

        let mut channel_setup_errors = Vec::new();
        for peer in recipients.iter().copied() {
            if let Err(error) = runtime.ensure_peer_channel(context_id, peer).await {
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
            recipients = recipient_peers_for_channel(app_core, channel_id, sender_id).await;
        } else {
            break;
        }
    }

    if !delivered_remote {
        if recipients.is_empty() {
            return Err(super::error::WorkflowError::DeliveryFailed {
                peer: channel_id.to_string(),
                attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                source: AuraError::agent("no recipient peers resolved after extended retries"),
            }
            .into());
        }
        if attempted_fanout_total == 0 {
            return Err(
                super::error::WorkflowError::DeliveryPrerequisitesNeverConverged {
                    peer: channel_id.to_string(),
                    attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                    detail: last_connectivity_error
                        .unwrap_or_else(|| "no recipient fanout attempt executed".to_string()),
                }
                .into(),
            );
        }
        if !failed_fanout.is_empty() {
            return Err(super::error::WorkflowError::DeliveryFanoutUnavailable {
                peer: channel_id.to_string(),
                attempts: REMOTE_DELIVERY_RETRY_ATTEMPTS,
                recipients: failed_fanout,
            }
            .into());
        }
    }

    converge_runtime(runtime).await;
    // Post-send connectivity verification.  The message is committed locally
    // regardless, but we surface a diagnostic so the UI can indicate that remote
    // delivery is pending.  This warning fires in both instrumented and
    // non-instrumented builds via the semantic-facts signal.
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

async fn ensure_runtime_note_to_self_channel(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    authority_id: AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    let context_id = note_to_self_context_id(authority_id);
    let channel_id = note_to_self_channel_id(authority_id);

    let create_result = runtime
        .amp_create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(channel_id),
            skip_window: None,
            topic: Some(NOTE_TO_SELF_CHANNEL_TOPIC.to_string()),
        })
        .await;

    let created_now = match create_result {
        Ok(_) => true,
        Err(error) if classify_amp_channel_error(&error) == AmpChannelErrorClass::AlreadyExists => {
            false
        }
        Err(error) => {
            return Err(super::error::runtime_call("create note-to-self channel", error).into());
        }
    };

    if let Err(error) = runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: authority_id,
        })
        .await
    {
        if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
            return Err(super::error::runtime_call("join note-to-self channel", error).into());
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

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| super::error::runtime_call("persist note-to-self channel", e))?;
    }

    with_chat_state(app_core, |chat_state| {
        chat_state.ensure_note_to_self_channel(authority_id);
    })
    .await?;

    Ok(channel_id)
}

fn bootstrap_required_for_recipients(recipient_count: usize) -> bool {
    recipient_count > 0
}

/// Refresh authoritative channel-membership readiness facts from the current channel coordinator.
pub(in crate::workflows) async fn refresh_authoritative_channel_membership_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core).await;
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let mut replacements = Vec::new();
    for state in coordinator.states() {
        let membership_ready = if let Some(runtime) = runtime.as_ref() {
            runtime_channel_state_exists(app_core, runtime, state.channel.id).await?
        } else {
            true
        };
        if membership_ready {
            replacements.push(state.membership_fact());
        }
    }
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            (
                AuthoritativeSemanticFactKind::ChannelMembershipReady,
                replacements,
            ),
        ),
    )
    .await
}

pub(in crate::workflows) async fn refresh_authoritative_recipient_resolution_readiness(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core).await;
    let replacements = coordinator
        .states()
        .iter()
        .filter_map(ChannelReadinessState::recipient_resolution_fact)
        .collect::<Vec<_>>();
    replace_authoritative_semantic_facts_of_kind(
        app_core,
        aura_core::AuthorizedReadinessPublication::authorize(
            semantic_readiness_publication_capability(),
            (
                AuthoritativeSemanticFactKind::RecipientPeersResolved,
                replacements,
            ),
        ),
    )
    .await
}

fn fact_matches_channel(fact: &AuthoritativeSemanticFact, channel_id: ChannelId) -> bool {
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
        | AuthoritativeSemanticFact::ContactLinkReady { .. }
        | AuthoritativeSemanticFact::PendingHomeInvitationReady => false,
    }
}

async fn refresh_authoritative_delivery_readiness_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
    context_id: ContextId,
) -> Result<(), AuraError> {
    let coordinator = ChannelReadinessCoordinator::load(app_core).await;
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
            if runtime.ensure_peer_channel(context_id, peer).await.is_ok() {
                ready_peers.push(peer);
            }
        }
    }
    let (peer_facts, delivery_fact) = channel_state.delivery_facts(context_id, &ready_peers);

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

fn channel_id_from_pending_channel_invitation(invitation: &InvitationInfo) -> Option<ChannelId> {
    match &invitation.invitation_type {
        InvitationBridgeType::Channel { home_id, .. } => home_id.parse().ok(),
        _ => None,
    }
}

fn select_pending_channel_invitation(
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

async fn try_join_via_pending_channel_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    requested_channel_id: ChannelId,
) -> Result<bool, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let pending = runtime
        .try_list_pending_invitations()
        .await
        .map_err(|e| AuraError::from(super::error::runtime_call("list pending invitations", e)))?;
    let Some(invitation) =
        select_pending_channel_invitation(&pending, runtime.authority_id(), requested_channel_id)
    else {
        return Ok(false);
    };
    let invited_channel_id =
        channel_id_from_pending_channel_invitation(&invitation).ok_or_else(|| {
            AuraError::invalid("pending channel invitation missing invited channel id")
        })?;

    if let Err(error) = runtime
        .accept_invitation(invitation.invitation_id.as_str())
        .await
    {
        if classify_invitation_accept_error(&error) != InvitationAcceptErrorClass::AlreadyHandled {
            return Err(
                super::error::runtime_call("accept pending channel invitation", error).into(),
            );
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

    // Best-effort: refresh account state after invitation convergence.
    if let Err(_e) = crate::workflows::system::refresh_account(app_core).await {
        #[cfg(feature = "instrumented")]
        tracing::debug!(error = %_e, "refresh_account after invitation accept failed");
    }

    let local_channel_id =
        local_channel_id_for_accepted_pending_invitation(app_core, &invitation, invited_channel_id)
            .await;

    // Joining by invited channel id is best-effort; some runtimes auto-join on accept.
    if let Err(_e) = join_channel(app_core, local_channel_id).await {
        #[cfg(feature = "instrumented")]
        tracing::debug!(error = %_e, channel_id = %local_channel_id, "best-effort join_channel after invitation accept failed");
    }
    if let Ok(context_id) =
        context_id_for_channel(app_core, local_channel_id, Some(runtime.authority_id())).await
    {
        warm_channel_connectivity(app_core, &runtime, local_channel_id, context_id).await;
    }
    Ok(true)
}

async fn resolve_target_authority_for_invite(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
) -> Result<AuthorityId, AuraError> {
    routing::resolve_target_authority_for_invite(app_core, target_user_id).await
}

/// Get current home channel id as a typed ChannelId.
///
/// Returns the actual home channel ChannelId from the homes signal.
/// Falls back to a deterministic default if no home is selected.
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

    // Fallback: derive a default channel ID from "home" string
    channel_id_from_input("home")
}

/// Get current home channel reference string (e.g., "home:<id>") for display.
///
/// Returns a formatted string suitable for display or legacy APIs that take strings.
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

#[cfg(test)]
fn join_error_is_not_found(error: &AuraError) -> bool {
    validation::join_error_is_not_found(error)
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

async fn ensure_channel_visible_after_join(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: ContextId,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let normalized_name = name_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_start_matches('#'))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| channel_id.to_string());

    with_chat_state(app_core, |chat| {
        if let Some(channel) = chat.channel_mut(&channel_id) {
            if channel.context_id.is_none() {
                channel.context_id = Some(context_id);
            }
            if name_hint.is_some() && channel.name != normalized_name {
                channel.name = normalized_name.clone();
            }
            return;
        }

        let canonical = Channel {
            id: channel_id,
            context_id: Some(context_id),
            name: normalized_name.clone(),
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
        };

        let stale_id = chat
            .all_channels()
            .find(|channel| {
                channel.id != channel_id
                    && !channel.is_dm
                    && channel.name.eq_ignore_ascii_case(&normalized_name)
            })
            .map(|channel| channel.id);
        if let Some(stale_id) = stale_id {
            chat.rebind_channel_identity(&stale_id, canonical);
            return;
        }

        chat.upsert_channel(canonical);
    })
    .await
}

async fn apply_authoritative_membership_projection(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: ContextId,
    joined: bool,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    if joined {
        ensure_channel_visible_after_join(app_core, channel_id, context_id, name_hint).await?;
        let chat = chat_snapshot(app_core).await;
        if chat.channel(&channel_id).is_none() {
            return Err(super::error::WorkflowError::Precondition(
                "join projection missing canonical channel",
            )
            .into());
        }
        return Ok(());
    }

    let updated = with_chat_state(app_core, |chat| {
        if let Some(channel) = chat.channel_mut(&channel_id) {
            channel.context_id = Some(context_id);
            channel.member_count = 0;
            return true;
        }
        false
    })
    .await?;

    if !updated {
        // Channel projections can be pruned transiently during reactive churn.
        // Preserve canonical identity by materializing a placeholder entry so
        // subsequent `/join` reuses the same ChannelId.
        let fallback_name = name_hint
            .map(normalize_channel_name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| channel_id.to_string());

        with_chat_state(app_core, |chat| {
            chat.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: fallback_name.clone(),
                topic: None,
                channel_type: ChannelType::Home,
                unread_count: 0,
                is_dm: false,
                member_ids: Vec::new(),
                member_count: 0,
                last_message: None,
                last_message_time: None,
                last_activity: 0,
                last_finalized_epoch: 0,
            });
        })
        .await?;
    }

    Ok(())
}

async fn restore_home_member_membership(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    authority_id: AuthorityId,
    joined_at_ms: u64,
) -> Result<(), AuraError> {
    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();

    let target_home_id = homes
        .iter()
        .find_map(|(home_id, home)| (home.context_id == Some(context_id)).then_some(*home_id));

    if let Some(home_id) = target_home_id {
        if let Some(home) = homes.home_mut(&home_id) {
            if home.member(&authority_id).is_none() {
                home.add_member(HomeMember {
                    id: authority_id,
                    name: authority_id.to_string(),
                    role: HomeRole::Participant,
                    is_online: true,
                    joined_at: joined_at_ms,
                    last_seen: Some(joined_at_ms),
                    storage_allocated: crate::views::home::HomeState::MEMBER_ALLOCATION,
                });
            }
        }
        core.views_mut().set_homes(homes);
    }

    Ok(())
}

/// Ensure a peer appears in the projected local membership for a channel.
pub async fn project_channel_peer_membership(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    peer_authority: AuthorityId,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    project_channel_peer_membership_with_context(
        app_core,
        channel_id,
        None,
        peer_authority,
        name_hint,
    )
    .await
}

/// Ensure a peer appears in the projected local membership for a channel,
/// using an explicit context hint when one is already known.
pub async fn project_channel_peer_membership_with_context(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: Option<ContextId>,
    peer_authority: AuthorityId,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    with_chat_state(app_core, |chat| {
        let fallback_name = name_hint
            .map(normalize_channel_name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| channel_id.to_string());
        let channel = chat.channel_mut(&channel_id);
        if let Some(channel) = channel {
            if let Some(context_id) = context_id {
                if channel.context_id != Some(context_id) {
                    channel.context_id = Some(context_id);
                }
            } else if channel.context_id.is_none() {
                channel.context_id = None;
            }
            if !channel.member_ids.contains(&peer_authority) {
                channel.member_ids.push(peer_authority);
            }
            channel.member_count = channel
                .member_count
                .max(channel.member_ids.len() as u32 + 1);
            if channel.name == channel.id.to_string() && fallback_name != channel.name {
                channel.name = fallback_name;
            }
            return;
        }

        chat.upsert_channel(Channel {
            id: channel_id,
            context_id,
            name: fallback_name,
            topic: None,
            channel_type: ChannelType::Home,
            unread_count: 0,
            is_dm: false,
            member_ids: vec![peer_authority],
            member_count: 2,
            last_message: None,
            last_message_time: None,
            last_activity: 0,
            last_finalized_epoch: 0,
        });
    })
    .await?;

    project_home_peer_membership(app_core, channel_id, context_id, peer_authority, name_hint)
        .await?;

    Ok(())
}

/// Authoritative channel identity returned by channel-creation workflows.
///
/// This bundle keeps the canonical `channel_id` and the authoritative
/// `context_id` together so frontend callers do not need to rediscover the
/// context immediately after create.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CreatedChannel {
    /// Canonical channel identifier created or selected by the workflow.
    pub channel_id: ChannelId,
    /// Authoritative context associated with the created channel, when known.
    pub context_id: Option<ContextId>,
}

async fn project_home_peer_membership(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: Option<ContextId>,
    peer_authority: AuthorityId,
    name_hint: Option<&str>,
) -> Result<(), AuraError> {
    let joined_at_ms = crate::workflows::time::current_time_ms(app_core).await?;
    let mut core = app_core.write().await;
    let mut homes = core.views().get_homes();
    let local_authority = core.authority().copied();

    let target_home_id = homes
        .home_state(&channel_id)
        .map(|_| channel_id)
        .or_else(|| {
            context_id.and_then(|context_id| {
                homes.iter().find_map(|(home_id, home)| {
                    (home.context_id == Some(context_id)).then_some(*home_id)
                })
            })
        });

    let target_home_id = match target_home_id {
        Some(target_home_id) => target_home_id,
        None => {
            let Some(context_id) = context_id else {
                return Ok(());
            };
            let mut home = HomeState::new(
                channel_id,
                Some(
                    name_hint
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToOwned::to_owned)
                        .unwrap_or_else(|| channel_id.to_string()),
                ),
                peer_authority,
                joined_at_ms,
                context_id,
            );
            if let Some(local_authority) = local_authority.filter(|id| *id != peer_authority) {
                if home.member(&local_authority).is_none() {
                    home.add_member(HomeMember {
                        id: local_authority,
                        name: "You".to_string(),
                        role: HomeRole::Participant,
                        is_online: true,
                        joined_at: joined_at_ms,
                        last_seen: Some(joined_at_ms),
                        storage_allocated: HomeState::MEMBER_ALLOCATION,
                    });
                }
                home.my_role = HomeRole::Participant;
            }
            homes.add_home(home);
            channel_id
        }
    };

    let Some(home) = homes.home_mut(&target_home_id) else {
        return Ok(());
    };

    if let Some(context_id) = context_id {
        if home.context_id != Some(context_id) {
            home.context_id = Some(context_id);
        }
    }

    if home.member(&peer_authority).is_some() {
        let homes_state = homes.clone();
        core.views_mut().set_homes(homes);
        drop(core);
        emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
        return Ok(());
    }

    home.add_member(HomeMember {
        id: peer_authority,
        name: name_hint
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| peer_authority.to_string()),
        role: HomeRole::Participant,
        is_online: true,
        joined_at: joined_at_ms,
        last_seen: Some(joined_at_ms),
        storage_allocated: HomeState::MEMBER_ALLOCATION,
    });
    let homes_state = homes.clone();
    core.views_mut().set_homes(homes);
    drop(core);
    emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;

    Ok(())
}

/// Return the canonical context currently associated with a channel in app state.
///
/// Frontend bindings use this when they must preserve a channel's ownership
/// bundle across screen transitions instead of re-resolving it later from a
/// weaker renderer-local snapshot.
pub async fn authoritative_context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Option<ContextId> {
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    if let Some(runtime) = runtime {
        if let Ok(Some(context_id)) = runtime.resolve_amp_channel_context(channel_id).await {
            return Some(context_id);
        }
    }

    let mut homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    if homes.iter().next().is_none() {
        let signal_homes = read_signal(app_core, &*HOMES_SIGNAL, HOMES_SIGNAL_NAME)
            .await
            .unwrap_or_default();
        if signal_homes.iter().next().is_some() {
            homes = signal_homes;
        }
    }

    if let Some(context_id) = homes
        .home_state(&channel_id)
        .and_then(|home| home.context_id)
    {
        return Some(context_id);
    }

    chat_snapshot(app_core)
        .await
        .channel(&channel_id)
        .and_then(|channel| channel.context_id)
}

pub(crate) async fn authoritative_local_channel_id_for_context(
    app_core: &Arc<RwLock<AppCore>>,
    context_id: ContextId,
    channel_name_hint: Option<&str>,
) -> Option<ChannelId> {
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    if let Some((home_id, _)) = homes
        .iter()
        .find(|(_, home)| home.context_id == Some(context_id))
    {
        return Some(*home_id);
    }

    let normalized_name_hint = channel_name_hint
        .map(normalize_channel_name)
        .filter(|value| !value.is_empty());
    let chat = chat_snapshot(app_core).await;
    if let Some(channel) = chat.all_channels().find(|channel| {
        if channel.context_id != Some(context_id) {
            return false;
        }
        normalized_name_hint
            .as_ref()
            .is_none_or(|name_hint| normalize_channel_name(&channel.name) == *name_hint)
    }) {
        return Some(channel.id);
    }

    if let Some(channel_name_hint) = channel_name_hint {
        return resolve_chat_channel_id_from_state_or_input(app_core, channel_name_hint)
            .await
            .ok();
    }

    None
}

async fn local_channel_id_for_accepted_pending_invitation(
    app_core: &Arc<RwLock<AppCore>>,
    invitation: &InvitationInfo,
    fallback_channel_id: ChannelId,
) -> ChannelId {
    match &invitation.invitation_type {
        InvitationBridgeType::Channel { .. } => {
            let _ = app_core;
            fallback_channel_id
        }
        _ => fallback_channel_id,
    }
}

pub(crate) async fn require_authoritative_context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
    _operation: &str,
) -> Result<ContextId, AuraError> {
    let policy = workflow_retry_policy(
        CHANNEL_CONTEXT_RETRY_ATTEMPTS as u32,
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
    )?;
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        if let Some(context_id) = authoritative_context_id_for_channel(app_core, channel_id).await {
            return Ok(context_id);
        }
        converge_runtime(runtime).await;
        Err(AuraError::from(super::error::WorkflowError::Precondition(
            "authoritative context required for channel",
        )))
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { .. } => AuraError::from(
            super::error::WorkflowError::Precondition("authoritative context required for channel"),
        ),
    })
}

pub(in crate::workflows) async fn runtime_channel_state_exists(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
) -> Result<bool, AuraError> {
    let Some(context_id) = authoritative_context_id_for_channel(app_core, channel_id).await else {
        return Ok(false);
    };

    runtime
        .amp_channel_state_exists(context_id, channel_id)
        .await
        .map_err(|error| super::error::runtime_call("inspect channel state", error).into())
}

pub(in crate::workflows) async fn wait_for_runtime_channel_state(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    let policy = workflow_retry_policy(
        CHANNEL_CONTEXT_RETRY_ATTEMPTS as u32,
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
        Duration::from_millis(CHANNEL_CONTEXT_RETRY_BACKOFF_MS),
    )?;
    execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        if runtime_channel_state_exists(app_core, runtime, channel_id).await? {
            refresh_authoritative_channel_membership_readiness(app_core).await?;
            return Ok(());
        }
        converge_runtime(runtime).await;
        Err(AuraError::from(super::error::WorkflowError::Precondition(
            "canonical AMP channel state required",
        )))
    })
    .await
    .map_err(|error| match error {
        RetryRunError::Timeout(timeout_error) => timeout_error.into(),
        RetryRunError::AttemptsExhausted { .. } => AuraError::from(
            super::error::WorkflowError::Precondition("canonical AMP channel state required"),
        ),
    })
}

async fn ensure_home_state_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    context_id: ContextId,
    owner_id: AuthorityId,
    channel_name: &str,
    members: &[AuthorityId],
    now_ms: u64,
) -> Result<(), AuraError> {
    let mut changed = false;
    let homes_state = {
        let mut core = app_core.write().await;
        let mut homes = core.views().get_homes();

        if let Some(home) = homes.home_mut(&channel_id) {
            if home.context_id.is_none() {
                home.context_id = Some(context_id);
                changed = true;
            }
            if let Some(member) = home.member_mut(&owner_id) {
                if member.role != HomeRole::Member {
                    member.role = HomeRole::Member;
                    changed = true;
                }
            } else {
                home.add_member(HomeMember {
                    id: owner_id,
                    name: owner_id.to_string(),
                    role: HomeRole::Member,
                    is_online: true,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
                changed = true;
            }

            for member in members {
                if *member == owner_id || home.member(member).is_some() {
                    continue;
                }
                home.add_member(HomeMember {
                    id: *member,
                    name: member.to_string(),
                    role: HomeRole::Participant,
                    is_online: true,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
                changed = true;
            }
        } else {
            let mut home = HomeState::new(
                channel_id,
                Some(channel_name.to_string()),
                owner_id,
                now_ms,
                context_id,
            );
            for member in members {
                if *member == owner_id || home.member(member).is_some() {
                    continue;
                }
                home.add_member(HomeMember {
                    id: *member,
                    name: member.to_string(),
                    role: HomeRole::Participant,
                    is_online: true,
                    joined_at: now_ms,
                    last_seen: Some(now_ms),
                    storage_allocated: HomeState::MEMBER_ALLOCATION,
                });
            }
            homes.add_home_with_auto_select(home);
            changed = true;
        }

        if homes.current_home_id().is_none() {
            homes.select_home(Some(channel_id));
            changed = true;
        }

        core.views_mut().set_homes(homes.clone());
        homes
    };

    if changed {
        emit_signal(app_core, &*HOMES_SIGNAL, homes_state, HOMES_SIGNAL_NAME).await?;
    }

    Ok(())
}
async fn recipient_peers_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    self_authority: AuthorityId,
) -> Vec<AuthorityId> {
    let chat = chat_snapshot(app_core).await;
    let Some(channel) = chat.channel(&channel_id).cloned() else {
        return Vec::new();
    };
    let contacts = contacts_snapshot(app_core).await;
    let homes = {
        let core = app_core.read().await;
        core.views().get_homes()
    };
    let runtime = {
        let core = app_core.read().await;
        core.runtime().cloned()
    };
    let discovered = if let Some(runtime) = runtime {
        match runtime.try_get_discovered_peers().await {
            Ok(peers) => peers
                .into_iter()
                .filter(|peer| *peer != self_authority)
                .collect::<Vec<_>>(),
            Err(_error) => {
                #[cfg(feature = "instrumented")]
                tracing::debug!(
                    error = %_error,
                    "resolved recipient peer lookup skipped discovered peers because runtime read failed"
                );
                Vec::new()
            }
        }
    } else {
        Vec::new()
    };
    resolved_recipient_peers_for_channel_view(
        &channel,
        &homes,
        &contacts,
        &discovered,
        self_authority,
    )
}

/// Best-effort channel connectivity warming.
///
/// Runs a bounded retry loop attempting to establish peer channels and
/// verify connectivity.  Returns `true` if warming succeeded, `false`
/// if retries were exhausted or policy creation failed.  Callers that
/// need guaranteed connectivity should check the return value.
async fn warm_channel_connectivity(
    app_core: &Arc<RwLock<AppCore>>,
    runtime: &Arc<dyn RuntimeBridge>,
    channel_id: ChannelId,
    context_id: ContextId,
) -> bool {
    let policy =
        match workflow_retry_policy(8, Duration::from_millis(150), Duration::from_millis(750)) {
            Ok(policy) => policy,
            Err(_) => return false,
        };
    let result = execute_with_runtime_retry_budget(runtime, &policy, |_attempt| async {
        let recipients =
            recipient_peers_for_channel(app_core, channel_id, runtime.authority_id()).await;
        let mut any_peer_ready = recipients.is_empty();
        for peer in recipients {
            if runtime.ensure_peer_channel(context_id, peer).await.is_ok() {
                any_peer_ready = true;
            }
        }
        let _ = refresh_authoritative_delivery_readiness_for_channel(
            app_core, runtime, channel_id, context_id,
        )
        .await;
        converge_runtime(runtime).await;
        let _ = crate::workflows::system::refresh_account(app_core).await;
        if any_peer_ready
            || ensure_runtime_peer_connectivity(runtime, "warm_channel_connectivity")
                .await
                .is_ok()
        {
            Ok(())
        } else {
            Err(AuraError::from(super::error::WorkflowError::Precondition(
                "channel peer connectivity not yet warmed",
            )))
        }
    })
    .await;
    let warmed = result.is_ok();
    if !warmed {
        #[cfg(feature = "instrumented")]
        tracing::warn!(
            channel_id = %channel_id,
            context_id = %context_id,
            "channel connectivity warming exhausted retries"
        );
    }
    warmed
}

/// Resolve recipient peers for a channel view from known channel, home, contact, and discovery state.
pub fn resolved_recipient_peers_for_channel_view(
    channel: &Channel,
    homes: &HomesState,
    contacts: &ContactsState,
    discovered: &[AuthorityId],
    self_authority: AuthorityId,
) -> Vec<AuthorityId> {
    let channel_context = channel.context_id;
    let mut recipients = BTreeSet::new();

    for member in &channel.member_ids {
        if *member != self_authority {
            recipients.insert(*member);
        }
    }

    if recipients.is_empty() {
        if let Some(home) = homes.home_state(&channel.id) {
            for member in home.members.iter().map(|member| member.id) {
                if member != self_authority {
                    recipients.insert(member);
                }
            }
        }

        if recipients.is_empty() {
            if let Some(context_id) = channel_context {
                for (_, home) in homes.iter() {
                    if home.context_id == Some(context_id) {
                        for member in home.members.iter().map(|member| member.id) {
                            if member != self_authority {
                                recipients.insert(member);
                            }
                        }
                    }
                }
            }
        }

        if recipients.is_empty() {
            if let Some(home) = homes.current_home() {
                if channel_context.is_none() || home.context_id == channel_context {
                    for member in home.members.iter().map(|member| member.id) {
                        if member != self_authority {
                            recipients.insert(member);
                        }
                    }
                }
            }
        }
    }

    if recipients.is_empty() && (channel.is_dm || discovered.len() == 1) {
        recipients.extend(discovered.iter().copied());
    }

    if recipients.is_empty() {
        for contact_id in contacts.contact_ids() {
            if *contact_id != self_authority {
                recipients.insert(*contact_id);
            }
        }
        if !channel.is_dm && recipients.len() > 1 {
            recipients.clear();
        }
    }

    recipients.into_iter().collect()
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
    let mut channel_id = ChannelId::from_bytes(hash(format!("local:{timestamp_ms}").as_bytes()));
    let mut channel_context: Option<ContextId> = None;
    let mut channel_owner: Option<AuthorityId> = None;
    if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        channel_owner = Some(runtime.authority_id());
        let (context_id, _is_home_context) =
            current_home_context_or_authority_default(app_core, runtime.authority_id()).await?;
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

        channel_id = runtime
            .amp_create_channel(params)
            .await
            .map_err(|e| super::error::runtime_call("create channel", e))?;

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: runtime.authority_id(),
            })
            .await
            .map_err(|e| super::error::runtime_call("join channel", e))?;

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

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|e| super::error::runtime_call("persist channel", e))?;

        let mut attempted_fanout = 0usize;
        let mut failed_fanout = Vec::new();
        for peer in member_ids.iter().copied() {
            if peer == runtime.authority_id() {
                continue;
            }
            attempted_fanout = attempted_fanout.saturating_add(1);
            if let Err(error) = runtime.send_chat_fact(peer, context_id, &fact).await {
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

    // Update UI state for responsiveness; reactive reductions may also update this later.
    with_chat_state(app_core, |chat_state| {
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

        // Upsert to avoid races with reactive ChannelCreated reductions that may
        // insert the channel first without populated member_ids.
        chat_state.upsert_channel(channel);
    })
    .await?;

    if backend == MessagingBackend::Runtime {
        if let (Some(context_id), Some(owner_id)) = (channel_context, channel_owner) {
            ensure_home_state_for_channel(
                app_core,
                channel_id,
                context_id,
                owner_id,
                name,
                &member_ids,
                timestamp_ms,
            )
            .await?;
        }
    }

    if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        wait_for_runtime_channel_state(app_core, &runtime, channel_id).await?;
    }

    // Create channel invitations for selected members (if any).
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

        // Always include bootstrap key material when issuing channel invitations.
        // Without this, recipients can join by name/context but fail to decrypt
        // channel payloads (rendered as "[sealed: N bytes]").
        let bootstrap = if bootstrap_required_for_recipients(member_ids.len()) {
            Some(
                runtime
                    .amp_create_channel_bootstrap(context_id, channel_id, member_ids.clone())
                    .await
                    .map_err(|e| super::error::runtime_call("bootstrap channel", e))?,
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
                    // Some runtime profiles do not grant invitation capabilities.
                    // Fall back to a direct membership join fact so chats remain usable.
                    if let Err(_join_error) = runtime
                        .amp_join_channel(ChannelJoinParams {
                            context: context_id,
                            channel: channel_id,
                            participant: *receiver,
                        })
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
                    if let Err(_join_error) = runtime
                        .amp_join_channel(ChannelJoinParams {
                            context: context_id,
                            channel: channel_id,
                            participant: *receiver,
                        })
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
            runtime
                .start_channel_invitation_monitor(invitation_ids, context_id, channel_id)
                .await
                .map_err(|e| super::error::runtime_call("start channel invitation monitor", e))?;
        }
    }

    refresh_authoritative_channel_membership_readiness(app_core).await?;

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
                .await?
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
pub async fn join_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };
    let context_id = authoritative_context_id_for_channel(app_core, channel_id)
        .await
        .ok_or_else(|| {
            JoinChannelError::MissingAuthoritativeContext { channel_id }.into_aura_error()
        })?;
    enforce_home_join_allowed(app_core, context_id, channel_id, runtime.authority_id()).await?;

    if let Err(error) = runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
    {
        let canonical_state_exists = runtime_channel_state_exists(app_core, &runtime, channel_id)
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

    restore_home_member_membership(
        app_core,
        context_id,
        runtime.authority_id(),
        crate::workflows::time::current_time_ms(app_core).await?,
    )
    .await?;
    apply_authoritative_membership_projection(app_core, channel_id, context_id, true, None).await?;
    wait_for_runtime_channel_state(app_core, &runtime, channel_id).await?;
    warm_channel_connectivity(app_core, &runtime, channel_id, context_id).await;

    Ok(())
}

async fn fail_join_channel<T>(
    owner: &SemanticWorkflowOwner,
    error: JoinChannelError,
) -> Result<T, AuraError> {
    owner.publish_failure(error.semantic_error()).await?;
    Err(error.into_aura_error())
}

/// Join an existing channel by name (legacy/convenience API).
pub async fn join_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<(), AuraError> {
    join_channel_by_name_with_instance(app_core, channel_name, None).await
}

/// Join an existing channel by name and attribute the semantic operation to a
/// specific UI instance when one exists.
pub async fn join_channel_by_name_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<(), AuraError> {
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

#[aura_macros::semantic_owner(
    owner = "join_channel_by_name_with_instance",
    terminal = "publish_success_with",
    postcondition = "channel_membership_ready",
    proof = crate::workflows::semantic_facts::ChannelMembershipReadyProof,
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
) -> Result<(), AuraError> {
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

    let channel_id = resolve_chat_channel_id_from_state_or_input(app_core, channel_name).await?;
    let channel_exists_locally = {
        let chat = chat_snapshot(app_core).await;
        chat.channel(&channel_id).is_some()
            || chat
                .all_channels()
                .any(|channel| channel.name.eq_ignore_ascii_case(channel_name))
    };
    let known_members: Vec<String> = contacts_snapshot(app_core)
        .await
        .contact_ids()
        .map(ToString::to_string)
        .collect();

    // Local-only frontends still need "/join" to create/select channels.
    if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
        if !channel_exists_locally {
            create_channel(app_core, channel_name, None, &known_members, 0, 0).await?;
        }
        owner
            .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
            .await?;
        return Ok(());
    }

    if !channel_exists_locally
        && try_join_via_pending_channel_invitation(app_core, channel_id).await?
    {
        owner
            .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
            .await?;
        return Ok(());
    }

    if let Err(error) = join_channel(app_core, channel_id).await {
        let join_error = match error {
            aura_core::AuraError::NotFound { .. } => {
                JoinChannelError::MissingAuthoritativeContext { channel_id }
            }
            _ => JoinChannelError::Transport {
                channel_id,
                detail: error.to_string(),
            },
        };
        return fail_join_channel(&owner, join_error).await;
    }
    let context_id = authoritative_context_id_for_channel(app_core, channel_id)
        .await
        .ok_or_else(|| {
            JoinChannelError::MissingAuthoritativeContext { channel_id }.into_aura_error()
        })?;
    apply_authoritative_membership_projection(
        app_core,
        channel_id,
        context_id,
        true,
        Some(channel_name),
    )
    .await?;
    let runtime = require_runtime(app_core).await?;
    converge_runtime(&runtime).await;
    if let Err(_error) = ensure_runtime_peer_connectivity(&runtime, "join_channel_by_name").await {
        messaging_warn!(
            "Channel {} joined before connectivity fully warmed: {}",
            channel_id,
            _error
        );
    }
    owner
        .publish_success_with(prove_channel_membership_ready(app_core, channel_id).await?)
        .await?;
    Ok(())
}

/// Leave a channel using a typed ChannelId.
pub async fn leave_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    if messaging_backend(app_core).await == MessagingBackend::LocalOnly {
        with_chat_state(app_core, |chat| {
            let _ = chat.remove_channel(&channel_id);
        })
        .await?;
        return Ok(());
    }

    let runtime = { require_runtime(app_core).await? };
    let context_id =
        context_id_for_channel(app_core, channel_id, Some(runtime.authority_id())).await?;

    runtime
        .amp_leave_channel(ChannelLeaveParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|e| super::error::runtime_call("leave channel", e))?;

    // Preserve canonical channel identity for future join operations.
    apply_authoritative_membership_projection(app_core, channel_id, context_id, false, None)
        .await?;

    Ok(())
}

/// Leave a channel by name (legacy/convenience API).
pub async fn leave_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<(), AuraError> {
    let mut candidate_ids = matching_chat_channel_ids(app_core, channel_name).await;
    if candidate_ids.is_empty() {
        candidate_ids
            .push(resolve_chat_channel_id_from_state_or_input(app_core, channel_name).await?);
    }

    // Channel views can transiently carry duplicate IDs for the same display name
    // (for example hash-derived and runtime-derived identifiers). Expand the leave
    // set to all channels sharing the resolved display name(s) so `/leave` is
    // semantically idempotent for a named channel.
    let snapshot = chat_snapshot(app_core).await;
    let mut candidate_names = BTreeSet::new();
    for channel_id in &candidate_ids {
        if let Some(channel) = snapshot.channel(channel_id) {
            candidate_names.insert(channel.name.to_ascii_lowercase());
        }
    }
    if !candidate_names.is_empty() {
        for channel in snapshot.all_channels() {
            if candidate_names.contains(&channel.name.to_ascii_lowercase())
                && !candidate_ids.contains(&channel.id)
            {
                candidate_ids.push(channel.id);
            }
        }
    }

    for channel_id in &candidate_ids {
        leave_channel(app_core, *channel_id).await?;
    }

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
    let runtime = { require_runtime(app_core).await? };
    let context_id = context_id_for_channel(app_core, channel_id, None).await?;

    runtime
        .amp_close_channel(ChannelCloseParams {
            context: context_id,
            channel: channel_id,
        })
        .await
        .map_err(|e| super::error::runtime_call("close channel", e))?;

    let fact =
        ChatFact::channel_closed_ms(context_id, channel_id, timestamp_ms, runtime.authority_id())
            .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| super::error::runtime_call("persist channel close", e))?;

    Ok(())
}

/// Close/archive a channel by name (legacy/convenience API).
pub async fn close_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_chat_channel_id_from_state_or_input(app_core, channel_name).await?;
    close_channel(app_core, channel_id, timestamp_ms).await
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
    let runtime = { require_runtime(app_core).await? };
    let context_id = context_id_for_channel(app_core, channel_id, None).await?;

    runtime
        .channel_set_topic(context_id, channel_id, text.to_string(), timestamp_ms)
        .await
        .map_err(|e| super::error::runtime_call("set channel topic", e))?;

    Ok(())
}

/// Set a channel topic by name (legacy/convenience API).
pub async fn set_topic_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_chat_channel_id_from_state_or_input(app_core, channel_name).await?;
    set_topic(app_core, channel_id, text, timestamp_ms).await
}

/// Send a message to a group/channel using a typed ChannelId.
///
/// **What it does**: Appends a message to the selected channel's message list
/// **Returns**: Message ID
/// **Signal pattern**: Updates `CHAT_SIGNAL` directly
///
/// # Type Safety
/// This function accepts `ChannelId` directly to ensure you're using the exact
/// channel identity returned by `create_channel`. Using the typed ID prevents
/// mismatches between runtime-generated IDs and name-based hash IDs.
///
/// **Note**: This is currently UI-local state only; persistence will be provided by
/// runtime-backed AMP/Chat facts when fully wired.
pub async fn send_message(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    send_message_ref(app_core, ChannelRef::Id(channel_id), content, timestamp_ms).await
}

/// Resolve workflow time and send a message by typed channel ID.
pub async fn send_message_now(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message(app_core, channel_id, content, timestamp_ms).await
}

/// Send a message to a group/channel by name (legacy/convenience API).
///
/// **What it does**: Looks up channel by name and sends message
/// **Returns**: Message ID
/// **Signal pattern**: Updates `CHAT_SIGNAL` directly
///
/// # Warning
/// Prefer `send_message` with a typed `ChannelId` when possible. Name-based
/// lookup uses hash derivation which may not match runtime-created channels.
/// Use this only when you don't have the original `ChannelId` from `create_channel`.
pub async fn send_message_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_ref = ChannelRef::Name(channel_name.to_string());
    send_message_ref(app_core, channel_ref, content, timestamp_ms).await
}

/// Resolve workflow time and send a message by channel name.
pub async fn send_message_by_name_now(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    content: &str,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message_by_name(app_core, channel_name, content, timestamp_ms).await
}

/// Send a message by typed channel ID while binding lifecycle publication to an
/// exact submitted operation instance.
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

/// Resolve workflow time and send a message by typed channel ID while binding
/// lifecycle publication to an exact submitted operation instance.
pub async fn send_message_now_with_instance(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    content: &str,
    instance_id: Option<OperationInstanceId>,
) -> Result<String, AuraError> {
    let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
    send_message_with_instance(app_core, channel_id, content, timestamp_ms, instance_id).await
}

/// Send a message by channel name while binding lifecycle publication to an
/// exact submitted operation instance.
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

/// Resolve workflow time and send a message by channel name while binding
/// lifecycle publication to an exact submitted operation instance.
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

/// Send a message by channel reference while binding lifecycle publication to
/// an exact submitted operation instance.
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
    send_message_ref_owned(app_core, channel, content, timestamp_ms, &owner).await
}

/// Send a message to a channel by reference.
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
    send_message_ref_owned(app_core, channel, content, timestamp_ms, &owner).await
}

async fn send_message_ref_owned(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
    owner: &SemanticWorkflowOwner,
) -> Result<String, AuraError> {
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;

    let (channel_id, channel_label) = match &channel {
        ChannelRef::Id(id) => (*id, id.to_string()),
        ChannelRef::Name(name) => {
            match resolve_chat_channel_id_from_state_or_input(app_core, name).await {
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

    let backend = messaging_backend(app_core).await;
    let mut channel_context: Option<ContextId> = None;
    let mut epoch_hint: Option<u32> = None;
    let (sender_id, message_id) = if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let sender_id = runtime.authority_id();
        let is_note_to_self = channel_id == note_to_self_channel_id(sender_id)
            || matches!(&channel, ChannelRef::Name(name) if is_note_to_self_channel_name(name));
        if is_note_to_self {
            ensure_runtime_note_to_self_channel(app_core, &runtime, sender_id, timestamp_ms)
                .await?;
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
            runtime
                .commit_relational_facts(std::slice::from_ref(&fact))
                .await
                .map_err(|e| super::error::runtime_call("persist note-to-self message", e))?;
            (sender_id, message_id)
        } else {
            let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
            let channel_view = chat_snapshot(app_core).await.channel(&channel_id).cloned();
            let context_id = match authoritative_context_id_for_channel(app_core, channel_id).await
            {
                Some(context_id) => context_id,
                None => {
                    return fail_send_message(
                        owner,
                        SendMessageError::MissingAuthoritativeContext { channel_id },
                    )
                    .await;
                }
            };
            owner
                .publish_phase(SemanticOperationPhase::AuthoritativeContextReady)
                .await?;
            if send_operation_requires_delivery_readiness(channel_view.as_ref()) {
                let readiness = match require_send_message_readiness(app_core, channel_id).await {
                    Ok(readiness) => readiness,
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
            let mut maybe_cipher = match runtime.amp_send_message(send_params.clone()).await {
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
                        match runtime.amp_send_message(send_params).await {
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
                let wire = AmpMessage::new(cipher.header.clone(), cipher.ciphertext.clone());
                let sealed = serialize_amp_message(&wire).map_err(super::error::fact_encoding)?;

                // Extract epoch from the AMP header (used for consensus finalization tracking)
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
                runtime
                    .commit_relational_facts_with_options(
                        std::slice::from_ref(&fact),
                        FactOptions::default().with_ack_tracking(),
                    )
                    .await
                    .map_err(|e| super::error::runtime_call("persist message", e))?;

                if let Some(spawner) = runtime.task_spawner() {
                    let background_app_core = Arc::clone(app_core);
                    let background_runtime = Arc::clone(&runtime);
                    let background_fact = fact.clone();
                    let background_message_id = message_id.clone();
                    spawn_message_delivery_task(&spawner, async move {
                        if let Err(_error) = deliver_message_fact_remotely(
                            &background_app_core,
                            &background_runtime,
                            context_id,
                            channel_id,
                            sender_id,
                            &background_fact,
                        )
                        .await
                        {
                            #[cfg(feature = "instrumented")]
                            tracing::warn!(
                                error = %_error,
                                channel_id = %channel_id,
                                message_id = %background_message_id,
                                "background remote message delivery failed"
                            );
                            if let Err(_mark_err) = mark_message_delivery_failed(
                                &background_app_core,
                                &background_message_id,
                            )
                            .await
                            {
                                #[cfg(feature = "instrumented")]
                                tracing::error!(
                                    delivery_error = %_error,
                                    mark_error = %_mark_err,
                                    message_id = %background_message_id,
                                    "double failure: delivery failed and mark-failed also failed — message stuck in Sending state"
                                );
                            }
                        }
                    });
                } else {
                    deliver_message_fact_remotely(
                        app_core, &runtime, context_id, channel_id, sender_id, &fact,
                    )
                    .await?;
                }
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

    // Update UI state for responsiveness.
    with_chat_state(app_core, |chat_state| {
        if !chat_state.has_channel(&channel_id) {
            chat_state.add_channel(Channel {
                id: channel_id,
                context_id: channel_context,
                name: channel_label,
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

    owner
        .publish_success_with(prove_message_committed(app_core, channel_id, &message_id).await?)
        .await?;

    Ok(message_id)
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

/// Start a direct chat with a canonical authority ID.
pub async fn start_direct_chat_with_authority(
    app_core: &Arc<RwLock<AppCore>>,
    contact_authority: AuthorityId,
    timestamp_ms: u64,
) -> Result<ChannelId, AuraError> {
    let backend = messaging_backend(app_core).await;
    let contacts = contacts_snapshot(app_core).await;
    let contact_id = contact_authority.to_string();

    // Get contact name from ViewState for the channel name
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
        // Runtime mode provisions a deterministic two-party channel without requiring
        // invitation capabilities in the active runtime profile.
        let runtime = require_runtime(app_core).await?;
        // Use a pairwise deterministic context so both peers converge on the same
        // transport/journal scope instead of each side's current home context.
        let context_id = pair_dm_context_id(runtime.authority_id(), contact_authority);
        let channel_name = if contact_name.trim().is_empty() {
            format!("dm-{}", &contact_id[..8.min(contact_id.len())])
        } else {
            format!("DM: {contact_name}")
        };
        let channel_id = pair_dm_channel_id(runtime.authority_id(), contact_authority);

        let create_result = runtime
            .amp_create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: Some(format!("Direct messages with {contact_id}")),
            })
            .await;
        if let Err(error) = create_result {
            if classify_amp_channel_error(&error) != AmpChannelErrorClass::AlreadyExists {
                return Err(super::error::runtime_call("create direct channel", error).into());
            }
        }

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: runtime.authority_id(),
            })
            .await
            .map_err(|error| super::error::runtime_call("join direct channel", error))?;

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: contact_authority,
            })
            .await
            .map_err(|error| super::error::runtime_call("add contact to direct channel", error))?;

        let fact = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            channel_name.clone(),
            Some(format!("Direct messages with {contact_id}")),
            true,
            timestamp_ms,
            runtime.authority_id(),
        )
        .to_generic();

        runtime
            .commit_relational_facts(std::slice::from_ref(&fact))
            .await
            .map_err(|error| super::error::runtime_call("persist direct channel", error))?;

        send_chat_fact_with_retry(&runtime, contact_authority, context_id, &fact).await?;

        with_chat_state(app_core, |chat_state| {
            chat_state.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: channel_name.clone(),
                topic: Some(format!("Direct messages with {contact_id}")),
                channel_type: ChannelType::DirectMessage,
                unread_count: 0,
                is_dm: true,
                member_ids: vec![contact_authority],
                member_count: 2,
                last_message: None,
                last_message_time: None,
                last_activity: timestamp_ms,
                last_finalized_epoch: 0,
            });
        })
        .await?;
        return Ok(channel_id);
    }

    let channel_id = dm_channel_id(&contact_id);
    let now = timestamp_ms;

    // Create the DM channel
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
        member_count: 2, // Self + contact
        last_message: None,
        last_message_time: None,
        last_activity: now,
        last_finalized_epoch: 0,
    };

    with_chat_state(app_core, |chat_state| {
        // Add the DM channel (add_channel avoids duplicates)
        chat_state.add_channel(dm_channel);
    })
    .await?;

    Ok(channel_id)
}

/// Get current chat state
///
/// **What it does**: Reads chat state from ViewState
/// **Returns**: Current chat state with channels and messages
/// **Signal pattern**: Read-only operation (no emission)
pub async fn get_chat_state(app_core: &Arc<RwLock<AppCore>>) -> Result<ChatState, AuraError> {
    Ok(chat_snapshot(app_core).await)
}

/// Send an action/emote message to a channel
///
/// **What it does**: Sends an IRC-style /me action to a channel
/// **Returns**: Message ID
/// **Signal pattern**: Updates ViewState; signal forwarding handles CHAT_SIGNAL
///
/// Action messages are formatted as "* Sender action text" and displayed
/// differently from regular messages in the UI.
///
/// # Arguments
/// * `app_core` - The application core
/// * `channel_id` - Target channel ID
/// * `action` - Action text (e.g., "waves hello")
/// * `timestamp_ms` - Current timestamp in milliseconds (caller provides via effect system)
pub async fn send_action(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    action: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let content = format!("* You {action}");
    send_message(app_core, channel_id, &content, timestamp_ms).await
}

/// Send an action/emote message to a channel by name (legacy/convenience API).
pub async fn send_action_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    action: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let content = format!("* You {action}");
    send_message_by_name(app_core, channel_name, &content, timestamp_ms).await
}

/// Invite a user to join a channel
///
/// **What it does**: Creates a channel invitation for the target user
/// **Returns**: Invitation ID
/// **Signal pattern**: RuntimeBridge handles signal emission
///
/// This delegates to the invitation workflow to create a channel invitation.
/// The target user receives the invitation and can accept to join the channel.
///
/// # Arguments
/// * `app_core` - The application core
/// * `target_user_id` - Target user's authority ID
/// * `channel_id` - Channel to invite user to (required - UI manages selection)
/// * `message` - Optional invitation message
/// * `ttl_ms` - Optional time-to-live for the invitation
pub async fn invite_user_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_id: &str,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    invite_user_to_channel_with_context(
        app_core,
        target_user_id,
        channel_id,
        None,
        None,
        message,
        ttl_ms,
    )
    .await
}

/// Invite a user to a channel while carrying an already-authoritative context,
/// when the caller has one.
pub async fn invite_user_to_channel_with_context(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_name_or_id: &str,
    context_id: Option<ContextId>,
    operation_instance_id: Option<OperationInstanceId>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        operation_instance_id.clone(),
        SemanticOperationKind::InviteActorToChannel,
    );
    owner
        .publish_phase(SemanticOperationPhase::WorkflowDispatched)
        .await?;
    invite_user_to_channel_with_context_owned(
        app_core,
        target_user_id,
        channel_name_or_id,
        context_id,
        operation_instance_id,
        message,
        ttl_ms,
        &owner,
        None,
    )
    .await
}

#[aura_macros::semantic_owner(
    owner = "invite_user_to_channel_with_context",
    terminal = "publish_success_with",
    postcondition = "channel_invitation_created",
    proof = crate::workflows::semantic_facts::ChannelInvitationCreatedProof,
    depends_on = "target_authority_resolved,channel_target_resolved",
    child_ops = "",
    category = "move_owned"
)]
async fn invite_user_to_channel_with_context_owned(
    app_core: &Arc<RwLock<AppCore>>,
    target_user_id: &str,
    channel_name_or_id: &str,
    context_id: Option<ContextId>,
    _operation_instance_id: Option<OperationInstanceId>,
    message: Option<String>,
    ttl_ms: Option<u64>,
    owner: &SemanticWorkflowOwner,
    _operation_context: Option<
        &mut OperationContext<OperationId, OperationInstanceId, TraceContext>,
    >,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let deadline = workflow_timeout_budget(
        &runtime,
        Duration::from_millis(INVITE_USER_OPERATION_TIMEOUT_MS),
    )
    .await?;
    let stage_tracker = Some(new_invite_stage_tracker("publish_workflow_dispatched"));

    let result = execute_with_runtime_timeout_budget(&runtime, &deadline, || async {
        // Resolve via contacts first so command targets can use IDs or contact names.
        update_invite_stage(&stage_tracker, "resolve_target_authority");
        let receiver = timeout_workflow_stage_with_deadline(
            &runtime,
            "invite_user_to_channel",
            "resolve_target_authority",
            Some(deadline),
            resolve_target_authority_for_invite(app_core, target_user_id),
        )
        .await?;
        update_invite_stage(&stage_tracker, "resolve_channel_id");
        let channel_id = timeout_workflow_stage_with_deadline(
            &runtime,
            "invite_user_to_channel",
            "resolve_channel_id",
            Some(deadline),
            resolve_chat_channel_id_from_state_or_input(app_core, channel_name_or_id),
        )
        .await?;
        let channel_name_hint = normalize_channel_name(channel_name_or_id);
        update_invite_stage(&stage_tracker, "invite_authority_to_channel");

        invite_authority_to_channel_with_context(
            app_core,
            receiver,
            channel_id,
            context_id,
            Some(channel_name_hint),
            &owner,
            None,
            Some(deadline),
            stage_tracker.clone(),
            message,
            ttl_ms,
        )
        .await
    })
    .await
    .map_err(|error| match error {
        TimeoutRunError::Timeout(TimeoutBudgetError::DeadlineExceeded { .. }) => {
            let stage = stage_tracker
                .as_ref()
                .and_then(|tracker| tracker.lock().ok().map(|guard| *guard))
                .unwrap_or("operation");
            AuraError::from(super::error::WorkflowError::TimedOut {
                operation: "invite_user_to_channel",
                stage,
                timeout_ms: deadline.timeout_ms(),
            })
        }
        TimeoutRunError::Timeout(timeout_error) => timeout_error.into(),
        TimeoutRunError::Operation(operation_error) => operation_error,
    });

    if let Err(error) = &result {
        let semantic_error = SemanticOperationError::new(
            SemanticFailureDomain::Command,
            SemanticFailureCode::InternalError,
        )
        .with_detail(error.to_string());
        if let Err(_pub_err) = owner.publish_failure(semantic_error).await {
            #[cfg(feature = "instrumented")]
            tracing::error!(
                operation_error = %error,
                publish_error = %_pub_err,
                "invite_authority_to_channel: failed to publish failure fact"
            );
        }
    }

    let invitation_id = result?;
    owner
        .publish_success_with(issue_channel_invitation_created_proof(
            invitation_id.clone(),
        ))
        .await?;
    Ok(invitation_id)
}

/// Invite a canonical authority to a canonical channel.
pub async fn invite_authority_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    let owner = SemanticWorkflowOwner::new(
        app_core,
        OperationId::invitation_create(),
        None,
        SemanticOperationKind::InviteActorToChannel,
    );
    invite_authority_to_channel_with_context(
        app_core, receiver, channel_id, None, None, &owner, None, None, None, message, ttl_ms,
    )
    .await
}

/// Invite a canonical authority to a canonical channel while carrying an
/// already-authoritative context, when the caller has one.
pub(in crate::workflows) async fn invite_authority_to_channel_with_context(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    context_id: Option<ContextId>,
    channel_name_hint: Option<String>,
    owner: &SemanticWorkflowOwner,
    _operation_instance_id: Option<OperationInstanceId>,
    deadline: Option<TimeoutBudget>,
    stage_tracker: Option<InviteStageTracker>,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    update_invite_stage(&stage_tracker, "require_runtime");
    let runtime = require_runtime(app_core).await?;
    let context_id = match context_id {
        Some(context_id) => context_id,
        None => {
            update_invite_stage(&stage_tracker, "require_authoritative_context");
            timeout_workflow_stage_with_deadline(
                &runtime,
                "invite_authority_to_channel",
                "require_authoritative_context",
                deadline,
                require_authoritative_context_id_for_channel(
                    app_core,
                    &runtime,
                    channel_id,
                    "channel invitation creation",
                ),
            )
            .await?
        }
    };
    update_invite_stage(&stage_tracker, "create_channel_invitation");
    let invitation = crate::workflows::invitation::create_channel_invitation_owned(
        app_core,
        receiver,
        channel_id.to_string(),
        Some(context_id),
        channel_name_hint,
        None,
        owner,
        deadline,
        stage_tracker.clone(),
        message,
        ttl_ms,
    )
    .await?;
    if let Some(spawner) = runtime.task_spawner() {
        let background_app_core = Arc::clone(app_core);
        let background_runtime = Arc::clone(&runtime);
        spawn_message_delivery_task(&spawner, async move {
            if let Err(_error) = project_channel_peer_membership_with_context(
                &background_app_core,
                channel_id,
                Some(context_id),
                receiver,
                None,
            )
            .await
            {
                #[cfg(feature = "instrumented")]
                tracing::warn!(
                    error = %_error,
                    channel_id = %channel_id,
                    receiver = %receiver,
                    "background channel invitation projection failed"
                );
                return;
            }
            warm_channel_connectivity(
                &background_app_core,
                &background_runtime,
                channel_id,
                context_id,
            )
            .await;
            converge_runtime(&background_runtime).await;
        });
    } else {
        update_invite_stage(&stage_tracker, "local_projection");
        timeout_workflow_stage_with_deadline(
            &runtime,
            "invite_authority_to_channel",
            "local_projection",
            deadline,
            async {
                project_channel_peer_membership_with_context(
                    app_core,
                    channel_id,
                    Some(context_id),
                    receiver,
                    None,
                )
                .await?;
                warm_channel_connectivity(app_core, &runtime, channel_id, context_id).await;
                converge_runtime(&runtime).await;
                Ok(())
            },
        )
        .await?;
    }

    Ok(invitation.invitation_id)
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime_bridge::InvitationBridgeStatus;
    use crate::signal_defs::{
        AUTHORITATIVE_SEMANTIC_FACTS_SIGNAL, CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME,
    };
    use crate::views::contacts::{Contact, ContactsState};
    use crate::views::home::{BanRecord, HomeRole, HomeState, HomesState, MuteRecord};
    use crate::workflows::signals::{emit_signal, read_signal_or_default};
    use crate::AppConfig;

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
    async fn test_refresh_authoritative_channel_membership_readiness_tracks_visible_channels() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"membership-ready"));
        let peer = AuthorityId::new_from_entropy([55u8; 32]);

        with_chat_state(&app_core, |chat| {
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
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::ChannelMembershipReady { channel, member_count }
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
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

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
    async fn test_join_channel_success_implies_membership_ready_postcondition() {
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
        let status = facts.iter().find_map(|fact| match fact {
            AuthoritativeSemanticFact::OperationStatus {
                operation_id,
                instance_id,
                status,
                ..
            } if *operation_id == OperationId::join_channel()
                && instance_id
                    .as_ref()
                    .is_some_and(|id| id.0 == "join-postcondition-1") =>
            {
                Some(status.clone())
            }
            _ => None,
        });
        let status = status.expect("join operation status must exist");
        assert_eq!(status.kind, SemanticOperationKind::JoinChannel);
        assert_eq!(status.phase, SemanticOperationPhase::Succeeded);
        assert!(facts.iter().any(|fact| matches!(
            fact,
            AuthoritativeSemanticFact::ChannelMembershipReady { channel, .. }
                if channel.name.as_deref() == Some("shared-parity-lab")
        )));
    }

    #[tokio::test]
    async fn authoritative_context_prefers_home_context_over_chat_projection() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let owner = AuthorityId::new_from_entropy([91u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"home-context-preferred"));
        let home_context = ContextId::new_from_entropy([92u8; 32]);
        let stale_chat_context = ContextId::new_from_entropy([93u8; 32]);

        {
            let mut core = app_core.write().await;
            core.set_authority(owner);
            let mut homes = core.views().get_homes();
            homes.add_home(HomeState::new(
                channel_id,
                Some("shared-parity-lab".to_string()),
                owner,
                0,
                home_context,
            ));
            core.views_mut().set_homes(homes);
        }

        with_chat_state(&app_core, |chat| {
            chat.upsert_channel(Channel {
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
            });
        })
        .await
        .unwrap();

        assert_eq!(
            authoritative_context_id_for_channel(&app_core, channel_id).await,
            Some(home_context)
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

        with_chat_state(&app_core, |chat| {
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
        assert!(facts.iter().any(|fact| {
            matches!(
                fact,
                AuthoritativeSemanticFact::RecipientPeersResolved { channel, member_count }
                    if channel.id.as_deref() == Some(channel_id_string.as_str())
                        && *member_count == 2
            )
        }));
    }

    #[test]
    fn test_delivery_readiness_facts_emit_peer_and_channel_facts() {
        let peer = AuthorityId::new_from_entropy([59u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"delivery-ready"));
        let context_id = ContextId::new_from_entropy([61u8; 32]);
        let state = ChannelReadinessState::new(
            Channel {
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
            },
            vec![peer],
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
    async fn test_channel_readiness_coordinator_tracks_shared_channel_state() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let local = AuthorityId::new_from_entropy([62u8; 32]);
        let peer = AuthorityId::new_from_entropy([63u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"channel-readiness-coordinator"));
        {
            let mut core = app_core.write().await;
            core.set_authority(local);
        }

        with_chat_state(&app_core, |chat| {
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

        let coordinator = ChannelReadinessCoordinator::load(&app_core).await;
        let state = coordinator
            .state_for_channel(channel_id)
            .unwrap_or_else(|| panic!("expected channel readiness state for {channel_id}"));

        assert_eq!(state.member_count, 2);
        assert_eq!(state.recipients, vec![peer]);
        assert!(state.delivery_supported);
    }

    #[test]
    fn test_authoritative_send_readiness_for_channel_requires_matching_facts() {
        let channel_id = ChannelId::from_bytes(hash(b"send-ready"));
        let other_channel_id = ChannelId::from_bytes(hash(b"other-send-ready"));
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

        let readiness = authoritative_send_readiness_for_channel(&facts, channel_id);
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
        with_chat_state(&app_core, |chat| {
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

        join_channel_by_name(&app_core, "#slash-lab")
            .await
            .expect("join should reuse existing channel");

        let state = get_chat_state(&app_core).await.unwrap();
        let count = state
            .all_channels()
            .filter(|channel| channel.name.eq_ignore_ascii_case("slash-lab"))
            .count();
        assert_eq!(count, 1, "join should not duplicate named channels");
        assert!(state.channel(&existing_id).is_some());
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

        let chat = chat_snapshot(&app_core).await;
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
        with_chat_state(&app_core, |chat| {
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

        let chat = chat_snapshot(&app_core).await;
        let channel = chat.channel(&channel_id).expect("channel should exist");
        assert_eq!(channel.context_id, Some(context_id));
        assert_eq!(channel.name, "slash-lab");
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

        with_chat_state(&app_core, |chat| {
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

        let chat = chat_snapshot(&app_core).await;
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
    async fn test_resolve_channel_id_from_state_matches_name_variants() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let channel_id = ChannelId::from_bytes(hash(b"resolve-name-variants"));
        with_chat_state(&app_core, |chat| {
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

        let by_name = resolve_chat_channel_id_from_state_or_input(&app_core, "slash-lab")
            .await
            .expect("name selector should resolve");
        let by_hash = resolve_chat_channel_id_from_state_or_input(&app_core, "#slash-lab")
            .await
            .expect("#name selector should resolve");
        let by_spaced_hash = resolve_chat_channel_id_from_state_or_input(&app_core, "# slash-lab")
            .await
            .expect("# spaced selector should resolve");

        assert_eq!(by_name, channel_id);
        assert_eq!(by_hash, channel_id);
        assert_eq!(by_spaced_hash, channel_id);
    }

    #[tokio::test]
    async fn test_resolve_chat_channel_id_ignores_home_projection_when_chat_missing() {
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

        let resolved = resolve_chat_channel_id_from_state_or_input(&app_core, "#slash-lab")
            .await
            .expect("channel selector should derive channel identity");
        assert_eq!(
            resolved,
            ChannelRef::Name("slash-lab".to_string()).to_channel_id()
        );
        assert_ne!(resolved, home_id);
    }

    #[tokio::test]
    async fn test_resolve_chat_channel_id_prefers_chat_match_over_home_name_collision() {
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

        with_chat_state(&app_core, |chat| {
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

        let resolved = resolve_chat_channel_id_from_state_or_input(&app_core, "shared-parity-lab")
            .await
            .expect("chat channel should win over home collision");
        assert_eq!(resolved, foreign_chat_id);
    }

    #[tokio::test]
    async fn test_resolve_channel_id_prefers_context_backed_chat_over_home_name_match() {
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

        with_chat_state(&app_core, |chat| {
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

        let resolved = resolve_chat_channel_id_from_state_or_input(&app_core, "shared-parity-lab")
            .await
            .expect("context-backed chat should win over home name collision");
        assert_eq!(resolved, chat_id);
    }

    #[tokio::test]
    async fn authoritative_local_channel_id_for_context_prefers_local_home_identity() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let local_home_id = ChannelId::from_bytes(hash(b"local-home-id"));
        let foreign_home_id = ChannelId::from_bytes(hash(b"foreign-home-id"));
        let owner = AuthorityId::new_from_entropy([19u8; 32]);
        let context_id = ContextId::new_from_entropy([20u8; 32]);

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

        with_chat_state(&app_core, |chat| {
            chat.upsert_channel(Channel {
                id: foreign_home_id,
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

        let resolved = authoritative_local_channel_id_for_context(
            &app_core,
            context_id,
            Some("shared-parity-lab"),
        )
        .await
        .expect("local home identity should resolve");

        assert_eq!(resolved, local_home_id);
    }

    #[tokio::test]
    async fn local_channel_id_for_accepted_pending_invitation_prefers_local_context_binding() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let local_home_id = ChannelId::from_bytes(hash(b"local-accepted-home-id"));
        let foreign_home_id = ChannelId::from_bytes(hash(b"foreign-accepted-home-id"));
        let owner = AuthorityId::new_from_entropy([21u8; 32]);
        let context_id = ContextId::new_from_entropy([22u8; 32]);

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

        let invitation = InvitationInfo {
            invitation_id: InvitationId::new("inv-local-context".to_string()),
            sender_id: AuthorityId::new_from_entropy([23u8; 32]),
            receiver_id: owner,
            invitation_type: InvitationBridgeType::Channel {
                home_id: foreign_home_id.to_string(),
                context_id: Some(context_id),
                nickname_suggestion: Some("shared-parity-lab".to_string()),
            },
            status: InvitationBridgeStatus::Accepted,
            created_at_ms: 0,
            expires_at_ms: None,
            message: None,
        };

        let resolved = local_channel_id_for_accepted_pending_invitation(
            &app_core,
            &invitation,
            foreign_home_id,
        )
        .await;

        assert_eq!(resolved, local_home_id);
    }

    #[tokio::test]
    async fn test_leave_then_join_name_reuses_canonical_channel_id() {
        let config = AppConfig::default();
        let core = AppCore::new(config).unwrap();
        let app_core = Arc::new(RwLock::new(core));
        AppCore::init_signals_with_hooks(&app_core).await.unwrap();

        let context_id = ContextId::new_from_entropy([19u8; 32]);
        let channel_id = ChannelId::from_bytes(hash(b"leave-join-canonical-reuse"));
        with_chat_state(&app_core, |chat| {
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

        let resolved = resolve_chat_channel_id_from_state_or_input(&app_core, "slash-lab")
            .await
            .expect("name selector should resolve");
        assert_eq!(resolved, channel_id);

        let chat = chat_snapshot(&app_core).await;
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
        with_chat_state(&app_core, |chat| {
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
        homes.add_home_with_auto_select(home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_moderation_for_sender(&app_core, context_id, home_id, sender, 1_000).await;
        assert!(result.is_ok(), "empty member list should not block sender");
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
        homes.add_home_with_auto_select(home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_moderation_for_sender(&app_core, context_id, home_id, sender, 2_000).await;
        assert!(result.is_err(), "muted sender should be blocked");
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("muted"),
            "expected muted error, got: {error}"
        );
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
        homes.add_home_with_auto_select(home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_join_allowed(&app_core, mismatched_context, home_id, banned).await;
        assert!(result.is_err(), "banned sender must be blocked");
        assert!(result.unwrap_err().to_string().contains("banned"));
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
        homes.add_home_with_auto_select(primary_home);
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
        assert!(result.is_err(), "muted sender should be blocked");
        assert!(result.unwrap_err().to_string().contains("muted"));
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
        homes.add_home_with_auto_select(primary_home);
        homes.add_home(moderation_home);
        {
            let mut core = app_core.write().await;
            core.views_mut().set_homes(homes);
        }

        let result =
            enforce_home_join_allowed(&app_core, context_id, channel_home_id, banned).await;
        assert!(result.is_err(), "banned sender must be blocked");
        assert!(result.unwrap_err().to_string().contains("banned"));
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
        };

        emit_signal(
            &app_core,
            &*CONTACTS_SIGNAL,
            ContactsState::from_contacts(vec![bob_contact]),
            CONTACTS_SIGNAL_NAME,
        )
        .await
        .unwrap();

        let by_name = resolve_target_authority_for_invite(&app_core, "bob")
            .await
            .expect("expected contact nickname to resolve");
        assert_eq!(by_name, bob_id);

        let by_id = resolve_target_authority_for_invite(&app_core, &bob_id.to_string())
            .await
            .expect("expected contact id string to resolve");
        assert_eq!(by_id, bob_id);
    }
}
