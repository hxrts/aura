//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::chat_commands::normalize_channel_name;
use crate::workflows::context::current_home_context_or_fallback;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::{
    converge_runtime, cooperative_yield, ensure_runtime_peer_connectivity, require_runtime,
};
use crate::workflows::signals::{emit_signal, read_signal};
use crate::workflows::snapshot_policy::{chat_snapshot, contacts_snapshot};
use crate::workflows::state_helpers::with_chat_state;
use crate::{
    core::IntentError,
    runtime_bridge::{InvitationBridgeType, InvitationInfo, RuntimeBridge},
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    thresholds::{default_channel_threshold, normalize_channel_threshold},
    views::{
        chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
        home::{HomeMember, HomeRole, HomeState},
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
    identifiers::{AuthorityId, ChannelId, ContextId, InvitationId},
    AuraError,
};
use aura_journal::fact::{FactOptions, RelationalFact};
use aura_journal::DomainFact;
use aura_protocol::amp::{serialize_amp_message, AmpMessage};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);
const CHAT_FACT_SEND_MAX_ATTEMPTS: usize = 4;
const CHAT_FACT_SEND_YIELDS_PER_RETRY: usize = 4;
const AMP_SEND_RETRY_ATTEMPTS: usize = 6;
const AMP_SEND_RETRY_BACKOFF_MS: u64 = 75;

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

async fn resolve_channel_id_from_state_or_input(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Result<ChannelId, AuraError> {
    routing::resolve_channel_id_from_state_or_input(app_core, channel_input).await
}

async fn matching_channel_ids(
    app_core: &Arc<RwLock<AppCore>>,
    channel_input: &str,
) -> Vec<ChannelId> {
    routing::matching_channel_ids(app_core, channel_input).await
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
    // Include a monotonic per-process counter to avoid same-millisecond collisions.
    let local_nonce = MESSAGE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
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
    let mut last_error: Option<String> = None;

    for attempt in 0..CHAT_FACT_SEND_MAX_ATTEMPTS {
        match runtime.send_chat_fact(peer, context, fact).await {
            Ok(()) => return Ok(()),
            Err(error) => last_error = Some(error.to_string()),
        }

        if attempt + 1 < CHAT_FACT_SEND_MAX_ATTEMPTS {
            converge_runtime(&runtime).await;
            for _ in 0..CHAT_FACT_SEND_YIELDS_PER_RETRY {
                cooperative_yield().await;
            }
        }
    }

    let message = last_error.unwrap_or_else(|| "unknown transport error".to_string());
    Err(AuraError::agent(format!(
        "Failed to deliver chat fact to {peer} after {CHAT_FACT_SEND_MAX_ATTEMPTS} attempts: {message}"
    )))
}

fn bootstrap_required_for_recipients(recipient_count: usize) -> bool {
    recipient_count > 0
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
) -> Option<(InvitationId, ChannelId)> {
    let candidates: Vec<(InvitationId, ChannelId)> = pending
        .iter()
        .filter(|invitation| invitation.sender_id != local_authority)
        .filter_map(|invitation| {
            channel_id_from_pending_channel_invitation(invitation)
                .map(|channel_id| (invitation.invitation_id.clone(), channel_id))
        })
        .collect();

    if let Some(exact) = candidates
        .iter()
        .find(|(_, channel_id)| *channel_id == requested_channel_id)
    {
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
    let pending = runtime.list_pending_invitations().await;
    let Some((invitation_id, invited_channel_id)) =
        select_pending_channel_invitation(&pending, runtime.authority_id(), requested_channel_id)
    else {
        return Ok(false);
    };

    if let Err(error) = runtime.accept_invitation(invitation_id.as_str()).await {
        let message = error.to_string();
        let lowered = message.to_lowercase();
        if !lowered.contains("already accepted") && !lowered.contains("not pending") {
            return Err(AuraError::agent(format!(
                "Failed to accept pending channel invitation: {message}"
            )));
        }
    }

    converge_runtime(&runtime).await;

    // Joining by invited channel id is best-effort; some runtimes auto-join on accept.
    let _ = join_channel(app_core, invited_channel_id).await;
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

async fn context_id_for_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
    local_authority: Option<AuthorityId>,
) -> Result<ContextId, AuraError> {
    routing::context_id_for_channel(app_core, channel_id, local_authority).await
}

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

        chat.upsert_channel(Channel {
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
        });
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
            return Err(AuraError::agent(format!(
                "join projection missing canonical channel {channel_id}"
            )));
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
    let Some(channel) = chat.channel(&channel_id) else {
        return Vec::new();
    };
    let channel_context = channel.context_id;

    let mut recipients = BTreeSet::new();
    for member in &channel.member_ids {
        if *member != self_authority {
            recipients.insert(*member);
        }
    }

    // Runtime ChatState reductions can briefly omit member_ids for freshly-created
    // channels. When that happens, fall back to home membership for this channel.
    if recipients.is_empty() {
        let homes = {
            let core = app_core.read().await;
            core.views().get_homes()
        };
        if let Some(home) = homes.home_state(&channel_id) {
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

    if recipients.is_empty() {
        let runtime = {
            let core = app_core.read().await;
            core.runtime().cloned()
        };
        if let Some(runtime) = runtime {
            let discovered: Vec<AuthorityId> = runtime
                .get_discovered_peers()
                .await
                .into_iter()
                .filter(|peer| *peer != self_authority)
                .collect();
            if channel.is_dm || discovered.len() == 1 {
                recipients.extend(discovered);
            }
        }
    }

    // Reactive channel reductions may temporarily omit explicit members.
    // Fall back to known contacts for two-party sessions so reply traffic keeps flowing.
    // Keep this conservative for non-DM channels: only apply if there is a single peer.
    if recipients.is_empty() {
        let contacts = contacts_snapshot(app_core).await;
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
    let _message_id = send_message(app_core, channel_id, content, timestamp_ms).await?;
    Ok(channel_id)
}

/// Create a group channel (home channel) in chat state.
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
        let context_id = current_home_context_or_fallback(app_core).await?;
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
            .map_err(|e| AuraError::agent(format!("Failed to create channel: {e}")))?;

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: runtime.authority_id(),
            })
            .await
            .map_err(|e| AuraError::agent(format!("Failed to join channel: {e}")))?;

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
            .map_err(|e| AuraError::agent(format!("Failed to persist channel: {e}")))?;

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
                    .map_err(|e| AuraError::agent(format!("Failed to bootstrap channel: {e}")))?,
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
                bootstrap.clone(),
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
            invitation_ids.push(invitation.invitation_id.as_str().to_string());
        }

        if !invitation_ids.is_empty() {
            runtime
                .start_channel_invitation_monitor(invitation_ids, context_id, channel_id)
                .await
                .map_err(|e| AuraError::agent(format!("{e}")))?;
        }
    }

    Ok(channel_id)
}

