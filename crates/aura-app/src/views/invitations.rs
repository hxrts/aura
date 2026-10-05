//! # Invitations View State
//!
//! This module defines the invitations state with computed counts (no sync bugs).

use aura_core::types::identifiers::{AuthorityId, ChannelId};
use aura_invitation::shareable::ValidatedImportedInvitation;
use aura_invitation::{InvitationFact, InvitationType as DomainInvitationType};
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
    /// Channel invitation
    Chat,
    /// Contact (relationship) invitation
    Contact,
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

/// Complete creation evidence for an observed invitation. Its fields are
/// private so status-only facts and raw ids cannot create a pending row.
pub struct InvitationCreationWitness {
    invitation: Invitation,
}

impl InvitationCreationWitness {
    /// Convert a pending imported domain invitation into observed creation evidence.
    pub fn from_imported(
        validated: &ValidatedImportedInvitation,
        own: AuthorityId,
    ) -> Option<Self> {
        let record = validated.invitation();
        (record.status == aura_invitation::InvitationStatus::Pending).then(|| {
            Self::from_parts(
                record.invitation_id.to_string(),
                record.sender_id,
                record.receiver_id,
                &record.invitation_type,
                record.receiver_nickname.as_deref(),
                record.created_at,
                record.expires_at,
                record.message.clone(),
                own,
            )
        })
    }

