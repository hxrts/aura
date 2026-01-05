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
    CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL, RECOVERY_SIGNAL,
};
use aura_app::views::{
    chat::{Channel, ChannelType, ChatState, Message, MessageDeliveryStatus},
    contacts::{Contact, ContactsState},
    home::{BanRecord, HomeState, HomesState, KickRecord, MuteRecord, PinnedMessageMeta},
    invitations::{
        Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState,
    },
    recovery::{Guardian, GuardianStatus, RecoveryState},
};
use aura_app::ReactiveHandler;
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::{DomainFact, ProtocolRelationalFact};
use aura_protocol::amp::amp_recv;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::scheduler::ReactiveView;

use crate::runtime::AuraEffectSystem;
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_invitation::{
    InvitationFact, InvitationType as DomainInvitationType, INVITATION_FACT_TYPE_ID,
};
use aura_recovery::{RecoveryFact, RECOVERY_FACT_TYPE_ID};
use aura_relational::{ContactFact, ReadReceiptPolicy, CONTACT_FACT_TYPE_ID};
use aura_social::moderation::facts::{
    HomePinFact, HomeUnpinFact, HOME_PIN_FACT_TYPE_ID, HOME_UNPIN_FACT_TYPE_ID,
};
use aura_social::moderation::{
    HomeBanFact, HomeKickFact, HomeMuteFact, HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_UNBAN_FACT_TYPE_ID,
    HOME_UNMUTE_FACT_TYPE_ID,
};

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

    fn map_invitation_type(inv_type: &DomainInvitationType) -> InvitationType {
        match inv_type {
            // Legacy mapping: the TUI maps aura-app Home → "Contact".
            DomainInvitationType::Contact { .. } => InvitationType::Home,
            DomainInvitationType::Guardian { .. } => InvitationType::Guardian,
            DomainInvitationType::Channel { .. } => InvitationType::Chat,
            DomainInvitationType::DeviceEnrollment { .. } => InvitationType::Home,
        }
    }
}

