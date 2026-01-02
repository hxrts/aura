//! Invitation cache manager.

use super::state::with_state_mut_validated;
use crate::handlers::Invitation;
use aura_core::identifiers::InvitationId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct InvitationState {
    invitations: HashMap<InvitationId, Invitation>,
}

/// Manages cached invitations for the invitation handler.
#[derive(Clone, Default)]
pub struct InvitationManager {
    state: Arc<RwLock<InvitationState>>,
}

impl InvitationManager {
    /// Create a new invitation manager.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(InvitationState::default())),
        }
    }

    /// Cache an invitation by ID.
    pub async fn cache_invitation(&self, invitation: Invitation) {
        with_state_mut_validated(
            &self.state,
            |state| {
                state
                    .invitations
                    .insert(invitation.invitation_id.clone(), invitation);
            },
            |_| Ok(()),
        )
        .await;
    }

    /// Get a cached invitation.
    pub async fn get_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        self.state
            .read()
            .await
            .invitations
            .get(invitation_id)
            .cloned()
    }

    /// Update a cached invitation if present.
    pub async fn update_invitation<R>(
        &self,
        invitation_id: &InvitationId,
        f: impl FnOnce(&mut Invitation) -> R,
    ) -> Option<R> {
        with_state_mut_validated(
            &self.state,
            |state| state.invitations.get_mut(invitation_id).map(f),
            |_| Ok(()),
        )
        .await
    }

    /// Remove a cached invitation.
    pub async fn remove_invitation(&self, invitation_id: &InvitationId) -> Option<Invitation> {
        with_state_mut_validated(
            &self.state,
            |state| state.invitations.remove(invitation_id),
            |_| Ok(()),
        )
        .await
    }

    /// List pending invitations.
    pub async fn list_pending(&self, is_pending: impl Fn(&Invitation) -> bool) -> Vec<Invitation> {
        self.state
            .read()
            .await
            .invitations
            .values()
            .filter(|inv| is_pending(inv))
            .cloned()
            .collect()
    }
}
