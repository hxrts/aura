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
use aura_app::signal_defs::{CHAT_SIGNAL, CONTACTS_SIGNAL, ERROR_SIGNAL, INVITATIONS_SIGNAL};
use aura_app::views::{
    chat::{Channel, ChannelType, ChatState, Message},
    contacts::{Contact, ContactsState},
    invitations::{Invitation, InvitationDirection, InvitationStatus, InvitationType, InvitationsState},
};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::identifiers::AuthorityId;
use aura_effects::ReactiveHandler;
use aura_journal::DomainFact;
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use tokio::sync::Mutex;

use super::scheduler::ReactiveView;

use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_invitation::{InvitationFact, INVITATION_FACT_TYPE_ID};
use aura_relational::{ContactFact, CONTACT_FACT_TYPE_ID};

async fn emit_internal_error(reactive: &ReactiveHandler, message: String) {
    let _ = reactive
        .emit(&*ERROR_SIGNAL, Some(AppError::internal("reactive_scheduler", message)))
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
                    format!("Failed to decode InvitationFact bytes (len={})", binding_data.len()),
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
                InvitationFact::CeremonyInitiated { .. }
                | InvitationFact::CeremonyAcceptanceReceived { .. }
                | InvitationFact::CeremonyCommitted { .. }
                | InvitationFact::CeremonyAborted { .. } => {
                    // TODO(C2): Map ceremony events into RecoveryState once recovery facts are canonical.
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
                            if state.contact(&contact_id).is_none() {
                                state.contacts.push(Contact {
                                    id: contact_id,
                                    nickname,
                                    suggested_name: None,
                                    is_guardian: false,
                                    is_resident: false,
                                    last_interaction: Some(added_at.ts_ms),
                                    is_online: false,
                                });
                            } else {
                                state.set_nickname(contact_id, nickname);
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
                FactContent::Relational(RelationalFact::GuardianBinding { guardian_id, .. }) => {
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
                    format!("Failed to decode ChatFact bytes (len={})", binding_data.len()),
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
                ChatFact::MessageSent {
                    channel_id,
                    message_id,
                    sender_id,
                    sender_name,
                    content,
                    sent_at,
                    reply_to,
                    ..
                } => {
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
                ChatFact::MessageRead { .. }
                | ChatFact::MessageDelivered { .. }
                | ChatFact::DeliveryAcknowledged { .. } => {
                    // TODO(C2): Map delivery/read status into ChatState when UI needs it.
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
