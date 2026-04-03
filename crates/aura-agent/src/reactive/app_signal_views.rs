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
    chat::{
        note_to_self_channel_id, Channel, ChannelType, ChatState, Message, MessageDeliveryStatus,
    },
    contacts::{Contact, ContactError, ContactRelationshipState, ContactsState},
    home::{
        BanRecord, HomeMember, HomeRole, HomeState, HomesState, KickRecord, MuteRecord,
        PinnedMessageMeta,
    },
    invitations::{Invitation, InvitationDirection, InvitationStatus, InvitationsState},
    recovery::{Guardian, GuardianStatus, RecoveryState},
};
use aura_app::ReactiveHandler;
use aura_core::effects::reactive::{ReactiveEffects, Signal};
use aura_core::effects::{AmpChannelEffects, ChannelCreateParams, ChannelJoinParams};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::{DomainFact, ProtocolRelationalFact};
use aura_protocol::amp::{
    amp_recv, get_channel_state, ChannelMembershipFact, ChannelParticipantEvent,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::scheduler::{ReactiveUpdateFuture, ReactiveView};
use crate::reactive::app_signal_projection;

use crate::runtime::AuraEffectSystem;
use aura_chat::{ChatFact, CHAT_FACT_TYPE_ID};
use aura_invitation::{
    InvitationFact, InvitationType as DomainInvitationType, INVITATION_FACT_TYPE_ID,
};
use aura_recovery::{RecoveryFact, RECOVERY_FACT_TYPE_ID};
use aura_relational::{
    ContactFact, FriendshipFact, ReadReceiptPolicy, CONTACT_FACT_TYPE_ID, FRIENDSHIP_FACT_TYPE_ID,
};
use aura_social::moderation::facts::{
    HomePinFact, HomeUnpinFact, HOME_PIN_FACT_TYPE_ID, HOME_UNPIN_FACT_TYPE_ID,
};
use aura_social::moderation::{
    HomeBanFact, HomeGrantModeratorFact, HomeKickFact, HomeMuteFact, HomeRevokeModeratorFact,
    HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID,
};

async fn emit_internal_error(reactive: &ReactiveHandler, message: String) {
    let _ = reactive
        .emit(
            &*ERROR_SIGNAL,
            Some(AppError::internal("reactive_scheduler", message)),
        )
        .await;
}

async fn read_registered_signal<T>(
    reactive: &ReactiveHandler,
    signal: &Signal<T>,
    signal_label: &str,
) -> Result<T, String>
where
    T: Clone + Send + Sync + 'static,
{
    reactive.read(signal).await.map_err(|error| {
        format!("{signal_label} materialization requires registered {signal_label}: {error}")
    })
}

async fn emit_registered_signal<T>(
    reactive: &ReactiveHandler,
    signal: &Signal<T>,
    value: T,
    signal_label: &str,
) -> Result<(), String>
where
    T: Clone + Send + Sync + 'static,
{
    reactive
        .emit(signal, value)
        .await
        .map_err(|error| format!("emit {signal_label}: {error}"))
}

async fn emit_signal_or_internal_error<T>(
    reactive: &ReactiveHandler,
    signal: &Signal<T>,
    value: T,
    signal_label: &str,
) where
    T: Clone + Send + Sync + 'static,
{
    if let Err(error) = reactive.emit(signal, value).await {
        emit_internal_error(reactive, format!("Failed to emit {signal_label}: {error}")).await;
    }
}

async fn read_registered_homes_state(reactive: &ReactiveHandler) -> Result<HomesState, String> {
    read_registered_signal(reactive, &*HOMES_SIGNAL, "homes signal").await
}

async fn read_registered_invitations_state(
    reactive: &ReactiveHandler,
) -> Result<InvitationsState, String> {
    read_registered_signal(reactive, &*INVITATIONS_SIGNAL, "invitations signal").await
}

pub(crate) async fn materialize_home_signal_for_channel_invitation(
    reactive: &ReactiveHandler,
    own_authority: AuthorityId,
    channel_id: ChannelId,
    home_name: &str,
    sender_id: AuthorityId,
    context_id: ContextId,
    now_ms: u64,
) -> Result<(), String> {
    let mut homes = read_registered_homes_state(reactive).await?;
    let mut changed = false;

    if !homes.has_home(&channel_id) {
        let mut home = HomeState::new(
            channel_id,
            Some(home_name.to_string()),
            sender_id,
            now_ms,
            context_id,
        );

        if sender_id != own_authority {
            if let Some(owner) = home.member_mut(&sender_id) {
                owner.name = sender_id.to_string();
                owner.is_online = false;
                owner.last_seen = Some(now_ms);
            }
            home.my_role = HomeRole::Participant;
        }

        if home.member(&own_authority).is_none() {
            home.add_member(HomeMember {
                id: own_authority,
                name: "You".to_string(),
                role: HomeRole::Participant,
                is_online: true,
                joined_at: now_ms,
                last_seen: Some(now_ms),
                storage_allocated: HomeState::MEMBER_ALLOCATION,
            });
        }

        homes.add_home(home);
        if homes.current_home_id().is_none() {
            homes.select_home(Some(channel_id));
        }
        changed = true;
    } else if let Some(home) = homes.home_mut(&channel_id) {
        if home.context_id != Some(context_id) {
            home.context_id = Some(context_id);
            changed = true;
        }

        if sender_id != own_authority && home.member(&own_authority).is_none() {
            home.add_member(HomeMember {
                id: own_authority,
                name: "You".to_string(),
                role: HomeRole::Participant,
                is_online: true,
                joined_at: now_ms,
                last_seen: Some(now_ms),
                storage_allocated: HomeState::MEMBER_ALLOCATION,
            });
            changed = true;
        }

        if sender_id != own_authority && matches!(home.my_role, HomeRole::Member) {
            home.my_role = HomeRole::Participant;
            changed = true;
        }
    }

    if !changed {
        return Ok(());
    }

    if homes.current_home_id().is_none() && homes.has_home(&channel_id) {
        homes.select_home(Some(channel_id));
    }

    emit_registered_signal(reactive, &*HOMES_SIGNAL, homes, "homes signal").await?;

    Ok(())
}

pub(crate) async fn materialize_home_signal_for_channel_acceptance(
    reactive: &ReactiveHandler,
    home_id: ChannelId,
    home_name: &str,
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    context_id: ContextId,
    now_ms: u64,
) -> Result<(), String> {
    let mut homes = read_registered_homes_state(reactive).await?;
    let mut changed = false;

    if !homes.has_home(&home_id) {
        let mut home = HomeState::new(
            home_id,
            Some(home_name.to_string()),
            sender_id,
            now_ms,
            context_id,
        );
        if home.member(&receiver_id).is_none() {
            home.add_member(HomeMember {
                id: receiver_id,
                name: receiver_id.to_string(),
                role: HomeRole::Participant,
                is_online: false,
                joined_at: now_ms,
                last_seen: Some(now_ms),
                storage_allocated: HomeState::MEMBER_ALLOCATION,
            });
        }
        homes.add_home(home);
        changed = true;
    } else if let Some(home) = homes.home_mut(&home_id) {
        if home.context_id != Some(context_id) {
            home.context_id = Some(context_id);
            changed = true;
        }
        if home.member(&receiver_id).is_none() {
            home.add_member(HomeMember {
                id: receiver_id,
                name: receiver_id.to_string(),
                role: HomeRole::Participant,
                is_online: false,
                joined_at: now_ms,
                last_seen: Some(now_ms),
                storage_allocated: HomeState::MEMBER_ALLOCATION,
            });
            changed = true;
        }
    }

    if !changed {
        return Ok(());
    }

    emit_registered_signal(reactive, &*HOMES_SIGNAL, homes, "homes signal").await?;

    Ok(())
}

pub(crate) async fn materialize_pending_invitation_signal(
    reactive: &ReactiveHandler,
    own_authority: AuthorityId,
    invitation_id: &str,
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    invitation_type: &DomainInvitationType,
    created_at: u64,
    expires_at: Option<u64>,
    message: Option<String>,
) -> Result<(), String> {
    let mut invitations = match read_registered_invitations_state(reactive).await {
        Ok(invitations) => invitations,
        Err(error) if error.contains("Signal not found") => return Ok(()),
        Err(error) => return Err(error),
    };
    if invitations.invitation(invitation_id).is_some() {
        return Ok(());
    }

    let direction = if sender_id == own_authority {
        InvitationDirection::Sent
    } else {
        InvitationDirection::Received
    };
    let is_generic_sent_contact_invitation = direction == InvitationDirection::Sent
        && matches!(invitation_type, DomainInvitationType::Contact { .. })
        && sender_id == receiver_id;
    let (home_id, home_name) = app_signal_projection::map_channel_metadata(invitation_type);
    invitations.add_invitation(Invitation {
        id: invitation_id.to_string(),
        invitation_type: app_signal_projection::map_invitation_type(invitation_type),
        status: InvitationStatus::Pending,
        direction,
        from_id: sender_id,
        from_name: "Unknown".to_string(),
        to_id: (direction == InvitationDirection::Sent && !is_generic_sent_contact_invitation)
            .then_some(receiver_id),
        to_name: (direction == InvitationDirection::Sent && !is_generic_sent_contact_invitation)
            .then_some("Unknown".to_string()),
        created_at,
        expires_at,
        message,
        home_id,
        home_name,
    });

    emit_registered_signal(
        reactive,
        &*INVITATIONS_SIGNAL,
        invitations,
        "invitations signal",
    )
    .await?;

    Ok(())
}

// =============================================================================
// Invitations
// =============================================================================

pub struct InvitationsSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    update_gate: Mutex<()>,
}

