//! # Invitations View State
//!
//! This module defines the invitations state with computed counts (no sync bugs).

use aura_core::identifiers::{AuthorityId, ChannelId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Invitation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum InvitationType {
    /// Home membership invitation
    #[default]
    Home,
    /// Guardian invitation
    Guardian,
    /// Chat/DM invitation
    Chat,
}

/// Invitation status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum InvitationStatus {
    /// Invitation is pending
    #[default]
    Pending,
    /// Invitation was accepted
    Accepted,
    /// Invitation was rejected
    Rejected,
    /// Invitation expired
    Expired,
    /// Invitation was revoked by sender
    Revoked,
}

/// Invitation direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum InvitationDirection {
    /// We received this invitation
    #[default]
    Received,
    /// We sent this invitation
    Sent,
}

/// An invitation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct Invitation {
    /// Invitation identifier (fact ID)
    pub id: String,
    /// Type of invitation
    pub invitation_type: InvitationType,
    /// Current status
    pub status: InvitationStatus,
    /// Direction (sent or received)
    pub direction: InvitationDirection,
    /// Sender ID
    pub from_id: AuthorityId,
    /// Sender name
    pub from_name: String,
    /// Recipient ID (for sent invitations)
    pub to_id: Option<AuthorityId>,
    /// Recipient name (for sent invitations)
    pub to_name: Option<String>,
    /// When invitation was created (ms since epoch)
    pub created_at: u64,
    /// When invitation expires (ms since epoch)
    pub expires_at: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
    /// Home ID (for home invitations)
    pub home_id: Option<ChannelId>,
    /// Home name (for home invitations)
    pub home_name: Option<String>,
}

/// Error type for invitation operations
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum InvitationError {
    /// Invitation not found
    #[error("invitation not found: {0}")]
    NotFound(String),

    /// Invitation already processed (not pending)
    #[error("invitation already processed: {0}")]
    AlreadyProcessed(String),

    /// Cannot revoke a received invitation
    #[error("cannot revoke received invitation: {0}")]
    CannotRevokeReceived(String),
}

/// Invitations state
///
/// Note: Counts are computed, not stored, to prevent sync bugs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct InvitationsState {
    /// Pending received invitations
    pending: Vec<Invitation>,
    /// Sent invitations (pending)
    sent: Vec<Invitation>,
    /// Recent history (accepted/rejected/expired)
    history: Vec<Invitation>,
}

impl InvitationsState {
    /// Maximum number of historical invitations retained in-memory.
    const MAX_HISTORY: usize = 200;

    /// Create a new invitations state from its component parts.
    ///
    /// This constructor is useful for query results and deserialization.
    /// Note: counts are computed, not stored.
    pub fn from_parts(
        pending: Vec<Invitation>,
        sent: Vec<Invitation>,
        history: Vec<Invitation>,
    ) -> Self {
        Self {
            pending,
            sent,
            history,
        }
    }

    // ─── Queries (Computed Properties) ───────────────────────

    /// Count of pending received invitations (computed, not stored).
    ///
    /// This is always accurate because it's derived from the actual data,
    /// eliminating sync bugs that can occur with stored counts.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Count of pending sent invitations (computed).
    pub fn sent_count(&self) -> usize {
        self.sent.len()
    }

    /// Count of historical invitations (computed).
    pub fn history_count(&self) -> usize {
        self.history.len()
    }

    /// Get all pending received invitations.
    pub fn all_pending(&self) -> &[Invitation] {
        &self.pending
    }

    /// Get all sent invitations.
    pub fn all_sent(&self) -> &[Invitation] {
        &self.sent
    }

    /// Get invitation history.
    pub fn all_history(&self) -> &[Invitation] {
        &self.history
    }

    /// Get invitation by ID (searches all lists).
    pub fn invitation(&self, id: &str) -> Option<&Invitation> {
        self.pending
            .iter()
            .chain(self.sent.iter())
            .chain(self.history.iter())
            .find(|inv| inv.id == id)
    }

    /// Get a pending invitation by ID.
    pub fn pending_by_id(&self, id: &str) -> Option<&Invitation> {
        self.pending.iter().find(|inv| inv.id == id)
    }

    /// Get a sent invitation by ID.
    pub fn sent_by_id(&self, id: &str) -> Option<&Invitation> {
        self.sent.iter().find(|inv| inv.id == id)
    }

    /// Count pending received invitations (filtered by status).
    pub fn pending_received_count(&self) -> usize {
        self.pending
            .iter()
            .filter(|inv| {
                inv.direction == InvitationDirection::Received
                    && inv.status == InvitationStatus::Pending
            })
            .count()
    }

