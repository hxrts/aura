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
        let peers = HashMap::new();

        // Current limitation: This returns an empty map because RendezvousService
        // caches descriptors by (context, authority) pairs but doesn't yet expose
        // iteration over all descriptors in a context.
        //
        // Full implementation requires RendezvousService to expose either:
        // - iter_descriptors(context_id) -> impl Iterator<Item = &RendezvousDescriptor>
        // - Or a separate index of authorities per context
        //
        // For now, use get_peer_info() to check specific peers.

        // Exclude self from peer list
        let local_authority = self.service.authority_id();

        // If we have a specific peer we want to check, we can do so:
        // let descriptor = self.service.get_cached_descriptor(context_id, peer);

        // For demonstration, return empty - full implementation pending
        // RendezvousService iterator support
        let _ = (context_id, now_ms, local_authority);

        peers
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
        let descriptor = self.service.get_cached_descriptor(context_id, peer)?;

        // Check validity
        if !descriptor.is_valid(now_ms) {
            return None;
        }

        Some(DiscoveredPeer {
            authority_id: peer,
            transport_hints: descriptor.transport_hints.clone(),
            valid_until: descriptor.valid_until,
            context_id,
        })
    }

    /// Get peers that need descriptor refresh
    ///
    /// Returns authorities whose descriptors will expire within the refresh window.
    pub fn peers_needing_refresh(&self, context_id: ContextId, now_ms: u64) -> Vec<AuthorityId> {
        self.service.descriptors_needing_refresh(context_id, now_ms)
    }

    /// Check if our own descriptor needs refresh
    pub fn needs_own_refresh(
        &self,
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        self.service
            .needs_refresh(context_id, now_ms, refresh_window_ms)
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
    use aura_rendezvous::service::RendezvousConfig;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn test_context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
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
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "10.0.0.1:8443".to_string(),
            }],
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
                TransportHint::QuicDirect {
                    addr: "10.0.0.1:8443".to_string(),
                },
                TransportHint::WebSocketRelay {
                    relay_authority: test_authority(99),
                },
            ],
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(direct_peer.has_direct_transport());
        assert!(!direct_peer.requires_relay());

        let relay_only_peer = DiscoveredPeer {
            authority_id: test_authority(3),
            transport_hints: vec![TransportHint::WebSocketRelay {
                relay_authority: test_authority(99),
            }],
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(!relay_only_peer.has_direct_transport());
        assert!(relay_only_peer.requires_relay());
    }

    #[test]
    fn test_get_peer_info() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);

        let config = RendezvousConfig::default();
        let mut service = RendezvousService::new(alice, config);

        // Cache Bob's descriptor
        let descriptor = aura_rendezvous::facts::RendezvousDescriptor {
            authority_id: bob,
            context_id: context,
            transport_hints: vec![TransportHint::QuicDirect {
                addr: "10.0.0.2:8443".to_string(),
            }],
            handshake_psk_commitment: [0u8; 32],
            valid_from: 0,
            valid_until: 10_000,
            nonce: [0u8; 32],
        };
        service.cache_descriptor(descriptor);

        let adapter = RendezvousAdapter::new(&service);

        // Should find Bob
        let peer_info = adapter.get_peer_info(context, bob, 5000);
        assert!(peer_info.is_some());
        let info = peer_info.unwrap();
        assert_eq!(info.authority_id, bob);
        assert!(info.has_direct_transport());

        // Should not find unknown peer
        let unknown = test_authority(99);
        assert!(adapter.get_peer_info(context, unknown, 5000).is_none());

        // Should not find expired descriptor
        assert!(adapter.get_peer_info(context, bob, 15_000).is_none());
    }
}