impl InvitationsSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            update_gate: Mutex::new(()),
        }
    }
}

impl ReactiveView for InvitationsSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let _update_gate = self.update_gate.lock().await;
            let mut state = match read_registered_invitations_state(&self.reactive).await {
                Ok(state) => state,
                Err(error) => {
                    emit_internal_error(&self.reactive, error).await;
                    return;
                }
            };
            let mut changed = false;

            for fact in facts {
                let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                    &fact.content
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
                        let is_generic_sent_contact_invitation = direction
                            == InvitationDirection::Sent
                            && matches!(invitation_type, DomainInvitationType::Contact { .. })
                            && sender_id == receiver_id;
                        let (home_id, home_name) =
                            app_signal_projection::map_channel_metadata(&invitation_type);

                        let invitation = Invitation {
                            id: invitation_id.to_string(),
                            invitation_type: app_signal_projection::map_invitation_type(
                                &invitation_type,
                            ),
                            status: InvitationStatus::Pending,
                            direction,
                            from_id: sender_id,
                            from_name: "Unknown".to_string(),
                            to_id: (direction == InvitationDirection::Sent
                                && !is_generic_sent_contact_invitation)
                                .then_some(receiver_id),
                            to_name: (direction == InvitationDirection::Sent
                                && !is_generic_sent_contact_invitation)
                                .then_some("Unknown".to_string()),
                            created_at: sent_at.ts_ms,
                            expires_at: expires_at.map(|t| t.ts_ms),
                            message,
                            home_id,
                            home_name,
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
                            relationship_id = %relationship_id,
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

            emit_signal_or_internal_error(
                &self.reactive,
                &*INVITATIONS_SIGNAL,
                snapshot,
                "INVITATIONS_SIGNAL",
            )
            .await;
        })
    }

    fn view_id(&self) -> &str {
        "signals:invitations"
    }
}

