//! Descriptors for invitation-based peer connection.
//!
//! # Invariants
//!
//! - `valid_from_ms <= valid_until_ms` (enforced by constructor)
//! - PSK commitment is hash of PSK derived from invitation secret
//! - At least one transport hint should be provided

#![forbid(unsafe_code)]

use aura_core::identifiers::{AuthorityId, InvitationId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum transport hints per descriptor.
pub const TRANSPORT_HINTS_MAX: usize = 8;

/// Typed direct endpoint address for invitation transport hints.
///
/// Serialized as a standard socket-address string (e.g. `192.168.1.1:8443`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DirectEndpointAddr {
    address: String,
    port: u16,
}

impl DirectEndpointAddr {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let trimmed = raw.trim();
        if trimmed != raw {
            return Err(format!(
                "invalid direct endpoint address '{raw}': address contains leading/trailing whitespace"
            ));
        }

        let port = parse_direct_endpoint_port(trimmed)
            .map_err(|reason| format!("invalid direct endpoint address '{raw}': {reason}"))?;

        Ok(Self {
            address: trimmed.to_string(),
            port,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.address
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl fmt::Display for DirectEndpointAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address)
    }
}

impl Serialize for DirectEndpointAddr {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for DirectEndpointAddr {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        DirectEndpointAddr::parse(&raw).map_err(serde::de::Error::custom)
    }
}

fn parse_direct_endpoint_port(addr: &str) -> Result<u16, &'static str> {
    if addr.is_empty() {
        return Err("address is empty");
    }

    if addr.chars().any(|c| c.is_whitespace()) {
        return Err("address contains whitespace");
    }

    if addr.contains("://") {
        return Err("address must not include a scheme");
    }

    let (host, port_str) = if let Some(remainder) = addr.strip_prefix('[') {
        let end = remainder
            .find(']')
            .ok_or("IPv6 address missing closing ']'")?;
        let host = &remainder[..end];
        let after = &remainder[end + 1..];
        let port_str = after
            .strip_prefix(':')
            .ok_or("missing port separator ':'")?;
        if host.is_empty() {
            return Err("IPv6 host is empty");
        }
        (host, port_str)
    } else {
        let idx = addr.rfind(':').ok_or("missing port separator ':'")?;
        let host = &addr[..idx];
        let port_str = &addr[idx + 1..];
        if host.is_empty() {
            return Err("host is empty");
        }
        if host.contains(':') {
            return Err("IPv6 addresses must be enclosed in brackets");
        }
        (host, port_str)
    };

    if host.contains('/') {
        return Err("host must not contain '/'");
    }

    if port_str.is_empty() {
        return Err("port is empty");
    }

    port_str
        .parse::<u16>()
        .map_err(|_| "port must be a valid u16 value")
}

/// Transport hint for establishing a connection.
///
/// Hints are tried in order of preference (first = highest priority).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportHint {
    /// Direct QUIC connection.
    QuicDirect {
        /// Socket address (e.g., "192.168.1.1:8443").
        addr: DirectEndpointAddr,
    },
    /// QUIC via STUN-discovered reflexive address.
    QuicReflexive {
        /// Reflexive address discovered via STUN.
        addr: DirectEndpointAddr,
        /// STUN server used for discovery.
        stun_server: String,
    },
    /// WebSocket relay through a relay authority.
    WebSocketRelay {
        /// Authority providing relay service.
        relay_authority: AuthorityId,
    },
    /// TCP direct connection.
    TcpDirect {
        /// Socket address.
        addr: DirectEndpointAddr,
    },
}

/// Descriptor for invitation-based connections.
///
/// Unlike `RendezvousDescriptor` (context-scoped), this is invitation-scoped
/// and derives its PSK from the invitation secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteDescriptor {
    /// Authority publishing this descriptor.
    pub authority_id: AuthorityId,

    /// Invitation this descriptor is for.
    pub invitation_id: InvitationId,

    /// Transport hints for connection (tried in order).
    ///
    /// Limited to `TRANSPORT_HINTS_MAX` entries.
    pub transport_hints: Vec<TransportHint>,

    /// Nickname suggestion for the inviter.
    ///
    /// Shown in UI when previewing the invitation.
    pub nickname_suggestion: Option<String>,

    /// PSK commitment (32-byte hash of PSK derived from invitation secret).
    ///
    /// Used during Noise handshake to verify both parties share the secret.
    pub psk_commitment: [u8; 32],

    /// Validity window start (milliseconds since Unix epoch).
    pub valid_from_ms: u64,

    /// Validity window end (milliseconds since Unix epoch).
    ///
    /// Invariant: `valid_from_ms <= valid_until_ms`.
    pub valid_until_ms: u64,

    /// Nonce for uniqueness (32 bytes).
    ///
    /// Prevents descriptor reuse and provides entropy for PSK derivation.
    pub nonce: [u8; 32],
}

impl InviteDescriptor {
    /// Check if the descriptor is valid at the given time.
    #[must_use]
    pub fn is_valid_at(&self, now_ms: u64) -> bool {
        now_ms >= self.valid_from_ms && now_ms < self.valid_until_ms
    }

    /// Check if the descriptor has expired.
    #[must_use]
    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.valid_until_ms
    }

    /// Duration until expiration (0 if already expired).
    #[must_use]
    pub fn ttl_ms(&self, now_ms: u64) -> u64 {
        self.valid_until_ms.saturating_sub(now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_invitation() -> InvitationId {
        InvitationId::new("inv-test-123")
    }

    fn test_descriptor() -> InviteDescriptor {
        InviteDescriptor {
            authority_id: test_authority(),
            invitation_id: test_invitation(),
            transport_hints: vec![TransportHint::QuicDirect {
                addr: DirectEndpointAddr::parse("192.168.1.1:8443")
                    .unwrap_or_else(|error| panic!("valid socket address: {error}")),
            }],
            nickname_suggestion: Some("Alice".to_string()),
            psk_commitment: [0u8; 32],
            valid_from_ms: 1000,
            valid_until_ms: 2000,
            nonce: [0u8; 32],
        }
    }

    #[test]
    fn test_is_valid_at() {
        let desc = test_descriptor();
        assert!(!desc.is_valid_at(999)); // Before start
        assert!(desc.is_valid_at(1000)); // At start
        assert!(desc.is_valid_at(1500)); // In middle
        assert!(desc.is_valid_at(1999)); // Just before end
        assert!(!desc.is_valid_at(2000)); // At end (exclusive)
    }

    #[test]
    fn test_is_expired() {
        let desc = test_descriptor();
        assert!(!desc.is_expired(1500));
        assert!(desc.is_expired(2000));
        assert!(desc.is_expired(3000));
    }

    #[test]
    fn test_ttl_ms() {
        let desc = test_descriptor();
        assert_eq!(desc.ttl_ms(1000), 1000);
        assert_eq!(desc.ttl_ms(1500), 500);
        assert_eq!(desc.ttl_ms(2000), 0);
        assert_eq!(desc.ttl_ms(3000), 0);
    }

    #[test]
    fn test_reject_invalid_direct_endpoint() {
        let parsed = DirectEndpointAddr::parse("not-an-address");
        assert!(parsed.is_err());
    }
}
