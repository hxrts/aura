//! Transport Descriptor and Probing
//!
//! This module provides descriptor building and transport probing for peer
//! discovery. Final establish-path selection is runtime-owned and lives in
//! `aura-agent`.

use crate::authority_hash::authority_hash_bytes;
use crate::facts::{RendezvousDescriptor, TransportAddress, TransportHint};
use aura_core::hash;
use aura_core::service::{LinkEndpoint, LinkProtocol};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult};

// =============================================================================
// Descriptor Builder
// =============================================================================

/// Builds transport descriptors for publication
#[derive(Debug, Clone)]
pub struct DescriptorBuilder {
    /// Local authority ID
    authority_id: AuthorityId,
    /// Default validity duration in milliseconds
    validity_ms: u64,
    /// STUN server for reflexive address discovery
    stun_server: Option<String>,
}

impl DescriptorBuilder {
    fn direct_hints_from_local_addresses(
        &self,
        local_addresses: &[String],
    ) -> AuraResult<Vec<TransportHint>> {
        local_addresses
            .iter()
            .map(|addr_str| {
                TransportHint::tcp_direct(addr_str)
                    .map_err(|e| AuraError::invalid(format!("Invalid transport address: {e}")))
            })
            .collect()
    }

    /// Create a new descriptor builder
    pub fn new(authority_id: AuthorityId, validity_ms: u64, stun_server: Option<String>) -> Self {
        Self {
            authority_id,
            validity_ms,
            stun_server,
        }
    }

    /// Build a descriptor with the given transport hints and identity public key
    pub fn build(
        &self,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        public_key: [u8; 32],
        now_ms: u64,
    ) -> RendezvousDescriptor {
        let nonce = generate_nonce(&self.authority_id, context_id, now_ms);
        let psk_commitment = compute_psk_commitment(context_id, &self.authority_id);

        RendezvousDescriptor {
            authority_id: self.authority_id,
            device_id: None,
            context_id,
            transport_hints,
            handshake_psk_commitment: psk_commitment,
            public_key,
            valid_from: now_ms,
            valid_until: now_ms + self.validity_ms,
            nonce,
            nickname_suggestion: None,
        }
    }

    /// Build a descriptor with automatic transport hint discovery
    ///
    /// # Arguments
    /// * `context_id` - The context for this descriptor
    /// * `public_key` - Identity public key
    /// * `local_addresses` - Local addresses to advertise (must be valid socket addresses)
    /// * `now_ms` - Current timestamp in milliseconds
    /// * `prober` - Transport prober for STUN discovery
    ///
    /// # Errors
    /// Returns an error if any local address is invalid.
    pub async fn build_with_discovery(
        &self,
        context_id: ContextId,
        public_key: [u8; 32],
        local_addresses: Vec<String>,
        now_ms: u64,
        prober: &TransportProber,
    ) -> AuraResult<RendezvousDescriptor> {
        let mut hints = self.direct_hints_from_local_addresses(&local_addresses)?;

        // Try to discover reflexive address via STUN
        if let Some(stun_server) = &self.stun_server {
            if let Ok(reflexive_addr_str) = prober.stun_probe(stun_server).await {
                // Parse both addresses
                if let (Ok(reflexive_addr), Ok(stun_addr)) = (
                    TransportAddress::new(&reflexive_addr_str),
                    TransportAddress::new(stun_server),
                ) {
                    hints.insert(
                        0,
                        TransportHint::QuicReflexive {
                            addr: reflexive_addr,
                            stun_server: stun_addr,
                            bound_local: None,
                        },
                    );
                }
            }
        }

        Ok(self.build(context_id, hints, public_key, now_ms))
    }
}

// =============================================================================
// Transport Prober
// =============================================================================

/// Probes transport endpoints for connectivity
pub struct TransportProber {
    /// Timeout for probes in milliseconds
    timeout_ms: u64,
    /// STUN server configuration
    #[allow(dead_code)]
    stun_config: Option<StunConfig>,
}

/// STUN server configuration
#[derive(Debug, Clone)]
pub struct StunConfig {
    /// Primary STUN server address
    pub primary: String,
    /// Fallback STUN server address
    pub fallback: Option<String>,
    /// Timeout for STUN requests in milliseconds
    pub timeout_ms: u64,
}

impl TransportProber {
    /// Create a new transport prober
    pub fn new(timeout_ms: u64) -> Self {
        Self {
            timeout_ms,
            stun_config: None,
        }
    }

