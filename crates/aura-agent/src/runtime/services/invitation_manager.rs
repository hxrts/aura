//! Invitation cache manager.

use crate::handlers::Invitation;
use super::state::with_state_mut_validated;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Default)]
struct InvitationState {
    invitations: HashMap<String, Invitation>,
}

impl InvitationState {
    fn validate(&self) -> Result<(), String> {
        for (id, invitation) in &self.invitations {
            if id != &invitation.invitation_id {
                return Err(format!(
                    "invitation id mismatch: key {} vs value {}",
                    id, invitation.invitation_id
                ));
            }
        }
        Ok(())
    }
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
            |state| state.validate(),
        )
        .await;
    }

    /// Get a cached invitation.
    pub async fn get_invitation(&self, invitation_id: &str) -> Option<Invitation> {
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
        invitation_id: &str,
        f: impl FnOnce(&mut Invitation) -> R,
    ) -> Option<R> {
        with_state_mut_validated(
            &self.state,
            |state| state.invitations.get_mut(invitation_id).map(f),
            |state| state.validate(),
        )
        .await
    }

    /// Remove a cached invitation.
    pub async fn remove_invitation(&self, invitation_id: &str) -> Option<Invitation> {
        with_state_mut_validated(
            &self.state,
            |state| state.invitations.remove(invitation_id),
            |state| state.validate(),
        )
        .await
    }

    /// List pending invitations.
    pub async fn list_pending(
        &self,
        is_pending: impl Fn(&Invitation) -> bool,
    ) -> Vec<Invitation> {
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
