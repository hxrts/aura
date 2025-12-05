//! Participant Identity Types
//!
//! Defines who can participate in threshold signing ceremonies.

use crate::{AuthorityId, DeviceId};
use serde::{Deserialize, Serialize};

/// Identity of a participant in threshold signing.
///
/// Participants can be devices (for multi-device), guardians (for recovery),
/// or group members (for shared authorities). The same signing protocol
/// handles all participant types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParticipantIdentity {
    /// One of your own devices
    ///
    /// Used in multi-device signing for personal accounts.
    Device(DeviceId),

    /// A guardian (another person's authority)
    ///
    /// Used when guardians are signing for recovery or as trustees.
    Guardian(AuthorityId),

    /// A group member
    ///
    /// Used for shared group authorities where multiple authorities
    /// together control a threshold key.
    GroupMember {
        /// The group authority
        group: AuthorityId,
        /// The individual member's authority
        member: AuthorityId,
    },
}

impl ParticipantIdentity {
    /// Create a device participant identity
    pub fn device(device_id: DeviceId) -> Self {
        Self::Device(device_id)
    }

    /// Create a guardian participant identity
    pub fn guardian(authority: AuthorityId) -> Self {
        Self::Guardian(authority)
    }

    /// Create a group member participant identity
    pub fn group_member(group: AuthorityId, member: AuthorityId) -> Self {
        Self::GroupMember { group, member }
    }

    /// Get a display name for this participant
    pub fn display_name(&self) -> String {
        match self {
            Self::Device(id) => format!("Device:{}", id),
            Self::Guardian(id) => format!("Guardian:{}", id),
            Self::GroupMember { group, member } => {
                format!("GroupMember:{}:{}", group, member)
            }
        }
    }
}

/// How to reach a participant for signing coordination.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParticipantEndpoint {
    /// Local participant (this device)
    ///
    /// No network communication needed.
    Local,

    /// Reachable via relay with a relay identifier
    Relay {
        /// Relay server identifier
        relay_id: String,
        /// Participant's address on the relay
        address: String,
    },

    /// Direct peer-to-peer connection
    Direct {
        /// Network address (e.g., IP:port, hostname)
        address: String,
    },

    /// Offline - needs out-of-band coordination
    Offline,
}

impl Default for ParticipantEndpoint {
    fn default() -> Self {
        Self::Local
    }
}

/// A participant in a threshold signing ceremony.
///
/// Combines identity, signer index (for FROST), and how to reach them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningParticipant {
    /// Who this participant is
    pub identity: ParticipantIdentity,

    /// Their FROST participant index (1-based, must be non-zero)
    pub signer_index: u16,

    /// How to reach them for coordination
    pub endpoint: ParticipantEndpoint,
}

impl SigningParticipant {
    /// Create a new signing participant
    pub fn new(
        identity: ParticipantIdentity,
        signer_index: u16,
        endpoint: ParticipantEndpoint,
    ) -> Self {
        Self {
            identity,
            signer_index,
            endpoint,
        }
    }

    /// Create a local device participant
    pub fn local_device(device_id: DeviceId, signer_index: u16) -> Self {
        Self {
            identity: ParticipantIdentity::Device(device_id),
            signer_index,
            endpoint: ParticipantEndpoint::Local,
        }
    }

    /// Create a remote guardian participant
    pub fn remote_guardian(
        authority: AuthorityId,
        signer_index: u16,
        relay_id: String,
        address: String,
    ) -> Self {
        Self {
            identity: ParticipantIdentity::Guardian(authority),
            signer_index,
            endpoint: ParticipantEndpoint::Relay { relay_id, address },
        }
    }

    /// Check if this is a local participant
    pub fn is_local(&self) -> bool {
        matches!(self.endpoint, ParticipantEndpoint::Local)
    }

    /// Check if this participant is reachable
    pub fn is_reachable(&self) -> bool {
        !matches!(self.endpoint, ParticipantEndpoint::Offline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_device_id() -> DeviceId {
        DeviceId::deterministic_test_id()
    }

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    #[test]
    fn test_participant_identity_device() {
        let identity = ParticipantIdentity::device(test_device_id());
        assert!(matches!(identity, ParticipantIdentity::Device(_)));
    }

    #[test]
    fn test_participant_identity_guardian() {
        let identity = ParticipantIdentity::guardian(test_authority());
        assert!(matches!(identity, ParticipantIdentity::Guardian(_)));
    }

    #[test]
    fn test_participant_identity_group_member() {
        let group = test_authority();
        let member = AuthorityId::new_from_entropy([2u8; 32]);
        let identity = ParticipantIdentity::group_member(group, member);
        assert!(matches!(identity, ParticipantIdentity::GroupMember { .. }));
    }

    #[test]
    fn test_signing_participant_local() {
        let participant = SigningParticipant::local_device(test_device_id(), 1);
        assert!(participant.is_local());
        assert!(participant.is_reachable());
        assert_eq!(participant.signer_index, 1);
    }

    #[test]
    fn test_signing_participant_remote() {
        let participant = SigningParticipant::remote_guardian(
            test_authority(),
            2,
            "relay-1".to_string(),
            "addr-123".to_string(),
        );
        assert!(!participant.is_local());
        assert!(participant.is_reachable());
        assert_eq!(participant.signer_index, 2);
    }

    #[test]
    fn test_signing_participant_offline() {
        let participant = SigningParticipant::new(
            ParticipantIdentity::guardian(test_authority()),
            3,
            ParticipantEndpoint::Offline,
        );
        assert!(!participant.is_reachable());
    }

    #[test]
    fn test_participant_serialization() {
        let participant = SigningParticipant::local_device(test_device_id(), 1);
        let json = serde_json::to_string(&participant).unwrap();
        let restored: SigningParticipant = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.signer_index, 1);
        assert!(restored.is_local());
    }
}