    /// Create a prober with STUN configuration
    pub fn with_stun(timeout_ms: u64, stun_config: StunConfig) -> Self {
        Self {
            timeout_ms,
            stun_config: Some(stun_config),
        }
    }

    /// Get the probe timeout
    pub fn timeout_ms(&self) -> u64 {
        self.timeout_ms
    }

    /// Probe an endpoint for connectivity
    ///
    /// Currently returns success unconditionally. Full implementation will
    /// perform actual TCP/QUIC connection attempts with the configured timeout.
    pub async fn probe_endpoint(&self, _addr: &str) -> AuraResult<()> {
        // Full implementation will:
        // 1. Attempt TCP or QUIC connection to addr
        // 2. Apply timeout from self.timeout_ms
        // 3. Return Err if connection fails
        Ok(())
    }

    /// Perform STUN probe to discover reflexive address
    pub async fn stun_probe(&self, stun_server: &str) -> AuraResult<String> {
        TransportAddress::new(stun_server)
            .map_err(|e| AuraError::invalid(format!("invalid STUN server address: {e}")))?;
        Err(AuraError::network(
            "stun_probe unavailable in aura-rendezvous layer; perform probing via runtime NetworkEffects",
        ))
    }

    /// Probe all split connectivity endpoints in a descriptor and return reachable ones
    pub async fn probe_descriptor(
        &self,
        descriptor: &RendezvousDescriptor,
    ) -> Vec<(LinkEndpoint, bool)> {
        let mut results = Vec::new();

        for endpoint in descriptor.advertised_link_endpoints() {
            let reachable = match endpoint.protocol {
                LinkProtocol::Quic | LinkProtocol::Tcp | LinkProtocol::WebSocket => {
                    if let Some(address) = endpoint.address.as_deref() {
                        self.probe_endpoint(address).await.is_ok()
                    } else {
                        false
                    }
                }
                LinkProtocol::QuicReflexive => {
                    let stun_ok = if let Some(stun_server) = endpoint.stun_server.as_deref() {
                        self.stun_probe(stun_server).await.is_ok()
                    } else {
                        false
                    };
                    let direct_ok = if let Some(address) = endpoint.address.as_deref() {
                        self.probe_endpoint(address).await.is_ok()
                    } else {
                        false
                    };
                    stun_ok || direct_ok
                }
                LinkProtocol::WebSocketRelay => true,
            };
            results.push((endpoint, reachable));
        }

        results
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate a nonce for descriptor uniqueness
fn generate_nonce(authority_id: &AuthorityId, context_id: ContextId, now_ms: u64) -> [u8; 32] {
    let mut hasher = hash::hasher();
    hasher.update(&authority_hash_bytes(authority_id));
    hasher.update(context_id.as_bytes());
    hasher.update(&now_ms.to_le_bytes());
    hasher.finalize()
}

/// Compute PSK commitment from context and authority
///
/// Uses a deterministic hash of context + authority. Full implementation
/// will derive the PSK from the context's shared secret.
fn compute_psk_commitment(context_id: ContextId, authority_id: &AuthorityId) -> [u8; 32] {
    let mut hasher = hash::hasher();
    hasher.update(b"PSK_COMMITMENT_V1");
    hasher.update(context_id.as_bytes());
    hasher.update(&authority_hash_bytes(authority_id));
    hasher.finalize()
}

// =============================================================================
// Tests
// =============================================================================

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
    fn test_descriptor_builder() {
        let builder = DescriptorBuilder::new(test_authority(), 3_600_000, None);

        let hints = vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()];

        let descriptor = builder.build(test_context(), hints, [0u8; 32], 1000);

        assert_eq!(descriptor.authority_id, test_authority());
        assert_eq!(descriptor.context_id, test_context());
        assert_eq!(descriptor.valid_from, 1000);
        assert_eq!(descriptor.valid_until, 1000 + 3_600_000);
        assert_eq!(descriptor.transport_hints.len(), 1);
        assert_eq!(descriptor.public_key, [0u8; 32]);
    }

    #[test]
    fn test_descriptor_builder_with_public_key() {
        let pubkey = [42u8; 32];
        let builder = DescriptorBuilder::new(test_authority(), 3_600_000, None);

        let hints = vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()];

        let descriptor = builder.build(test_context(), hints, pubkey, 1000);
        assert_eq!(descriptor.public_key, pubkey);
    }
}
