//! # Invitations View State

use serde::{Deserialize, Serialize};

/// Invitation type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum InvitationType {
    /// Block membership invitation
    #[default]
    Block,
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    pub from_id: String,
    /// Sender name
    pub from_name: String,
    /// Recipient ID (for sent invitations)
    pub to_id: Option<String>,
    /// Recipient name (for sent invitations)
    pub to_name: Option<String>,
    /// When invitation was created (ms since epoch)
    pub created_at: u64,
    /// When invitation expires (ms since epoch)
    pub expires_at: Option<u64>,
    /// Optional message from sender
    pub message: Option<String>,
    /// Block ID (for block invitations)
    pub block_id: Option<String>,
    /// Block name (for block invitations)
    pub block_name: Option<String>,
}

/// Invitations state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct InvitationsState {
    /// Pending received invitations
    pub pending: Vec<Invitation>,
    /// Sent invitations (pending)
    pub sent: Vec<Invitation>,
    /// Recent history (accepted/rejected/expired)
    pub history: Vec<Invitation>,
    /// Count of pending invitations requiring action
    pub pending_count: u32,
}

impl InvitationsState {
    /// Get invitation by ID
    pub fn invitation(&self, id: &str) -> Option<&Invitation> {
        self.pending
            .iter()
            .chain(self.sent.iter())
            .chain(self.history.iter())
            .find(|inv| inv.id == id)
    }

    /// Count pending received invitations
    pub fn pending_received_count(&self) -> usize {
        self.pending
            .iter()
            .filter(|inv| {
                inv.direction == InvitationDirection::Received
                    && inv.status == InvitationStatus::Pending
            })
            .count()
    }

    /// Add a new invitation
    pub fn add_invitation(&mut self, invitation: Invitation) {
        match invitation.direction {
            InvitationDirection::Sent => {
                self.sent.push(invitation);
            }
            InvitationDirection::Received => {
                self.pending.push(invitation);
                self.pending_count += 1;
            }
        }
    }

    /// Mark an invitation as accepted
    pub fn accept_invitation(&mut self, invitation_id: &str) {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Accepted;
            self.pending_count = self.pending_count.saturating_sub(1);
            self.history.push(inv);
            return;
        }
        // Check in sent
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Accepted;
            self.history.push(inv);
        }
    }

    /// Mark an invitation as rejected
    pub fn reject_invitation(&mut self, invitation_id: &str) {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Rejected;
            self.pending_count = self.pending_count.saturating_sub(1);
            self.history.push(inv);
            return;
        }
        // Check in sent
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Rejected;
            self.history.push(inv);
        }
    }

    /// Revoke a sent invitation
    pub fn revoke_invitation(&mut self, invitation_id: &str) {
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Revoked;
            self.history.push(inv);
        }
    }

    /// Mark an invitation as expired
    pub fn expire_invitation(&mut self, invitation_id: &str) {
        // Check in pending first
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Expired;
            self.pending_count = self.pending_count.saturating_sub(1);
            self.history.push(inv);
            return;
        }
        // Check in sent
        if let Some(idx) = self.sent.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.sent.remove(idx);
            inv.status = InvitationStatus::Expired;
            self.history.push(inv);
        }
    }

    /// Get all pending invitations (both sent and received)
    pub fn all_pending(&self) -> impl Iterator<Item = &Invitation> {
        self.pending
            .iter()
            .chain(self.sent.iter())
            .filter(|inv| inv.status == InvitationStatus::Pending)
    }
}
