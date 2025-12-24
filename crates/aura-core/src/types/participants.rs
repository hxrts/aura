//! Participant identity and addressing types.
//!
//! These types describe "who can participate" in multi-party protocols such as
//! threshold signing and how to reach them.

use crate::{AuthorityId, DeviceId};
use serde::{Deserialize, Serialize};

/// Threshold value for FROST key-rotation ceremonies (k-of-n signing).
///
/// This type enforces FROST's requirement that threshold must be >= 2 for
/// multi-party signing. Single-signer (1-of-1) configurations should use
/// Ed25519 directly instead of FROST.
///
/// Used by all ceremony types: guardian key rotation, device enrollment,
/// device removal, and group/block membership changes.
///
/// # Construction
///
/// Use `new()` which validates the minimum, or `new_unchecked()` for trusted contexts.
///
/// ```
/// use aura_core::types::FrostThreshold;
///
/// // Valid threshold
/// let t = FrostThreshold::new(2).unwrap();
/// assert_eq!(t.value(), 2);
///
/// // Invalid threshold (below minimum)
/// assert!(FrostThreshold::new(1).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct FrostThreshold(u16);

/// Error when constructing a `FrostThreshold` with an invalid value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidThresholdError {
    /// The invalid value that was provided
    pub value: u16,
    /// Minimum allowed threshold
    pub minimum: u16,
}

impl std::fmt::Display for InvalidThresholdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid FROST threshold {}: requires minimum {} for multi-party signing",
            self.value, self.minimum
        )
    }
}

impl std::error::Error for InvalidThresholdError {}

impl FrostThreshold {
    /// Minimum threshold value required by FROST.
    pub const MINIMUM: u16 = 2;

    /// Create a new FROST threshold, validating the minimum.
    ///
    /// Returns `Err` if `value < 2` since FROST requires at least 2 signers.
    pub fn new(value: u16) -> Result<Self, InvalidThresholdError> {
        if value < Self::MINIMUM {
            Err(InvalidThresholdError {
                value,
                minimum: Self::MINIMUM,
            })
        } else {
            Ok(Self(value))
        }
    }

    /// Create a FROST threshold without validation.
    ///
    /// # Safety
    ///
    /// The caller must ensure `value >= 2`. Using a value less than 2 will
    /// cause FROST key generation to fail at runtime.
    ///
    /// This is provided for deserialization and trusted internal contexts.
    pub const fn new_unchecked(value: u16) -> Self {
        Self(value)
    }

    /// Get the threshold value.
    pub const fn value(self) -> u16 {
        self.0
    }

    /// Create the minimum valid threshold (2).
    pub const fn minimum() -> Self {
        Self(Self::MINIMUM)
    }
}

impl From<FrostThreshold> for u16 {
    fn from(t: FrostThreshold) -> u16 {
        t.0
    }
}

impl TryFrom<u16> for FrostThreshold {
    type Error = InvalidThresholdError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl std::fmt::Display for FrostThreshold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identity of a participant in a threshold signing ceremony.
///
/// Participants can be devices (for multi-device), guardians (for recovery),
/// or group members (for shared authorities). The same signing protocol
/// handles all participant types.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParticipantIdentity {
    /// One of your own devices
    Device(DeviceId),

    /// A guardian (another person's authority)
    Guardian(AuthorityId),

    /// A group member (an authority participating in a group authority)
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

    /// Stable key for storage paths and maps.
    ///
    /// This is intended for persistence (e.g. `SecureStorageLocation` subkeys),
    /// so it avoids characters that tend to be awkward in filesystem-like keys.
    pub fn storage_key(&self) -> String {
        match self {
            Self::Device(id) => format!("device_{}", id),
            Self::Guardian(id) => format!("guardian_{}", id),
            Self::GroupMember { group, member } => {
                format!("group_{}_member_{}", group, member)
            }
        }
    }
}

/// How to reach a participant for coordination.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ParticipantEndpoint {
    /// Local participant (this device)
    #[default]
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

/// A participant in a signing ceremony.
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

    /// Create a remote guardian participant (relay-routed)
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