/// Join an existing channel using a typed ChannelId.
pub async fn join_channel(
    app_core: &Arc<RwLock<AppCore>>,
    channel_id: ChannelId,
) -> Result<(), AuraError> {
    let runtime = { require_runtime(app_core).await? };
    let context_id =
        context_id_for_channel(app_core, channel_id, Some(runtime.authority_id())).await?;
    enforce_home_join_allowed(app_core, context_id, channel_id, runtime.authority_id()).await?;

    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|error| {
            if intent_error_is_not_found(&error) {
                AuraError::not_found(format!("Failed to join channel: {error}"))
            } else {
                AuraError::agent(format!("Failed to join channel: {error}"))
            }
        })?;

    restore_home_member_membership(
        app_core,
        context_id,
        runtime.authority_id(),
        crate::workflows::time::current_time_ms(app_core).await?,
    )
    .await?;
    apply_authoritative_membership_projection(app_core, channel_id, context_id, true, None).await?;

    Ok(())
}

/// Join an existing channel by name (legacy/convenience API).
pub async fn join_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<(), AuraError> {
    let channel_name = channel_name.trim();
    if channel_name.is_empty() {
        return Err(AuraError::invalid("Channel name cannot be empty"));
    }

    let channel_id = resolve_channel_id_from_state_or_input(app_core, channel_name).await?;
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
        return Ok(());
    }

    match join_channel(app_core, channel_id).await {
        Ok(()) => {
            let context_id = context_id_for_channel(
                app_core,
                channel_id,
                Some(require_runtime(app_core).await?.authority_id()),
            )
            .await?;
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
            ensure_runtime_peer_connectivity(&runtime, "join_channel_by_name").await?;
            Ok(())
        }
        Err(join_error) => {
            // "/join" is "join or create". If the channel is unknown locally,
            // create it in the current context as a fallback.
            if channel_exists_locally || !join_error_is_not_found(&join_error) {
                return Err(join_error);
            }

            let runtime = require_runtime(app_core).await?;
            let fallback_context = current_home_context_or_fallback(app_core).await?;
            enforce_home_join_allowed(
                app_core,
                fallback_context,
                channel_id,
                runtime.authority_id(),
            )
            .await?;

            if try_join_via_pending_channel_invitation(app_core, channel_id).await? {
                return Ok(());
            }

            let timestamp_ms = crate::workflows::time::current_time_ms(app_core).await?;
            create_channel(
                app_core,
                channel_name,
                None,
                &known_members,
                0,
                timestamp_ms,
            )
                .await
                .map(|_| ())
                .map_err(|create_error| {
                    AuraError::agent(format!(
                        "Failed to join channel: {join_error}; failed to create missing channel: {create_error}"
                    ))
                })
        }
    }
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
        .map_err(|e| AuraError::agent(format!("Failed to leave channel: {e}")))?;

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
    let mut candidate_ids = matching_channel_ids(app_core, channel_name).await;
    if candidate_ids.is_empty() {
        candidate_ids.push(resolve_channel_id_from_state_or_input(app_core, channel_name).await?);
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
        .map_err(|e| AuraError::agent(format!("Failed to close channel: {e}")))?;

    let fact =
        ChatFact::channel_closed_ms(context_id, channel_id, timestamp_ms, runtime.authority_id())
            .to_generic();

    runtime
        .commit_relational_facts(&[fact])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to persist channel close: {e}")))?;

    Ok(())
}

