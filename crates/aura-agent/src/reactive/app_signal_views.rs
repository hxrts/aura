//! ReactiveScheduler views that emit Aura application signals.
//!
//! These views are the bridge between:
//! - The canonical typed-fact pipeline (`aura_journal::fact::Fact`)
//! - The UI-facing reactive signals in `aura-app` (`*_SIGNAL`)
//!
//! The scheduler calls `update(facts)` with each processed batch. Each view:
//! - Applies the relevant domain facts to its aggregate state
//! - Emits a full snapshot into the corresponding signal (eventual consistency)

use aura_app::errors::AppError;
use aura_app::signal_defs::{
    BLOCKS_SIGNAL, BLOCK_SIGNAL, CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL,
};
use aura_app::views::{
    block::{BanRecord, BlockState, BlocksState, KickRecord, MuteRecord},
    chat::{Channel, ChannelType, ChatState, Message},
    contacts::{Contact, ContactsState},
    invitations::{
        Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
    },
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::AuthorityId;
use aura_effects::ReactiveHandler;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::DomainFact;
use tokio::sync::Mutex;

use super::scheduler::ReactiveView;

use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
use aura_protocol::moderation::facts::{
    BlockPinFact, BlockUnpinFact, BLOCK_PIN_FACT_TYPE_ID, BLOCK_UNPIN_FACT_TYPE_ID,
};
use aura_protocol::moderation::{
    BlockBanFact, BlockKickFact, BlockMuteFact, BlockUnbanFact, BlockUnmuteFact,
    BLOCK_BAN_FACT_TYPE_ID, BLOCK_KICK_FACT_TYPE_ID, BLOCK_MUTE_FACT_TYPE_ID,
    BLOCK_UNBAN_FACT_TYPE_ID, BLOCK_UNMUTE_FACT_TYPE_ID,
};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};

async fn emit_internal_error(reactive: &ReactiveHandler, message: String) {
    let _ = reactive
        .emit(
            &*ERROR_SIGNAL,
            Some(AppError::internal("reactive_scheduler", message)),
        )
        .await;
}

// =============================================================================
// Invitations
// =============================================================================

pub struct InvitationsSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    state: Mutex<InvitationsState>,
}

impl InvitationsSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            state: Mutex::new(InvitationsState::default()),
        }
    }

    fn map_invitation_type(inv_type: &str) -> InvitationType {
        match inv_type.to_lowercase().as_str() {
            // Legacy mapping: the TUI maps aura-app Block → "Contact".
            "contact" => InvitationType::Block,
            "guardian" => InvitationType::Guardian,
            "channel" | "chat" => InvitationType::Chat,
            _ => InvitationType::Block,
        }
    }
}

impl ReactiveView for InvitationsSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            let FactContent::Relational(RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            }) = &fact.content
            else {
                continue;
            };

            if binding_type != INVITATION_FACT_TYPE_ID {
                continue;
            }

            let Some(inv) = InvitationFact::from_bytes(binding_data) else {
                emit_internal_error(
                    &self.reactive,
                    format!(
                        "Failed to decode InvitationFact bytes (len={})",
                        binding_data.len()
                    ),
                )
                .await;
                continue;
            };

            match inv {
                InvitationFact::Sent {
                    invitation_id,
                    sender_id,
                    receiver_id,
                    invitation_type,
                    sent_at,
                    expires_at,
                    message,
                    ..
                } => {
                    let direction = if sender_id == self.own_authority {
                        InvitationDirection::Sent
                    } else {
                        InvitationDirection::Received
                    };

                    let invitation = Invitation {
                        id: invitation_id,
                        invitation_type: Self::map_invitation_type(&invitation_type),
                        status: InvitationStatus::Pending,
                        direction,
                        from_id: sender_id,
                        from_name: "Unknown".to_string(),
                        to_id: (direction == InvitationDirection::Sent).then_some(receiver_id),
                        to_name: (direction == InvitationDirection::Sent)
                            .then_some("Unknown".to_string()),
                        created_at: sent_at.ts_ms,
                        expires_at: expires_at.map(|t| t.ts_ms),
                        message,
                        block_id: None,
                        block_name: None,
                    };

                    state.add_invitation(invitation);
                    changed = true;
                }
                InvitationFact::Accepted { invitation_id, .. } => {
                    state.accept_invitation(&invitation_id);
                    changed = true;
                }
                InvitationFact::Declined { invitation_id, .. } => {
                    state.reject_invitation(&invitation_id);
                    changed = true;
                }
                InvitationFact::Cancelled { invitation_id, .. } => {
                    state.revoke_invitation(&invitation_id);
                    changed = true;
                }
                InvitationFact::CeremonyInitiated {
                    ceremony_id,
                    sender,
                    timestamp_ms,
                } => {
                    // Invitation ceremony events don't map to InvitationsState.
                    // They track the consensus-based invitation exchange protocol.
                    // For RecoveryState updates, use RecoveryFacts or the ceremony tracker.
                    tracing::debug!(
                        ceremony_id,
                        sender,
                        timestamp_ms,
                        "Invitation ceremony initiated"
                    );
                }
                InvitationFact::CeremonyAcceptanceReceived {
                    ceremony_id,
                    timestamp_ms,
                } => {
                    tracing::debug!(
                        ceremony_id,
                        timestamp_ms,
                        "Invitation ceremony acceptance received"
                    );
                }
                InvitationFact::CeremonyCommitted {
                    ceremony_id,
                    relationship_id,
                    timestamp_ms,
                } => {
                    tracing::info!(
                        ceremony_id,
                        relationship_id,
                        timestamp_ms,
                        "Invitation ceremony committed - relationship established"
                    );
                }
                InvitationFact::CeremonyAborted {
                    ceremony_id,
                    reason,
                    timestamp_ms,
                } => {
                    tracing::warn!(
                        ceremony_id,
                        reason,
                        timestamp_ms,
                        "Invitation ceremony aborted"
                    );
                }
            }
        }

        if !changed {
            return;
        }

        let snapshot = state.clone();
        drop(state);

        if let Err(e) = self.reactive.emit(&*INVITATIONS_SIGNAL, snapshot).await {
            emit_internal_error(
                &self.reactive,
                format!("Failed to emit INVITATIONS_SIGNAL: {e}"),
            )
            .await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:invitations"
    }
}

