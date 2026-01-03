//! Participant identity and addressing types.
//!
//! These types describe "who can participate" in multi-party protocols such as
//! threshold signing and how to reach them.

use crate::{AuthorityId, DeviceId, RelayId};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::NonZeroU16;
use thiserror::Error;

/// Threshold value for FROST key-rotation ceremonies (k-of-n signing).
///
/// This type enforces FROST's requirement that threshold must be >= 2 for
/// multi-party signing. Single-signer (1-of-1) configurations should use
/// Ed25519 directly instead of FROST.
///
/// Used by all ceremony types: guardian key rotation, device enrollment,
/// device removal, and group/home membership changes.
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
    pub const MINIMUM_THRESHOLD_COUNT: u16 = 2;

    /// Create a new FROST threshold, validating the minimum.
    ///
    /// Returns `Err` if `value < 2` since FROST requires at least 2 signers.
    pub fn new(value: u16) -> Result<Self, InvalidThresholdError> {
        if value < Self::MINIMUM_THRESHOLD_COUNT {
            Err(InvalidThresholdError {
                value,
                minimum: Self::MINIMUM_THRESHOLD_COUNT,
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
        Self(Self::MINIMUM_THRESHOLD_COUNT)
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
            Self::Device(id) => format!("Device:{id}"),
            Self::Guardian(id) => format!("Guardian:{id}"),
            Self::GroupMember { group, member } => {
                format!("GroupMember:{group}:{member}")
            }
        }
    }

    /// Stable key for storage paths and maps.
    ///
    /// This is intended for persistence (e.g. `SecureStorageLocation` subkeys),
    /// so it avoids characters that tend to be awkward in filesystem-like keys.
    pub fn storage_key(&self) -> String {
        match self {
            Self::Device(id) => format!("device_{id}"),
            Self::Guardian(id) => format!("guardian_{id}"),
            Self::GroupMember { group, member } => {
                format!("group_{group}_member_{member}")
            }
        }
    }
}

/// Validated network address for participant endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct NetworkAddress(String);

/// Errors that can occur when parsing a network address.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NetworkAddressError {
    /// Address is empty.
    #[error("network address is empty")]
    Empty,
    /// Address contains whitespace or control characters.
    #[error("network address contains invalid characters")]
    InvalidCharacters,
    /// Address must include a host and port.
    #[error("network address must include a host and port")]
    MissingPort,
    /// Address has an invalid port.
    #[error("network address has an invalid port")]
    InvalidPort,
    /// Address has an invalid host.
    #[error("network address has an invalid host")]
    InvalidHost,
}

impl NetworkAddress {
    /// Parse and validate a network address.
    pub fn parse(input: &str) -> Result<Self, NetworkAddressError> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(NetworkAddressError::Empty);
        }
        if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(NetworkAddressError::InvalidCharacters);
        }

        let (host, port_str) = split_host_port(trimmed).ok_or(NetworkAddressError::MissingPort)?;
        let port: u16 = port_str
            .parse()
            .map_err(|_| NetworkAddressError::InvalidPort)?;
        if port == 0 {
            return Err(NetworkAddressError::InvalidPort);
        }
        if !valid_host(host) {
            return Err(NetworkAddressError::InvalidHost);
        }

        Ok(Self(trimmed.to_string()))
    }

    /// Access the normalized address string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NetworkAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for NetworkAddress {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for NetworkAddress {
    type Error = NetworkAddressError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        NetworkAddress::parse(&value)
    }
}

impl TryFrom<&str> for NetworkAddress {
    type Error = NetworkAddressError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        NetworkAddress::parse(value)
    }
}

impl From<NetworkAddress> for String {
    fn from(address: NetworkAddress) -> Self {
        address.0
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
        relay_id: RelayId,
        /// Participant's address on the relay
        address: NetworkAddress,
    },