// =============================================================================
// Contacts
// =============================================================================

pub struct ContactsSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    state: Mutex<ContactsState>,
}

impl ContactsSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            state: Mutex::new(ContactsState::default()),
        }
    }

    fn apply_friendship_fact(&self, state: &mut ContactsState, fact: &FriendshipFact) -> bool {
        let Some(other) = fact.other_participant(self.own_authority) else {
            return false;
        };

        let relationship_state = match fact {
            FriendshipFact::Proposed { requester, .. } if *requester == self.own_authority => {
                ContactRelationshipState::PendingOutbound
            }
            FriendshipFact::Proposed { .. } => ContactRelationshipState::PendingInbound,
            FriendshipFact::Accepted { .. } => ContactRelationshipState::Friend,
            FriendshipFact::Revoked { .. } => ContactRelationshipState::Contact,
        };
        state.set_relationship_state(other, relationship_state);
        true
    }
}

impl ReactiveView for ContactsSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
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
                                tracing::info!(
                                    contact_id = %contact_id,
                                    nickname = %nickname,
                                    added_at = added_at.ts_ms,
                                    "ContactsSignalView: Processing ContactFact::Added"
                                );

                                let suggested_name = if nickname.trim().is_empty()
                                    || nickname == contact_id.to_string()
                                {
                                    None
                                } else {
                                    Some(nickname.clone())
                                };

                                if let Some(contact) = state.contact_mut(&contact_id) {
                                    // Preserve user-set local nickname and keep any existing
                                    // human-friendly suggestion when incoming facts only carry
                                    // fallback identity strings.
                                    if let Some(suggested_name) = suggested_name {
                                        contact.nickname_suggestion = Some(suggested_name);
                                    }
                                    contact.last_interaction = Some(added_at.ts_ms);
                                } else {
                                    // Contact invitations carry an optional nickname, which we treat as
                                    // a nickname_suggestion. The user's nickname is a separate local label.
                                    tracing::info!(
                                        contact_id = %contact_id,
                                        "ContactsSignalView: Creating new contact entry"
                                    );
                                    state.apply_contact(Contact {
                                        id: contact_id,
                                        nickname: String::new(),
                                        nickname_suggestion: suggested_name,
                                        is_guardian: false,
                                        is_member: false,
                                        last_interaction: Some(added_at.ts_ms),
                                        is_online: false,
                                        read_receipt_policy: ReadReceiptPolicy::default(),
                                        relationship_state: ContactRelationshipState::Contact,
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
                    FactContent::Relational(RelationalFact::Generic { envelope, .. })
                        if envelope.type_id.as_str() == FRIENDSHIP_FACT_TYPE_ID =>
                    {
                        let Some(friendship_fact) = FriendshipFact::from_envelope(envelope) else {
                            emit_internal_error(
                                &self.reactive,
                                format!(
                                    "Failed to decode FriendshipFact envelope (payload len={})",
                                    envelope.payload.len()
                                ),
                            )
                            .await;
                            continue;
                        };
                        changed |= self.apply_friendship_fact(&mut state, &friendship_fact);
                    }
                    FactContent::Relational(RelationalFact::Protocol(
                        aura_journal::ProtocolRelationalFact::GuardianBinding {
                            guardian_id, ..
                        },
                    )) => {
                        // Reflect guardian status into contacts for details screens.
                        // Collect contact IDs first for diagnostic logging.
                        let contact_ids: Vec<AuthorityId> = state.contact_ids().cloned().collect();
                        tracing::info!(
                            guardian_id = %guardian_id,
                            existing_contacts = ?contact_ids,
                            "ContactsSignalView: Processing GuardianBinding"
                        );
                        match state.set_guardian_status(guardian_id, true) {
                            Ok(()) => {
                                tracing::info!(
                                    guardian_id = %guardian_id,
                                    "ContactsSignalView: Successfully set guardian status"
                                );
                                changed = true;
                            }
                            Err(ContactError::NotFound(id)) => {
                                tracing::warn!(
                                    guardian_id = %id,
                                    existing_contacts = ?contact_ids,
                                    "GuardianBinding received but contact not found - \
                                     contact should be added before guardian ceremony completes"
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }

            if !changed {
                return;
            }

            let snapshot = state.clone();
            let contact_count = snapshot.contact_count();
            let guardian_contacts: Vec<_> = snapshot
                .all_contacts()
                .filter(|c| c.is_guardian)
                .map(|c| c.id)
                .collect();
            let all_contact_ids: Vec<_> = snapshot.all_contacts().map(|c| c.id).collect();
            tracing::info!(
                contact_count,
                all_contacts = ?all_contact_ids,
                guardians = ?guardian_contacts,
                "ContactsSignalView: Emitting updated contacts"
            );
            drop(state);

            emit_signal_or_internal_error(
                &self.reactive,
                &*CONTACTS_SIGNAL,
                snapshot,
                "CONTACTS_SIGNAL",
            )
            .await;
        })
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
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            let mut changed = false;

            for fact in facts {
                match &fact.content {
                    FactContent::Relational(RelationalFact::Protocol(
                        aura_journal::ProtocolRelationalFact::GuardianBinding {
                            guardian_id, ..
                        },
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

            emit_signal_or_internal_error(
                &self.reactive,
                &*RECOVERY_SIGNAL,
                snapshot,
                "RECOVERY_SIGNAL",
            )
            .await;
        })
    }

    fn view_id(&self) -> &str {
        "signals:recovery"
    }
}

// =============================================================================
// Homes (Moderation + Pins)
// =============================================================================

pub struct HomeSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
}

impl HomeSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
        }
    }

    fn synthetic_home_id_for_context(context_id: &ContextId) -> ChannelId {
        let mut bytes = [0u8; 32];
        bytes[..16].copy_from_slice(context_id.as_bytes());
        ChannelId::from_bytes(bytes)
    }

    fn home_for_context_mut<'a>(
        &self,
        homes: &'a mut HomesState,
        context_id: &ContextId,
    ) -> &'a mut HomeState {
        let existing_home_id = homes
            .iter()
            .find_map(|(home_id, home)| (home.context_id == Some(*context_id)).then_some(*home_id));
        if let Some(home_id) = existing_home_id {
            return homes
                .home_mut(&home_id)
                .expect("home id from iter() must exist in map");
        }

        let mut placeholder = HomeState::new(
            Self::synthetic_home_id_for_context(context_id),
            Some("Shared Home".to_string()),
            self.own_authority,
            0,
            *context_id,
        );
        // Placeholder state exists to host moderation facts for shared contexts
        // that have not been materialized as local homes yet.
        placeholder.my_role = HomeRole::Participant;
        placeholder.members.clear();
        placeholder.online_count = 0;
        placeholder.member_count = 0;
        let placeholder_id = placeholder.id;
        let _ = homes.add_home(placeholder);
        homes
            .home_mut(&placeholder_id)
            .expect("placeholder home should exist immediately after insertion")
    }
}

