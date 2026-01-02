//! Rendezvous Integration for Peer Discovery
//!
//! This module bridges `aura-sync` peer discovery with `aura-rendezvous`
//! for fact-based peer discovery. Instead of custom flooding, peer descriptors
//! are propagated as `RendezvousFact` through the journal sync mechanism.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────┐     ┌───────────────────────┐
//! │  RendezvousService │     │       PeerManager     │
//! │   (descriptor cache)│<───│   (sync peer tracking)│
//! └───────────────────┘     └───────────────────────┘
//!           ↓                          ↓
//! ┌───────────────────────────────────────────────┐
//! │             Context Journal                    │
//! │  (RendezvousFact::Descriptor propagation)      │
//! └───────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use aura_sync::infrastructure::{PeerManager, RendezvousAdapter};
//! use aura_rendezvous::service::RendezvousService;
//!
//! // Create adapter linking rendezvous to sync
//! let adapter = RendezvousAdapter::new(&rendezvous_service);
//!
//! // Discover peers from cached descriptors
//! let peers = adapter.discover_context_peers(context_id, 1000);
//! ```

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_rendezvous::facts::TransportHint;
use aura_rendezvous::service::RendezvousService;
use std::collections::HashMap;

/// Adapter for integrating rendezvous discovery with sync peer management
///
/// This adapter queries `RendezvousService` for cached descriptors and
/// provides peer information suitable for sync operations.
pub struct RendezvousAdapter<'a> {
    /// Reference to the rendezvous service
    service: &'a RendezvousService,
}

impl<'a> RendezvousAdapter<'a> {
    /// Create a new rendezvous adapter
    pub fn new(service: &'a RendezvousService) -> Self {
        Self { service }
    }

    /// Discover peers available for sync in a given context
    ///
    /// Queries the rendezvous service's descriptor cache for peers
    /// that have published valid descriptors in this context.
    ///
    /// # Arguments
    /// * `context_id` - The context to discover peers in
    /// * `now_ms` - Current time for validity checking
    ///
    /// # Returns
    /// A map of AuthorityId to DiscoveredPeer containing transport hints
    pub fn discover_context_peers(
        &self,
        context_id: ContextId,
        now_ms: u64,
    ) -> HashMap<AuthorityId, DiscoveredPeer> {
        let local_authority = self.service.authority_id();

        self.service
            .iter_descriptors_in_context(context_id)
            .filter(|d| d.is_valid(now_ms) && d.authority_id != local_authority)
            .map(|d| {
                let peer = DiscoveredPeer {
                    authority_id: d.authority_id,
                    transport_hints: d.transport_hints.clone(),
                    valid_until: d.valid_until,
                    context_id: d.context_id,
                };
                (d.authority_id, peer)
            })
            .collect()
    }

    /// Check if a specific peer is reachable in a context
    ///
    /// Queries the descriptor cache for a specific peer's descriptor.
    pub fn get_peer_info(
        &self,
        context_id: ContextId,
        peer: AuthorityId,
        now_ms: u64,
    ) -> Option<DiscoveredPeer> {
        self.service
            .get_cached_descriptor(context_id, peer, now_ms)
            .map(|d| DiscoveredPeer {
                authority_id: d.authority_id,
                transport_hints: d.transport_hints.clone(),
                valid_until: d.valid_until,
                context_id: d.context_id,
            })
    }

    /// Get peers that need descriptor refresh
    ///
    /// Returns authorities whose descriptors will expire within the refresh window.
    pub fn peers_needing_refresh(&self, context_id: ContextId, now_ms: u64) -> Vec<AuthorityId> {
        self.service.peers_needing_refresh(context_id, now_ms)
    }

    /// Check if our own descriptor needs refresh
    pub fn needs_own_refresh(
        &self,
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        self.service
            .needs_own_refresh(context_id, now_ms, refresh_window_ms)
    }
}