// =============================================================================
// Contacts
// =============================================================================

pub struct ContactsSignalView {
    reactive: ReactiveHandler,
    state: Mutex<ContactsState>,
}

impl ContactsSignalView {
    pub fn new(reactive: ReactiveHandler) -> Self {
        Self {
            reactive,
            state: Mutex::new(ContactsState::default()),
        }
    }
}

impl ReactiveView for ContactsSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            match &fact.content {
                FactContent::Relational(RelationalFact::Generic {
                    binding_type,
                    binding_data,
                    ..
                }) if binding_type == CONTACT_FACT_TYPE_ID => {
                    let Some(contact_fact) = ContactFact::from_bytes(binding_data) else {
                        emit_internal_error(
                            &self.reactive,
                            format!(
                                "Failed to decode ContactFact bytes (len={})",
                                binding_data.len()
                            ),
                        )
                        .await;
                        continue;
                    };

                    match contact_fact {
                        ContactFact::Added {
                            contact_id,
                            nickname,
                            added_at,
                            ..
                        } => {
                            let suggested_name = if nickname.trim().is_empty()
                                || nickname == contact_id.to_string()
                            {
                                None
                            } else {
                                Some(nickname)
                            };

                            if let Some(contact) =
                                state.contacts.iter_mut().find(|c| c.id == contact_id)
                            {
                                // Preserve any user-set nickname; only fill suggestion if missing.
                                if contact.suggested_name.is_none() {
                                    contact.suggested_name = suggested_name;
                                }
                                contact.last_interaction = Some(added_at.ts_ms);
                            } else {
                                // Contact invitations carry an optional nickname, which we treat as
                                // a suggested name. The user's nickname is a separate local label.
                                state.contacts.push(Contact {
                                    id: contact_id,
                                    nickname: String::new(),
                                    suggested_name,
                                    is_guardian: false,
                                    is_resident: false,
                                    last_interaction: Some(added_at.ts_ms),
                                    is_online: false,
                                });
                            }
                            changed = true;
                        }
                        ContactFact::Removed { contact_id, .. } => {
                            state.contacts.retain(|c| c.id != contact_id);
                            changed = true;
                        }
                        ContactFact::Renamed {
                            contact_id,
                            new_nickname,
                            renamed_at,
                            ..
                        } => {
                            state.set_nickname(contact_id, new_nickname);
                            if let Some(contact) =
                                state.contacts.iter_mut().find(|c| c.id == contact_id)
                            {
                                contact.last_interaction = Some(renamed_at.ts_ms);
                            }
                            changed = true;
                        }
                    }
                }
                FactContent::Relational(RelationalFact::GuardianBinding {
                    guardian_id, ..
                }) => {
                    // Reflect guardian status into contacts for details screens.
                    state.set_guardian_status(*guardian_id, true);
                    changed = true;
                }
                _ => {}
            }
        }

        if !changed {
            return;
        }

        let snapshot = state.clone();
        drop(state);

        if let Err(e) = self.reactive.emit(&*CONTACTS_SIGNAL, snapshot).await {
            emit_internal_error(
                &self.reactive,
                format!("Failed to emit CONTACTS_SIGNAL: {e}"),
            )
            .await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:contacts"
    }
}