impl ReactiveView for HomeSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let mut homes = match self.reactive.read(&*HOMES_SIGNAL).await {
                Ok(state) => state,
                Err(e) => {
                    emit_internal_error(
                        &self.reactive,
                        format!("Failed to read HOMES_SIGNAL: {e}"),
                    )
                    .await;
                    return;
                }
            };

            let mut changed = false;

            for fact in facts {
                let FactContent::Relational(RelationalFact::Generic {
                    context_id,
                    envelope,
                }) = &fact.content
                else {
                    continue;
                };

                let home_state = self.home_for_context_mut(&mut homes, context_id);

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
                            let _ = home_state.remove_member(&ban.banned_authority);
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
                            let _ = home_state.remove_member(&kick.kicked_authority);
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
                    HOME_GRANT_MODERATOR_FACT_TYPE_ID => {
                        if let Some(grant) = HomeGrantModeratorFact::from_envelope(envelope) {
                            if let Some(member) = home_state.member_mut(&grant.target_authority) {
                                if matches!(member.role, HomeRole::Member | HomeRole::Moderator) {
                                    member.role = HomeRole::Moderator;
                                    changed = true;
                                }
                            }
                            if grant.target_authority == self.own_authority
                                && matches!(
                                    home_state.my_role,
                                    HomeRole::Member | HomeRole::Moderator
                                )
                            {
                                home_state.my_role = HomeRole::Moderator;
                                changed = true;
                            }
                        }
                    }
                    HOME_REVOKE_MODERATOR_FACT_TYPE_ID => {
                        if let Some(revoke) = HomeRevokeModeratorFact::from_envelope(envelope) {
                            if let Some(member) = home_state.member_mut(&revoke.target_authority) {
                                if matches!(member.role, HomeRole::Moderator) {
                                    member.role = HomeRole::Member;
                                    changed = true;
                                }
                            }
                            if revoke.target_authority == self.own_authority
                                && matches!(home_state.my_role, HomeRole::Moderator)
                            {
                                home_state.my_role = HomeRole::Member;
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

            emit_signal_or_internal_error(
                &self.reactive,
                &*HOMES_SIGNAL,
                snapshot.clone(),
                "HOMES_SIGNAL",
            )
            .await;
        })
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
    hidden_channels_after_leave: Mutex<BTreeSet<ChannelId>>,
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
            hidden_channels_after_leave: Mutex::new(BTreeSet::new()),
            effects,
        }
    }

    async fn ensure_amp_channel_state(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        creator_id: AuthorityId,
    ) {
        if get_channel_state(self.effects.as_ref(), context_id, channel_id)
            .await
            .is_ok()
        {
            return;
        }

        tracing::debug!(
            context_id = %context_id,
            channel_id = %channel_id,
            creator_id = %creator_id,
            "Provisioning AMP channel state from inbound ChannelCreated fact"
        );

        if let Err(err) = self
            .effects
            .create_channel(ChannelCreateParams {
                context: context_id,
                channel: Some(channel_id),
                skip_window: None,
                topic: None,
            })
            .await
        {
            if get_channel_state(self.effects.as_ref(), context_id, channel_id)
                .await
                .is_err()
            {
                tracing::warn!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    error = %err,
                    "Failed to provision AMP channel checkpoint from chat fact"
                );
                return;
            }
        }

        let mut participants = vec![self.own_authority];
        if creator_id != self.own_authority {
            participants.push(creator_id);
        }

        for participant in participants {
            if let Err(err) = self
                .effects
                .join_channel(ChannelJoinParams {
                    context: context_id,
                    channel: channel_id,
                    participant,
                })
                .await
            {
                tracing::debug!(
                    context_id = %context_id,
                    channel_id = %channel_id,
                    participant = %participant,
                    error = %err,
                    "AMP join from chat fact provisioning failed (continuing)"
                );
            }
        }
    }

    async fn sender_allowed_for_context(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        sender_id: AuthorityId,
        sent_at_ms: u64,
    ) -> bool {
        if sender_id == self.own_authority {
            return true;
        }

        let homes = match self.reactive.read(&*HOMES_SIGNAL).await {
            Ok(homes) => homes,
            Err(_) => return false,
        };
        let candidates =
            app_signal_projection::collect_moderation_homes(&homes, context_id, channel_id);
        if candidates.is_empty() {
            return false;
        }

        if candidates.iter().any(|home| home.is_banned(&sender_id)) {
            return false;
        }
        if candidates
            .iter()
            .any(|home| home.is_muted(&sender_id, sent_at_ms))
        {
            return false;
        }
        let has_member_roster = candidates.iter().any(|home| !home.members.is_empty());
        let sender_is_member = candidates
            .iter()
            .any(|home| home.member(&sender_id).is_some());
        if !has_member_roster {
            return false;
        }
        if !sender_is_member {
            tracing::debug!(
                context_id = %context_id,
                channel_id = %channel_id,
                sender_id = %sender_id,
                "Dropping inbound message because moderation membership is unavailable or denies sender"
            );
            return false;
        }

        true
    }
}

