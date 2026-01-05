//! Rendezvous Domain Facts
//!
//! Fact types for peer discovery and channel establishment.
//! These facts are stored in context journals and propagated via `aura-sync`.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::extensibility::FactReducer;
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use aura_journal::DomainFact;
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

const CHANNEL_CONTEXT_DOMAIN: &[u8] = b"AURA_RENDEZVOUS_CHANNEL_CONTEXT";

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

/// Derive a deterministic context id for a rendezvous channel between two authorities.
///
/// The derivation is commutative so initiator/responder ordering does not matter.
fn channel_context_id(initiator: &AuthorityId, responder: &AuthorityId) -> ContextId {
    let mut a = initiator.to_bytes();
    let mut b = responder.to_bytes();
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let mut hasher = Sha256::new();
    hasher.update(CHANNEL_CONTEXT_DOMAIN);
    hasher.update(a);
    hasher.update(b);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    ContextId::new_from_entropy(out)
}

/// Type identifier for rendezvous facts
pub const RENDEZVOUS_FACT_TYPE_ID: &str = "rendezvous";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendezvousFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Rendezvous domain facts stored in context journals
#[derive(Debug, Clone, Serialize, Deserialize, DomainFact)]
#[domain_fact(
    type_id = "rendezvous",
    schema_version = 1,
    context_fn = "context_id_for_fact"
)]
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

impl RendezvousFact {
    /// Derive the binding key data used by the reducer.
    pub fn binding_key(&self) -> RendezvousFactKey {
        match self {
            RendezvousFact::Descriptor(descriptor) => {
                let mut key = authority_hash_bytes(&descriptor.authority_id).to_vec();
                key.extend_from_slice(&descriptor.nonce);
                RendezvousFactKey {
                    sub_type: "rendezvous-descriptor",
                    data: key,
                }
            }
            RendezvousFact::ChannelEstablished { channel_id, .. } => RendezvousFactKey {
                sub_type: "rendezvous-channel-established",
                data: channel_id.to_vec(),
            },
            RendezvousFact::DescriptorRevoked {
                authority_id,
                nonce,
            } => {
                let mut key = authority_hash_bytes(authority_id).to_vec();
                key.extend_from_slice(nonce);
                RendezvousFactKey {
                    sub_type: "rendezvous-descriptor-revoked",
                    data: key,
                }
            }
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        match self {
            RendezvousFact::Descriptor(descriptor) => descriptor.context_id == context_id,
            RendezvousFact::ChannelEstablished {
                initiator,
                responder,
                ..
            } => context_id == channel_context_id(initiator, responder),
            RendezvousFact::DescriptorRevoked { authority_id, .. } => {
                context_id == ContextId::new_from_entropy(authority_hash_bytes(authority_id))
            }
        }
    }
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
    /// What this peer wants to be called (optional, for UI purposes)
    #[serde(default)]
    pub nickname_suggestion: Option<String>,
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

// =============================================================================
// Transport Address Types
// =============================================================================

/// Error type for transport address parsing/validation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportAddressError {
    /// The invalid address string
    pub input: String,
    /// Description of what went wrong
    pub reason: String,
}

impl fmt::Display for TransportAddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid transport address '{}': {}",
            self.input, self.reason
        )
    }
}

impl std::error::Error for TransportAddressError {}

/// A validated transport address (IP:port format).
///
/// This type ensures that transport addresses are always valid socket addresses.
/// It stores the address as a string and serializes as a string for backwards
/// compatibility with existing fact storage.
///
/// # Example
///
/// ```
/// use aura_rendezvous::facts::TransportAddress;
///
/// let addr = TransportAddress::new("127.0.0.1:8080").unwrap();
/// assert_eq!(addr.to_string(), "127.0.0.1:8080");
///
/// // Invalid addresses are rejected
/// assert!(TransportAddress::new("not-an-address").is_err());
/// assert!(TransportAddress::new("127.0.0.1").is_err()); // missing port
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransportAddress {
    address: String,
    port: u16,
}

impl TransportAddress {
    /// Create a new transport address from a string.
    ///
    /// The string must be a valid socket address in the format "IP:port".
    /// Both IPv4 and IPv6 addresses are supported.
    pub fn new(addr: &str) -> Result<Self, TransportAddressError> {
        let trimmed = addr.trim();
        if trimmed != addr {
            return Err(TransportAddressError {
                input: addr.to_string(),
                reason: "address contains leading/trailing whitespace".to_string(),
            });
        }

        let port = parse_transport_port(trimmed)?;

        Ok(Self {
            address: trimmed.to_string(),
            port,
        })
    }