// =============================================================================
// Blocks (Moderation + Pins)
// =============================================================================

pub struct BlockSignalView {
    reactive: ReactiveHandler,
}

impl BlockSignalView {
    pub fn new(reactive: ReactiveHandler) -> Self {
        Self { reactive }
    }

    fn block_for_context<'a>(
        blocks: &'a mut BlocksState,
        context_id: &str,
    ) -> Option<&'a mut BlockState> {
        blocks
            .blocks
            .values_mut()
            .find(|block| block.context_id == context_id)
    }
}

impl ReactiveView for BlockSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut blocks = match self.reactive.read(&*BLOCKS_SIGNAL).await {
            Ok(state) => state,
            Err(e) => {
                emit_internal_error(&self.reactive, format!("Failed to read BLOCKS_SIGNAL: {e}"))
                    .await;
                return;
            }
        };

        let mut changed = false;

        for fact in facts {
            let FactContent::Relational(RelationalFact::Generic {
                context_id,
                binding_type,
                binding_data,
            }) = &fact.content
            else {
                continue;
            };

            let context_key = context_id.to_string();
            let Some(block) = Self::block_for_context(&mut blocks, &context_key) else {
                continue;
            };

            match binding_type.as_str() {
                BLOCK_BAN_FACT_TYPE_ID => {
                    if let Some(ban) = BlockBanFact::from_bytes(binding_data) {
                        let record = BanRecord {
                            authority_id: ban.banned_authority,
                            reason: ban.reason,
                            actor: ban.actor_authority,
                            banned_at: ban.banned_at.ts_ms,
                        };
                        block.add_ban(record);
                        let _ = block.remove_resident(&ban.banned_authority);
                        changed = true;
                    }
                }
                BLOCK_UNBAN_FACT_TYPE_ID => {
                    if let Some(unban) = BlockUnbanFact::from_bytes(binding_data) {
                        if block.remove_ban(&unban.unbanned_authority).is_some() {
                            changed = true;
                        }
                    }
                }
                BLOCK_MUTE_FACT_TYPE_ID => {
                    if let Some(mute) = BlockMuteFact::from_bytes(binding_data) {
                        let record = MuteRecord {
                            authority_id: mute.muted_authority,
                            duration_secs: mute.duration_secs,
                            muted_at: mute.muted_at.ts_ms,
                            expires_at: mute.expires_at.as_ref().map(|t| t.ts_ms),
                            actor: mute.actor_authority,
                        };
                        block.add_mute(record);
                        changed = true;
                    }
                }
                BLOCK_UNMUTE_FACT_TYPE_ID => {
                    if let Some(unmute) = BlockUnmuteFact::from_bytes(binding_data) {
                        if block.remove_mute(&unmute.unmuted_authority).is_some() {
                            changed = true;
                        }
                    }
                }
                BLOCK_KICK_FACT_TYPE_ID => {
                    if let Some(kick) = BlockKickFact::from_bytes(binding_data) {
                        let record = KickRecord {
                            authority_id: kick.kicked_authority,
                            channel: kick.channel_id,
                            reason: kick.reason,
                            actor: kick.actor_authority,
                            kicked_at: kick.kicked_at.ts_ms,
                        };
                        block.add_kick(record);
                        let _ = block.remove_resident(&kick.kicked_authority);
                        changed = true;
                    }
                }
                BLOCK_PIN_FACT_TYPE_ID => {
                    if let Some(pin) = BlockPinFact::from_bytes(binding_data) {
                        block.pin_message(pin.message_id);
                        changed = true;
                    }
                }
                BLOCK_UNPIN_FACT_TYPE_ID => {
                    if let Some(unpin) = BlockUnpinFact::from_bytes(binding_data) {
                        if block.unpin_message(&unpin.message_id) {
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }

        if !changed {
            return;
        }

        let snapshot = blocks.clone();
        drop(blocks);

        if let Err(e) = self.reactive.emit(&*BLOCKS_SIGNAL, snapshot.clone()).await {
            emit_internal_error(&self.reactive, format!("Failed to emit BLOCKS_SIGNAL: {e}")).await;
        }

        let block_snapshot = snapshot.current_block().cloned().unwrap_or_default();
        if let Err(e) = self.reactive.emit(&*BLOCK_SIGNAL, block_snapshot).await {
            emit_internal_error(&self.reactive, format!("Failed to emit BLOCK_SIGNAL: {e}")).await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:blocks"
    }
}

// =============================================================================
// Chat
// =============================================================================

pub struct ChatSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    state: Mutex<ChatState>,
}

impl ChatSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            state: Mutex::new(ChatState::default()),
        }
    }
}