    /// Direct peer-to-peer connection
    Direct {
        /// Network address (e.g., IP:port, hostname)
        address: NetworkAddress,
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
    pub signer_index: NonZeroU16,
    /// How to reach them for coordination
    pub endpoint: ParticipantEndpoint,
}

/// Errors when validating signer indices.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SignerIndexError {
    /// Signer index must be non-zero.
    #[error("signer index must be non-zero")]
    Zero,
}

impl SigningParticipant {
    /// Create a new signing participant
    pub fn new(
        identity: ParticipantIdentity,
        signer_index: NonZeroU16,
        endpoint: ParticipantEndpoint,
    ) -> Self {
        Self {
            identity,
            signer_index,
            endpoint,
        }
    }

    /// Create a new signing participant with checked signer index.
    pub fn try_new(
        identity: ParticipantIdentity,
        signer_index: u16,
        endpoint: ParticipantEndpoint,
    ) -> Result<Self, SignerIndexError> {
        let signer_index = NonZeroU16::new(signer_index).ok_or(SignerIndexError::Zero)?;
        Ok(Self::new(identity, signer_index, endpoint))
    }

    /// Create a local device participant
    pub fn local_device(device_id: DeviceId, signer_index: u16) -> Result<Self, SignerIndexError> {
        Self::try_new(
            ParticipantIdentity::Device(device_id),
            signer_index,
            ParticipantEndpoint::Local,
        )
    }

    /// Create a remote guardian participant (relay-routed)
    pub fn remote_guardian(
        authority: AuthorityId,
        signer_index: u16,
        relay_id: RelayId,
        address: NetworkAddress,
    ) -> Result<Self, SignerIndexError> {
        Self::try_new(
            ParticipantIdentity::Guardian(authority),
            signer_index,
            ParticipantEndpoint::Relay { relay_id, address },
        )
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

fn split_host_port(input: &str) -> Option<(&str, &str)> {
    if let Some(stripped) = input.strip_prefix('[') {
        let end = stripped.find(']')?;
        let host = &stripped[..end];
        let rest = &stripped[end + 1..];
        let port = rest.strip_prefix(':')?;
        if port.is_empty() {
            return None;
        }
        return Some((host, port));
    }

    let (host, port) = input.rsplit_once(':')?;
    if host.is_empty() || port.is_empty() {
        return None;
    }
    Some((host, port))
}

fn valid_host(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    if host.contains(':') {
        let has_hex = host.chars().any(|c| c.is_ascii_hexdigit());
        return has_hex && host.chars().all(|c| c.is_ascii_hexdigit() || c == ':');
    }
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        if host.starts_with('.') || host.ends_with('.') {
            return false;
        }
        return host.split('.').all(|segment| {
            !segment.is_empty() && segment.len() <= 3 && segment.parse::<u8>().is_ok()
        });
    }
    if host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    if host.starts_with('-') || host.ends_with('-') {
        return false;
    }
    host.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_address_validation() {
        let addr = NetworkAddress::parse("example.com:443").unwrap();
        assert_eq!(addr.as_str(), "example.com:443");
        assert!(matches!(
            NetworkAddress::parse("example.com"),
            Err(NetworkAddressError::MissingPort)
        ));
        assert!(matches!(
            NetworkAddress::parse("example.com:0"),
            Err(NetworkAddressError::InvalidPort)
        ));
        assert!(matches!(
            NetworkAddress::parse("example.com:99999"),
            Err(NetworkAddressError::InvalidPort)
        ));
        assert!(matches!(
            NetworkAddress::parse("bad host:443"),
            Err(NetworkAddressError::InvalidCharacters)
        ));
    }

    #[test]
    fn signer_index_validation() {
        let identity = ParticipantIdentity::device(DeviceId::new_from_entropy([1u8; 32]));
        let endpoint = ParticipantEndpoint::Local;

        assert!(matches!(
            SigningParticipant::try_new(identity.clone(), 0, endpoint.clone()),
            Err(SignerIndexError::Zero)
        ));
        assert!(SigningParticipant::try_new(identity, 1, endpoint).is_ok());
    }
}
