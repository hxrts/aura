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

/// Maximum transport hints per descriptor.
pub const TRANSPORT_HINTS_MAX: usize = 8;

/// Transport hint for establishing a connection.
///
/// Hints are tried in order of preference (first = highest priority).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransportHint {
    /// Direct QUIC connection.
    QuicDirect {
        /// Socket address (e.g., "192.168.1.1:8443").
        addr: String,
    },
    /// QUIC via STUN-discovered reflexive address.
    QuicReflexive {
        /// Reflexive address discovered via STUN.
        addr: String,
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
        addr: String,
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
                addr: "192.168.1.1:8443".to_string(),
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
}
