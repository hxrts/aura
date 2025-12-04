//! Rendezvous Domain Facts
//!
//! Fact types for peer discovery and channel establishment.
//! These facts are stored in context journals and propagated via `aura-sync`.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::extensibility::{DomainFact, FactReducer};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Convert an AuthorityId to a 32-byte hash for commitment/indexing purposes.
/// AuthorityId is 16 bytes (UUID), so we hash it to get a canonical 32-byte representation.
fn authority_hash_bytes(authority: &AuthorityId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(authority.to_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Type identifier for rendezvous facts
pub const RENDEZVOUS_FACT_TYPE_ID: &str = "rendezvous";

/// Rendezvous domain facts stored in context journals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendezvousFact {
    /// Transport descriptor advertisement
    Descriptor(RendezvousDescriptor),

    /// Channel established acknowledgment
    ChannelEstablished {
        initiator: AuthorityId,
        responder: AuthorityId,
        channel_id: [u8; 32],
        epoch: u64,
    },

    /// Descriptor revocation (peer is no longer reachable at these hints)
    DescriptorRevoked {
        authority_id: AuthorityId,
        nonce: [u8; 32],
    },
}

/// Transport descriptor for peer discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RendezvousDescriptor {
    /// Authority publishing this descriptor
    pub authority_id: AuthorityId,
    /// Context this descriptor is for
    pub context_id: ContextId,
    /// Available transport endpoints
    pub transport_hints: Vec<TransportHint>,
    /// Handshake PSK commitment (hash of PSK derived from context)
    pub handshake_psk_commitment: [u8; 32],
    /// Validity window start (ms since epoch)
    pub valid_from: u64,
    /// Validity window end (ms since epoch)
    pub valid_until: u64,
    /// Nonce for uniqueness
    pub nonce: [u8; 32],
}

impl RendezvousDescriptor {
    /// Check if descriptor is currently valid
    pub fn is_valid(&self, now_ms: u64) -> bool {
        now_ms >= self.valid_from && now_ms < self.valid_until
    }

    /// Check if descriptor needs refresh (within 10% of expiry)
    pub fn needs_refresh(&self, now_ms: u64) -> bool {
        let validity_window = self.valid_until.saturating_sub(self.valid_from);
        let refresh_threshold = self.valid_until.saturating_sub(validity_window / 10);
        now_ms >= refresh_threshold
    }
}

/// Transport endpoint hint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportHint {
    /// Direct QUIC connection
    QuicDirect { addr: String },
    /// QUIC via STUN-discovered reflexive address
    QuicReflexive { addr: String, stun_server: String },
    /// WebSocket relay through a relay authority
    WebSocketRelay { relay_authority: AuthorityId },
    /// TCP direct connection
    TcpDirect { addr: String },
}

impl DomainFact for RendezvousFact {
    fn type_id(&self) -> &'static str {
        RENDEZVOUS_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            RendezvousFact::Descriptor(d) => d.context_id,
            RendezvousFact::ChannelEstablished { .. } => {
                // Channel facts are global (no specific context)
                // Use a deterministic context derived from initiator+responder
                ContextId::new_from_entropy([0u8; 32])
            }
            RendezvousFact::DescriptorRevoked { authority_id, .. } => {
                // Revocation context derived from authority
                ContextId::new_from_entropy(authority_hash_bytes(authority_id))
            }
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("RendezvousFact serialization should not fail")
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

impl RendezvousFact {
    /// Get authority bindings for this fact (for journal indexing)
    pub fn authority_bindings(&self) -> Vec<[u8; 32]> {
        match self {
            RendezvousFact::Descriptor(d) => {
                vec![authority_hash_bytes(&d.authority_id)]
            }
            RendezvousFact::ChannelEstablished {
                initiator,
                responder,
                ..
            } => {
                vec![
                    authority_hash_bytes(initiator),
                    authority_hash_bytes(responder),
                ]
            }
            RendezvousFact::DescriptorRevoked { authority_id, .. } => {
                vec![authority_hash_bytes(authority_id)]
            }
        }
    }
}

/// Reducer for rendezvous facts
pub struct RendezvousFactReducer;

impl FactReducer for RendezvousFactReducer {
    fn handles_type(&self) -> &'static str {
        RENDEZVOUS_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != RENDEZVOUS_FACT_TYPE_ID {
            return None;
        }

