//! Invitation cache manager.

use super::state::with_state_mut_validated;
use crate::handlers::Invitation;
use aura_chat::{ChannelContextIndex, ChatFact};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId, InvitationId};
use aura_relational::{ContactExistenceIndex, ContactFact};
use std::collections::{BTreeMap, HashMap};
use tokio::sync::RwLock;

const DEFAULT_INVITATION_CACHE_CAPACITY: usize = 1_000;

#[allow(dead_code)] // Declaration-layer ingress inventory; runtime actor wiring lands incrementally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InvitationManagerCommand {
    CacheInvitation,
    UpdateInvitation,
    RemoveInvitation,
    ReplaceContactIndex,
    ReplaceChannelContextIndex,
}

#[derive(Debug)]
struct InvitationState {
    invitations: HashMap<InvitationId, Invitation>,
    invitation_access: HashMap<InvitationId, u64>,
    invitation_lru: BTreeMap<u64, InvitationId>,
    next_access_tick: u64,
    contact_index: ContactExistenceIndex,
    contact_index_seeded: bool,
    channel_context_index: ChannelContextIndex,
    channel_context_index_seeded: bool,
}

impl Default for InvitationState {
    fn default() -> Self {
        Self {
            invitations: HashMap::new(),
            invitation_access: HashMap::new(),
            invitation_lru: BTreeMap::new(),
            next_access_tick: 0,
            contact_index: ContactExistenceIndex::new(),
            contact_index_seeded: false,
            channel_context_index: ChannelContextIndex::new(),
            channel_context_index_seeded: false,
        }
    }
}

/// Manages cached invitations for the invitation handler.
#[aura_macros::actor_owned(
    owner = "invitation_manager",
    domain = "invitation_cache",
    gate = "invitation_cache_command_ingress",
    command = InvitationManagerCommand,
    capacity = 128,
    category = "actor_owned"
)]
pub struct InvitationManager {
    state: RwLock<InvitationState>,
    max_invitations: usize,
}

impl Default for InvitationManager {
    fn default() -> Self {
        Self::new()
    }
}

