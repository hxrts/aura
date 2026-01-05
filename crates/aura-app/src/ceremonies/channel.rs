//! # Channel Participants
//!
//! Type-safe channel participant set ensuring minimum participant count.

use aura_core::identifiers::AuthorityId;
use std::fmt;

/// Minimum number of participants for a channel
pub const MIN_CHANNEL_PARTICIPANTS: usize = 2;

/// Error when constructing channel participants
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelError {
    /// Not enough participants for a channel
    InsufficientParticipants {
        /// Number of participants required (always 2)
        required: usize,
        /// Number of participants provided
        available: usize,
    },
}

impl fmt::Display for ChannelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChannelError::InsufficientParticipants {
                required,
                available,
            } => {
                write!(
                    f,
                    "Channels require at least {required} participants, but only {available} provided"
                )
            }
        }
    }
}

impl std::error::Error for ChannelError {}

/// A participant set with at least 2 authorities for channel creation
///
/// Invariants:
/// - At least 2 participants must be present
///
/// # Example
///
/// ```rust,ignore
/// let participants = ChannelParticipants::new(vec![self_authority, other_authority])?;
///
/// // Can now safely create channel
/// create_channel(participants);
/// ```
#[derive(Debug, Clone)]
pub struct ChannelParticipants {
    participants: Vec<AuthorityId>,
}

impl ChannelParticipants {
    /// Create a channel participant set
    ///
    /// Returns an error if fewer than 2 participants are provided.
    pub fn new(participants: Vec<AuthorityId>) -> Result<Self, ChannelError> {
        if participants.len() < MIN_CHANNEL_PARTICIPANTS {
            return Err(ChannelError::InsufficientParticipants {
                required: MIN_CHANNEL_PARTICIPANTS,
                available: participants.len(),
            });
        }
        Ok(Self { participants })
    }

    /// Create a channel between self and one other participant
    pub fn pairwise(self_authority: AuthorityId, other: AuthorityId) -> Self {
        Self {
            participants: vec![self_authority, other],
        }
    }

    /// Get the number of participants
    pub fn count(&self) -> usize {
        self.participants.len()
    }

    /// Get the participant authority IDs
    pub fn participants(&self) -> &[AuthorityId] {
        &self.participants
    }

    /// Consume and return the inner participant list
    pub fn into_participants(self) -> Vec<AuthorityId> {
        self.participants
    }

    /// Check if this is a pairwise (2-party) channel
    pub fn is_pairwise(&self) -> bool {
        self.participants.len() == 2
    }

    /// Check if this is a group (3+ party) channel
    pub fn is_group(&self) -> bool {
        self.participants.len() > 2
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

    #[test]
    fn test_insufficient_participants() {
        // 0 participants
        let result = ChannelParticipants::new(vec![]);
        assert_eq!(
            result.unwrap_err(),
            ChannelError::InsufficientParticipants {
                required: 2,
                available: 0
            }
        );

        // 1 participant
        let result = ChannelParticipants::new(vec![make_authority()]);
        assert_eq!(
            result.unwrap_err(),
            ChannelError::InsufficientParticipants {
                required: 2,
                available: 1
            }
        );
    }

    #[test]
    fn test_valid_participants() {
        let participants = ChannelParticipants::new(vec![make_authority(), make_authority()]);
        assert!(participants.is_ok());
        assert!(participants.unwrap().is_pairwise());
    }

    #[test]
    fn test_pairwise() {
        let p = ChannelParticipants::pairwise(make_authority(), make_authority());
        assert!(p.is_pairwise());
        assert!(!p.is_group());
    }

    #[test]
    fn test_group() {
        let p =
            ChannelParticipants::new(vec![make_authority(), make_authority(), make_authority()])
                .unwrap();
        assert!(!p.is_pairwise());
        assert!(p.is_group());
    }
}