/// Close/archive a channel by name (legacy/convenience API).
pub async fn close_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_channel_id_from_state_or_input(app_core, channel_name).await?;
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
        .map_err(|e| AuraError::agent(format!("Failed to set channel topic: {e}")))?;

    Ok(())
}

/// Set a channel topic by name (legacy/convenience API).
pub async fn set_topic_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
    text: &str,
    timestamp_ms: u64,
) -> Result<(), AuraError> {
    let channel_id = resolve_channel_id_from_state_or_input(app_core, channel_name).await?;
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
    let channel_id = resolve_channel_id_from_state_or_input(app_core, channel_name).await?;
    let channel_ref = ChannelRef::Id(channel_id);
    send_message_ref(app_core, channel_ref, content, timestamp_ms).await
}

/// Send a message to a channel by reference.
pub async fn send_message_ref(
    app_core: &Arc<RwLock<AppCore>>,
    channel: ChannelRef,
    content: &str,
    timestamp_ms: u64,
) -> Result<String, AuraError> {
    let channel_id = channel.to_channel_id();
    let channel_label = match &channel {
        ChannelRef::Id(id) => id.to_string(),
        ChannelRef::Name(name) => name.clone(),
    };

    let backend = messaging_backend(app_core).await;
    let mut channel_context: Option<ContextId> = None;
    let mut epoch_hint: Option<u32> = None;
    let (sender_id, message_id) = if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let sender_id = runtime.authority_id();
        let message_id = next_message_id(channel_id, sender_id, timestamp_ms, content);
        let context_id =
            context_id_for_channel(app_core, channel_id, Some(runtime.authority_id())).await?;
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
                let error_text = error.to_string();
                if error_text.contains("channel state not found") {
                    None
                } else {
                    return Err(AuraError::agent(format!(
                        "Failed to send message on context {context_id} channel {channel_id}: {error}"
                    )));
                }
            }
        };
        if maybe_cipher.is_none() {
            for attempt in 1..=AMP_SEND_RETRY_ATTEMPTS {
                runtime
                    .sleep_ms(AMP_SEND_RETRY_BACKOFF_MS * attempt as u64)
                    .await;
                match runtime.amp_send_message(send_params.clone()).await {
                    Ok(cipher) => {
                        maybe_cipher = Some(cipher);
                        break;
                    }
                    Err(error) => {
                        let error_text = error.to_string();
                        if error_text.contains("channel state not found") {
                            continue;
                        }
                        return Err(AuraError::agent(format!(
                            "Failed to send message on context {context_id} channel {channel_id}: {error}"
                        )));
                    }
                }
            }
        }

        let maybe_fact = if let Some(cipher) = maybe_cipher {
            let wire = AmpMessage::new(cipher.header.clone(), cipher.ciphertext.clone());
            let sealed = serialize_amp_message(&wire)
                .map_err(|e| AuraError::agent(format!("Failed to encode AMP message: {e}")))?;

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
            // Enable ack tracking for message facts to support delivery confirmation
            runtime
                .commit_relational_facts_with_options(
                    std::slice::from_ref(&fact),
                    FactOptions::default().with_ack_tracking(),
                )
                .await
                .map_err(|e| AuraError::agent(format!("Failed to persist message: {e}")))?;

            let recipients = recipient_peers_for_channel(app_core, channel_id, sender_id).await;
            let mut attempted_fanout = 0usize;
            let mut failed_fanout = Vec::new();
            let channel_requires_remote_delivery = chat_snapshot(app_core)
                .await
                .channel(&channel_id)
                .map(|channel| channel.is_dm || channel.member_count > 1)
                .unwrap_or(false);
            if recipients.is_empty() && channel_requires_remote_delivery {
                return Err(AuraError::agent(format!(
                    "Missing sync prerequisite for channel {channel_id}: no recipient peers resolved"
                )));
            }
            for peer in recipients {
                attempted_fanout = attempted_fanout.saturating_add(1);
                if let Err(error) =
                    send_chat_fact_with_retry(&runtime, peer, context_id, &fact).await
                {
                    failed_fanout.push(format!("{peer}: {error}"));
                }
            }
            if attempted_fanout == 0 {
                messaging_warn!(
                    "No recipient peers resolved for channel {channel_id}; treating send as locally persisted"
                );
            }
            if attempted_fanout > 0 && failed_fanout.len() == attempted_fanout {
                messaging_warn!(
                    "Message fanout unavailable for all recipients on {channel_id}: {}",
                    failed_fanout.join("; ")
                );
            }
            converge_runtime(&runtime).await;
            if let Err(_error) =
                ensure_runtime_peer_connectivity(&runtime, "send_message_ref").await
            {
                #[cfg(feature = "instrumented")]
                tracing::warn!(
                    error = %_error,
                    channel_id = %channel_id,
                    "message send completed without reachable peers"
                );
            }
        }

        (sender_id, message_id)
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
            let lowered = error.to_string().to_lowercase();
            if !lowered.contains("already") && !lowered.contains("exists") {
                return Err(AuraError::agent(format!(
                    "Failed to create direct channel: {error}"
                )));
            }
        }

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: runtime.authority_id(),
            })
            .await
            .map_err(|error| AuraError::agent(format!("Failed to join direct channel: {error}")))?;

        runtime
            .amp_join_channel(ChannelJoinParams {
                context: context_id,
                channel: channel_id,
                participant: contact_authority,
            })
            .await
            .map_err(|error| {
                AuraError::agent(format!("Failed to add contact to direct channel: {error}"))
            })?;

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
            .map_err(|error| {
                AuraError::agent(format!("Failed to persist direct channel: {error}"))
            })?;

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
    // Resolve via contacts first so command targets can use IDs or contact names.
    let receiver = resolve_target_authority_for_invite(app_core, target_user_id).await?;
    let channel_id = resolve_channel_id_from_state_or_input(app_core, channel_id).await?;

    invite_authority_to_channel(app_core, receiver, channel_id, message, ttl_ms).await
}

