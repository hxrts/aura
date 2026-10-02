//! ReactiveScheduler views that emit Aura application signals.
//!
//! These views are the bridge between:
//! - The canonical typed-fact pipeline (`aura_journal::fact::Fact`)
//! - The UI-facing reactive signals in `aura-app` (`*_SIGNAL`)
//!
//! The scheduler calls `update(facts)` with each processed batch. Each view:
//! - Applies the relevant domain facts to its aggregate state
//! - Emits a full snapshot into the corresponding signal (eventual consistency)

use aura_app::effects::reactive::ConditionalEmit;
use aura_app::errors::AppError;
use aura_app::projection_owner::{ProjectionOwner, ProjectionSlot};
use aura_app::signal_defs::{ERROR_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL};
pub(crate) use aura_app::views::invitations::InvitationCreationWitness;
use aura_app::views::{
    chat::{note_to_self_channel_id, ChatState, Message, MessageDeliveryStatus},
    contacts::{ContactError, ContactRelationshipState, ContactsState},
    home::{
        BanRecord, HomeCreationWitness, HomeMember, HomeRole, HomeState, HomesState, KickRecord,
        MuteRecord, PinnedMessageMeta,
    },
    invitations::{InvitationDirection, InvitationStatus},
    recovery::{Guardian, GuardianStatus, RecoveryProcess, RecoveryProcessStatus, RecoveryState},
};
use aura_app::ReactiveHandler;
use aura_composition::{downcast_delta_owned, ViewDeltaReducer};
use aura_core::effects::reactive::ReactiveEffects;
use aura_core::effects::{AmpChannelEffects, ChannelCreateParams, ChannelJoinParams};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_journal::fact::{Fact, FactContent, RelationalFact};
use aura_journal::{DomainFact, ProtocolRelationalFact};
use aura_protocol::amp::{
    amp_open_committed, get_channel_state, ChannelMembershipFact, ChannelParticipantEvent,
};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use tokio::sync::Mutex;

use super::scheduler::{ReactiveUpdateFuture, ReactiveView};
use crate::reactive::app_signal_projection;

use crate::handlers::invitation::ChannelInviteDetails;
use crate::runtime::AuraEffectSystem;
use aura_chat::{ChatDelta, ChatFact, ChatViewReducer, CHAT_FACT_TYPE_ID};
use aura_invitation::{
    Invitation as CachedInvitation, InvitationFact, InvitationStatus as DomainInvitationStatus,
    InvitationType as DomainInvitationType, INVITATION_FACT_TYPE_ID,
};
use aura_recovery::{RecoveryFact, RECOVERY_FACT_TYPE_ID};
use aura_relational::{ContactFact, FriendshipFact, CONTACT_FACT_TYPE_ID, FRIENDSHIP_FACT_TYPE_ID};
use aura_social::moderation::facts::{
    HomePinFact, HomeUnpinFact, HOME_PIN_FACT_TYPE_ID, HOME_UNPIN_FACT_TYPE_ID,
};
use aura_social::moderation::{
    HomeBanFact, HomeGrantModeratorFact, HomeKickFact, HomeMuteFact, HomeRevokeModeratorFact,
    HomeUnbanFact, HomeUnmuteFact, HOME_BAN_FACT_TYPE_ID, HOME_GRANT_MODERATOR_FACT_TYPE_ID,
    HOME_KICK_FACT_TYPE_ID, HOME_MUTE_FACT_TYPE_ID, HOME_REVOKE_MODERATOR_FACT_TYPE_ID,
    HOME_UNBAN_FACT_TYPE_ID, HOME_UNMUTE_FACT_TYPE_ID,
};
use aura_social::{SocialFact, SOCIAL_FACT_TYPE_ID};

async fn emit_internal_error(reactive: &ReactiveHandler, message: String) {
    let _ = reactive
        .emit(
            &*ERROR_SIGNAL,
            Some(AppError::internal("reactive_scheduler", message)),
        )
        .await;
}

/// Canonical AMP checkpoint and joined-participant evidence for projecting an
/// accepted home on the joining runtime. Fields are private to the owner.
pub(crate) struct VerifiedJoinedHome {
    channel_id: ChannelId,
    context_id: ContextId,
    name: String,
    sender_id: AuthorityId,
    own_authority: AuthorityId,
    now_ms: u64,
}

impl VerifiedJoinedHome {
    pub(crate) async fn verify(
        effects: &AuraEffectSystem,
        invite: &ChannelInviteDetails,
        own_authority: AuthorityId,
        now_ms: u64,
    ) -> Result<Self, String> {
        if !invite.home || invite.home_name.trim().is_empty() {
            return Err("home materialization requires a named home invitation".into());
        }
        get_channel_state(effects, invite.context_id, invite.channel_id)
            .await
            .map_err(|error| {
                format!("home materialization requires channel checkpoint: {error}")
            })?;
        let participants = aura_protocol::amp::list_channel_participants(
            effects,
            invite.context_id,
            invite.channel_id,
        )
        .await
        .map_err(|error| format!("home materialization requires channel membership: {error}"))?;
        if !participants.contains(&own_authority) {
            return Err("home materialization requires the local joined participant".into());
        }
        Ok(Self {
            channel_id: invite.channel_id,
            context_id: invite.context_id,
            name: invite.home_name.clone(),
            sender_id: invite.sender_id,
            own_authority,
            now_ms,
        })
    }

    pub(crate) fn bind_committed_creation(
        self,
        owner: &ProjectionOwner,
        fact: &SocialFact,
    ) -> Result<JoinedHomeEvidence, String> {
        let SocialFact::HomeCreated {
            home_id,
            context_id,
            creator_id,
            name,
            ..
        } = fact
        else {
            return Err("joined home requires a committed HomeCreated fact".into());
        };
        if home_id.as_bytes() != self.channel_id.as_bytes()
            || *context_id != self.context_id
            || *creator_id != self.sender_id
            || name != &self.name
        {
            return Err("committed HomeCreated fact does not match joined channel".into());
        }
        let creation = owner
            .home_created_witness(fact)
            .ok_or_else(|| "joined home requires a HomeCreated fact".to_string())?;
        Ok(JoinedHomeEvidence {
            verified: self,
            creation,
        })
    }
}

/// Joined AMP channel plus its durably committed home creation fact.
pub struct JoinedHomeEvidence {
    verified: VerifiedJoinedHome,
    creation: HomeCreationWitness,
}

/// Accepted invitation evidence for the inviter's home projection.
pub struct AcceptedHomeEvidence {
    home_id: ChannelId,
    context_id: ContextId,
    sender_id: AuthorityId,
    receiver_id: AuthorityId,
    now_ms: u64,
}

impl AcceptedHomeEvidence {
    fn from_invitation(
        invitation: &CachedInvitation,
        name: &str,
        now_ms: u64,
    ) -> Result<Self, String> {
        let DomainInvitationType::Channel {
            home_id,
            home: true,
            ..
        } = &invitation.invitation_type
        else {
            return Err("home materialization requires a home invitation".into());
        };
        if name.trim().is_empty() {
            return Err("home materialization requires a nonempty home name".into());
        }
        Ok(Self {
            home_id: *home_id,
            context_id: invitation.context_id,
            sender_id: invitation.sender_id,
            receiver_id: invitation.receiver_id,
            now_ms,
        })
    }

    pub(crate) fn from_accepted_invitation(
        invitation: &CachedInvitation,
        name: &str,
        now_ms: u64,
    ) -> Result<Self, String> {
        if invitation.status != DomainInvitationStatus::Accepted {
            return Err("home materialization requires an accepted invitation".into());
        }
        Self::from_invitation(invitation, name, now_ms)
    }

    pub(crate) fn from_exchange_response(
        invitation: &CachedInvitation,
        response: &aura_invitation::protocol::InvitationResponse,
        name: &str,
        now_ms: u64,
    ) -> Result<Self, String> {
        if !response.accepted || response.invitation_id != invitation.invitation_id {
            return Err("home materialization requires a matching acceptance response".into());
        }
        Self::from_invitation(invitation, name, now_ms)
    }
}

pub(crate) async fn materialize_home_signal_for_channel_invitation(
    reactive: &ReactiveHandler,
    evidence: JoinedHomeEvidence,
) -> Result<(), String> {
    let JoinedHomeEvidence { verified, creation } = evidence;
    let VerifiedJoinedHome {
        own_authority,
        channel_id,
        context_id,
        name: _,
        sender_id,
        now_ms,
    } = verified;
    let result = ProjectionOwner::new(reactive.clone())
        .update(ProjectionSlot::homes(), |homes| -> Result<(), ()> {
            let mut changed = false;

            if !homes.has_home(&channel_id) {
                let _ = homes.materialize_created_home(creation, own_authority);
                let home = homes
                    .home_mut(&channel_id)
                    .expect("creation witness materialized home");

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
                return Err(());
            }

            if homes.current_home_id().is_none() && homes.has_home(&channel_id) {
                homes.select_home(Some(channel_id));
            }

            Ok(())
        })
        .await
        .map_err(|error| {
            format!("homes signal materialization requires registered homes signal: {error}")
        })?;
    // Err(()) is the no-change path and does not publish a new revision.
    let _ = result;
    Ok(())
}