    /// Convert a canonical Sent fact into observed creation evidence.
    pub(crate) fn from_sent_fact(fact: &InvitationFact, own: AuthorityId) -> Option<Self> {
        let InvitationFact::Sent {
            invitation_id,
            sender_id,
            receiver_id,
            invitation_type,
            sent_at,
            expires_at,
            receiver_nickname,
            message,
            ..
        } = fact
        else {
            return None;
        };
        Some(Self::from_parts(
            invitation_id.to_string(),
            *sender_id,
            *receiver_id,
            invitation_type,
            receiver_nickname.as_deref(),
            sent_at.ts_ms,
            expires_at.as_ref().map(|time| time.ts_ms),
            message.clone(),
            own,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        id: String,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        domain_type: &DomainInvitationType,
        receiver_nickname: Option<&str>,
        created_at: u64,
        expires_at: Option<u64>,
        message: Option<String>,
        own: AuthorityId,
    ) -> Self {
        let direction = if sender_id == own {
            InvitationDirection::Sent
        } else {
            InvitationDirection::Received
        };
        let generic_contact = direction == InvitationDirection::Sent
            && matches!(domain_type, DomainInvitationType::Contact { .. })
            && sender_id == receiver_id;
        let (invitation_type, home_id, home_name) = match domain_type {
            DomainInvitationType::Contact { .. } => (InvitationType::Contact, None, None),
            DomainInvitationType::Guardian { .. } => (InvitationType::Guardian, None, None),
            DomainInvitationType::Channel {
                home_id,
                nickname_suggestion,
                home,
                ..
            } => (
                // A home invitation joins a home; others join a chat channel.
                if *home {
                    InvitationType::Home
                } else {
                    InvitationType::Chat
                },
                Some(*home_id),
                nickname_suggestion
                    .as_deref()
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned),
            ),
            DomainInvitationType::DeviceEnrollment { .. } => (InvitationType::Home, None, None),
        };
        let from_name = match domain_type {
            DomainInvitationType::Contact {
                nickname: Some(name),
            } if !name.trim().is_empty() => name.trim().to_string(),
            _ => "Unknown".to_string(),
        };
        Self {
            invitation: Invitation {
                id,
                invitation_type,
                status: InvitationStatus::Pending,
                direction,
                from_id: sender_id,
                from_name,
                to_id: (direction == InvitationDirection::Sent && !generic_contact)
                    .then_some(receiver_id),
                to_name: if direction == InvitationDirection::Sent {
                    if generic_contact {
                        receiver_nickname
                            .map(str::trim)
                            .filter(|name| !name.is_empty())
                            .map(ToOwned::to_owned)
                    } else {
                        Some("Unknown".to_string())
                    }
                } else {
                    None
                },
                created_at,
                expires_at,
                message,
                home_id,
                home_name,
            },
        }
    }

    /// Return the stable invitation id without exposing the witness payload.
    pub fn id(&self) -> &str {
        &self.invitation.id
    }

    /// The invitation's sender authority.
    #[must_use]
    pub fn sender_id(&self) -> AuthorityId {
        self.invitation.from_id
    }
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
    /// Use this for trusted query results and test fixtures. Reactive
    /// publication must use [`Self::add_invitation`] with creation evidence.
    /// Counts are computed, not stored.
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

    /// Name the sender of an invitation that carries no sender nickname
    /// (guardian and channel invitations) with a known contact's name.
    /// Returns whether the name changed.
    pub fn name_unknown_sender(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(invitation) = self
            .pending
            .iter_mut()
            .chain(self.sent.iter_mut())
            .chain(self.history.iter_mut())
            .find(|inv| inv.id == id)
        else {
            return false;
        };
        if invitation.from_name != "Unknown" {
            return false;
        }
        invitation.from_name = name.to_string();
        true
    }

    /// Get invitation by ID (searches all lists).
    pub fn invitation(&self, id: &str) -> Option<&Invitation> {
        self.pending
            .iter()
            .chain(self.sent.iter())
            .chain(self.history.iter())
            .find(|inv| inv.id == id)
    }

    /// Invitations still awaiting a response, received first and then sent.
    ///
    /// Sent invitations are listed here so frontends can offer Copy/Revoke;
    /// they are not contacts and must not be counted as such.
    pub fn open_invitations(&self) -> impl Iterator<Item = &Invitation> {
        self.pending
            .iter()
            .chain(self.sent.iter())
            .filter(|inv| inv.status == InvitationStatus::Pending)
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
    pub fn add_invitation(&mut self, witness: InvitationCreationWitness) {
        let invitation = witness.invitation;
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
    pub fn accept_invitation(
        &mut self,
        invitation_id: &str,
    ) -> Result<Invitation, InvitationError> {
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
    pub fn reject_invitation(
        &mut self,
        invitation_id: &str,
    ) -> Result<Invitation, InvitationError> {
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
    pub fn revoke_invitation(
        &mut self,
        invitation_id: &str,
    ) -> Result<Invitation, InvitationError> {
        // Check if it's in pending (cannot revoke received)
        if self.pending.iter().any(|inv| inv.id == invitation_id) {
            return Err(InvitationError::CannotRevokeReceived(
                invitation_id.to_string(),
            ));
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

    /// Apply a sender-authored cancellation observed from the journal.
    /// A received invitation can be revoked by its sender even though the
    /// local receiver cannot initiate revocation.
    pub fn observe_cancelled_invitation(
        &mut self,
        invitation_id: &str,
    ) -> Result<Invitation, InvitationError> {
        if let Some(idx) = self.pending.iter().position(|inv| inv.id == invitation_id) {
            let mut inv = self.pending.remove(idx);
            inv.status = InvitationStatus::Revoked;
            self.history.push(inv.clone());
            self.trim_history();
            return Ok(inv);
        }
        self.revoke_invitation(invitation_id)
    }

    /// Mark an invitation as expired.
    ///
    /// Returns the expired invitation on success, or an error if not found.
    pub fn expire_invitation(
        &mut self,
        invitation_id: &str,
    ) -> Result<Invitation, InvitationError> {
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

    fn add_fixture(state: &mut InvitationsState, invitation: Invitation) {
        match invitation.direction {
            InvitationDirection::Sent => state.sent.push(invitation),
            InvitationDirection::Received => state.pending.push(invitation),
        }
    }

    #[test]
    fn test_pending_count_is_computed() {
        let mut state = InvitationsState::default();
        assert_eq!(state.pending_count(), 0);

        add_fixture(
            &mut state,
            make_invitation("inv1", InvitationDirection::Received),
        );
        assert_eq!(state.pending_count(), 1);

        add_fixture(
            &mut state,
            make_invitation("inv2", InvitationDirection::Received),
        );
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

        add_fixture(
            &mut state,
            make_invitation("inv1", InvitationDirection::Sent),
        );
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
        add_fixture(
            &mut state,
            make_invitation("inv1", InvitationDirection::Received),
        );

        let result = state.revoke_invitation("inv1");
        assert!(matches!(
            result,
            Err(InvitationError::CannotRevokeReceived(_))
        ));
    }

    #[test]
    fn observed_sender_cancellation_settles_received_invitation() {
        let mut state = InvitationsState::default();
        add_fixture(
            &mut state,
            make_invitation("received", InvitationDirection::Received),
        );
        let settled = state.observe_cancelled_invitation("received").unwrap();
        assert_eq!(settled.status, InvitationStatus::Revoked);
        assert_eq!(state.open_invitations().count(), 0);
    }

    #[test]
    fn test_revoke_sent_works() {
        let mut state = InvitationsState::default();
        add_fixture(
            &mut state,
            make_invitation("inv1", InvitationDirection::Sent),
        );

        let result = state.revoke_invitation("inv1");
        assert!(result.is_ok());
        assert_eq!(state.sent_count(), 0);
        assert_eq!(state.history_count(), 1);
    }

    #[test]
    fn test_open_invitations_include_sent_until_revoked() {
        let mut state = InvitationsState::default();
        add_fixture(
            &mut state,
            make_invitation("received", InvitationDirection::Received),
        );
        add_fixture(
            &mut state,
            make_invitation("sent", InvitationDirection::Sent),
        );

        let open: Vec<_> = state
            .open_invitations()
            .map(|inv| inv.id.as_str())
            .collect();
        assert_eq!(open, vec!["received", "sent"]);

        state.revoke_invitation("sent").unwrap();
        let open: Vec<_> = state
            .open_invitations()
            .map(|inv| inv.id.as_str())
            .collect();
        assert_eq!(open, vec!["received"]);
    }
}