    /// Check if there are any pending invitations requiring action.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    // ─── Mutations (Return Result for Error Handling) ────────

    /// Add a new invitation.
    pub fn add_invitation(&mut self, invitation: Invitation) {
        match invitation.direction {
            InvitationDirection::Sent => {
                self.sent.push(invitation);
            }
            InvitationDirection::Received => {
                self.pending.push(invitation);
            }
        }
    }

    /// Mark an invitation as accepted.
    ///
    /// Returns the accepted invitation on success, or an error if not found.
    pub fn accept_invitation(&mut self, invitation_id: &str) -> Result<Invitation, InvitationError> {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Accepted;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        // Check in sent (someone accepted our invitation)
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Accepted;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        Err(InvitationError::NotFound(invitation_id.to_string()))
    }

    /// Mark an invitation as rejected.
    ///
    /// Returns the rejected invitation on success, or an error if not found.
    pub fn reject_invitation(&mut self, invitation_id: &str) -> Result<Invitation, InvitationError> {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Rejected;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        // Check in sent (someone rejected our invitation)
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Rejected;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        Err(InvitationError::NotFound(invitation_id.to_string()))
    }

    /// Revoke a sent invitation.
    ///
    /// Returns the revoked invitation on success, or an error if not found
    /// or if attempting to revoke a received invitation.
    pub fn revoke_invitation(&mut self, invitation_id: &str) -> Result<Invitation, InvitationError> {
        // Check if it's in pending (cannot revoke received)
        if self.pending.iter().any(|inv| inv.id == invitation_id) {
            return Err(InvitationError::CannotRevokeReceived(invitation_id.to_string()));
        }
        // Check in sent
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Revoked;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        Err(InvitationError::NotFound(invitation_id.to_string()))
    }

    /// Mark an invitation as expired.
    ///
    /// Returns the expired invitation on success, or an error if not found.
    pub fn expire_invitation(&mut self, invitation_id: &str) -> Result<Invitation, InvitationError> {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Expired;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        // Check in sent
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Expired;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        Err(InvitationError::NotFound(invitation_id.to_string()))
    }

    // ─── Private Helpers ─────────────────────────────────────

    fn trim_history(&mut self) {
        if self.history.len() > Self::MAX_HISTORY {
            let overflow = self.history.len() - Self::MAX_HISTORY;
            self.history.drain(0..overflow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_invitation(id: &str, direction: InvitationDirection) -> Invitation {
        Invitation {
            id: id.to_string(),
            invitation_type: InvitationType::Home,
            status: InvitationStatus::Pending,
            direction,
            from_id: AuthorityId::new_from_entropy([0u8; 32]),
            from_name: "Test".to_string(),
            to_id: None,
            to_name: None,
            created_at: 0,
            expires_at: None,
            message: None,
            home_id: None,
            home_name: None,
        }
    }

    #[test]
    fn test_pending_count_is_computed() {
        let mut state = InvitationsState::default();
        assert_eq!(state.pending_count(), 0);

        state.add_invitation(make_invitation("inv1", InvitationDirection::Received));
        assert_eq!(state.pending_count(), 1);

        state.add_invitation(make_invitation("inv2", InvitationDirection::Received));
        assert_eq!(state.pending_count(), 2);

        // Accept removes from pending
        let _ = state.accept_invitation("inv1");
        assert_eq!(state.pending_count(), 1);
        assert_eq!(state.history_count(), 1);
    }

    #[test]
    fn test_sent_count_is_computed() {
        let mut state = InvitationsState::default();
        assert_eq!(state.sent_count(), 0);

        state.add_invitation(make_invitation("inv1", InvitationDirection::Sent));
        assert_eq!(state.sent_count(), 1);
        assert_eq!(state.pending_count(), 0); // Sent doesn't affect pending
    }

    #[test]
    fn test_accept_returns_error_if_not_found() {
        let mut state = InvitationsState::default();
        let result = state.accept_invitation("nonexistent");
        assert!(matches!(result, Err(InvitationError::NotFound(_))));
    }

    #[test]
    fn test_revoke_prevents_revoking_received() {
        let mut state = InvitationsState::default();
        state.add_invitation(make_invitation("inv1", InvitationDirection::Received));

        let result = state.revoke_invitation("inv1");
        assert!(matches!(result, Err(InvitationError::CannotRevokeReceived(_))));
    }

    #[test]
    fn test_revoke_sent_works() {
        let mut state = InvitationsState::default();
        state.add_invitation(make_invitation("inv1", InvitationDirection::Sent));

        let result = state.revoke_invitation("inv1");
        assert!(result.is_ok());
        assert_eq!(state.sent_count(), 0);
        assert_eq!(state.history_count(), 1);
    }
}