impl ReactiveView for ChatSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let mut state = self.state.lock().await;
            let mut changed = false;

            for fact in facts {
                match &fact.content {
                    // Handle consensus finalization: mark messages as finalized when epoch is committed
                    FactContent::Relational(RelationalFact::Protocol(
                        ProtocolRelationalFact::AmpCommittedChannelEpochBump(bump),
                    )) => {
                        // When a channel epoch is committed, all messages with epoch_hint <= parent_epoch are finalized
                        let count = state
                            .mark_finalized_up_to_epoch(&bump.channel, bump.parent_epoch as u32)
                            .unwrap_or(0);
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
                                creator_id,
                                ..
                            } => {
                                let hidden_after_leave = {
                                    self.hidden_channels_after_leave
                                        .lock()
                                        .await
                                        .contains(&channel_id)
                                };
                                if hidden_after_leave {
                                    continue;
                                }

                                drop(state);
                                self.ensure_amp_channel_state(context_id, channel_id, creator_id)
                                    .await;
                                state = self.state.lock().await;

                                // Seed membership from inbound channel facts so reply routing
                                // has at least one deterministic peer even before richer
                                // membership reductions are available.
                                let (member_ids, member_count) = if is_dm {
                                    let mut members = vec![self.own_authority];
                                    if creator_id != self.own_authority {
                                        members.push(creator_id);
                                    }
                                    let count = members.len().max(2) as u32;
                                    (members, count)
                                } else if creator_id != self.own_authority {
                                    // For group channels we may not know the full roster yet.
                                    // Seed the creator as an initial peer so recipients can reply.
                                    (vec![creator_id], 2)
                                } else {
                                    (Vec::new(), 0)
                                };

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
                                    member_ids,
                                    member_count,
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
                                context_id,
                                channel_id,
                                name,
                                topic,
                                member_count,
                                member_ids,
                                updated_at,
                                ..
                            } => {
                                if let Some(channel) = state.channel_mut(&channel_id) {
                                    channel.context_id = Some(context_id);
                                    if let Some(name) = name {
                                        channel.name = name;
                                    }
                                    if topic.is_some() {
                                        channel.topic = topic;
                                    }
                                    if let Some(member_count) = member_count {
                                        channel.member_count = member_count;
                                    }
                                    if let Some(member_ids) = member_ids {
                                        channel.member_ids = member_ids;
                                    }
                                    channel.last_activity = updated_at.ts_ms;
                                } else {
                                    let Some(name) = name else {
                                        tracing::debug!(
                                            channel_id = %channel_id,
                                            context_id = %context_id,
                                            "ignoring ChannelUpdated without canonical name for unknown channel"
                                        );
                                        continue;
                                    };
                                    state.upsert_channel(Channel {
                                        id: channel_id,
                                        context_id: Some(context_id),
                                        name,
                                        topic,
                                        channel_type: ChannelType::Home,
                                        unread_count: 0,
                                        is_dm: false,
                                        member_ids: member_ids.unwrap_or_default(),
                                        member_count: member_count.unwrap_or(1),
                                        last_message: None,
                                        last_message_time: None,
                                        last_activity: updated_at.ts_ms,
                                        last_finalized_epoch: 0,
                                    });
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
                                let note_to_self_channel =
                                    note_to_self_channel_id(self.own_authority);
                                drop(state);
                                if !self
                                    .sender_allowed_for_context(
                                        context,
                                        channel_id,
                                        sender_id,
                                        sent_at.ts_ms,
                                    )
                                    .await
                                {
                                    tracing::debug!(
                                        context_id = %context,
                                        channel_id = %channel_id,
                                        message_id = %message_id,
                                        sender_id = %sender_id,
                                        "Dropping message due to moderation policy"
                                    );
                                    state = self.state.lock().await;
                                    continue;
                                }
                                let content = if channel_id == note_to_self_channel {
                                    String::from_utf8(payload_bytes.clone()).unwrap_or_else(|_| {
                                        format!("[sealed: {} bytes]", sealed_len)
                                    })
                                } else {
                                    match amp_recv(self.effects.as_ref(), context, payload_bytes)
                                        .await
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
                                    }
                                };
                                state = self.state.lock().await;
                                tracing::info!(
                                    channel_id = %channel_id,
                                    sender_id = %sender_id,
                                    own_authority = %self.own_authority,
                                    is_own = sender_id == self.own_authority,
                                    message_id = %message_id,
                                    "ChatSignalView applying MessageSentSealed"
                                );
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
                            ChatFact::MessageDeliveryUpdated {
                                channel_id,
                                message_id,
                                delivery_status,
                                ..
                            } => {
                                let updated = match delivery_status {
                                    aura_chat::ChatMessageDeliveryStatus::Sent => false,
                                    aura_chat::ChatMessageDeliveryStatus::Delivered => {
                                        state.mark_delivered(&message_id)
                                    }
                                    aura_chat::ChatMessageDeliveryStatus::Read => {
                                        state.mark_read_by_recipient(&message_id)
                                    }
                                    aura_chat::ChatMessageDeliveryStatus::Failed => {
                                        state.mark_failed(&message_id)
                                    }
                                };
                                tracing::debug!(
                                    channel_id = %channel_id,
                                    message_id,
                                    ?delivery_status,
                                    "Message delivery status updated"
                                );
                                changed |= updated;
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
                    FactContent::Relational(RelationalFact::Generic { envelope, .. }) => {
                        let Some(membership) = ChannelMembershipFact::from_envelope(envelope)
                        else {
                            continue;
                        };

                        let channel_id = membership.channel();
                        let participant = membership.participant();
                        match membership.event() {
                            ChannelParticipantEvent::Joined => {
                                if participant == self.own_authority {
                                    self.hidden_channels_after_leave
                                        .lock()
                                        .await
                                        .remove(&channel_id);
                                }
                                if state.channel(&channel_id).is_none() {
                                    tracing::debug!(
                                        channel_id = %channel_id,
                                        participant = %participant,
                                        "ignoring ChannelParticipantEvent::Joined without canonical channel metadata"
                                    );
                                    continue;
                                }
                                if let Some(channel) = state.channel_mut(&channel_id) {
                                    if participant != self.own_authority
                                        && !channel.member_ids.contains(&participant)
                                    {
                                        channel.member_ids.push(participant);
                                    }
                                    let known_members =
                                        channel.member_ids.len().saturating_add(1) as u32;
                                    if known_members > channel.member_count {
                                        channel.member_count = known_members;
                                    }
                                    changed = true;
                                }
                            }
                            ChannelParticipantEvent::Left => {
                                if participant == self.own_authority {
                                    self.hidden_channels_after_leave
                                        .lock()
                                        .await
                                        .insert(channel_id);
                                    if state.remove_channel(&channel_id).is_some() {
                                        changed = true;
                                    }
                                } else if let Some(channel) = state.channel_mut(&channel_id) {
                                    let before = channel.member_ids.len();
                                    channel.member_ids.retain(|member| *member != participant);
                                    if channel.member_ids.len() != before {
                                        channel.member_count =
                                            channel.member_count.saturating_sub(1);
                                        changed = true;
                                    }
                                }
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

            emit_signal_or_internal_error(&self.reactive, &*CHAT_SIGNAL, snapshot, "CHAT_SIGNAL")
                .await;
        })
    }

    fn view_id(&self) -> &str {
        "signals:chat"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::effects::AuraEffectSystem;
    use crate::AgentConfig;
    use aura_app::signal_defs::{
        register_app_signals, CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL,
    };
    use aura_app::views::chat::ChatState;
    use aura_core::effects::reactive::ReactiveEffects;
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};
    use aura_relational::{ContactFact, FriendshipFact};
    use aura_social::moderation::facts::{
        HomeGrantModeratorFact, HomePinFact, HomeRevokeModeratorFact, HomeUnpinFact,
    };
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
        let result = homes.add_home(home_state);
        if result.was_first {
            homes.select_home(Some(result.home_id));
        }
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

    #[test]
    fn select_moderation_home_prefers_channel_authoritative_match() {
        let context_id = ContextId::new_from_entropy([11u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);

        let channel_home_id = ChannelId::from_bytes([21u8; 32]);
        let synthetic_home_id = ChannelId::from_bytes([22u8; 32]);
        let channel_home = HomeState::new(
            channel_home_id,
            Some("channel-home".to_string()),
            owner,
            0,
            context_id,
        );
        let synthetic_home = HomeState::new(
            synthetic_home_id,
            Some("synthetic-home".to_string()),
            owner,
            0,
            context_id,
        );

        let mut homes = HomesState::new();
        homes.add_home(channel_home);
        homes.add_home(synthetic_home);

        let selected =
            app_signal_projection::select_moderation_home(&homes, context_id, channel_home_id)
                .expect("channel-authoritative home should be selected");
        assert_eq!(selected.id, channel_home_id);
    }

    #[test]
    fn invitation_signal_view_preserves_channel_metadata() {
        let channel_id = ChannelId::from_bytes([41u8; 32]);
        let (home_id, home_name) =
            app_signal_projection::map_channel_metadata(&DomainInvitationType::Channel {
                home_id: channel_id,
                nickname_suggestion: Some("shared-parity-lab".to_string()),
                bootstrap: None,
            });

        assert_eq!(home_id, Some(channel_id));
        assert_eq!(home_name.as_deref(), Some("shared-parity-lab"));
    }

    #[tokio::test]
    async fn generic_sent_contact_invitation_hides_receiver_identity() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();

        let own_authority = AuthorityId::new_from_entropy([81u8; 32]);
        materialize_pending_invitation_signal(
            &reactive,
            own_authority,
            "generic-contact-invite",
            own_authority,
            own_authority,
            &DomainInvitationType::Contact {
                nickname: Some("friend".to_string()),
            },
            1234,
            Some(5678),
            Some("share this code".to_string()),
        )
        .await
        .expect("materialize generic sent contact invitation");

        let invitations = reactive
            .read(&*INVITATIONS_SIGNAL)
            .await
            .expect("invitation signal should be registered");
        let invitation = invitations
            .invitation("generic-contact-invite")
            .expect("generic invitation should be present");

        assert_eq!(
            invitation.direction,
            aura_app::views::invitations::InvitationDirection::Sent
        );
        assert_eq!(invitation.to_id, None);
        assert_eq!(invitation.to_name, None);
    }

    #[test]
    fn select_moderation_home_rejects_ambiguous_context() {
        let context_id = ContextId::new_from_entropy([12u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);

        let home_a_id = ChannelId::from_bytes([31u8; 32]);
        let home_b_id = ChannelId::from_bytes([32u8; 32]);
        let unknown_channel_id = ChannelId::from_bytes([99u8; 32]);

        let mut homes = HomesState::new();
        homes.add_home(HomeState::new(
            home_a_id,
            Some("home-a".to_string()),
            owner,
            0,
            context_id,
        ));
        homes.add_home(HomeState::new(
            home_b_id,
            Some("home-b".to_string()),
            owner,
            0,
            context_id,
        ));

        let selected =
            app_signal_projection::select_moderation_home(&homes, context_id, unknown_channel_id);
        assert!(
            selected.is_none(),
            "ambiguous context without channel-authoritative home should be rejected"
        );
    }

    #[tokio::test]
    async fn sender_allowed_for_context_denies_when_homes_unavailable() {
        let reactive = ReactiveHandler::new();
        let own_authority = AuthorityId::new_from_entropy([35u8; 32]);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                own_authority,
            )
            .unwrap(),
        );
        let view = ChatSignalView::new(own_authority, reactive, effects);

        let allowed = view
            .sender_allowed_for_context(
                ContextId::new_from_entropy([36u8; 32]),
                ChannelId::from_bytes([37u8; 32]),
                AuthorityId::new_from_entropy([38u8; 32]),
                1_700_000_000_000,
            )
            .await;

        assert!(
            !allowed,
            "missing moderation state must fail closed for inbound sender gating"
        );
    }

    #[tokio::test]
    async fn sender_allowed_for_context_denies_when_context_is_ambiguous() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([39u8; 32]);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                own_authority,
            )
            .unwrap(),
        );
        let view = ChatSignalView::new(own_authority, reactive.clone(), effects);
        let context_id = ContextId::new_from_entropy([40u8; 32]);
        let sender_id = AuthorityId::new_from_entropy([41u8; 32]);

        let mut homes = HomesState::new();
        homes.add_home(HomeState::new(
            ChannelId::from_bytes([42u8; 32]),
            Some("home-a".to_string()),
            own_authority,
            0,
            context_id,
        ));
        homes.add_home(HomeState::new(
            ChannelId::from_bytes([43u8; 32]),
            Some("home-b".to_string()),
            own_authority,
            0,
            context_id,
        ));
        reactive.emit(&*HOMES_SIGNAL, homes).await.unwrap();

        let allowed = view
            .sender_allowed_for_context(
                context_id,
                ChannelId::from_bytes([44u8; 32]),
                sender_id,
                1_700_000_000_001,
            )
            .await;

        assert!(
            !allowed,
            "ambiguous moderation context must fail closed for inbound sender gating"
        );
    }

    #[tokio::test]
    async fn home_signal_view_updates_pins() {
        let reactive = ReactiveHandler::new();
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let homes = setup_homes(&reactive, context_id).await;
        let home_id = homes.current_home().unwrap().id;

        let view = HomeSignalView::new(AuthorityId::new_from_entropy([1u8; 32]), reactive.clone());

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

        let view = HomeSignalView::new(AuthorityId::new_from_entropy([1u8; 32]), reactive.clone());

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

    #[tokio::test]
    async fn home_signal_view_updates_moderator_roles() {
        let reactive = ReactiveHandler::new();
        let context_id = ContextId::new_from_entropy([3u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);
        let target = AuthorityId::new_from_entropy([9u8; 32]);
        let mut homes = setup_homes(&reactive, context_id).await;

        {
            let home = homes.current_home_mut().expect("home exists");
            home.add_member(aura_app::views::home::HomeMember {
                id: target,
                name: "target".to_string(),
                role: aura_app::views::home::HomeRole::Member,
                is_online: true,
                joined_at: 1,
                last_seen: Some(1),
                storage_allocated: 0,
            });
            reactive.emit(&*HOMES_SIGNAL, homes.clone()).await.unwrap();
        }

        let view = HomeSignalView::new(target, reactive.clone());

        let grant = HomeGrantModeratorFact::new_ms(context_id, target, owner, 100).to_generic();
        view.update(&[fact_from_relational(grant)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated.current_home().unwrap();
        let member = home_state.member(&target).expect("target member exists");
        assert!(matches!(
            member.role,
            aura_app::views::home::HomeRole::Moderator
        ));
        assert!(matches!(
            home_state.my_role,
            aura_app::views::home::HomeRole::Moderator
        ));

        let revoke = HomeRevokeModeratorFact::new_ms(context_id, target, owner, 101).to_generic();
        view.update(&[fact_from_relational(revoke)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated.current_home().unwrap();
        let member = home_state.member(&target).expect("target member exists");
        assert!(matches!(
            member.role,
            aura_app::views::home::HomeRole::Member
        ));
        assert!(matches!(
            home_state.my_role,
            aura_app::views::home::HomeRole::Member
        ));
    }

    #[tokio::test]
    async fn home_signal_view_materializes_unknown_context_for_mutes() {
        let reactive = ReactiveHandler::new();
        let known_context = ContextId::new_from_entropy([2u8; 32]);
        let unknown_context = ContextId::new_from_entropy([4u8; 32]);
        let actor = AuthorityId::new_from_entropy([1u8; 32]);
        let target = AuthorityId::new_from_entropy([9u8; 32]);
        let _ = setup_homes(&reactive, known_context).await;

        let view = HomeSignalView::new(actor, reactive.clone());

        let mute = HomeMuteFact::new_ms(
            unknown_context,
            None,
            target,
            actor,
            Some(60),
            100,
            Some(160_000),
        )
        .to_generic();
        view.update(&[fact_from_relational(mute)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated
            .iter()
            .find_map(|(_, home)| (home.context_id == Some(unknown_context)).then_some(home))
            .expect("unknown context home should be materialized");
        assert!(home_state.mute_list.contains_key(&target));
        assert!(home_state.members.is_empty());
        assert!(matches!(home_state.my_role, HomeRole::Participant));

        let unmute = HomeUnmuteFact::new_ms(unknown_context, None, target, actor, 200).to_generic();
        view.update(&[fact_from_relational(unmute)]).await;

        let updated = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home_state = updated
            .iter()
            .find_map(|(_, home)| (home.context_id == Some(unknown_context)).then_some(home))
            .expect("unknown context home should still exist");
        assert!(!home_state.mute_list.contains_key(&target));
    }

    #[tokio::test]
    async fn chat_signal_view_ignores_membership_join_without_canonical_channel_metadata() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([31u8; 32]);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                own_authority,
            )
            .unwrap(),
        );
        let view = ChatSignalView::new(own_authority, reactive.clone(), effects);
        let context_id = ContextId::new_from_entropy([32u8; 32]);
        let channel_id = ChannelId::from_bytes([33u8; 32]);
        let peer = AuthorityId::new_from_entropy([34u8; 32]);
        let membership = ChannelMembershipFact::new(
            context_id,
            channel_id,
            peer,
            ChannelParticipantEvent::Joined,
            TimeStamp::OrderClock(OrderTime([1u8; 32])),
        )
        .to_generic();

        view.update(&[fact_from_relational(membership)]).await;

        let chat: ChatState = reactive.read(&*CHAT_SIGNAL).await.unwrap_or_default();
        assert!(
            chat.channel(&channel_id).is_none(),
            "membership-only facts must not fabricate channel projection without canonical metadata"
        );
    }

    #[tokio::test]
    async fn contacts_signal_view_projects_friendship_states_from_relational_facts() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([51u8; 32]);
        let peer = AuthorityId::new_from_entropy([52u8; 32]);
        let inbound_peer = AuthorityId::new_from_entropy([53u8; 32]);
        let contact_context = ContextId::new_from_entropy([54u8; 32]);
        let friendship_context = ContextId::new_from_entropy([55u8; 32]);
        let inbound_friendship_context = ContextId::new_from_entropy([56u8; 32]);
        let view = ContactsSignalView::new(own_authority, reactive.clone());

        let contact_added = ContactFact::Added {
            context_id: contact_context,
            owner_id: own_authority,
            contact_id: peer,
            nickname: "Peer".to_string(),
            added_at: PhysicalTime {
                ts_ms: 10,
                uncertainty: None,
            },
        }
        .to_generic();
        view.update(&[fact_from_relational(contact_added)]).await;

        let contacts = reactive
            .read(&*CONTACTS_SIGNAL)
            .await
            .expect("contacts signal should be published");
        assert_eq!(
            contacts
                .contact(&peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::Contact)
        );

        let outbound_proposed = FriendshipFact::Proposed {
            context_id: friendship_context,
            requester: own_authority,
            accepter: peer,
            proposed_at: PhysicalTime {
                ts_ms: 11,
                uncertainty: None,
            },
        }
        .to_generic();
        view.update(&[fact_from_relational(outbound_proposed)])
            .await;

        let contacts = reactive
            .read(&*CONTACTS_SIGNAL)
            .await
            .expect("contacts signal should remain published");
        assert_eq!(
            contacts
                .contact(&peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::PendingOutbound)
        );

        let accepted = FriendshipFact::Accepted {
            context_id: friendship_context,
            requester: own_authority,
            accepter: peer,
            accepted_at: PhysicalTime {
                ts_ms: 12,
                uncertainty: None,
            },
        }
        .to_generic();
        view.update(&[fact_from_relational(accepted)]).await;

        let contacts = reactive
            .read(&*CONTACTS_SIGNAL)
            .await
            .expect("contacts signal should remain published");
        assert_eq!(
            contacts
                .contact(&peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::Friend)
        );

        let revoked = FriendshipFact::Revoked {
            context_id: friendship_context,
            requester: own_authority,
            accepter: peer,
            revoked_at: PhysicalTime {
                ts_ms: 13,
                uncertainty: None,
            },
        }
        .to_generic();
        view.update(&[fact_from_relational(revoked)]).await;

        let contacts = reactive
            .read(&*CONTACTS_SIGNAL)
            .await
            .expect("contacts signal should remain published");
        assert_eq!(
            contacts
                .contact(&peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::Contact)
        );

        let inbound_proposed = FriendshipFact::Proposed {
            context_id: inbound_friendship_context,
            requester: inbound_peer,
            accepter: own_authority,
            proposed_at: PhysicalTime {
                ts_ms: 14,
                uncertainty: None,
            },
        }
        .to_generic();
        view.update(&[fact_from_relational(inbound_proposed)]).await;

        let contacts = reactive
            .read(&*CONTACTS_SIGNAL)
            .await
            .expect("contacts signal should remain published");
        assert_eq!(
            contacts
                .contact(&inbound_peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::PendingInbound)
        );
    }
}