pub(crate) async fn materialize_home_signal_for_channel_acceptance(
    effects: &AuraEffectSystem,
    evidence: AcceptedHomeEvidence,
) -> Result<(), String> {
    let AcceptedHomeEvidence {
        home_id,
        context_id,
        sender_id,
        receiver_id,
        now_ms,
    } = evidence;
    let reactive = effects.reactive_handler();
    let owner = ProjectionOwner::new(reactive);
    owner
        .snapshot(ProjectionSlot::homes())
        .await
        .map_err(|error| {
            format!("homes signal materialization requires registered homes signal: {error}")
        })?;
    let created = effects
        .load_committed_facts(sender_id)
        .await
        .map_err(|error| format!("load canonical HomeCreated fact: {error}"))?
        .into_iter()
        .filter_map(|fact| match fact.content {
            FactContent::Relational(RelationalFact::Generic { envelope, .. })
                if envelope.type_id.as_str() == SOCIAL_FACT_TYPE_ID =>
            {
                SocialFact::from_envelope(&envelope)
            }
            _ => None,
        })
        .find(|fact| matches!(fact,
            SocialFact::HomeCreated { home_id: fact_home_id, context_id: fact_context_id, creator_id, .. }
            if fact_home_id.as_bytes() == home_id.as_bytes()
                && *fact_context_id == context_id
                && *creator_id == sender_id
        ))
        .ok_or_else(|| "accepted home has no committed HomeCreated fact".to_string())?;
    let creation = owner
        .home_created_witness(&created)
        .ok_or_else(|| "accepted home requires HomeCreated evidence".to_string())?;
    let mut materialization_error = None;
    let result = owner
        .update(ProjectionSlot::homes(), |homes| -> Result<(), ()> {
            let mut changed = false;

            if !homes.has_home(&home_id) {
                let _ = homes.materialize_created_home(creation, sender_id);
                changed = true;
            }
            let home = homes
                .home_mut(&home_id)
                .expect("creation witness materialized home");
            if home.context_id != Some(context_id) {
                materialization_error = Some("accepted home context differs from canonical home");
                return Err(());
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

            if !changed {
                return Err(());
            }
            Ok(())
        })
        .await
        .map_err(|error| {
            format!("homes signal materialization requires registered homes signal: {error}")
        })?;
    if let Some(error) = materialization_error {
        return Err(error.to_string());
    }
    let _ = result;
    Ok(())
}

pub(crate) async fn materialize_pending_invitation_signal(
    reactive: &ReactiveHandler,
    own_authority: AuthorityId,
    validated_import: aura_invitation::shareable::ValidatedImportedInvitation,
) -> Result<(), String> {
    let witness = InvitationCreationWitness::from_imported(&validated_import, own_authority)
        .ok_or_else(|| "validated import is not pending".to_string())?;
    materialize_pending_invitation_witness(reactive, witness).await
}

/// Unsigned unit fixtures enter through a synthetic full creation fact in the
/// agent's test build. This path cannot construct a validated import token.
#[cfg(test)]
pub(crate) async fn materialize_unverified_invitation_fixture_signal(
    reactive: &ReactiveHandler,
    own_authority: AuthorityId,
    invitation: &CachedInvitation,
) -> Result<(), String> {
    let sent = InvitationFact::Sent {
        context_id: invitation.context_id,
        invitation_id: invitation.invitation_id.clone(),
        sender_id: invitation.sender_id,
        receiver_id: invitation.receiver_id,
        invitation_type: invitation.invitation_type.clone(),
        sent_at: aura_core::time::PhysicalTime {
            ts_ms: invitation.created_at,
            uncertainty: None,
        },
        expires_at: invitation
            .expires_at
            .map(|ts_ms| aura_core::time::PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
        receiver_nickname: invitation.receiver_nickname.clone(),
        message: invitation.message.clone(),
    };
    let witness = ProjectionOwner::new(reactive.clone())
        .invitation_sent_witness(&sent, own_authority)
        .expect("synthetic Sent fixture has creation evidence");
    materialize_pending_invitation_witness(reactive, witness).await
}

async fn materialize_pending_invitation_witness(
    reactive: &ReactiveHandler,
    witness: InvitationCreationWitness,
) -> Result<(), String> {
    let invitation_id = witness.id().to_string();
    let result = ProjectionOwner::new(reactive.clone())
        .update(
            ProjectionSlot::invitations(),
            |invitations| -> Result<(), ()> {
                if invitations.invitation(&invitation_id).is_some() {
                    return Err(());
                }
                invitations.add_invitation(witness);

                Ok(())
            },
        )
        .await;
    match result {
        Ok(_) => {}
        // Invitation signals may be deliberately absent before app registration.
        Err(error) if error.to_string().contains("Signal not found") => return Ok(()),
        Err(error) => return Err(format!("materialize invitations signal: {error}")),
    }
    Ok(())
}

// =============================================================================
// Invitations
// =============================================================================

pub struct InvitationsSignalView {
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    update_gate: Mutex<()>,
    deferred_status: Mutex<HashMap<String, InvitationStatus>>,
}

impl InvitationsSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            update_gate: Mutex::new(()),
            deferred_status: Mutex::new(HashMap::new()),
        }
    }
}

impl ReactiveView for InvitationsSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let _update_gate = self.update_gate.lock().await;
            let owner = ProjectionOwner::new(self.reactive.clone());
            loop {
                let current = match owner.snapshot(ProjectionSlot::invitations()).await {
                    Ok(current) => current,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to read INVITATIONS_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                };
                let mut state = current.value;
                let mut deferred_status = self.deferred_status.lock().await;
                let mut next_deferred_status = deferred_status.clone();
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
                        sent_fact @ InvitationFact::Sent { .. } => {
                            let invitation = owner
                                .invitation_sent_witness(&sent_fact, self.own_authority)
                                .expect("matched InvitationFact::Sent");
                            let invitation_id = invitation.id().to_string();
                            // A replayed Sent fact cannot recreate a pending row after
                            // a newer acceptance, rejection, or cancellation.
                            if state.invitation(&invitation_id).is_some() {
                                if let Some(status) = next_deferred_status.remove(&invitation_id) {
                                    let id = invitation_id.as_str();
                                    changed |= match status {
                                        InvitationStatus::Accepted => {
                                            state.accept_invitation(id).is_ok()
                                        }
                                        InvitationStatus::Rejected => {
                                            state.reject_invitation(id).is_ok()
                                        }
                                        InvitationStatus::Revoked => {
                                            state.observe_cancelled_invitation(id).is_ok()
                                        }
                                        _ => false,
                                    };
                                }
                                continue;
                            }
                            let pending_status = next_deferred_status.remove(&invitation_id);
                            state.add_invitation(invitation);
                            match pending_status {
                                Some(InvitationStatus::Accepted) => {
                                    let _ = state.accept_invitation(&invitation_id);
                                }
                                Some(InvitationStatus::Rejected) => {
                                    let _ = state.reject_invitation(&invitation_id);
                                }
                                Some(InvitationStatus::Revoked) => {
                                    let _ = state.observe_cancelled_invitation(&invitation_id);
                                }
                                _ => {}
                            }
                            changed = true;
                        }
                        InvitationFact::Accepted { invitation_id, .. } => {
                            if state.accept_invitation(invitation_id.as_str()).is_ok() {
                                changed = true;
                            } else if state.invitation(invitation_id.as_str()).is_none() {
                                next_deferred_status
                                    .insert(invitation_id.to_string(), InvitationStatus::Accepted);
                            }
                        }
                        InvitationFact::Declined { invitation_id, .. } => {
                            if state.reject_invitation(invitation_id.as_str()).is_ok() {
                                changed = true;
                            } else if state.invitation(invitation_id.as_str()).is_none() {
                                next_deferred_status
                                    .insert(invitation_id.to_string(), InvitationStatus::Rejected);
                            }
                        }
                        InvitationFact::Cancelled { invitation_id, .. } => {
                            if state
                                .observe_cancelled_invitation(invitation_id.as_str())
                                .is_ok()
                            {
                                changed = true;
                            } else if state.invitation(invitation_id.as_str()).is_none() {
                                next_deferred_status
                                    .insert(invitation_id.to_string(), InvitationStatus::Revoked);
                            }
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
                    *deferred_status = next_deferred_status;
                    return;
                }

                match owner
                    .replace_if_current(ProjectionSlot::invitations(), current.revision, state)
                    .await
                {
                    Ok(ConditionalEmit::Published { .. }) => {
                        *deferred_status = next_deferred_status;
                        return;
                    }
                    Ok(ConditionalEmit::Stale { .. }) => continue,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to publish INVITATIONS_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
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
    pending_relationships: Mutex<HashMap<AuthorityId, ContactRelationshipState>>,
}

impl ContactsSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            state: Mutex::new(ContactsState::default()),
            pending_relationships: Mutex::new(HashMap::new()),
        }
    }

    fn apply_friendship_fact(
        &self,
        state: &mut ContactsState,
        pending: &mut HashMap<AuthorityId, ContactRelationshipState>,
        fact: &FriendshipFact,
    ) -> bool {
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
        if state.set_relationship_state(other, relationship_state) {
            pending.remove(&other);
            true
        } else {
            pending.insert(other, relationship_state);
            false
        }
    }
}