impl ReactiveView for InvitationsSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content
            else {
                continue;
            };

            if envelope.type_id.as_str() != INVITATION_FACT_TYPE_ID {
                continue;
            }

            let Some(inv) = InvitationFact::from_envelope(envelope) else {
                emit_internal_error(
                    &self.reactive,
                    format!(
                        "Failed to decode InvitationFact envelope (payload len={})",
                        envelope.payload.len()
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
                        id: invitation_id.to_string(),
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
                        home_id: None,
                        home_name: None,
                    };

                    state.add_invitation(invitation);
                    changed = true;
                }
                InvitationFact::Accepted { invitation_id, .. } => {
                    let _ = state.accept_invitation(invitation_id.as_str());
                    changed = true;
                }
                InvitationFact::Declined { invitation_id, .. } => {
                    let _ = state.reject_invitation(invitation_id.as_str());
                    changed = true;
                }
                InvitationFact::Cancelled { invitation_id, .. } => {
                    let _ = state.revoke_invitation(invitation_id.as_str());
                    changed = true;
                }
                InvitationFact::CeremonyInitiated {
                    ceremony_id,
                    sender,
                    timestamp_ms,
                    ..
                } => {
                    // Invitation ceremony events don't map to InvitationsState.
                    // They track the consensus-based invitation exchange protocol.
                    // For RecoveryState updates, use RecoveryFacts or the ceremony tracker.
                    tracing::debug!(
                        ceremony_id = %ceremony_id,
                        sender = %sender,
                        timestamp_ms,
                        "Invitation ceremony initiated"
                    );
                }
                InvitationFact::CeremonyAcceptanceReceived {
                    ceremony_id,
                    timestamp_ms,
                    ..
                } => {
                    tracing::debug!(
                        ceremony_id = %ceremony_id,
                        timestamp_ms,
                        "Invitation ceremony acceptance received"
                    );
                }
                InvitationFact::CeremonyCommitted {
                    ceremony_id,
                    relationship_id,
                    timestamp_ms,
                    ..
                } => {
                    tracing::info!(
                        ceremony_id = %ceremony_id,
                        relationship_id,
                        timestamp_ms,
                        "Invitation ceremony committed - relationship established"
                    );
                }
                InvitationFact::CeremonyAborted {
                    ceremony_id,
                    reason,
                    timestamp_ms,
                    ..
                } => {
                    tracing::warn!(
                        ceremony_id = %ceremony_id,
                        reason,
                        timestamp_ms,
                        "Invitation ceremony aborted"
                    );
                }
                InvitationFact::CeremonySuperseded {
                    superseded_ceremony_id,
                    superseding_ceremony_id,
                    reason,
                    timestamp_ms,
                    ..
                } => {
                    tracing::warn!(
                        superseded_ceremony_id = %superseded_ceremony_id,
                        superseding_ceremony_id = %superseding_ceremony_id,
                        reason,
                        timestamp_ms,
                        "Invitation ceremony superseded"
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
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == CONTACT_FACT_TYPE_ID =>
                {
                    let Some(contact_fact) = ContactFact::from_envelope(envelope) else {
                        emit_internal_error(
                            &self.reactive,
                            format!(
                                "Failed to decode ContactFact envelope (payload len={})",
                                envelope.payload.len()
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

                            if let Some(contact) = state.contact_mut(&contact_id) {
                                // Preserve any user-set nickname; only fill suggestion if missing.
                                if contact.nickname_suggestion.is_none() {
                                    contact.nickname_suggestion = suggested_name;
                                }
                                contact.last_interaction = Some(added_at.ts_ms);
                            } else {
                                // Contact invitations carry an optional nickname, which we treat as
                                // a nickname_suggestion. The user's nickname is a separate local label.
                                state.apply_contact(Contact {
                                    id: contact_id,
                                    nickname: String::new(),
                                    nickname_suggestion: suggested_name,
                                    is_guardian: false,
                                    is_resident: false,
                                    last_interaction: Some(added_at.ts_ms),
                                    is_online: false,
                                    read_receipt_policy: ReadReceiptPolicy::default(),
                                });
                            }
                            changed = true;
                        }
                        ContactFact::Removed { contact_id, .. } => {
                            state.remove_contact(&contact_id);
                            changed = true;
                        }
                        ContactFact::Renamed {
                            contact_id,
                            new_nickname,
                            renamed_at,
                            ..
                        } => {
                            state.set_nickname(contact_id, new_nickname);
                            if let Some(contact) = state.contact_mut(&contact_id) {
                                contact.last_interaction = Some(renamed_at.ts_ms);
                            }
                            changed = true;
                        }
                        ContactFact::ReadReceiptPolicyUpdated {
                            contact_id, policy, ..
                        } => {
                            state.set_read_receipt_policy(&contact_id, policy);
                            changed = true;
                        }
                    }
                }
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding { guardian_id, .. },
                )) => {
                    // Reflect guardian status into contacts for details screens.
                    state.set_guardian_status(guardian_id, true);
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
// Recovery
// =============================================================================

pub struct RecoverySignalView {
    reactive: ReactiveHandler,
    state: Mutex<RecoveryState>,
}

impl RecoverySignalView {
    pub fn new(reactive: ReactiveHandler) -> Self {
        Self {
            reactive,
            state: Mutex::new(RecoveryState::default()),
        }
    }

    fn ensure_guardian(state: &mut RecoveryState, guardian_id: AuthorityId) {
        // Try to activate existing guardian, otherwise add new one
        if state.activate_guardian(&guardian_id).is_err() {
            state.upsert_guardian(Guardian {
                id: guardian_id,
                name: String::new(),
                status: GuardianStatus::Active,
                added_at: 0,
                last_seen: None,
            });
        }
    }
}

impl ReactiveView for RecoverySignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            match &fact.content {
                FactContent::Relational(RelationalFact::Protocol(
                    aura_journal::ProtocolRelationalFact::GuardianBinding { guardian_id, .. },
                )) => {
                    Self::ensure_guardian(&mut state, *guardian_id);
                    changed = true;
                }
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == RECOVERY_FACT_TYPE_ID =>
                {
                    let Some(recovery_fact) = RecoveryFact::from_envelope(envelope) else {
                        emit_internal_error(
                            &self.reactive,
                            format!(
                                "Failed to decode RecoveryFact envelope (payload len={})",
                                envelope.payload.len()
                            ),
                        )
                        .await;
                        continue;
                    };

                    match recovery_fact {
                        RecoveryFact::GuardianSetupInitiated {
                            guardian_ids,
                            threshold,
                            ..
                        } => {
                            for guardian_id in guardian_ids {
                                Self::ensure_guardian(&mut state, guardian_id);
                            }
                            state.set_threshold(threshold as u32);
                            changed = true;
                        }
                        RecoveryFact::GuardianSetupCompleted {
                            guardian_ids,
                            threshold,
                            ..
                        } => {
                            // Replace guardian set with the ceremony-completed list.
                            state.retain_guardians(&guardian_ids);
                            for guardian_id in guardian_ids {
                                Self::ensure_guardian(&mut state, guardian_id);
                            }
                            state.set_threshold(threshold as u32);
                            changed = true;
                        }
                        RecoveryFact::MembershipChangeCompleted {
                            new_guardian_ids,
                            new_threshold,
                            ..
                        } => {
                            state.set_threshold(new_threshold as u32);
                            // Update guardian set to match membership change
                            state.retain_guardians(&new_guardian_ids);
                            for guardian_id in new_guardian_ids {
                                Self::ensure_guardian(&mut state, guardian_id);
                            }
                            changed = true;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        if !changed {
            return;
        }

        let snapshot = state.clone();
        drop(state);

        if let Err(e) = self.reactive.emit(&*RECOVERY_SIGNAL, snapshot).await {
            emit_internal_error(
                &self.reactive,
                format!("Failed to emit RECOVERY_SIGNAL: {e}"),
            )
            .await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:recovery"
    }
}

// =============================================================================
// Homes (Moderation + Pins)
// =============================================================================

pub struct HomeSignalView {
    reactive: ReactiveHandler,
}

impl HomeSignalView {
    pub fn new(reactive: ReactiveHandler) -> Self {
        Self { reactive }
    }

    fn home_for_context<'a>(
        homes: &'a mut HomesState,
        context_id: &ContextId,
    ) -> Option<&'a mut HomeState> {
        homes
            .all_homes_mut()
            .find(|home_state| home_state.context_id.as_ref() == Some(context_id))
    }
}

impl ReactiveView for HomeSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut homes = match self.reactive.read(&*HOMES_SIGNAL).await {
            Ok(state) => state,
            Err(e) => {
                emit_internal_error(&self.reactive, format!("Failed to read HOMES_SIGNAL: {e}"))
                    .await;
                return;
            }
        };

        let mut changed = false;

        for fact in facts {
            let FactContent::Relational(RelationalFact::Generic { context_id, envelope }) =
                &fact.content
            else {
                continue;
            };

            let Some(home_state) = Self::home_for_context(&mut homes, context_id) else {
                continue;
            };

            match envelope.type_id.as_str() {
                HOME_BAN_FACT_TYPE_ID => {
                    if let Some(ban) = HomeBanFact::from_envelope(envelope) {
                        let record = BanRecord {
                            authority_id: ban.banned_authority,
                            reason: ban.reason,
                            actor: ban.actor_authority,
                            banned_at: ban.banned_at.ts_ms,
                        };
                        home_state.add_ban(record);
                        let _ = home_state.remove_resident(&ban.banned_authority);
                        changed = true;
                    }
                }
                HOME_UNBAN_FACT_TYPE_ID => {
                    if let Some(unban) = HomeUnbanFact::from_envelope(envelope) {
                        if home_state.remove_ban(&unban.unbanned_authority).is_some() {
                            changed = true;
                        }
                    }
                }
                HOME_MUTE_FACT_TYPE_ID => {
                    if let Some(mute) = HomeMuteFact::from_envelope(envelope) {
                        let record = MuteRecord {
                            authority_id: mute.muted_authority,
                            duration_secs: mute.duration_secs,
                            muted_at: mute.muted_at.ts_ms,
                            expires_at: mute.expires_at.as_ref().map(|t| t.ts_ms),
                            actor: mute.actor_authority,
                        };
                        home_state.add_mute(record);
                        changed = true;
                    }
                }
                HOME_UNMUTE_FACT_TYPE_ID => {
                    if let Some(unmute) = HomeUnmuteFact::from_envelope(envelope) {
                        if home_state.remove_mute(&unmute.unmuted_authority).is_some() {
                            changed = true;
                        }
                    }
                }
                HOME_KICK_FACT_TYPE_ID => {
                    if let Some(kick) = HomeKickFact::from_envelope(envelope) {
                        let record = KickRecord {
                            authority_id: kick.kicked_authority,
                            channel: kick.channel_id,
                            reason: kick.reason,
                            actor: kick.actor_authority,
                            kicked_at: kick.kicked_at.ts_ms,
                        };
                        home_state.add_kick(record);
                        let _ = home_state.remove_resident(&kick.kicked_authority);
                        changed = true;
                    }
                }
                HOME_PIN_FACT_TYPE_ID => {
                    if let Some(pin) = HomePinFact::from_envelope(envelope) {
                        home_state.pin_message_with_meta(PinnedMessageMeta {
                            message_id: pin.message_id,
                            pinned_by: pin.actor_authority,
                            pinned_at: pin.pinned_at.ts_ms,
                        });
                        changed = true;
                    }
                }
                HOME_UNPIN_FACT_TYPE_ID => {
                    if let Some(unpin) = HomeUnpinFact::from_envelope(envelope) {
                        if home_state.unpin_message(&unpin.message_id) {
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

        let snapshot = homes.clone();
        drop(homes);

        if let Err(e) = self.reactive.emit(&*HOMES_SIGNAL, snapshot.clone()).await {
            emit_internal_error(&self.reactive, format!("Failed to emit HOMES_SIGNAL: {e}")).await;
        }
    }

    fn view_id(&self) -> &str {
        "signals:homes"
    }
}

// =============================================================================
// Chat
// =============================================================================

pub struct ChatSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    state: Mutex<ChatState>,
    effects: Arc<AuraEffectSystem>,
}

impl ChatSignalView {
    pub fn new(
        own_authority: AuthorityId,
        reactive: ReactiveHandler,
        effects: Arc<AuraEffectSystem>,
    ) -> Self {
        Self {
            own_authority,
            reactive,
            state: Mutex::new(ChatState::default()),
            effects,
        }
    }
}

impl ReactiveView for ChatSignalView {
    async fn update(&self, facts: &[Fact]) {
        let mut state = self.state.lock().await;
        let mut changed = false;

        for fact in facts {
            match &fact.content {
                // Handle consensus finalization: mark messages as finalized when epoch is committed
                FactContent::Relational(RelationalFact::Protocol(
                    ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump),
                )) => {
                    // When a channel epoch is committed, all messages with epoch_hint <= parent_epoch are finalized
                    let count =
                        state.mark_finalized_up_to_epoch(&bump.channel, bump.parent_epoch as u32);
                    if count > 0 {
                        tracing::debug!(
                            channel_id = %bump.channel,
                            parent_epoch = bump.parent_epoch,
                            new_epoch = bump.new_epoch,
                            finalized_count = count,
                            "Finalized messages up to epoch"
                        );
                        changed = true;
                    }
                    continue;
                }

                // Handle generic chat facts
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == CHAT_FACT_TYPE_ID =>
                {
                    let Some(chat_fact) = ChatFact::from_envelope(envelope) else {
                        emit_internal_error(
                            &self.reactive,
                            format!(
                                "Failed to decode ChatFact envelope (payload len={})",
                                envelope.payload.len()
                            ),
                        )
                        .await;
                        continue;
                    };

                    match chat_fact {
                        ChatFact::ChannelCreated {
                            channel_id,
                            context_id,
                            name,
                            topic,
                            is_dm,
                            created_at,
                            ..
                        } => {
                            let channel = Channel {
                                id: channel_id,
                                context_id: Some(context_id),
                                name,
                                topic,
                                channel_type: if is_dm {
                                    ChannelType::DirectMessage
                                } else {
                                    ChannelType::Home
                                },
                                unread_count: 0,
                                is_dm,
                                member_ids: Vec::new(),
                                member_count: 0,
                                last_message: None,
                                last_message_time: None,
                                last_activity: created_at.ts_ms,
                                last_finalized_epoch: 0,
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
                            context_id,
                            channel_id,
                            message_id,
                            sender_id,
                            sender_name,
                            payload,
                            sent_at,
                            reply_to,
                            epoch_hint,
                        } => {
                            let sealed_len = payload.len();
                            let payload_bytes = payload.clone();
                            let context = context_id;
                            drop(state);
                            let content =
                                match amp_recv(self.effects.as_ref(), context, payload_bytes).await
                                {
                                    Ok(msg) => {
                                        String::from_utf8(msg.payload).unwrap_or_else(|_| {
                                            format!("[sealed: {} bytes]", sealed_len)
                                        })
                                    }
                                    Err(err) => {
                                        tracing::debug!(
                                            channel_id = %channel_id,
                                            message_id = %message_id,
                                            error = %err,
                                            "AMP decrypt failed; rendering sealed payload"
                                        );
                                        format!("[sealed: {} bytes]", sealed_len)
                                    }
                                };
                            state = self.state.lock().await;
                            let is_own = sender_id == self.own_authority;

                            // Derive delivery status from fact's consistency metadata
                            let delivery_status = if is_own {
                                // For messages we sent, derive status from agreement level
                                // Finalized (A3) messages have consensus confirmation = Delivered
                                // Ack-tracked messages will transition based on acknowledgments
                                if fact.is_finalized() {
                                    MessageDeliveryStatus::Delivered
                                } else {
                                    MessageDeliveryStatus::Sent
                                }
                            } else {
                                // Messages we received are already delivered to us
                                MessageDeliveryStatus::Delivered
                            };

                            let message = Message {
                                id: message_id,
                                channel_id,
                                sender_id,
                                sender_name,
                                content,
                                timestamp: sent_at.ts_ms,
                                reply_to,
                                is_own,
                                is_read: is_own,
                                delivery_status,
                                epoch_hint,
                                is_finalized: fact.is_finalized(),
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
                            // Two cases:
                            // 1. Reader is us - mark message as read in our local state
                            // 2. Reader is someone else - update our message's delivery_status to Read
                            if reader_id == self.own_authority {
                                // We read someone else's message
                                if state.mark_message_read(&channel_id, &message_id) {
                                    state.decrement_unread(&channel_id);
                                    changed = true;
                                }
                                tracing::debug!(
                                    channel_id = %channel_id,
                                    message_id,
                                    read_at = read_at.ts_ms,
                                    "Message marked as read by us"
                                );
                            } else {
                                // Someone else read our message - update delivery status
                                if state.mark_read_by_recipient(&message_id) {
                                    tracing::debug!(
                                        channel_id = %channel_id,
                                        message_id,
                                        reader_id = %reader_id,
                                        read_at = read_at.ts_ms,
                                        "Message delivery status updated to Read"
                                    );
                                    changed = true;
                                }
                            }
                        }
                        ChatFact::MessageEdited {
                            channel_id,
                            message_id,
                            editor_id,
                            new_payload,
                            edited_at,
                            ..
                        } => {
                            // Update the message content in local state
                            let new_content = String::from_utf8_lossy(&new_payload).to_string();
                            if let Some(msg) = state.message_mut(&channel_id, &message_id) {
                                msg.content = new_content;
                            }
                            tracing::debug!(
                                channel_id = %channel_id,
                                message_id,
                                editor_id = %editor_id,
                                edited_at = edited_at.ts_ms,
                                "Message edited"
                            );
                            changed = true;
                        }
                        ChatFact::MessageDeleted {
                            channel_id,
                            message_id,
                            deleter_id,
                            deleted_at,
                            ..
                        } => {
                            // Remove the message from local state
                            state.remove_message(&channel_id, &message_id);
                            tracing::debug!(
                                channel_id = %channel_id,
                                message_id,
                                deleter_id = %deleter_id,
                                deleted_at = deleted_at.ts_ms,
                                "Message deleted"
                            );
                            changed = true;
                        }
                    }
                }

                // Ignore other fact types in ChatSignalView
                _ => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use aura_app::signal_defs::{register_app_signals, HOMES_SIGNAL};
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};
    use aura_social::moderation::facts::{HomePinFact, HomeUnpinFact};
    use aura_social::moderation::HomeBanFact;

    async fn setup_homes(reactive: &ReactiveHandler, context: ContextId) -> HomesState {
        register_app_signals(reactive).await.unwrap();

        let home_id = ChannelId::from_bytes([7u8; 32]);
        let home_state = HomeState::new(
            home_id,
            Some("test-home".to_string()),
            AuthorityId::new_from_entropy([1u8; 32]),
            0,
            context,
        );

        let mut homes = HomesState::new();
        homes.add_home_with_auto_select(home_state);
        reactive.emit(&*HOMES_SIGNAL, homes.clone()).await.unwrap();
        homes
    }

    fn fact_from_relational(relational: RelationalFact) -> Fact {
        Fact::new(
            OrderTime([0u8; 32]),
            TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 0,
                uncertainty: None,
            }),
            FactContent::Relational(relational),
        )
    }

    #[tokio::test]
    async fn home_signal_view_updates_pins() {
        let reactive = ReactiveHandler::new();
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let homes = setup_homes(&reactive, context_id).await;
        let home_id = homes.current_home().unwrap().id;

        let view = HomeSignalView::new(reactive.clone());

        let pin = HomePinFact::new_ms(
            context_id,
            home_id,
            "msg-1".to_string(),
            AuthorityId::new_from_entropy([1u8; 32]),
            123,
        )
        .to_generic();
        view.update(&[fact_from_relational(pin)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated.current_home().unwrap();
        assert!(home_state.pinned_messages.contains(&"msg-1".to_string()));

        let unpin = HomeUnpinFact::new_ms(
            context_id,
            home_id,
            "msg-1".to_string(),
            AuthorityId::new_from_entropy([1u8; 32]),
            124,
        )
        .to_generic();
        view.update(&[fact_from_relational(unpin)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated.current_home().unwrap();
        assert!(!home_state.pinned_messages.contains(&"msg-1".to_string()));
    }

    #[tokio::test]
    async fn home_signal_view_updates_bans() {
        let reactive = ReactiveHandler::new();
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let homes = setup_homes(&reactive, context_id).await;
        let home_id = homes.current_home().unwrap().id;
        let target = AuthorityId::new_from_entropy([9u8; 32]);

        let view = HomeSignalView::new(reactive.clone());

        let ban = HomeBanFact::new_ms(
            context_id,
            None,
            target,
            AuthorityId::new_from_entropy([1u8; 32]),
            "spamming".to_string(),
            999,
            None,
        )
        .to_generic();
        view.update(&[fact_from_relational(ban)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated.current_home().unwrap();
        assert!(home_state.ban_list.contains_key(&target));
        assert_eq!(home_state.ban_list.get(&target).unwrap().reason, "spamming");
        assert_eq!(home_state.id, home_id);
    }
}
