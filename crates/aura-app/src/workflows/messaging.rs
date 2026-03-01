//! Messaging Workflow - Portable Business Logic
//!
//! This module contains messaging operations that are portable across all frontends.
//! Uses typed reactive signals for state reads/writes.

use crate::workflows::channel_ref::ChannelRef;
use crate::workflows::context::current_home_context_or_fallback;
use crate::workflows::parse::parse_authority_id;
use crate::workflows::runtime::require_runtime;
use crate::workflows::signals::read_signal;
use crate::workflows::snapshot_policy::{chat_snapshot, contacts_snapshot};
use crate::workflows::state_helpers::with_chat_state;
use crate::{
    signal_defs::{HOMES_SIGNAL, HOMES_SIGNAL_NAME},
    thresholds::{default_channel_threshold, normalize_channel_threshold},
    views::chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
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
use aura_journal::fact::FactOptions;
use aura_journal::DomainFact;
use aura_protocol::amp::{serialize_amp_message, AmpMessage};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static MESSAGE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

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

/// Parse a channel string into a ChannelRef.
fn parse_channel_ref(channel: &str) -> ChannelRef {
    ChannelRef::parse(channel)
}

fn channel_id_from_input(channel: &str) -> ChannelId {
    parse_channel_ref(channel).to_channel_id()
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
    let digest = hash(
        format!("{channel_id}:{sender_id}:{timestamp_ms}:{local_nonce}:{content}").as_bytes(),
    );
    let suffix = hex_prefix(&digest, 8);
    format!("msg-{channel_id}-{timestamp_ms}-{suffix}")
}

fn is_invitation_capability_missing(error: &AuraError) -> bool {
    error.to_string().contains("invitation:capability-missing")
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
    Ok(channel_id_from_input("home"))
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
    {
        let chat = chat_snapshot(app_core).await;
        if let Some(channel) = chat.channel(&channel_id) {
            if let Some(ctx_id) = channel.context_id {
                return Ok(ctx_id);
            }
            if channel.is_dm {
                if let Some(self_authority) = local_authority {
                    if let Some(peer_authority) = channel
                        .member_ids
                        .iter()
                        .copied()
                        .find(|member| *member != self_authority)
                        .or_else(|| channel.member_ids.first().copied())
                    {
                        return Ok(pair_dm_context_id(self_authority, peer_authority));
                    }
                }
            }
        }
    }
    {
        let core = app_core.read().await;
        let homes = core.views().get_homes();
        if let Some(home_state) = homes.home_state(&channel_id) {
            if let Some(ctx_id) = home_state.context_id {
                return Ok(ctx_id);
            }
        }
    }

    // Not all channels correspond to a "home" entry in the homes view yet
    // (e.g. AMP-created channels in demos/tests). Fall back to the currently
    // selected home context (or per-authority demo context).
    current_home_context_or_fallback(app_core).await
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

    let mut recipients = BTreeSet::new();
    for member in &channel.member_ids {
        if *member != self_authority {
            recipients.insert(*member);
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
    let target_id = contact.id;

    if let Ok(runtime) = require_runtime(app_core).await {
        if target_id == runtime.authority_id() {
            return Err(AuraError::invalid("Cannot send direct message to yourself"));
        }
    }

    let channel_id = start_direct_chat(app_core, &target_id.to_string(), timestamp_ms)
        .await?
        .parse::<ChannelId>()
        .map_err(|e| AuraError::agent(format!("Invalid direct channel ID: {e}")))?;

    let _message_id = send_message(app_core, channel_id, content, timestamp_ms).await?;
    Ok(channel_id.to_string())
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

    if backend == MessagingBackend::Runtime {
        let runtime = require_runtime(app_core).await?;
        let context_id = current_home_context_or_fallback(app_core).await?;
        channel_context = Some(context_id);
        let channel_hint = (!name.trim().is_empty()).then(|| channel_id_from_input(name));
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
            return Err(AuraError::agent(format!(
                "Failed to deliver channel fact to members: {}",
                failed_fanout.join("; ")
            )));
        }
    } else if !name.trim().is_empty() {
        channel_id = channel_id_from_input(name);
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

    // Create channel invitations for selected members (if any).
    if backend == MessagingBackend::Runtime && !member_ids.is_empty() {
        let runtime = require_runtime(app_core).await?;
        let context_id = current_home_context_or_fallback(app_core).await?;

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

        // Two-party groups can operate without explicit bootstrap handoff.
        // This keeps delivery/decryption functional when invitation capability
        // grants are unavailable for bootstrap exchange.
        let bootstrap = if member_ids.len() > 1 {
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
                    runtime
                        .amp_join_channel(ChannelJoinParams {
                            context: context_id,
                            channel: channel_id,
                            participant: *receiver,
                        })
                        .await
                        .map_err(|join_error| {
                            AuraError::agent(format!(
                                "Failed to add member after invitation capability fallback: {join_error}"
                            ))
                        })?;
                    continue;
                }
                Err(error) => return Err(error),
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
    let context_id = current_home_context_or_fallback(app_core).await?;

    runtime
        .amp_join_channel(ChannelJoinParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to join channel: {e}")))?;

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

    let channel_id = channel_id_from_input(channel_name);
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
        Ok(()) => Ok(()),
        Err(join_error) => {
            // "/join" is "join or create". If the channel is unknown locally,
            // create it in the current context as a fallback.
            if channel_exists_locally {
                return Err(join_error);
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
    let runtime = { require_runtime(app_core).await? };
    let context_id = current_home_context_or_fallback(app_core).await?;

    runtime
        .amp_leave_channel(ChannelLeaveParams {
            context: context_id,
            channel: channel_id,
            participant: runtime.authority_id(),
        })
        .await
        .map_err(|e| AuraError::agent(format!("Failed to leave channel: {e}")))?;

    Ok(())
}

/// Leave a channel by name (legacy/convenience API).
pub async fn leave_channel_by_name(
    app_core: &Arc<RwLock<AppCore>>,
    channel_name: &str,
) -> Result<(), AuraError> {
    let channel_id = channel_id_from_input(channel_name);
    leave_channel(app_core, channel_id).await
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
    let channel_id = channel_id_from_input(channel_name);
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
    let channel_id = channel_id_from_input(channel_name);
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
    let channel_ref = parse_channel_ref(channel_name);
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
        channel_context = Some(context_id);

        let cipher = runtime
            .amp_send_message(ChannelSendParams {
                context: context_id,
                channel: channel_id,
                sender: sender_id,
                plaintext: content.as_bytes().to_vec(),
                reply_to: None,
            })
            .await
            .map_err(|e| {
                AuraError::agent(format!(
                    "Failed to send message on context {context_id} channel {channel_id}: {e}"
                ))
            })?;

        let wire = AmpMessage::new(cipher.header.clone(), cipher.ciphertext.clone());
        let sealed = serialize_amp_message(&wire)
            .map_err(|e| AuraError::agent(format!("Failed to encode AMP message: {e}")))?;

        // Extract epoch from the AMP header (used for consensus finalization tracking)
        epoch_hint = Some(cipher.header.chan_epoch as u32);

        let fact = ChatFact::message_sent_sealed_ms(
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
        .to_generic();

        // Enable ack tracking for message facts to support delivery confirmation
        runtime
            .commit_relational_facts_with_options(
                std::slice::from_ref(&fact),
                FactOptions::default().with_ack_tracking(),
            )
            .await
            .map_err(|e| AuraError::agent(format!("Failed to persist message: {e}")))?;

        let recipients =
            recipient_peers_for_channel(app_core, channel_id, sender_id).await;
        let mut attempted_fanout = 0usize;
        let mut failed_fanout = Vec::new();
        for peer in recipients {
            attempted_fanout = attempted_fanout.saturating_add(1);
            if let Err(error) = runtime.send_chat_fact(peer, context_id, &fact).await {
                failed_fanout.push(format!("{peer}: {error}"));
            }
        }
        if attempted_fanout == 0 {
            return Err(AuraError::agent(format!(
                "No recipient peers resolved for channel {channel_id}"
            )));
        }
        if attempted_fanout > 0 && failed_fanout.len() == attempted_fanout {
            return Err(AuraError::agent(format!(
                "Failed to deliver message fact to recipients: {}",
                failed_fanout.join("; ")
            )));
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
    let backend = messaging_backend(app_core).await;
    let contacts = contacts_snapshot(app_core).await;

    // Get contact name from ViewState for the channel name
    let contact_name = contacts
        .contact(
            &parse_authority_id(contact_id)
                .unwrap_or_else(|_| AuthorityId::new_from_entropy([1u8; 32])),
        )
        .map(|c| c.nickname.clone())
        .unwrap_or_else(|| format!("DM with {}", &contact_id[..8.min(contact_id.len())]));

    if backend == MessagingBackend::Runtime {
        // Runtime mode provisions a deterministic two-party channel without requiring
        // invitation capabilities in the active runtime profile.
        let contact_authority = parse_authority_id(contact_id)?;
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

        runtime
            .send_chat_fact(contact_authority, context_id, &fact)
            .await
            .map_err(|error| {
                AuraError::agent(format!(
                    "Failed to deliver direct channel fact to {contact_authority}: {error}"
                ))
            })?;

        with_chat_state(app_core, |chat_state| {
            chat_state.upsert_channel(Channel {
                id: channel_id,
                context_id: Some(context_id),
                name: contact_name.clone(),
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
        return Ok(channel_id.to_string());
    }

    let channel_id = dm_channel_id(contact_id);
    let authority_id =
        parse_authority_id(contact_id).unwrap_or_else(|_| AuthorityId::new_from_entropy([1u8; 32]));
    let now = timestamp_ms;

    // Create the DM channel
    let dm_channel = Channel {
        id: channel_id,
        context_id: None,
        name: contact_name,
        topic: Some(format!("Direct messages with {contact_id}")),
        channel_type: ChannelType::DirectMessage,
        unread_count: 0,
        is_dm: true,
        member_ids: vec![authority_id],
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

    Ok(channel_id.to_string())
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
    // Parse target user ID as AuthorityId
    let receiver = parse_authority_id(target_user_id)?;

    // Delegate to invitation workflow
    let invitation = crate::workflows::invitation::create_channel_invitation(
        app_core,
        receiver,
        channel_id.to_string(),
        None,
        message,
        ttl_ms,
    )
    .await?;

    Ok(invitation.invitation_id)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
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
}