impl InvitationManager {
    /// Create a new invitation manager.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_INVITATION_CACHE_CAPACITY)
    }

    /// Create a new invitation manager with an explicit cache bound.
    pub fn with_capacity(max_invitations: usize) -> Self {
        Self {
            state: RwLock::new(InvitationState::default()),
            max_invitations: max_invitations.max(1),
        }
    }

    fn touch_invitation(state: &mut InvitationState, invitation_id: &InvitationId) {
        state.next_access_tick = state.next_access_tick.saturating_add(1);
        let access_tick = state.next_access_tick;
        if let Some(previous_tick) = state
            .invitation_access
            .insert(invitation_id.clone(), access_tick)
        {
            state.invitation_lru.remove(&previous_tick);
        }
        state
            .invitation_lru
            .insert(access_tick, invitation_id.clone());
    }

    fn remove_invitation_tracking(state: &mut InvitationState, invitation_id: &InvitationId) {
        if let Some(access_tick) = state.invitation_access.remove(invitation_id) {
            state.invitation_lru.remove(&access_tick);
        }
    }

    fn evict_excess_invitations(state: &mut InvitationState, max_invitations: usize) {
        while state.invitations.len() > max_invitations {
            let Some((oldest_tick, oldest_invitation_id)) = state
                .invitation_lru
                .first_key_value()
                .map(|(tick, invitation_id)| (*tick, invitation_id.clone()))
            else {
                break;
            };
            state.invitation_lru.remove(&oldest_tick);
            state.invitation_access.remove(&oldest_invitation_id);
            state.invitations.remove(&oldest_invitation_id);
        }
    }

    /// Cache an invitation by ID.
    pub async fn cache_invitation(&self, invitation: Invitation) {
        let max_invitations = self.max_invitations;
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .invitations
                    .insert(invitation.invitation_id.clone(), invitation.clone());
                Self::touch_invitation(state, &invitation.invitation_id);
                Self::evict_excess_invitations(state, max_invitations);
            },
            |_| Ok(()),
        )
        .await;
    }

    /// Get a cached invitation.
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let invitation = state.invitations.get(invitation_id).cloned();
                if invitation.is_some() {
                    Self::touch_invitation(state, invitation_id);
                }
                invitation
            },
            |_| Ok(()),
        )
        .await
    }

    /// Update a cached invitation if present.
    pub async fn update_invitation<R>(
        &self,
        invitation_id: &InvitationId,
        f: impl FnOnce(&mut Invitation) -> R,
    ) -> Option<R> {
        with_state_mut_validated(
            &self.state,
            |state| {
                let result = state.invitations.get_mut(invitation_id).map(f);
                if result.is_some() {
                    Self::touch_invitation(state, invitation_id);
                }
                result
            },
            |_| Ok(()),
        )
        .await
    }

    /// Remove a cached invitation.
    pub async fn remove_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        with_state_mut_validated(
            &self.state,
            |state| {
                Self::remove_invitation_tracking(state, invitation_id);
                state.invitations.remove(invitation_id)
            },
            |_| Ok(()),
        )
        .await
    }

    /// List cached invitations matching a predicate.
    pub async fn list_matching(&self, predicate: impl Fn(&Invitation) -> bool) -> Vec<Invitation> {
        self.state
            .read()
            .await
            .invitations
            .values()
            .filter(|inv| predicate(inv))
            .cloned()
            .collect()
    }

    pub async fn contact_index_seeded(&self) -> bool {
        self.state.read().await.contact_index_seeded
    }

    pub async fn replace_contact_index(&self, index: ContactExistenceIndex) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.contact_index = index;
                state.contact_index_seeded = true;
            },
            |_| Ok(()),
        )
        .await;
    }

    pub async fn record_contact_fact(&self, fact: &ContactFact) {
        with_state_mut_validated(
            &self.state,
            |state| state.contact_index.apply_fact(fact),
            |_| Ok(()),
        )
        .await;
    }

    pub async fn contact_exists(&self, owner_id: AuthorityId, contact_id: AuthorityId) -> bool {
        self.state
            .read()
            .await
            .contact_index
            .contains(owner_id, contact_id)
    }

    pub async fn channel_context_index_seeded(&self) -> bool {
        self.state.read().await.channel_context_index_seeded
    }

    pub async fn replace_channel_context_index(&self, index: ChannelContextIndex) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state.channel_context_index = index;
                state.channel_context_index_seeded = true;
            },
            |_| Ok(()),
        )
        .await;
    }

    pub async fn record_chat_fact(&self, fact: &ChatFact) {
        with_state_mut_validated(
            &self.state,
            |state| state.channel_context_index.apply_fact(fact),
            |_| Ok(()),
        )
        .await;
    }

    pub async fn channel_context(
        &self,
        channel_id: ChannelId,
        creator_id: AuthorityId,
    ) -> Option<ContextId> {
        self.state
            .read()
            .await
            .channel_context_index
            .context_for_channel(channel_id, creator_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::default_context_id_for_authority;
    use aura_core::types::identifiers::AuthorityId;

    fn invitation(seed: u8) -> Invitation {
        let sender_id = AuthorityId::new_from_entropy([seed; 32]);
        let receiver_id = AuthorityId::new_from_entropy([seed.wrapping_add(1); 32]);
        Invitation {
            invitation_id: InvitationId::new(format!("invitation-{seed}")),
            context_id: default_context_id_for_authority(sender_id),
            sender_id,
            receiver_id,
            invitation_type: crate::handlers::InvitationType::Contact { nickname: None },
            status: crate::handlers::InvitationStatus::Pending,
            created_at: u64::from(seed),
            expires_at: None,
            message: None,
        }
    }

    #[tokio::test]
    async fn invitation_cache_respects_capacity_bound() {
        let manager = InvitationManager::with_capacity(2);

        manager.cache_invitation(invitation(1)).await;
        manager.cache_invitation(invitation(2)).await;
        manager.cache_invitation(invitation(3)).await;

        let cached = manager.list_matching(|_| true).await;
        assert_eq!(cached.len(), 2);
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-1"))
            .await
            .is_none());
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-2"))
            .await
            .is_some());
        assert!(manager
            .get_invitation(&InvitationId::new("invitation-3"))
            .await
            .is_some());
    }
}