impl ReactiveView for ChatSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            let FactContent::Relational(RelationalFact::Generic {
                binding_type,
                binding_data,
                ..
            }) = &fact.content
            else {
                continue;
            };

            if binding_type != CHAT_FACT_TYPE_ID {
                continue;
            }

            let Some(chat_fact) = ChatFact::from_bytes(binding_data) else {
                emit_internal_error(
                    &self.reactive,
                    format!(
                        "Failed to decode ChatFact bytes (len={})",
                        binding_data.len()
                    ),
                )
                .await;
                continue;
            };

            match chat_fact {
                ChatFact::ChannelCreated {
                    channel_id,
                    name,
                    topic,
                    is_dm,
                    created_at,
                    ..
                } => {
                    let channel = Channel {
                        id: channel_id,
                        name,
                        topic,
                        channel_type: if is_dm {
                            ChannelType::DirectMessage
                        } else {
                            ChannelType::Block
                        },
                        unread_count: 0,
                        is_dm,
                        member_count: 0,
                        last_message: None,
                        last_message_time: None,
                        last_activity: created_at.ts_ms,
                    };
                    state.add_channel(channel);
                    changed = true;
                }
                ChatFact::ChannelClosed { channel_id, .. } => {
                    state.remove_channel(&channel_id);
                    changed = true;
                }
                ChatFact::ChannelUpdated {
                    channel_id,
                    name,
                    topic,
                    updated_at,
                    ..
                } => {
                    if let Some(channel) = state.channel_mut(&channel_id) {
                        if let Some(name) = name {
                            channel.name = name;
                        }
                        if topic.is_some() {
                            channel.topic = topic;
                        }
                        channel.last_activity = updated_at.ts_ms;
                    }
                    changed = true;
                }
                ChatFact::MessageSentSealed {
                    channel_id,
                    message_id,
                    sender_id,
                    sender_name,
                    payload,
                    sent_at,
                    reply_to,
                    ..
                } => {
                    // Sealed messages are opaque at this layer; decoding/decryption belongs above.
                    let content = format!("[sealed: {} bytes]", payload.len());
                    let message = Message {
                        id: message_id,
                        channel_id,
                        sender_id,
                        sender_name,
                        content,
                        timestamp: sent_at.ts_ms,
                        reply_to,
                        is_own: sender_id == self.own_authority,
                        is_read: sender_id == self.own_authority,
                    };
                    state.apply_message(channel_id, message);
                    changed = true;
                }
                ChatFact::MessageRead {
                    channel_id,
                    message_id,
                    reader_id,
                    read_at,
                    ..
                } => {
                    // If the reader is us, this is a confirmation of our own read action.
                    // If the reader is someone else, update the message as read by others.
                    // For now, mark the message as read in our local state.
                    if state.mark_message_read(&message_id) {
                        state.decrement_unread(&channel_id);
                        changed = true;
                    }
                    tracing::debug!(
                        channel_id = %channel_id,
                        message_id,
                        reader_id = %reader_id,
                        read_at = read_at.ts_ms,
                        "Message marked as read"
                    );
                }
                ChatFact::MessageDelivered {
                    channel_id,
                    message_id,
                    recipient_id,
                    device_id,
                    delivered_at,
                    ..
                } => {
                    // Message was delivered to a recipient's device (before they read it).
                    // Currently the Message struct doesn't have a delivery status field,
                    // so we just log this event. A future UI could show delivery checkmarks.
                    tracing::debug!(
                        channel_id = %channel_id,
                        message_id,
                        recipient_id = %recipient_id,
                        device_id = ?device_id,
                        delivered_at = delivered_at.ts_ms,
                        "Message delivered to recipient device"
                    );
                }
                ChatFact::DeliveryAcknowledged {
                    channel_id,
                    message_id,
                    acknowledged_at,
                    ..
                } => {
                    // Sender acknowledged the delivery receipt - closes the receipt loop.
                    // Used for internal state management and GC of pending receipts.
                    tracing::trace!(
                        channel_id = %channel_id,
                        message_id,
                        acknowledged_at = acknowledged_at.ts_ms,
                        "Delivery receipt acknowledged"
                    );
                }
            }
        }

        if !changed {
            return;
        }

        let snapshot = state.clone();
        drop(state);

        if let Err(e) = self.reactive.emit(&*CHAT_SIGNAL, snapshot).await {
            emit_internal_error(&self.reactive, format!("Failed to emit CHAT_SIGNAL: {e}")).await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:chat"
    }
}