/// A discovered peer with transport information
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// The peer's authority ID
    pub authority_id: AuthorityId,
    /// Available transport endpoints for reaching this peer
    pub transport_hints: Vec<TransportHint>,
    /// When the descriptor expires
    pub valid_until: u64,
    /// Context this peer was discovered in
    pub context_id: ContextId,
}

impl DiscoveredPeer {
    /// Check if the descriptor is still valid
    pub fn is_valid(&self, now_ms: u64) -> bool {
        now_ms < self.valid_until
    }

    /// Check if the descriptor needs refresh (within 10% of expiry)
    pub fn needs_refresh(&self, now_ms: u64, valid_from: u64) -> bool {
        let validity_window = self.valid_until.saturating_sub(valid_from);
        let refresh_threshold = self.valid_until.saturating_sub(validity_window / 10);
        now_ms >= refresh_threshold
    }

    /// Get the preferred transport hint (first in priority order)
    pub fn preferred_transport(&self) -> Option<&TransportHint> {
        self.transport_hints.first()
    }

    /// Check if peer has any direct transport options
    pub fn has_direct_transport(&self) -> bool {
        self.transport_hints.iter().any(|h| {
            matches!(
                h,
                TransportHint::QuicDirect { .. } | TransportHint::TcpDirect { .. }
            )
        })
    }

    /// Check if peer requires relay
    pub fn requires_relay(&self) -> bool {
        self.transport_hints
            .iter()
            .all(|h| matches!(h, TransportHint::WebSocketRelay { .. }))
    }
}

/// Events for peer discovery changes
#[derive(Debug, Clone)]
pub enum RendezvousEvent {
    /// A new descriptor was received for a peer
    DescriptorReceived {
        context_id: ContextId,
        authority_id: AuthorityId,
    },
    /// A descriptor was revoked
    DescriptorRevoked {
        context_id: ContextId,
        authority_id: AuthorityId,
    },
    /// A descriptor expired
    DescriptorExpired {
        context_id: ContextId,
        authority_id: AuthorityId,
    },
    /// A channel was established
    ChannelEstablished {
        context_id: ContextId,
        peer: AuthorityId,
        channel_id: [u8; 32],
    },
}