    /// Get the address as a string slice.
    pub fn as_str(&self) -> &str {
        &self.address
    }

    /// Get the port number.
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl fmt::Display for TransportAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address)
    }
}

impl FromStr for TransportAddress {
    type Err = TransportAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        TransportAddress::new(s)
    }
}

impl Serialize for TransportAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as string for backwards compatibility
        serializer.serialize_str(&self.address)
    }
}

impl<'de> Deserialize<'de> for TransportAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        TransportAddress::new(&s).map_err(serde::de::Error::custom)
    }
}

fn parse_transport_port(addr: &str) -> Result<u16, TransportAddressError> {
    if addr.is_empty() {
        return Err(TransportAddressError {
            input: addr.to_string(),
            reason: "address is empty".to_string(),
        });
    }

    if addr.chars().any(|c| c.is_whitespace()) {
        return Err(TransportAddressError {
            input: addr.to_string(),
            reason: "address contains whitespace".to_string(),
        });
    }

    if addr.contains("://") {
        return Err(TransportAddressError {
            input: addr.to_string(),
            reason: "address must not include a scheme".to_string(),
        });
    }

    let (host, port_str) = if let Some(remainder) = addr.strip_prefix('[') {
        let end = remainder.find(']').ok_or_else(|| TransportAddressError {
            input: addr.to_string(),
            reason: "IPv6 address missing closing ']'".to_string(),
        })?;

        let host = &remainder[..end];
        let after = &remainder[end + 1..];
        let port_str = after
            .strip_prefix(':')
            .ok_or_else(|| TransportAddressError {
                input: addr.to_string(),
                reason: "missing port separator ':'".to_string(),
            })?;

        if host.is_empty() {
            return Err(TransportAddressError {
                input: addr.to_string(),
                reason: "IPv6 host is empty".to_string(),
            });
        }

        (host, port_str)
    } else {
        let idx = addr.rfind(':').ok_or_else(|| TransportAddressError {
            input: addr.to_string(),
            reason: "missing port separator ':'".to_string(),
        })?;

        let host = &addr[..idx];
        let port_str = &addr[idx + 1..];

        if host.is_empty() {
            return Err(TransportAddressError {
                input: addr.to_string(),
                reason: "host is empty".to_string(),
            });
        }

        if host.contains(':') {
            return Err(TransportAddressError {
                input: addr.to_string(),
                reason: "IPv6 addresses must be enclosed in brackets".to_string(),
            });
        }

        (host, port_str)
    };

    if host.contains('/') {
        return Err(TransportAddressError {
            input: addr.to_string(),
            reason: "host must not contain '/'".to_string(),
        });
    }

    if port_str.is_empty() {
        return Err(TransportAddressError {
            input: addr.to_string(),
            reason: "port is empty".to_string(),
        });
    }

    let port = port_str.parse::<u16>().map_err(|_| TransportAddressError {
        input: addr.to_string(),
        reason: "port must be a valid u16 value".to_string(),
    })?;

    Ok(port)
}

// =============================================================================
// Transport Hint
// =============================================================================

/// Transport endpoint hint
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportHint {
    /// Direct QUIC connection
    QuicDirect { addr: TransportAddress },
    /// QUIC via STUN-discovered reflexive address
    QuicReflexive {
        addr: TransportAddress,
        stun_server: TransportAddress,
    },
    /// WebSocket relay through a relay authority
    WebSocketRelay { relay_authority: AuthorityId },
    /// TCP direct connection
    TcpDirect { addr: TransportAddress },
}

impl TransportHint {
    /// Create a QuicDirect hint, validating the address.
    pub fn quic_direct(addr: &str) -> Result<Self, TransportAddressError> {
        Ok(TransportHint::QuicDirect {
            addr: TransportAddress::new(addr)?,
        })
    }

    /// Create a QuicReflexive hint, validating both addresses.
    pub fn quic_reflexive(addr: &str, stun_server: &str) -> Result<Self, TransportAddressError> {
        Ok(TransportHint::QuicReflexive {
            addr: TransportAddress::new(addr)?,
            stun_server: TransportAddress::new(stun_server)?,
        })
    }

    /// Create a TcpDirect hint, validating the address.
    pub fn tcp_direct(addr: &str) -> Result<Self, TransportAddressError> {
        Ok(TransportHint::TcpDirect {
            addr: TransportAddress::new(addr)?,
        })
    }

    /// Create a WebSocketRelay hint.
    pub fn websocket_relay(relay_authority: AuthorityId) -> Self {
        TransportHint::WebSocketRelay { relay_authority }
    }