        let fact: RendezvousFact = serde_json::from_slice(binding_data).ok()?;

        // Extract the primary key for this fact
        let key_data = match &fact {
            RendezvousFact::Descriptor(d) => {
                // Key: authority_id + nonce
                let mut key = authority_hash_bytes(&d.authority_id).to_vec();
                key.extend_from_slice(&d.nonce);
                key
            }
            RendezvousFact::ChannelEstablished { channel_id, .. } => {
                // Key: channel_id
                channel_id.to_vec()
            }
            RendezvousFact::DescriptorRevoked {
                authority_id,
                nonce,
            } => {
                // Key: authority_id + nonce
                let mut key = authority_hash_bytes(authority_id).to_vec();
                key.extend_from_slice(nonce);
                key
            }
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(RENDEZVOUS_FACT_TYPE_ID.to_string()),
            context_id,
            data: key_data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([2u8; 32])
    }

    #[test]
    fn test_descriptor_serialization() {
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![TransportHint::TcpDirect {
                addr: "127.0.0.1:8080".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 1000,
            valid_until: 2000,
            nonce: [42u8; 32],
        };

        let fact = RendezvousFact::Descriptor(descriptor);
        let bytes = fact.to_bytes();
        let restored = RendezvousFact::from_bytes(&bytes);

        assert!(restored.is_some());
        match restored.unwrap() {
            RendezvousFact::Descriptor(d) => {
                assert_eq!(d.transport_hints.len(), 1);
                assert_eq!(d.valid_from, 1000);
            }
            _ => panic!("Expected Descriptor variant"),
        }
    }

    #[test]
    fn test_descriptor_validity() {
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 1000,
            valid_until: 2000,
            nonce: [0u8; 32],
        };

        assert!(!descriptor.is_valid(500)); // Before valid_from
        assert!(descriptor.is_valid(1000)); // At valid_from
        assert!(descriptor.is_valid(1500)); // In range
        assert!(!descriptor.is_valid(2000)); // At valid_until (exclusive)
        assert!(!descriptor.is_valid(2500)); // After valid_until
    }

    #[test]
    fn test_descriptor_needs_refresh() {
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 1000,
            nonce: [0u8; 32],
        };

        // Refresh threshold is at 900 (10% before expiry)
        assert!(!descriptor.needs_refresh(800));
        assert!(descriptor.needs_refresh(900));
        assert!(descriptor.needs_refresh(950));
    }

    #[test]
    fn test_channel_established_serialization() {
        let fact = RendezvousFact::ChannelEstablished {
            initiator: test_authority(),
            responder: AuthorityId::new_from_entropy([3u8; 32]),
            channel_id: [99u8; 32],
            epoch: 5,
        };

        let bytes = fact.to_bytes();
        let restored = RendezvousFact::from_bytes(&bytes).unwrap();

        match restored {
            RendezvousFact::ChannelEstablished { epoch, .. } => {
                assert_eq!(epoch, 5);
            }
            _ => panic!("Expected ChannelEstablished variant"),
        }
    }

    #[test]
    fn test_reducer() {
        let reducer = RendezvousFactReducer;
        assert_eq!(reducer.handles_type(), RENDEZVOUS_FACT_TYPE_ID);

        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 1000,
            nonce: [7u8; 32],
        };

        let fact = RendezvousFact::Descriptor(descriptor);
        let bytes = fact.to_bytes();

        let binding = reducer.reduce(test_context(), RENDEZVOUS_FACT_TYPE_ID, &bytes);
        assert!(binding.is_some());
    }

    #[test]
    fn test_authority_bindings() {
        let auth1 = test_authority();
        let auth2 = AuthorityId::new_from_entropy([3u8; 32]);

        let descriptor_fact = RendezvousFact::Descriptor(RendezvousDescriptor {
            authority_id: auth1,
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 1000,
            nonce: [0u8; 32],
        });

        let bindings = descriptor_fact.authority_bindings();
        assert_eq!(bindings.len(), 1);

        let channel_fact = RendezvousFact::ChannelEstablished {
            initiator: auth1,
            responder: auth2,
            channel_id: [0u8; 32],
            epoch: 1,
        };

        let bindings = channel_fact.authority_bindings();
        assert_eq!(bindings.len(), 2);
    }
}