/// Invite a canonical authority to a canonical channel.
pub async fn invite_authority_to_channel(
    app_core: &Arc<RwLock<AppCore>>,
    receiver: AuthorityId,
    channel_id: ChannelId,
    message: Option<String>,
    ttl_ms: Option<u64>,
) -> Result<InvitationId, AuraError> {
    let runtime = require_runtime(app_core).await?;
    let context_id =
        context_id_for_channel(app_core, channel_id, Some(runtime.authority_id())).await?;

    // Channel invitations must carry bootstrap key material so recipients can
    // decrypt channel traffic immediately after acceptance.
    let bootstrap = runtime
        .amp_create_channel_bootstrap(context_id, channel_id, vec![receiver])
        .await
        .map_err(|e| AuraError::agent(format!("Failed to bootstrap channel invitation: {e}")))?;

    // Delegate to invitation workflow.
    let invitation = crate::workflows::invitation::create_channel_invitation(
        app_core,
        receiver,
        channel_id.to_string(),
        Some(context_id),
        Some(bootstrap),
        message,
        ttl_ms,
    )
    .await?;

    Ok(invitation.invitation_id)
}

#[cfg(test)]
#[allow(clippy::default_trait_access, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::runtime_bridge::InvitationBridgeStatus;
    use crate::signal_defs::{CONTACTS_SIGNAL, CONTACTS_SIGNAL_NAME};
    use crate::views::contacts::{Contact, ContactsState};
    use crate::views::home::{BanRecord, HomeRole, HomeState, HomesState, MuteRecord};
    use crate::workflows::signals::emit_signal;
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
        assert_eq!(selected.1, requested);
        assert_eq!(selected.0.as_str(), "inv-target");
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
        assert_eq!(selected.1, invited);
        assert_eq!(selected.0.as_str(), "inv-single");
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

        let by_name = resolve_channel_id_from_state_or_input(&app_core, "slash-lab")
            .await
            .expect("name selector should resolve");
        let by_hash = resolve_channel_id_from_state_or_input(&app_core, "#slash-lab")
            .await
            .expect("#name selector should resolve");
        let by_spaced_hash = resolve_channel_id_from_state_or_input(&app_core, "# slash-lab")
            .await
            .expect("# spaced selector should resolve");

        assert_eq!(by_name, channel_id);
        assert_eq!(by_hash, channel_id);
        assert_eq!(by_spaced_hash, channel_id);
    }

    #[tokio::test]
    async fn test_resolve_channel_id_uses_home_projection_when_chat_missing() {
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

        let resolved = resolve_channel_id_from_state_or_input(&app_core, "#slash-lab")
            .await
            .expect("home projection selector should resolve");
        assert_eq!(resolved, home_id);
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

        let resolved = resolve_channel_id_from_state_or_input(&app_core, "slash-lab")
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
