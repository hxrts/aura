//! Transport Descriptor and Selection
//!
//! This module provides transport hint selection, descriptor building,
//! and transport probing for peer discovery.

use crate::facts::{RendezvousDescriptor, TransportAddress, TransportHint};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::{AuraError, AuraResult};
use sha2::{Digest, Sha256};

/// Convert an AuthorityId to a 32-byte hash for commitment/indexing purposes.
fn authority_hash_bytes(authority: &AuthorityId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(authority.to_bytes());
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

// =============================================================================
// Selected Transport
// =============================================================================

/// Result of transport selection - which transport method to use
#[derive(Debug, Clone)]
pub enum SelectedTransport {
    /// Direct connection to address
    Direct(String),
    /// Connection via relay authority
    Relayed(AuthorityId),
}

// =============================================================================
// Transport Selector
// =============================================================================

/// Selects the best available transport from a descriptor's hints
///
/// Priority order:
/// 1. Direct QUIC (lowest latency)
/// 2. Reflexive QUIC (NAT traversal)
/// 3. Direct TCP (fallback)
/// 4. WebSocket Relay (last resort)
pub struct TransportSelector {
    /// Timeout for probes in milliseconds
    probe_timeout_ms: u64,
}

impl TransportSelector {
    /// Create a new transport selector
    pub fn new(probe_timeout_ms: u64) -> Self {
        Self { probe_timeout_ms }
    }

    /// Get the probe timeout
    pub fn probe_timeout_ms(&self) -> u64 {
        self.probe_timeout_ms
    }

    /// Select best transport from descriptor
    ///
    /// This performs a quick selection based on hint type priority.
    /// For actual connectivity testing, use `TransportProber`.
    pub fn select(&self, descriptor: &RendezvousDescriptor) -> AuraResult<SelectedTransport> {
        // Priority: QuicDirect > QuicReflexive > TcpDirect > WebSocketRelay
        let mut best_direct: Option<&TransportAddress> = None;
        let mut best_reflexive: Option<&TransportAddress> = None;
        let mut best_tcp: Option<&TransportAddress> = None;
        let mut relay: Option<AuthorityId> = None;

        for hint in &descriptor.transport_hints {
            match hint {
                TransportHint::QuicDirect { addr } => {
                    if best_direct.is_none() {
                        best_direct = Some(addr);
                    }
                }
                TransportHint::QuicReflexive { addr, .. } => {
                    if best_reflexive.is_none() {
                        best_reflexive = Some(addr);
                    }
                }
                TransportHint::TcpDirect { addr } => {
                    if best_tcp.is_none() {
                        best_tcp = Some(addr);
                    }
                }
                TransportHint::WebSocketRelay { relay_authority } => {
                    if relay.is_none() {
                        relay = Some(*relay_authority);
                    }
                }
            }
        }

        // Select in priority order
        if let Some(addr) = best_direct {
            return Ok(SelectedTransport::Direct(addr.to_string()));
        }
        if let Some(addr) = best_reflexive {
            return Ok(SelectedTransport::Direct(addr.to_string()));
        }
        if let Some(addr) = best_tcp {
            return Ok(SelectedTransport::Direct(addr.to_string()));
        }
        if let Some(relay_authority) = relay {
            return Ok(SelectedTransport::Relayed(relay_authority));
        }

        Err(AuraError::not_found("No reachable transport in descriptor"))
    }

    /// Select transport with connectivity probing
    ///
    /// This actually tests connectivity to each hint before selection.
    pub async fn select_with_probing(
        &self,
        descriptor: &RendezvousDescriptor,
        prober: &TransportProber,
    ) -> AuraResult<SelectedTransport> {
        // Try each hint in priority order with actual probing
        for hint in &descriptor.transport_hints {
            match hint {
                TransportHint::QuicDirect { addr } => {
                    if prober.probe_endpoint(&addr.to_string()).await.is_ok() {
                        return Ok(SelectedTransport::Direct(addr.to_string()));
                    }
                }
                TransportHint::QuicReflexive { addr, stun_server } => {
                    if let Ok(reflexive_addr) = prober.stun_probe(&stun_server.to_string()).await {
                        // Use the reflexive address discovered via STUN
                        return Ok(SelectedTransport::Direct(reflexive_addr));
                    } else if prober.probe_endpoint(&addr.to_string()).await.is_ok() {
                        // Fall back to the advertised address
                        return Ok(SelectedTransport::Direct(addr.to_string()));
                    }
                }
                TransportHint::TcpDirect { addr } => {
                    if prober.probe_endpoint(&addr.to_string()).await.is_ok() {
                        return Ok(SelectedTransport::Direct(addr.to_string()));
                    }
                }
                TransportHint::WebSocketRelay { relay_authority } => {
                    // Relay is always assumed reachable as fallback
                    return Ok(SelectedTransport::Relayed(*relay_authority));
                }
            }
        }

        Err(AuraError::not_found("No reachable transport after probing"))
    }
}

// =============================================================================
// Descriptor Builder
// =============================================================================

/// Builds transport descriptors for publication
pub struct DescriptorBuilder {
    /// Local authority ID
    authority_id: AuthorityId,
    /// Default validity duration in milliseconds
    validity_ms: u64,
    /// STUN server for reflexive address discovery
    stun_server: Option<String>,
}

impl DescriptorBuilder {
    /// Create a new descriptor builder
    pub fn new(authority_id: AuthorityId, validity_ms: u64, stun_server: Option<String>) -> Self {
        Self {
            authority_id,
            validity_ms,
            stun_server,
        }
    }

    /// Build a descriptor with the given transport hints
    pub fn build(
        &self,
        context_id: ContextId,
        transport_hints: Vec<TransportHint>,
        now_ms: u64,
    ) -> RendezvousDescriptor {
        let nonce = generate_nonce(&self.authority_id, context_id, now_ms);
        let psk_commitment = compute_psk_commitment(context_id, &self.authority_id);

        RendezvousDescriptor {
            authority_id: self.authority_id,
            context_id,
            transport_hints,
            handshake_psk_commitment: psk_commitment,
            valid_from: now_ms,
            valid_until: now_ms + self.validity_ms,
            nonce,
            display_name: None,
        }
    }

    /// Build a descriptor with automatic transport hint discovery
    ///
    /// # Arguments
    /// * `context_id` - The context for this descriptor
    /// * `local_addresses` - Local addresses to advertise (must be valid socket addresses)
    /// * `now_ms` - Current timestamp in milliseconds
    /// * `prober` - Transport prober for STUN discovery
    ///
    /// # Errors
    /// Returns an error if any local address is invalid.
    pub async fn build_with_discovery(
        &self,
        context_id: ContextId,
        local_addresses: Vec<String>,
        now_ms: u64,
        prober: &TransportProber,
    ) -> AuraResult<RendezvousDescriptor> {
        let mut hints = Vec::new();

        // Add direct hints for each local address (validated)
        for addr_str in &local_addresses {
            let hint = TransportHint::tcp_direct(addr_str)
                .map_err(|e| AuraError::invalid(format!("Invalid transport address: {e}")))?;
            hints.push(hint);
        }

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
                        },
                    );
                }
            }
        }

        Ok(self.build(context_id, hints, now_ms))
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
    ///
    /// Currently returns an error. Full implementation will perform STUN
    /// binding requests to discover the external NAT-mapped address.
    pub async fn stun_probe(&self, stun_server: &str) -> AuraResult<String> {
        // Full implementation will:
        // 1. Send STUN binding request to stun_server
        // 2. Parse response to get reflexive address
        // 3. Return the discovered external address
        let _ = stun_server;
        Err(AuraError::internal("STUN probe not yet implemented"))
    }

    /// Probe all hints in a descriptor and return reachable ones
    pub async fn probe_descriptor(
        &self,
        descriptor: &RendezvousDescriptor,
    ) -> Vec<(TransportHint, bool)> {
        let mut results = Vec::new();

        for hint in &descriptor.transport_hints {
            let reachable = match hint {
                TransportHint::QuicDirect { addr } | TransportHint::TcpDirect { addr } => {
                    self.probe_endpoint(&addr.to_string()).await.is_ok()
                }
                TransportHint::QuicReflexive { addr, stun_server } => {
                    // Try STUN first, then direct
                    self.stun_probe(&stun_server.to_string()).await.is_ok()
                        || self.probe_endpoint(&addr.to_string()).await.is_ok()
                }
                TransportHint::WebSocketRelay { .. } => {
                    // Relay is assumed reachable
                    true
                }
            };
            results.push((hint.clone(), reachable));
        }

        results
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Generate a nonce for descriptor uniqueness
fn generate_nonce(authority_id: &AuthorityId, context_id: ContextId, now_ms: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(authority_hash_bytes(authority_id));
    hasher.update(context_id.as_bytes());
    hasher.update(now_ms.to_le_bytes());
    let result = hasher.finalize();
    let mut nonce = [0u8; 32];
    nonce.copy_from_slice(&result);
    nonce
}

/// Compute PSK commitment from context and authority
///
/// Uses a deterministic hash of context + authority. Full implementation
/// will derive the PSK from the context's shared secret.
fn compute_psk_commitment(context_id: ContextId, authority_id: &AuthorityId) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"PSK_COMMITMENT_V1");
    hasher.update(context_id.as_bytes());
    hasher.update(authority_hash_bytes(authority_id));
    let result = hasher.finalize();
    let mut commitment = [0u8; 32];
    commitment.copy_from_slice(&result);
    commitment
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
    fn test_transport_selector_priority() {
        let selector = TransportSelector::new(5000);

        // Descriptor with multiple hints - should select QuicDirect first
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![
                TransportHint::tcp_direct("192.168.1.1:8080").unwrap(),
                TransportHint::quic_direct("192.168.1.1:4433").unwrap(),
                TransportHint::websocket_relay(AuthorityId::new_from_entropy([3u8; 32])),
            ],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            display_name: None,
        };

        let result = selector.select(&descriptor).unwrap();
        match result {
            SelectedTransport::Direct(addr) => {
                assert_eq!(addr, "192.168.1.1:4433");
            }
            _ => panic!("Expected Direct transport"),
        }
    }

    #[test]
    fn test_transport_selector_fallback_to_relay() {
        let selector = TransportSelector::new(5000);

        // Descriptor with only relay
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![TransportHint::websocket_relay(
                AuthorityId::new_from_entropy([3u8; 32]),
            )],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            display_name: None,
        };

        let result = selector.select(&descriptor).unwrap();
        assert!(matches!(result, SelectedTransport::Relayed(_)));
    }

    #[test]
    fn test_transport_selector_no_hints() {
        let selector = TransportSelector::new(5000);

        // Empty descriptor
        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            display_name: None,
        };

        let result = selector.select(&descriptor);
        assert!(result.is_err());
    }

    #[test]
    fn test_descriptor_builder() {
        let builder = DescriptorBuilder::new(test_authority(), 3_600_000, None);

        let hints = vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()];

        let descriptor = builder.build(test_context(), hints, 1000);

        assert_eq!(descriptor.authority_id, test_authority());
        assert_eq!(descriptor.context_id, test_context());
        assert_eq!(descriptor.valid_from, 1000);
        assert_eq!(descriptor.valid_until, 1000 + 3_600_000);
        assert_eq!(descriptor.transport_hints.len(), 1);
    }

    #[test]
    fn test_descriptor_builder_with_stun() {
        let builder = DescriptorBuilder::new(
            test_authority(),
            3_600_000,
            Some("1.2.3.4:3478".to_string()),
        );

        let hints = vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()];

        let descriptor = builder.build(test_context(), hints, 1000);
        assert_eq!(descriptor.transport_hints.len(), 1);
    }

    #[test]
    fn test_nonce_generation() {
        let authority = test_authority();
        let context = test_context();

        let nonce1 = generate_nonce(&authority, context, 1000);
        let nonce2 = generate_nonce(&authority, context, 1001);

        // Different timestamps should produce different nonces
        assert_ne!(nonce1, nonce2);

        // Same inputs should produce same nonce
        let nonce3 = generate_nonce(&authority, context, 1000);
        assert_eq!(nonce1, nonce3);
    }

    #[test]
    fn test_psk_commitment() {
        let authority = test_authority();
        let context = test_context();

        let commitment1 = compute_psk_commitment(context, &authority);
        let commitment2 = compute_psk_commitment(context, &authority);

        // Same inputs should produce same commitment
        assert_eq!(commitment1, commitment2);

        // Different authority should produce different commitment
        let other_authority = AuthorityId::new_from_entropy([99u8; 32]);
        let commitment3 = compute_psk_commitment(context, &other_authority);
        assert_ne!(commitment1, commitment3);
    }

    #[tokio::test]
    async fn test_transport_prober() {
        let prober = TransportProber::new(5000);

        // Endpoint probe succeeds (actual connectivity check pending)
        let result = prober.probe_endpoint("127.0.0.1:8080").await;
        assert!(result.is_ok());

        // STUN probe returns error until STUN support is added
        let stun_result = prober.stun_probe("stun.example.com:3478").await;
        assert!(stun_result.is_err());
    }

    #[tokio::test]
    async fn test_probe_descriptor() {
        let prober = TransportProber::new(5000);

        let descriptor = RendezvousDescriptor {
            authority_id: test_authority(),
            context_id: test_context(),
            transport_hints: vec![
                TransportHint::tcp_direct("127.0.0.1:8080").unwrap(),
                TransportHint::websocket_relay(AuthorityId::new_from_entropy([3u8; 32])),
            ],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10000,
            nonce: [0u8; 32],
            display_name: None,
        };

        let results = prober.probe_descriptor(&descriptor).await;
        assert_eq!(results.len(), 2);

        // Both are reachable (TCP succeeds, relay assumed reachable)
        assert!(results[0].1); // TcpDirect
        assert!(results[1].1); // WebSocketRelay
    }
}