/// Callback type for rendezvous events
pub type RendezvousEventCallback = Box<dyn Fn(RendezvousEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;
    use aura_rendezvous::facts::RendezvousDescriptor;
    use aura_rendezvous::service::RendezvousConfig;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn test_descriptor(
        authority: AuthorityId,
        context: ContextId,
        valid_from: u64,
        valid_until: u64,
    ) -> RendezvousDescriptor {
        RendezvousDescriptor {
            authority_id: authority,
            context_id: context,
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            valid_from,
            valid_until,
            nonce: [0u8; 32],
            display_name: None,
        }
    }

    #[test]
    fn test_adapter_creation() {
        let alice = test_authority(1);
        let config = RendezvousConfig::default();
        let service = RendezvousService::new(alice, config);

        let adapter = RendezvousAdapter::new(&service);
        assert_eq!(adapter.service.authority_id(), alice);
    }

    #[test]
    fn test_discovered_peer_validity() {
        let peer = DiscoveredPeer {
            authority_id: test_authority(2),
            transport_hints: vec![TransportHint::quic_direct("10.0.0.1:8443").unwrap()],
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(peer.is_valid(5000));
        assert!(!peer.is_valid(10_000));
        assert!(!peer.is_valid(15_000));
    }

    #[test]
    fn test_discovered_peer_transport_checks() {
        let direct_peer = DiscoveredPeer {
            authority_id: test_authority(2),
            transport_hints: vec![
                TransportHint::quic_direct("10.0.0.1:8443").unwrap(),
                TransportHint::websocket_relay(test_authority(99)),
            ],
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(direct_peer.has_direct_transport());
        assert!(!direct_peer.requires_relay());

        let relay_only_peer = DiscoveredPeer {
            authority_id: test_authority(3),
            transport_hints: vec![TransportHint::websocket_relay(test_authority(99))],
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(!relay_only_peer.has_direct_transport());
        assert!(relay_only_peer.requires_relay());
    }

    #[test]
    fn test_get_peer_info_with_cached_descriptor() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);

        let config = RendezvousConfig::default();
        let mut service = RendezvousService::new(alice, config);

        // Cache Bob's descriptor
        let bob_descriptor = test_descriptor(bob, context, 0, 10_000);
        service.cache_descriptor(bob_descriptor);

        let adapter = RendezvousAdapter::new(&service);

        // Should find Bob's cached descriptor
        let peer_info = adapter.get_peer_info(context, bob, 5000);
        assert!(peer_info.is_some());
        let peer = peer_info.unwrap();
        assert_eq!(peer.authority_id, bob);
        assert_eq!(peer.valid_until, 10_000);

        // Should not find unknown peer
        let unknown = test_authority(99);
        assert!(adapter.get_peer_info(context, unknown, 5000).is_none());

        // Should not find expired descriptor
        assert!(adapter.get_peer_info(context, bob, 15_000).is_none());
    }

    #[test]
    fn test_discover_context_peers() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let carol = test_authority(3);
        let context = test_context(100);
        let other_context = test_context(200);

        let config = RendezvousConfig::default();
        let mut service = RendezvousService::new(alice, config);

        // Cache descriptors for Bob and Carol in the same context
        service.cache_descriptor(test_descriptor(bob, context, 0, 10_000));
        service.cache_descriptor(test_descriptor(carol, context, 0, 10_000));
        // Cache a descriptor in a different context
        service.cache_descriptor(test_descriptor(test_authority(4), other_context, 0, 10_000));
        // Cache Alice's own descriptor (should be excluded)
        service.cache_descriptor(test_descriptor(alice, context, 0, 10_000));

        let adapter = RendezvousAdapter::new(&service);

        // Discover peers in the context
        let peers = adapter.discover_context_peers(context, 5000);

        // Should find Bob and Carol, but not Alice (self) or the other context peer
        assert_eq!(peers.len(), 2);
        assert!(peers.contains_key(&bob));
        assert!(peers.contains_key(&carol));
        assert!(!peers.contains_key(&alice)); // Self excluded

        // Expired descriptors should be filtered
        let peers_after_expiry = adapter.discover_context_peers(context, 15_000);
        assert!(peers_after_expiry.is_empty());
    }

    #[test]
    fn test_peers_needing_refresh() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);

        let config = RendezvousConfig::default();
        let mut service = RendezvousService::new(alice, config);

        // Bob's descriptor valid from 0 to 10_000, refresh window starts at 9000 (10% before expiry)
        service.cache_descriptor(test_descriptor(bob, context, 0, 10_000));

        let adapter = RendezvousAdapter::new(&service);

        // At 5000, Bob doesn't need refresh yet
        let needing_refresh = adapter.peers_needing_refresh(context, 5000);
        assert!(needing_refresh.is_empty());

        // At 9500 (within 10% of expiry), Bob needs refresh
        let needing_refresh = adapter.peers_needing_refresh(context, 9500);
        assert_eq!(needing_refresh.len(), 1);
        assert_eq!(needing_refresh[0], bob);
    }

    #[test]
    fn test_needs_own_refresh() {
        let alice = test_authority(1);
        let context = test_context(100);

        let config = RendezvousConfig::default();
        let mut service = RendezvousService::new(alice, config);

        let adapter = RendezvousAdapter::new(&service);

        // No cached descriptor - needs refresh
        assert!(adapter.needs_own_refresh(context, 5000, 1000));

        // Cache Alice's own descriptor
        service.cache_descriptor(test_descriptor(alice, context, 0, 10_000));

        let adapter = RendezvousAdapter::new(&service);

        // At 5000 with 1000ms window - doesn't need refresh (expiry is 10_000)
        assert!(!adapter.needs_own_refresh(context, 5000, 1000));

        // At 9500 with 1000ms window - needs refresh (within window)
        assert!(adapter.needs_own_refresh(context, 9500, 1000));

        // After expiry - needs refresh
        assert!(adapter.needs_own_refresh(context, 15_000, 1000));
    }
}