impl ReactiveView for ContactsSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let owner = ProjectionOwner::new(self.reactive.clone());
            loop {
                let current = match owner.snapshot(ProjectionSlot::contacts()).await {
                    Ok(current) => current,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to read CONTACTS_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                };
                let mut state = self.state.lock().await;
                let mut pending = self.pending_relationships.lock().await;
                *state = current.value;
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

                            let creation_witness = owner.contact_added_witness(&contact_fact);
                            match contact_fact {
                                ContactFact::Added {
                                    contact_id,
                                    nickname,
                                    added_at,
                                    invitation_code,
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
                                        // Only overwrite the invitation code if the incoming
                                        // fact carries one — later plain contact updates (e.g.
                                        // nickname changes) should preserve the code that was
                                        // recorded at establishment time.
                                        if invitation_code.is_some() {
                                            contact.invitation_code = invitation_code;
                                        }
                                    } else {
                                        // Contact invitations carry an optional nickname, which we treat as
                                        // a nickname_suggestion. The user's nickname is a separate local label.
                                        tracing::info!(
                                            contact_id = %contact_id,
                                            "ContactsSignalView: Creating new contact entry"
                                        );
                                        state.apply_contact(
                                            creation_witness.expect("matched ContactFact::Added"),
                                        );
                                    }
                                    if let Some(relationship_state) = pending.remove(&contact_id) {
                                        state
                                            .set_relationship_state(contact_id, relationship_state);
                                    }
                                    changed = true;
                                }
                                ContactFact::Removed { contact_id, .. } => {
                                    state.remove_contact(&contact_id);
                                    pending.remove(&contact_id);
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
                                    contact_id,
                                    policy,
                                    ..
                                } => {
                                    state.set_read_receipt_policy(&contact_id, policy);
                                    changed = true;
                                }
                            }
                        }
                        FactContent::Relational(RelationalFact::Generic { envelope, .. })
                            if envelope.type_id.as_str() == FRIENDSHIP_FACT_TYPE_ID =>
                        {
                            let Some(friendship_fact) = FriendshipFact::from_envelope(envelope)
                            else {
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
                            changed |= self.apply_friendship_fact(
                                &mut state,
                                &mut pending,
                                &friendship_fact,
                            );
                        }
                        FactContent::Relational(RelationalFact::Protocol(
                            aura_journal::ProtocolRelationalFact::GuardianBinding {
                                guardian_id,
                                ..
                            },
                        )) => {
                            // Reflect guardian status into contacts for details screens.
                            // Collect contact IDs first for diagnostic logging.
                            let contact_ids: Vec<AuthorityId> =
                                state.contact_ids().cloned().collect();
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

                match owner
                    .replace_if_current(ProjectionSlot::contacts(), current.revision, snapshot)
                    .await
                {
                    Ok(ConditionalEmit::Published { .. }) => return,
                    Ok(ConditionalEmit::Stale { .. }) => continue,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to publish CONTACTS_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
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
    own_authority: AuthorityId,
    reactive: ReactiveHandler,
    state: Mutex<RecoveryState>,
}

impl RecoverySignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
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
            let owner = ProjectionOwner::new(self.reactive.clone());
            loop {
                let current = match owner.snapshot(ProjectionSlot::recovery()).await {
                    Ok(current) => current,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to read RECOVERY_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                };
                let mut state = self.state.lock().await;
                *state = current.value;
                let mut changed = false;

                for fact in facts {
                    match &fact.content {
                        FactContent::Relational(RelationalFact::Protocol(
                            aura_journal::ProtocolRelationalFact::GuardianBinding {
                                guardian_id,
                                ..
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
                                    initiator_id,
                                    trace_id,
                                    guardian_ids,
                                    threshold,
                                    initiated_at,
                                    ..
                                } if initiator_id != self.own_authority => {
                                    // Another authority asks us to be one of its
                                    // guardians: surface it as a pending approval.
                                    if let Some(ceremony_id) = trace_id
                                        .filter(|_| guardian_ids.contains(&self.own_authority))
                                    {
                                        let id = aura_core::types::identifiers::CeremonyId::new(
                                            ceremony_id,
                                        );
                                        let requests = state.pending_requests_mut();
                                        if !requests.iter().any(|request| request.id == id) {
                                            requests.push(RecoveryProcess {
                                                id,
                                                account_id: initiator_id,
                                                status: RecoveryProcessStatus::WaitingForApprovals,
                                                approvals_received: 0,
                                                approvals_required: u32::from(threshold),
                                                approved_by: Vec::new(),
                                                approvals: Vec::new(),
                                                initiated_at: initiated_at.ts_ms,
                                                expires_at: None,
                                                progress: 0,
                                            });
                                            changed = true;
                                        }
                                    }
                                }
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
                                RecoveryFact::GuardianAccepted {
                                    guardian_id,
                                    trace_id: Some(ceremony_id),
                                    ..
                                }
                                | RecoveryFact::GuardianDeclined {
                                    guardian_id,
                                    trace_id: Some(ceremony_id),
                                    ..
                                } if guardian_id == self.own_authority => {
                                    // Our response resolves the pending request.
                                    let requests = state.pending_requests_mut();
                                    let before = requests.len();
                                    requests.retain(|request| request.id.as_str() != ceremony_id);
                                    changed |= requests.len() != before;
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

                match owner
                    .replace_if_current(ProjectionSlot::recovery(), current.revision, snapshot)
                    .await
                {
                    Ok(ConditionalEmit::Published { .. }) => return,
                    Ok(ConditionalEmit::Stale { .. }) => continue,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to publish RECOVERY_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
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
    pending_memberships: Mutex<Vec<SocialFact>>,
}

impl HomeSignalView {
    pub fn new(own_authority: AuthorityId, reactive: ReactiveHandler) -> Self {
        Self {
            own_authority,
            reactive,
            pending_memberships: Mutex::new(Vec::new()),
        }
    }

    /// The materialized home for a context. Homes exist only once their
    /// `SocialFact::HomeCreated` is reduced; facts for an unknown context are
    /// not given a fabricated placeholder home.
    fn home_for_context_mut<'a>(
        homes: &'a mut HomesState,
        context_id: &ContextId,
    ) -> Option<&'a mut HomeState> {
        let home_id = homes.iter().find_map(|(home_id, home)| {
            (home.context_id == Some(*context_id)).then_some(*home_id)
        })?;
        homes.home_mut(&home_id)
    }

    /// Applies a social fact that creates a home or changes its membership.
    fn materialize_created_home(
        &self,
        homes: &mut HomesState,
        witness: HomeCreationWitness,
    ) -> bool {
        let first_home = homes.is_empty();
        let before = homes.iter().count();
        let result = homes.materialize_created_home(witness, self.own_authority);
        if homes.iter().count() == before {
            return false;
        }
        tracing::info!(home_id = %result.home_id, "materialized home from HomeCreated fact");
        if first_home {
            homes.select_home(Some(result.home_id));
        }
        true
    }

    fn apply_member_joined(homes: &mut HomesState, fact: &SocialFact) -> Option<bool> {
        let SocialFact::MemberJoined {
            authority_id,
            home_id,
            context_id,
            joined_at,
            name,
            storage_allocated,
        } = fact
        else {
            return Some(false);
        };
        let channel_id = ChannelId::from_bytes(*home_id.as_bytes());
        let home = homes.home_mut(&channel_id)?;
        if home.context_id != Some(*context_id) {
            return Some(false);
        }
        if home.member(authority_id).is_some() {
            return Some(false);
        }
        home.add_member(HomeMember {
            id: *authority_id,
            name: name.clone(),
            role: HomeRole::Participant,
            is_online: false,
            joined_at: joined_at.ts_ms,
            last_seen: Some(joined_at.ts_ms),
            storage_allocated: *storage_allocated,
        });
        Some(true)
    }
}

impl ReactiveView for HomeSignalView {
    fn update<'a>(&'a self, facts: &'a [Fact]) -> ReactiveUpdateFuture<'a> {
        Box::pin(async move {
            let owner = ProjectionOwner::new(self.reactive.clone());
            // Keep unresolved joins across scheduler batches. The lock also
            // serializes this view's retries while other projection owners
            // may publish to the same signal.
            let mut pending = self.pending_memberships.lock().await;
            loop {
                let current = match owner.snapshot(ProjectionSlot::homes()).await {
                    Ok(current) => current,
                    Err(e) => {
                        tracing::warn!(error = %e, facts = facts.len(), "home view could not read HOMES_SIGNAL; facts not applied");
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to read HOMES_SIGNAL: {e}"),
                        )
                        .await;
                        return;
                    }
                };
                let mut homes = current.value;

                let mut changed = false;

                // Creation is reduced first even when journal replay presents
                // membership before creation in the same batch.
                for fact in facts {
                    let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                        &fact.content
                    else {
                        continue;
                    };
                    if envelope.type_id.as_str() != SOCIAL_FACT_TYPE_ID {
                        continue;
                    }
                    if let Some(witness) = SocialFact::from_envelope(envelope)
                        .as_ref()
                        .and_then(|fact| owner.home_created_witness(fact))
                    {
                        changed |= self.materialize_created_home(&mut homes, witness);
                    }
                }

                let mut unresolved = Vec::new();
                for join in pending
                    .iter()
                    .cloned()
                    .chain(facts.iter().filter_map(|fact| {
                        let FactContent::Relational(RelationalFact::Generic { envelope, .. }) =
                            &fact.content
                        else {
                            return None;
                        };
                        (envelope.type_id.as_str() == SOCIAL_FACT_TYPE_ID)
                            .then(|| SocialFact::from_envelope(envelope))
                            .flatten()
                            .filter(|social| matches!(social, SocialFact::MemberJoined { .. }))
                    }))
                {
                    match Self::apply_member_joined(&mut homes, &join) {
                        Some(applied) => changed |= applied,
                        None if !unresolved.contains(&join) => unresolved.push(join),
                        None => {}
                    }
                }

                for fact in facts {
                    let FactContent::Relational(RelationalFact::Generic {
                        context_id,
                        envelope,
                    }) = &fact.content
                    else {
                        continue;
                    };

                    if envelope.type_id.as_str() == SOCIAL_FACT_TYPE_ID {
                        continue;
                    }
                    let Some(home_state) = Self::home_for_context_mut(&mut homes, context_id)
                    else {
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
                                if let Some(member) = home_state.member_mut(&grant.target_authority)
                                {
                                    if matches!(member.role, HomeRole::Member | HomeRole::Moderator)
                                    {
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
                                if let Some(member) =
                                    home_state.member_mut(&revoke.target_authority)
                                {
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
                    *pending = unresolved;
                    return;
                }

                match owner
                    .replace_if_current(ProjectionSlot::homes(), current.revision, homes)
                    .await
                {
                    Ok(ConditionalEmit::Published { .. }) => {
                        *pending = unresolved;
                        return;
                    }
                    Ok(ConditionalEmit::Stale { .. }) => continue,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to publish HOMES_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
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
    update_gate: Mutex<()>,
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
            update_gate: Mutex::new(()),
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

    async fn sender_allowed_via_channel_invitation(
        &self,
        channel_id: ChannelId,
        sender_id: AuthorityId,
    ) -> bool {
        let invitations = match self.reactive.read(&*INVITATIONS_SIGNAL).await {
            Ok(invitations) => invitations,
            Err(_) => return false,
        };

        invitations.all_sent().iter().any(|invitation| {
            invitation.to_id == Some(sender_id)
                && invitation.home_id == Some(channel_id)
                && (invitation.invitation_type
                    == aura_app::views::invitations::InvitationType::Chat
                    || invitation.home_id.is_some())
                && matches!(
                    invitation.status,
                    InvitationStatus::Pending | InvitationStatus::Accepted
                )
        }) || invitations.all_history().iter().any(|invitation| {
            invitation.direction == InvitationDirection::Sent
                && invitation.to_id == Some(sender_id)
                && invitation.home_id == Some(channel_id)
                && (invitation.invitation_type
                    == aura_app::views::invitations::InvitationType::Chat
                    || invitation.home_id.is_some())
                && invitation.status == InvitationStatus::Accepted
        })
    }

    async fn sender_allowed_for_context(
        &self,
        context_id: ContextId,
        channel_id: ChannelId,
        sender_id: AuthorityId,
        sent_at_ms: u64,
        // Sender is a recorded member of the channel (e.g. its creator).
        known_channel_member: bool,
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
            return known_channel_member
                || self
                    .sender_allowed_via_channel_invitation(channel_id, sender_id)
                    .await;
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
            return known_channel_member
                || self
                    .sender_allowed_via_channel_invitation(channel_id, sender_id)
                    .await;
        }
        if !sender_is_member {
            if self
                .sender_allowed_via_channel_invitation(channel_id, sender_id)
                .await
            {
                return true;
            }
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
            let _update_gate = self.update_gate.lock().await;
            let owner = ProjectionOwner::new(self.reactive.clone());
            loop {
                let source = match owner.snapshot(ProjectionSlot::chat()).await {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to read CHAT_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                };
                let mut state = self.state.lock().await;
                *state = source.value;
                let mut changed = false;

                // Apply channel creation before messages in the same batch (journal
                // replay after a restart), so senders are known channel members.
                let mut ordered: Vec<&Fact> = facts.iter().collect();
                ordered.sort_by_key(|fact| u8::from(!is_chat_channel_created(fact)));
                for fact in ordered {
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

                            let canonical_creation = ChatViewReducer
                                .reduce_fact(CHAT_FACT_TYPE_ID, &envelope.payload, None)
                                .into_iter()
                                .filter_map(downcast_delta_owned::<ChatDelta>)
                                .find_map(|delta| match delta {
                                    ChatDelta::ChannelAdded(creation) => Some(creation),
                                    _ => None,
                                });
                            match chat_fact {
                                ChatFact::ChannelCreated {
                                    channel_id,
                                    context_id,
                                    name: _,
                                    topic: _,
                                    is_dm,
                                    created_at: _,
                                    creator_id,
                                    ..
                                } => {
                                    let creation = canonical_creation.expect(
                                        "ChannelCreated carries canonical creation evidence",
                                    );
                                    tracing::debug!(
                                        channel_id = %channel_id,
                                        %creator_id,
                                        is_dm,
                                        "ChatSignalView: ChannelCreated"
                                    );
                                    let hidden_after_leave = {
                                        self.hidden_channels_after_leave
                                            .lock()
                                            .await
                                            .contains(&channel_id)
                                    };
                                    if hidden_after_leave {
                                        tracing::debug!(channel_id = %channel_id, "ChatSignalView: channel hidden after leave");
                                        continue;
                                    }

                                    drop(state);
                                    self.ensure_amp_channel_state(
                                        context_id, channel_id, creator_id,
                                    )
                                    .await;
                                    state = self.state.lock().await;

                                    state.materialize_canonical_channel(
                                        creation,
                                        Some(self.own_authority),
                                    );
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
                                    // A channel we left stays gone; updates must not re-create it.
                                    if self
                                        .hidden_channels_after_leave
                                        .lock()
                                        .await
                                        .contains(&channel_id)
                                    {
                                        continue;
                                    }
                                    state.apply_or_stage_channel_update(
                                        channel_id,
                                        aura_app::views::chat::ChannelProjectionUpdate {
                                            context_id: Some(context_id),
                                            name,
                                            topic,
                                            member_count,
                                            member_ids,
                                            updated_at: updated_at.ts_ms,
                                        },
                                    );
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
                                    let known_member =
                                        state.channel(&channel_id).is_some_and(|channel| {
                                            channel.member_ids.contains(&sender_id)
                                        });
                                    drop(state);
                                    if !self
                                        .sender_allowed_for_context(
                                            context,
                                            channel_id,
                                            sender_id,
                                            sent_at.ts_ms,
                                            known_member,
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
                                        String::from_utf8(payload_bytes.clone()).unwrap_or_else(
                                            |_| format!("[sealed: {} bytes]", sealed_len),
                                        )
                                    } else {
                                        match amp_open_committed(
                                            self.effects.as_ref(),
                                            context,
                                            sender_id,
                                            payload_bytes,
                                        )
                                        .await
                                        {
                                            Ok(msg) => String::from_utf8(msg.payload)
                                                .unwrap_or_else(|_| {
                                                    format!("[sealed: {} bytes]", sealed_len)
                                                }),
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
                                    let new_content =
                                        String::from_utf8_lossy(&new_payload).to_string();
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

                match owner
                    .replace_if_current(ProjectionSlot::chat(), source.revision, snapshot)
                    .await
                {
                    Ok(ConditionalEmit::Published { .. }) => return,
                    Ok(ConditionalEmit::Stale { .. }) => continue,
                    Err(error) => {
                        emit_internal_error(
                            &self.reactive,
                            format!("Failed to publish CHAT_SIGNAL: {error}"),
                        )
                        .await;
                        return;
                    }
                }
            }
        })
    }

    fn view_id(&self) -> &str {
        "signals:chat"
    }
}

/// Whether a fact creates a chat channel.
fn is_chat_channel_created(fact: &Fact) -> bool {
    let FactContent::Relational(RelationalFact::Generic { envelope, .. }) = &fact.content else {
        return false;
    };
    envelope.type_id.as_str() == CHAT_FACT_TYPE_ID
        && matches!(
            ChatFact::from_envelope(envelope),
            Some(ChatFact::ChannelCreated { .. })
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::effects::AuraEffectSystem;
    use crate::AgentConfig;
    use aura_app::signal_defs::{
        register_app_signals, CHAT_SIGNAL, CONTACTS_SIGNAL, HOMES_SIGNAL, INVITATIONS_SIGNAL,
        RECOVERY_SIGNAL,
    };
    use aura_app::views::chat::ChatState;
    use aura_core::effects::reactive::ReactiveEffects;

    fn add_fixture_home(
        homes: &mut HomesState,
        home: HomeState,
    ) -> aura_app::views::home::AddHomeResult {
        let home_id = home.id;
        let was_first = homes.is_empty();
        let mut detached = serde_json::to_value(&*homes).unwrap();
        detached["homes"]
            .as_object_mut()
            .unwrap()
            .insert(home_id.to_string(), serde_json::to_value(home).unwrap());
        *homes = serde_json::from_value(detached).unwrap();
        aura_app::views::home::AddHomeResult { home_id, was_first }
    }
    use aura_core::time::{OrderTime, PhysicalTime, TimeStamp};
    use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
    use aura_journal::fact::{Fact, FactContent, RelationalFact};
    use aura_relational::{ContactFact, FriendshipFact};
    use aura_social::moderation::facts::{
        HomeGrantModeratorFact, HomePinFact, HomeRevokeModeratorFact, HomeUnpinFact,
    };
    use aura_social::moderation::HomeBanFact;

    #[test]
    fn runtime_projection_publications_use_the_versioned_owner() {
        let production = include_str!("app_signal_views.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("production section");
        for signal in [
            "CHAT_SIGNAL",
            "CONTACTS_SIGNAL",
            "HOMES_SIGNAL",
            "INVITATIONS_SIGNAL",
            "RECOVERY_SIGNAL",
        ] {
            assert!(
                !production.contains(&format!(".emit(&*{signal}")),
                "{signal} bypasses ProjectionOwner"
            );
        }
    }

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
        let result = add_fixture_home(&mut homes, home_state);
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
    fn canonical_entity_creation_stays_in_owned_publication_paths() {
        fn has_forbidden_creation_bypass(source: &str) -> bool {
            let production = source.split("\nmod tests {").next().expect("source prefix");
            production.contains("ChatState::from_channels(")
                || production.contains("ContactsState::from_contacts(")
                || production.contains("InvitationsState::from_parts(")
                || production.contains("ContactAddedWitness::from_fact(")
                || production.contains("InvitationCreationWitness::from_sent_fact(")
        }

        fn check_tree(root: &std::path::Path) {
            for entry in std::fs::read_dir(root).expect("publication source directory") {
                let path = entry.expect("publication source entry").path();
                if path.is_dir() {
                    check_tree(&path);
                } else if path.extension().is_some_and(|extension| extension == "rs")
                    && path
                        .file_name()
                        .map(|name| name != "tests.rs")
                        .unwrap_or(true)
                {
                    let source = std::fs::read_to_string(&path).expect("publication source");
                    assert!(
                        !has_forbidden_creation_bypass(&source),
                        "entity creation must use owned witnesses; raw hydration is reserved for typed query decoding and tests: {}",
                        path.display()
                    );
                }
            }
        }

        assert!(has_forbidden_creation_bypass(
            "fn publish() { ChatState::from_channels(rows); }"
        ));
        assert!(has_forbidden_creation_bypass(
            "fn publish() { ContactsState::from_contacts(rows); }"
        ));
        assert!(has_forbidden_creation_bypass(
            "fn publish() { InvitationsState::from_parts(pending, sent, history); }"
        ));
        assert!(has_forbidden_creation_bypass(
            "fn publish() { ContactAddedWitness::from_fact(&fact); }"
        ));
        assert!(has_forbidden_creation_bypass(
            "fn publish() { InvitationCreationWitness::from_sent_fact(&fact, own); }"
        ));
        assert!(!has_forbidden_creation_bypass(
            "fn publish() {}\nmod tests { ContactsState::from_contacts(rows); }"
        ));

        let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for relative in [
            "crates/aura-agent/src/reactive",
            "crates/aura-app/src/runtime_bridge",
            "crates/aura-app/src/workflows",
            "crates/aura-ui/src/app/runtime_views",
            "crates/aura-terminal/src/tui",
            "crates/aura-web/src",
        ] {
            check_tree(&workspace.join(relative));
        }
    }

    async fn materialize_test_invitation(
        reactive: &ReactiveHandler,
        own_authority: AuthorityId,
        invitation_id: &str,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        invitation_type: &DomainInvitationType,
        receiver_nickname: Option<&str>,
        created_at: u64,
        expires_at: Option<u64>,
        message: Option<String>,
    ) -> Result<(), String> {
        let sent = InvitationFact::Sent {
            invitation_id: aura_core::types::identifiers::InvitationId::new(invitation_id),
            context_id: ContextId::new_from_entropy([0x91; 32]),
            sender_id,
            receiver_id,
            invitation_type: invitation_type.clone(),
            sent_at: PhysicalTime {
                ts_ms: created_at,
                uncertainty: None,
            },
            expires_at: expires_at.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
            message,
            receiver_nickname: receiver_nickname.map(ToOwned::to_owned),
        };
        materialize_pending_invitation_witness(
            reactive,
            ProjectionOwner::new(reactive.clone())
                .invitation_sent_witness(&sent, own_authority)
                .expect("sent invitation has creation evidence"),
        )
        .await
    }

    #[test]
    fn invitation_creation_witness_rejects_status_only_evidence() {
        let authority = AuthorityId::new_from_entropy([0x81; 32]);
        let invitation_id = aura_core::types::identifiers::InvitationId::new("status-only");
        let at = PhysicalTime {
            ts_ms: 1,
            uncertainty: None,
        };
        let accepted = InvitationFact::Accepted {
            context_id: None,
            invitation_id: invitation_id.clone(),
            acceptor_id: authority,
            accepted_at: at,
        };
        let reactive = ReactiveHandler::new();
        assert!(ProjectionOwner::new(reactive)
            .invitation_sent_witness(&accepted, authority)
            .is_none());
    }

    #[tokio::test]
    async fn invitation_status_before_sent_replays_without_pending_phantom() {
        let own = AuthorityId::new_from_entropy([0x84; 32]);
        let sender = AuthorityId::new_from_entropy([0x85; 32]);
        let context = ContextId::new_from_entropy([0x86; 32]);
        let invitation_id = aura_core::types::identifiers::InvitationId::new("early-acceptance");
        let sent = fact_from_relational(
            InvitationFact::sent_ms(
                context,
                invitation_id.clone(),
                sender,
                own,
                DomainInvitationType::Contact { nickname: None },
                1,
                None,
                None,
            )
            .to_generic(),
        );
        let accepted = fact_from_relational(
            InvitationFact::Accepted {
                context_id: Some(context),
                invitation_id,
                acceptor_id: own,
                accepted_at: PhysicalTime {
                    ts_ms: 2,
                    uncertainty: None,
                },
            }
            .to_generic(),
        );

        // The second pass simulates a fresh projection rebuilding from facts.
        for _ in 0..2 {
            let reactive = ReactiveHandler::new();
            register_app_signals(&reactive).await.unwrap();
            let view = InvitationsSignalView::new(own, reactive.clone());
            view.update(std::slice::from_ref(&accepted)).await;
            let before = reactive.read(&*INVITATIONS_SIGNAL).await.unwrap();
            assert!(before.invitation("early-acceptance").is_none());

            view.update(std::slice::from_ref(&sent)).await;
            let after = reactive.read(&*INVITATIONS_SIGNAL).await.unwrap();
            assert_eq!(
                after
                    .invitation("early-acceptance")
                    .map(|invite| invite.status),
                Some(InvitationStatus::Accepted)
            );
            assert_eq!(after.open_invitations().count(), 0);
        }
    }

    async fn assert_terminal_status_before_sent_converges(
        invitation_id: &str,
        terminal_fact: InvitationFact,
        expected_status: InvitationStatus,
    ) {
        let own = AuthorityId::new_from_entropy([0x88; 32]);
        let sender = AuthorityId::new_from_entropy([0x89; 32]);
        let context = ContextId::new_from_entropy([0x8a; 32]);
        let sent = fact_from_relational(
            InvitationFact::sent_ms(
                context,
                aura_core::types::identifiers::InvitationId::new(invitation_id),
                sender,
                own,
                DomainInvitationType::Contact { nickname: None },
                1,
                None,
                None,
            )
            .to_generic(),
        );
        let terminal = fact_from_relational(terminal_fact.to_generic());

        // First deliver the terminal fact alone. Then rebuild a separate view
        // from the same out-of-order fact sequence to cover fresh replay.
        for fresh_replay in [false, true] {
            let reactive = ReactiveHandler::new();
            register_app_signals(&reactive).await.unwrap();
            let view = InvitationsSignalView::new(own, reactive.clone());
            if fresh_replay {
                view.update(&[terminal.clone(), sent.clone()]).await;
            } else {
                view.update(std::slice::from_ref(&terminal)).await;
                let before = reactive.read(&*INVITATIONS_SIGNAL).await.unwrap();
                assert!(before.invitation(invitation_id).is_none());
                assert_eq!(before.pending_count(), 0);
                view.update(std::slice::from_ref(&sent)).await;
            }

            let settled = reactive.read(&*INVITATIONS_SIGNAL).await.unwrap();
            assert_eq!(
                settled
                    .invitation(invitation_id)
                    .map(|invite| invite.status),
                Some(expected_status)
            );
            assert_eq!(settled.pending_count(), 0);
            assert_eq!(settled.open_invitations().count(), 0);
            assert_eq!(settled.history_count(), 1);

            // A duplicate creation fact cannot resurrect the settled row.
            view.update(std::slice::from_ref(&sent)).await;
            let replayed = reactive.read(&*INVITATIONS_SIGNAL).await.unwrap();
            assert_eq!(
                replayed
                    .invitation(invitation_id)
                    .map(|invite| invite.status),
                Some(expected_status)
            );
            assert_eq!(replayed.pending_count(), 0);
        }
    }

    #[tokio::test]
    async fn declined_before_sent_converges_without_pending_phantom_after_fresh_replay() {
        let own = AuthorityId::new_from_entropy([0x88; 32]);
        let context = ContextId::new_from_entropy([0x8a; 32]);
        let invitation_id = "early-decline";
        assert_terminal_status_before_sent_converges(
            invitation_id,
            InvitationFact::Declined {
                context_id: Some(context),
                invitation_id: aura_core::types::identifiers::InvitationId::new(invitation_id),
                decliner_id: own,
                declined_at: PhysicalTime {
                    ts_ms: 2,
                    uncertainty: None,
                },
            },
            InvitationStatus::Rejected,
        )
        .await;
    }

    #[tokio::test]
    async fn cancelled_before_sent_converges_without_pending_phantom_after_fresh_replay() {
        let sender = AuthorityId::new_from_entropy([0x89; 32]);
        let context = ContextId::new_from_entropy([0x8a; 32]);
        let invitation_id = "early-cancellation";
        assert_terminal_status_before_sent_converges(
            invitation_id,
            InvitationFact::Cancelled {
                context_id: Some(context),
                invitation_id: aura_core::types::identifiers::InvitationId::new(invitation_id),
                canceller_id: sender,
                cancelled_at: PhysicalTime {
                    ts_ms: 2,
                    uncertainty: None,
                },
            },
            InvitationStatus::Revoked,
        )
        .await;
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
        add_fixture_home(&mut homes, channel_home);
        add_fixture_home(&mut homes, synthetic_home);

        let selected =
            app_signal_projection::select_moderation_home(&homes, context_id, channel_home_id)
                .expect("channel-authoritative home should be selected");
        assert_eq!(selected.id, channel_home_id);
    }

    #[tokio::test]
    async fn guardian_setup_request_from_another_authority_is_pending_until_answered() {
        use aura_journal::DomainFact as _;
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([91u8; 32]);
        let initiator = AuthorityId::new_from_entropy([92u8; 32]);
        let other_guardian = AuthorityId::new_from_entropy([93u8; 32]);
        let context_id = ContextId::new_from_entropy([94u8; 32]);
        let view = RecoverySignalView::new(own_authority, reactive.clone());
        let at = PhysicalTime {
            ts_ms: 10,
            uncertainty: None,
        };

        let request = RecoveryFact::GuardianSetupInitiated {
            context_id,
            initiator_id: initiator,
            trace_id: Some("ceremony-1".to_string()),
            guardian_ids: vec![own_authority, other_guardian],
            threshold: 2,
            initiated_at: at.clone(),
        };
        view.update(&[fact_from_relational(request.to_generic())])
            .await;
        let state = reactive.read(&*RECOVERY_SIGNAL).await.unwrap();
        assert_eq!(state.pending_requests().len(), 1);
        assert_eq!(state.pending_requests()[0].account_id, initiator);
        assert_eq!(state.pending_requests()[0].approvals_required, 2);
        assert!(
            state.all_guardians().next().is_none(),
            "another authority's setup must not change our own guardians"
        );

        let accepted = RecoveryFact::GuardianAccepted {
            context_id,
            guardian_id: own_authority,
            trace_id: Some("ceremony-1".to_string()),
            accepted_at: at,
        };
        view.update(&[fact_from_relational(accepted.to_generic())])
            .await;
        let state = reactive.read(&*RECOVERY_SIGNAL).await.unwrap();
        assert!(state.pending_requests().is_empty());
    }

    #[tokio::test]
    async fn generic_sent_contact_invitation_hides_receiver_identity() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();

        let own_authority = AuthorityId::new_from_entropy([81u8; 32]);
        materialize_test_invitation(
            &reactive,
            own_authority,
            "generic-contact-invite",
            own_authority,
            own_authority,
            &DomainInvitationType::Contact {
                nickname: Some("friend".to_string()),
            },
            None,
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

    #[tokio::test]
    async fn concurrent_home_materializers_preserve_both_entities_and_revisions() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own = AuthorityId::new_from_entropy([61u8; 32]);
        let sender = AuthorityId::new_from_entropy([62u8; 32]);
        let first = ChannelId::from_bytes([63u8; 32]);
        let second = ChannelId::from_bytes([64u8; 32]);
        let before = ProjectionOwner::new(reactive.clone())
            .snapshot(ProjectionSlot::homes())
            .await
            .unwrap();

        let evidence = |channel_id: ChannelId, context_id: ContextId, name: &str, now_ms| {
            let verified = VerifiedJoinedHome {
                own_authority: own,
                channel_id,
                name: name.into(),
                sender_id: sender,
                context_id,
                now_ms,
            };
            let created = SocialFact::home_created_ms(
                aura_social::HomeId::from_bytes(*channel_id.as_bytes()),
                context_id,
                now_ms,
                sender,
                name.into(),
            );
            verified
                .bind_committed_creation(&ProjectionOwner::new(reactive.clone()), &created)
                .unwrap()
        };

        let (a, b) = tokio::join!(
            materialize_home_signal_for_channel_invitation(
                &reactive,
                evidence(first, ContextId::new_from_entropy([65u8; 32]), "First", 1),
            ),
            materialize_home_signal_for_channel_invitation(
                &reactive,
                evidence(second, ContextId::new_from_entropy([66u8; 32]), "Second", 2),
            )
        );
        a.unwrap();
        b.unwrap();

        let after = ProjectionOwner::new(reactive)
            .snapshot(ProjectionSlot::homes())
            .await
            .unwrap();
        assert!(after.value.has_home(&first));
        assert!(after.value.has_home(&second));
        assert_eq!(after.revision, before.revision + 2);
    }

    #[tokio::test]
    async fn replayed_sent_fact_does_not_resurrect_accepted_invitation() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own = AuthorityId::new_from_entropy([71u8; 32]);
        let peer = AuthorityId::new_from_entropy([72u8; 32]);
        let invitation_id = aura_core::types::identifiers::InvitationId::new("accepted-invite");
        let invitation_type = DomainInvitationType::Contact { nickname: None };
        materialize_test_invitation(
            &reactive,
            own,
            invitation_id.as_str(),
            peer,
            own,
            &invitation_type,
            None,
            1,
            None,
            None,
        )
        .await
        .unwrap();
        let owner = ProjectionOwner::new(reactive.clone());
        owner
            .update(ProjectionSlot::invitations(), |state| -> Result<(), ()> {
                state.accept_invitation(invitation_id.as_str()).unwrap();
                Ok(())
            })
            .await
            .unwrap()
            .unwrap();
        let before = owner.snapshot(ProjectionSlot::invitations()).await.unwrap();

        let fact = InvitationFact::sent_ms(
            ContextId::new_from_entropy([73u8; 32]),
            invitation_id,
            peer,
            own,
            invitation_type,
            1,
            None,
            None,
        );
        InvitationsSignalView::new(own, reactive)
            .update(&[fact_from_relational(fact.to_generic())])
            .await;

        let after = owner.snapshot(ProjectionSlot::invitations()).await.unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.value.open_invitations().count(), 0);
        assert_eq!(
            after.value.invitation("accepted-invite").unwrap().status,
            InvitationStatus::Accepted
        );
    }

    #[tokio::test]
    async fn generic_sent_contact_invitation_preserves_sender_local_receiver_nickname() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();

        let own_authority = AuthorityId::new_from_entropy([82u8; 32]);
        materialize_test_invitation(
            &reactive,
            own_authority,
            "generic-contact-invite-labeled",
            own_authority,
            own_authority,
            &DomainInvitationType::Contact {
                nickname: Some("friend".to_string()),
            },
            Some("Bob from cafe"),
            1234,
            Some(5678),
            Some("share this code".to_string()),
        )
        .await
        .expect("materialize labeled generic sent contact invitation");

        let invitations = reactive
            .read(&*INVITATIONS_SIGNAL)
            .await
            .expect("invitation signal should be registered");
        let invitation = invitations
            .invitation("generic-contact-invite-labeled")
            .expect("generic invitation should be present");

        assert_eq!(invitation.to_id, None);
        assert_eq!(invitation.to_name.as_deref(), Some("Bob from cafe"));
    }

    #[test]
    fn select_moderation_home_rejects_ambiguous_context() {
        let context_id = ContextId::new_from_entropy([12u8; 32]);
        let owner = AuthorityId::new_from_entropy([1u8; 32]);

        let home_a_id = ChannelId::from_bytes([31u8; 32]);
        let home_b_id = ChannelId::from_bytes([32u8; 32]);
        let unknown_channel_id = ChannelId::from_bytes([99u8; 32]);

        let mut homes = HomesState::new();
        add_fixture_home(
            &mut homes,
            HomeState::new(home_a_id, Some("home-a".to_string()), owner, 0, context_id),
        );
        add_fixture_home(
            &mut homes,
            HomeState::new(home_b_id, Some("home-b".to_string()), owner, 0, context_id),
        );

        let selected =
            app_signal_projection::select_moderation_home(&homes, context_id, unknown_channel_id);
        assert!(
            selected.is_none(),
            "ambiguous context without channel-authoritative home should be rejected"
        );
    }

    #[tokio::test]
    async fn replayed_message_before_channel_creation_is_kept() {
        use aura_journal::DomainFact as _;
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([61u8; 32]);
        let creator = AuthorityId::new_from_entropy([62u8; 32]);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(
                &AgentConfig::default(),
                own_authority,
            )
            .unwrap(),
        );
        let view = ChatSignalView::new(own_authority, reactive.clone(), effects);
        let context_id = ContextId::new_from_entropy([63u8; 32]);
        let channel_id = ChannelId::from_bytes([64u8; 32]);
        let message = ChatFact::message_sent_sealed_ms(
            context_id,
            channel_id,
            "m1".to_string(),
            creator,
            "Creator".to_string(),
            vec![1, 2, 3],
            20,
            None,
            None,
        );
        let created = ChatFact::channel_created_ms(
            context_id,
            channel_id,
            "DM: Creator".to_string(),
            None,
            true,
            10,
            creator,
        );
        // Journal replay may deliver the message ahead of its channel.
        view.update(&[
            fact_from_relational(message.to_generic()),
            fact_from_relational(created.to_generic()),
        ])
        .await;
        let chat = reactive.read(&*CHAT_SIGNAL).await.unwrap();
        assert_eq!(chat.messages_for_channel(&channel_id).len(), 1);
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
                false,
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
        add_fixture_home(
            &mut homes,
            HomeState::new(
                ChannelId::from_bytes([42u8; 32]),
                Some("home-a".to_string()),
                own_authority,
                0,
                context_id,
            ),
        );
        add_fixture_home(
            &mut homes,
            HomeState::new(
                ChannelId::from_bytes([43u8; 32]),
                Some("home-b".to_string()),
                own_authority,
                0,
                context_id,
            ),
        );
        reactive.emit(&*HOMES_SIGNAL, homes).await.unwrap();

        let allowed = view
            .sender_allowed_for_context(
                context_id,
                ChannelId::from_bytes([44u8; 32]),
                sender_id,
                1_700_000_000_001,
                false,
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
    async fn home_signal_view_materializes_homes_only_from_home_created_facts() {
        let reactive = ReactiveHandler::new();
        let known_context = ContextId::new_from_entropy([2u8; 32]);
        let new_context = ContextId::new_from_entropy([4u8; 32]);
        let actor = AuthorityId::new_from_entropy([1u8; 32]);
        let target = AuthorityId::new_from_entropy([9u8; 32]);
        let _ = setup_homes(&reactive, known_context).await;
        let view = HomeSignalView::new(actor, reactive.clone());
        let home_count = |homes: &HomesState| homes.iter().count();
        let before = home_count(&reactive.read(&*HOMES_SIGNAL).await.unwrap());

        // A moderation fact for a context with no home does not fabricate one.
        let mute = HomeMuteFact::new_ms(
            new_context,
            None,
            target,
            actor,
            Some(60),
            100,
            Some(160_000),
        )
        .to_generic();
        view.update(&[fact_from_relational(mute.clone())]).await;
        let homes = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        assert_eq!(home_count(&homes), before);
        assert!(homes
            .iter()
            .all(|(_, home)| home.context_id != Some(new_context)));

        // HomeCreated and MemberJoined materialize the home and its member.
        let home_id = aura_social::HomeId::from_bytes([44u8; 32]);
        let created =
            SocialFact::home_created_ms(home_id, new_context, 50, actor, "Den".to_string())
                .to_generic();
        let joined =
            SocialFact::member_joined_ms(target, home_id, new_context, 60, "Bob".to_string())
                .to_generic();
        view.update(&[fact_from_relational(created), fact_from_relational(joined)])
            .await;
        let homes = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home = homes
            .home_state(&ChannelId::from_bytes([44u8; 32]))
            .expect("HomeCreated should materialize the home");
        assert_eq!(home.name, "Den");
        assert_eq!(home.context_id, Some(new_context));
        assert!(home.member(&target).is_some());

        // Moderation now applies to the materialized home.
        view.update(&[fact_from_relational(mute)]).await;
        let homes = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home = homes
            .home_state(&ChannelId::from_bytes([44u8; 32]))
            .unwrap();
        assert!(home.mute_list.contains_key(&target));
    }

    #[tokio::test]
    async fn home_join_before_creation_is_replayed_once_and_bound_to_home_and_context() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own = AuthorityId::new_from_entropy([81u8; 32]);
        let creator = AuthorityId::new_from_entropy([82u8; 32]);
        let member = AuthorityId::new_from_entropy([83u8; 32]);
        let context = ContextId::new_from_entropy([84u8; 32]);
        let wrong_context = ContextId::new_from_entropy([85u8; 32]);
        let home_id = aura_social::HomeId::from_bytes([86u8; 32]);
        let other_home_id = aura_social::HomeId::from_bytes([87u8; 32]);
        let view = HomeSignalView::new(own, reactive.clone());

        let joined = SocialFact::member_joined_ms(member, home_id, context, 20, "Member".into())
            .to_generic();
        let wrong_home = SocialFact::member_joined_ms(
            AuthorityId::new_from_entropy([88u8; 32]),
            other_home_id,
            context,
            21,
            "Other".into(),
        )
        .to_generic();
        let wrong_context_join = SocialFact::member_joined_ms(
            AuthorityId::new_from_entropy([89u8; 32]),
            home_id,
            wrong_context,
            22,
            "Wrong".into(),
        )
        .to_generic();
        view.update(&[
            fact_from_relational(joined.clone()),
            fact_from_relational(wrong_home.clone()),
            fact_from_relational(wrong_context_join.clone()),
        ])
        .await;
        assert!(reactive.read(&*HOMES_SIGNAL).await.unwrap().is_empty());

        let created =
            SocialFact::home_created_ms(home_id, context, 10, creator, "Den".into()).to_generic();
        view.update(&[fact_from_relational(created.clone())]).await;
        let homes = reactive.read(&*HOMES_SIGNAL).await.unwrap();
        let home = homes
            .home_state(&ChannelId::from_bytes([86u8; 32]))
            .unwrap();
        assert_eq!(home.member_count, 2);
        assert_eq!(home.members.len(), 2);
        assert_eq!(home.online_count, 0);
        assert_eq!(home.my_role, HomeRole::Participant);
        assert!(home.member(&member).is_some());
        assert!(home
            .member(&AuthorityId::new_from_entropy([88u8; 32]))
            .is_none());
        assert!(home
            .member(&AuthorityId::new_from_entropy([89u8; 32]))
            .is_none());

        // Duplicate delivery does not inflate counts.
        view.update(&[fact_from_relational(joined.clone())]).await;
        let restarted_reactive = ReactiveHandler::new();
        register_app_signals(&restarted_reactive).await.unwrap();
        let restarted = HomeSignalView::new(own, restarted_reactive.clone());
        restarted
            .update(&[
                fact_from_relational(joined),
                fact_from_relational(wrong_home),
                fact_from_relational(wrong_context_join),
                fact_from_relational(created),
            ])
            .await;
        let home = restarted_reactive
            .read(&*HOMES_SIGNAL)
            .await
            .unwrap()
            .home_state(&ChannelId::from_bytes([86u8; 32]))
            .unwrap()
            .clone();
        assert_eq!(home.member_count, 2);
        assert_eq!(home.members.len(), 2);
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
    async fn chat_signal_view_stages_named_update_until_creation_and_keeps_newer_metadata() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own = AuthorityId::new_from_entropy([111u8; 32]);
        let effects = Arc::new(
            AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), own)
                .unwrap(),
        );
        let view = ChatSignalView::new(own, reactive.clone(), effects);
        let context = ContextId::new_from_entropy([112u8; 32]);
        let channel_id = ChannelId::from_bytes([113u8; 32]);
        let update = ChatFact::channel_updated_ms(
            context,
            channel_id,
            Some("current".to_string()),
            Some("topic".to_string()),
            Some(3),
            None,
            30,
            own,
        )
        .to_generic();
        let creation = ChatFact::channel_created_ms(
            context,
            channel_id,
            "initial".to_string(),
            None,
            false,
            10,
            own,
        )
        .to_generic();

        view.update(&[fact_from_relational(update.clone())]).await;
        assert!(reactive
            .read(&*CHAT_SIGNAL)
            .await
            .unwrap()
            .channel(&channel_id)
            .is_none());

        view.update(&[fact_from_relational(creation.clone())]).await;
        let chat = reactive.read(&*CHAT_SIGNAL).await.unwrap();
        let channel = chat
            .channel(&channel_id)
            .expect("creation fact materializes channel");
        assert_eq!(channel.name, "current");
        assert_eq!(channel.topic.as_deref(), Some("topic"));
        assert_eq!(channel.member_count, 3);

        let restarted = ChatSignalView::new(
            own,
            reactive.clone(),
            Arc::new(
                AuraEffectSystem::simulation_for_test_for_authority(&AgentConfig::default(), own)
                    .unwrap(),
            ),
        );
        restarted
            .update(&[fact_from_relational(update), fact_from_relational(creation)])
            .await;
        let chat = reactive.read(&*CHAT_SIGNAL).await.unwrap();
        let channel = chat.channel(&channel_id).unwrap();
        assert_eq!(
            channel.name, "current",
            "duplicate replay must not reset later metadata"
        );
        assert_eq!(chat.channel_count(), 1);
    }

    #[tokio::test]
    async fn contacts_signal_view_surfaces_invitation_code_from_contact_added_fact() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([61u8; 32]);
        let peer = AuthorityId::new_from_entropy([62u8; 32]);
        let contact_context = ContextId::new_from_entropy([63u8; 32]);
        let view = ContactsSignalView::new(own_authority, reactive.clone());

        // New contact established with an invitation code: the code must
        // land on the Contact view.
        let contact_added = ContactFact::Added {
            context_id: contact_context,
            owner_id: own_authority,
            contact_id: peer,
            nickname: "Peer".to_string(),
            added_at: PhysicalTime {
                ts_ms: 10,
                uncertainty: None,
            },
            invitation_code: Some("aura:v1:INITIAL".to_string()),
        }
        .to_generic();
        view.update(&[fact_from_relational(contact_added)]).await;

        let contacts = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();
        assert_eq!(
            contacts
                .contact(&peer)
                .and_then(|c| c.invitation_code.clone()),
            Some("aura:v1:INITIAL".to_string())
        );

        // Reissuance: a later Added fact with a different code overwrites
        // the previous code (last-writer-wins on the code field).
        let reissued = ContactFact::Added {
            context_id: contact_context,
            owner_id: own_authority,
            contact_id: peer,
            nickname: "Peer".to_string(),
            added_at: PhysicalTime {
                ts_ms: 20,
                uncertainty: None,
            },
            invitation_code: Some("aura:v1:REISSUED".to_string()),
        }
        .to_generic();
        view.update(&[fact_from_relational(reissued)]).await;

        let contacts = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();
        assert_eq!(
            contacts
                .contact(&peer)
                .and_then(|c| c.invitation_code.clone()),
            Some("aura:v1:REISSUED".to_string())
        );

        // A later fact without a code (e.g. a rename-equivalent) must
        // preserve the previously recorded code.
        let no_code = ContactFact::Added {
            context_id: contact_context,
            owner_id: own_authority,
            contact_id: peer,
            nickname: "Peer".to_string(),
            added_at: PhysicalTime {
                ts_ms: 30,
                uncertainty: None,
            },
            invitation_code: None,
        }
        .to_generic();
        view.update(&[fact_from_relational(no_code)]).await;

        let contacts = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();
        assert_eq!(
            contacts
                .contact(&peer)
                .and_then(|c| c.invitation_code.clone()),
            Some("aura:v1:REISSUED".to_string()),
            "absent invitation_code must preserve previously recorded code"
        );
    }

    #[tokio::test]
    async fn runtime_contact_removal_rejects_delayed_observed_enrichment() {
        let reactive = ReactiveHandler::new();
        register_app_signals(&reactive).await.unwrap();
        let own_authority = AuthorityId::new_from_entropy([81u8; 32]);
        let peer = AuthorityId::new_from_entropy([82u8; 32]);
        let context_id = ContextId::new_from_entropy([83u8; 32]);
        let view = ContactsSignalView::new(own_authority, reactive.clone());
        let owner = ProjectionOwner::new(reactive.clone());

        view.update(&[fact_from_relational(
            ContactFact::Added {
                context_id,
                owner_id: own_authority,
                contact_id: peer,
                nickname: "Peer".to_string(),
                added_at: PhysicalTime {
                    ts_ms: 1,
                    uncertainty: None,
                },
                invitation_code: None,
            }
            .to_generic(),
        )])
        .await;

        owner
            .update(ProjectionSlot::contacts(), |contacts| {
                contacts
                    .contact_mut(&peer)
                    .expect("created contact")
                    .is_online = true;
                Ok::<(), ()>(())
            })
            .await
            .unwrap()
            .unwrap();
        let delayed = owner.snapshot(ProjectionSlot::contacts()).await.unwrap();

        view.update(&[fact_from_relational(
            ContactFact::Removed {
                context_id,
                owner_id: own_authority,
                contact_id: peer,
                removed_at: PhysicalTime {
                    ts_ms: 2,
                    uncertainty: None,
                },
            }
            .to_generic(),
        )])
        .await;

        assert!(matches!(
            owner
                .replace_if_current(ProjectionSlot::contacts(), delayed.revision, delayed.value,)
                .await
                .unwrap(),
            ConditionalEmit::Stale { .. }
        ));
        let current = owner.snapshot(ProjectionSlot::contacts()).await.unwrap();
        assert!(current.value.contact(&peer).is_none());
        assert!(current.revision > delayed.revision);
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
            invitation_code: None,
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
            None,
            "friendship evidence must not create a contact without ContactFact::Added"
        );

        let inbound_added = ContactFact::Added {
            context_id: contact_context,
            owner_id: own_authority,
            contact_id: inbound_peer,
            nickname: "Inbound peer".to_string(),
            added_at: PhysicalTime {
                ts_ms: 15,
                uncertainty: None,
            },
            invitation_code: None,
        }
        .to_generic();
        view.update(&[fact_from_relational(inbound_added)]).await;
        let contacts = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();
        assert_eq!(
            contacts
                .contact(&inbound_peer)
                .map(|contact| contact.relationship_state),
            Some(ContactRelationshipState::PendingInbound),
            "canonical contact creation applies previously observed friendship state"
        );
    }

    #[tokio::test]
    async fn replayed_friendship_waits_for_contact_creation_after_restart() {
        let own = AuthorityId::new_from_entropy([0x61; 32]);
        let peer = AuthorityId::new_from_entropy([0x62; 32]);
        let context = ContextId::new_from_entropy([0x63; 32]);
        let at = PhysicalTime {
            ts_ms: 10,
            uncertainty: None,
        };
        let friendship = fact_from_relational(
            FriendshipFact::Accepted {
                context_id: context,
                requester: own,
                accepter: peer,
                accepted_at: at.clone(),
            }
            .to_generic(),
        );
        let added = fact_from_relational(
            ContactFact::Added {
                context_id: context,
                owner_id: own,
                contact_id: peer,
                nickname: "Peer".to_string(),
                added_at: at,
                invitation_code: None,
            }
            .to_generic(),
        );

        for replay in [false, true] {
            let reactive = ReactiveHandler::new();
            register_app_signals(&reactive).await.unwrap();
            let view = ContactsSignalView::new(own, reactive.clone());
            view.update(std::slice::from_ref(&friendship)).await;
            assert!(reactive
                .read(&*CONTACTS_SIGNAL)
                .await
                .unwrap()
                .contact(&peer)
                .is_none());
            view.update(std::slice::from_ref(&added)).await;
            let contacts = reactive.read(&*CONTACTS_SIGNAL).await.unwrap();
            assert_eq!(
                contacts
                    .contact(&peer)
                    .map(|contact| contact.relationship_state),
                Some(ContactRelationshipState::Friend),
                "friendship should enrich the canonical contact after {}",
                if replay {
                    "restart replay"
                } else {
                    "out-of-order delivery"
                }
            );
        }
    }
}
