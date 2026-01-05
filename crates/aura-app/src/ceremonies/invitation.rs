//! # Invitation Configuration
//!
//! Type-safe invitation configuration for contact, channel, and guardian invitations.

use super::ThresholdConfig;
use aura_core::identifiers::{AuthorityId, ChannelId};
use std::fmt;

/// Error when constructing an invitation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvitationError {
    /// No authority configured to send invitation from
    NoAuthority,
    /// Channel not found for channel invitation
    ChannelNotFound,
    /// No threshold configured for guardian invitation
    NoThresholdForGuardian,
    /// Guardian set is already full
    GuardianSetFull {
        /// Current number of guardians
        current: u8,
        /// Maximum guardians allowed (n from threshold)
        max: u8,
    },
}

impl fmt::Display for InvitationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InvitationError::NoAuthority => {
                write!(f, "Create an account before sending invitations")
            }
            InvitationError::ChannelNotFound => {
                write!(f, "Channel not found")
            }
            InvitationError::NoThresholdForGuardian => {
                write!(
                    f,
                    "Configure guardian threshold before sending guardian invitations"
                )
            }
            InvitationError::GuardianSetFull { current, max } => {
                write!(
                    f,
                    "Guardian set is full ({current}/{max}). Remove a guardian first."
                )
            }
        }
    }
}

impl std::error::Error for InvitationError {}

/// Role for channel invitations
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChannelRole {
    /// Full participant with send/receive
    #[default]
    Participant,
    /// Read-only observer
    Observer,
    /// Administrator with moderation rights
    Admin,
}

/// A validated invitation configuration
///
/// # Variants
///
/// - `Contact`: Invite someone to become a contact
/// - `Channel`: Invite someone to join a channel
/// - `Guardian`: Invite someone to become a guardian
///
/// Each variant enforces its specific prerequisites at construction time.
#[derive(Debug, Clone)]
pub enum InvitationConfig {
    /// Contact invitation - requires authority
    Contact {
        /// The authority sending the invitation
        from: AuthorityId,
    },
    /// Channel invitation - requires channel to exist
    Channel {
        /// The authority sending the invitation
        from: AuthorityId,
        /// The channel to invite to
        channel_id: ChannelId,
        /// Role for the invitee
        role: ChannelRole,
    },
    /// Guardian invitation - requires threshold to be configured with room
    Guardian {
        /// The authority sending the invitation
        from: AuthorityId,
        /// Current threshold configuration
        threshold: ThresholdConfig,
        /// Current guardian count
        current_guardians: u8,
    },
}

impl InvitationConfig {
    /// Create a contact invitation
    pub fn contact(from: AuthorityId) -> Self {
        Self::Contact { from }
    }

    /// Create a channel invitation
    ///
    /// # Arguments
    ///
    /// * `from` - The authority sending the invitation
    /// * `channel_id` - The channel to invite to
    /// * `role` - The role for the invitee (defaults to Participant)
    pub fn channel(from: AuthorityId, channel_id: ChannelId, role: ChannelRole) -> Self {
        Self::Channel {
            from,
            channel_id,
            role,
        }
    }

    /// Create a guardian invitation
    ///
    /// Returns an error if:
    /// - No threshold is configured
    /// - The guardian set is already at capacity (n)
    pub fn guardian(
        from: AuthorityId,
        threshold: Option<ThresholdConfig>,
        current_guardians: u8,
    ) -> Result<Self, InvitationError> {
        let threshold = threshold.ok_or(InvitationError::NoThresholdForGuardian)?;

        if current_guardians >= threshold.n() {
            return Err(InvitationError::GuardianSetFull {
                current: current_guardians,
                max: threshold.n(),
            });
        }

        Ok(Self::Guardian {
            from,
            threshold,
            current_guardians,
        })
    }

    /// Get the sending authority
    pub fn from_authority(&self) -> &AuthorityId {
        match self {
            Self::Contact { from } => from,
            Self::Channel { from, .. } => from,
            Self::Guardian { from, .. } => from,
        }
    }

    /// Check if this is a contact invitation
    pub fn is_contact(&self) -> bool {
        matches!(self, Self::Contact { .. })
    }

    /// Check if this is a channel invitation
    pub fn is_channel(&self) -> bool {
        matches!(self, Self::Channel { .. })
    }

    /// Check if this is a guardian invitation
    pub fn is_guardian(&self) -> bool {
        matches!(self, Self::Guardian { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use uuid::Uuid;

    static COUNTER: AtomicU8 = AtomicU8::new(1);

    fn make_authority() -> AuthorityId {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        AuthorityId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    fn make_channel_id() -> ChannelId {
        ChannelId::from_bytes([0u8; 32])
    }

    #[test]
    fn test_contact_invitation() {
        let inv = InvitationConfig::contact(make_authority());
        assert!(inv.is_contact());
    }

    #[test]
    fn test_channel_invitation() {
        let inv = InvitationConfig::channel(
            make_authority(),
            make_channel_id(),
            ChannelRole::Participant,
        );
        assert!(inv.is_channel());
    }

    #[test]
    fn test_guardian_invitation_no_threshold() {
        let result = InvitationConfig::guardian(make_authority(), None, 0);
        assert_eq!(result.unwrap_err(), InvitationError::NoThresholdForGuardian);
    }

    #[test]
    fn test_guardian_invitation_full() {
        let threshold = ThresholdConfig::new(2, 3).unwrap();
        let result = InvitationConfig::guardian(make_authority(), Some(threshold), 3);
        assert_eq!(
            result.unwrap_err(),
            InvitationError::GuardianSetFull { current: 3, max: 3 }
        );
    }

    #[test]
    fn test_guardian_invitation_valid() {
        let threshold = ThresholdConfig::new(2, 3).unwrap();
        let inv = InvitationConfig::guardian(make_authority(), Some(threshold), 1).unwrap();
        assert!(inv.is_guardian());
    }
}