    /// Get the primary address for this hint, if any.
    pub fn primary_address(&self) -> Option<&TransportAddress> {
        match self {
            TransportHint::QuicDirect { addr } => Some(addr),
            TransportHint::QuicReflexive { addr, .. } => Some(addr),
            TransportHint::TcpDirect { addr } => Some(addr),
            TransportHint::WebSocketRelay { .. } => None,
        }
    }

    /// Get the address as a string, if this hint has an address.
    pub fn address_string(&self) -> Option<String> {
        self.primary_address().map(|a| a.to_string())
    }
}

impl RendezvousFact {
    pub fn context_id_for_fact(&self) -> ContextId {
        match self {
            RendezvousFact::Descriptor(d) => d.context_id,
            RendezvousFact::ChannelEstablished {
                initiator,
                responder,
                ..
            } => {
                // Channel facts are scoped to a deterministic pairwise context.
                channel_context_id(initiator, responder)
            }
            RendezvousFact::DescriptorRevoked { authority_id, .. } => {
                // Revocation context derived from authority
                ContextId::new_from_entropy(authority_hash_bytes(authority_id))
            }
        }
    }

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

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != RENDEZVOUS_FACT_TYPE_ID {
            return None;
        }

        let fact = RendezvousFact::from_envelope(envelope)?;
        if !fact.validate_for_reduction(context_id) {
            return None;
        }

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 1000,
            valid_until: 2000,
            nonce: [42u8; 32],
            nickname_suggestion: None,
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
            nickname_suggestion: None,
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
            nickname_suggestion: None,
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
    fn test_channel_context_is_commutative() {
        let a = test_authority();
        let b = AuthorityId::new_from_entropy([9u8; 32]);
        assert_eq!(channel_context_id(&a, &b), channel_context_id(&b, &a));
    }

    proptest! {
        #[test]
        fn prop_channel_context_deterministic(seed_a in any::<[u8; 32]>(), seed_b in any::<[u8; 32]>()) {
            let a = AuthorityId::new_from_entropy(seed_a);
            let b = AuthorityId::new_from_entropy(seed_b);
            let ctx1 = channel_context_id(&a, &b);
            let ctx2 = channel_context_id(&a, &b);
            prop_assert_eq!(ctx1, ctx2);
        }

        #[test]
        fn prop_channel_context_distinct_for_pairs(
            seed_a in any::<[u8; 32]>(),
            seed_b in any::<[u8; 32]>(),
            seed_c in any::<[u8; 32]>()
        ) {
            prop_assume!(seed_a != seed_b);
            prop_assume!(seed_a != seed_c);
            prop_assume!(seed_b != seed_c);
            let a = AuthorityId::new_from_entropy(seed_a);
            let b = AuthorityId::new_from_entropy(seed_b);
            let c = AuthorityId::new_from_entropy(seed_c);
            let ctx_ab = channel_context_id(&a, &b);
            let ctx_ac = channel_context_id(&a, &c);
            prop_assert_ne!(ctx_ab, ctx_ac);
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
            nickname_suggestion: None,
        };

        let fact = RendezvousFact::Descriptor(descriptor);
        let envelope = fact.to_envelope();

        let binding = reducer.reduce_envelope(test_context(), &envelope);
        assert!(binding.is_some());
    }

    #[test]
    fn test_reducer_rejects_context_mismatch_for_descriptor() {
        let reducer = RendezvousFactReducer;

        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 1000,
            nonce: [7u8; 32],
            nickname_suggestion: None,
        };

        let fact = RendezvousFact::Descriptor(descriptor);
        let other_context = ContextId::new_from_entropy([9u8; 32]);
        let binding = reducer.reduce_envelope(other_context, &fact.to_envelope());
        assert!(binding.is_none());
    }

    #[test]
    fn test_binding_key_derivation() {
        let fact = RendezvousFact::DescriptorRevoked {
            authority_id: test_authority(),
            nonce: [9u8; 32],
        };

        let key = fact.binding_key();
        assert_eq!(key.sub_type, "rendezvous-descriptor-revoked");
        assert_eq!(key.data.len(), 64);
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = RendezvousFactReducer;
        let context_id = test_context();
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id,
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 1000,
            nonce: [7u8; 32],
            nickname_suggestion: None,
        };

        let fact = RendezvousFact::Descriptor(descriptor);
        let envelope = fact.to_envelope();
        let binding1 = reducer.reduce_envelope(context_id, &envelope);
        let binding2 = reducer.reduce_envelope(context_id, &envelope);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
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
            nickname_suggestion: None,
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
