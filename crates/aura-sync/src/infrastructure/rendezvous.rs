//! Rendezvous Integration for Peer Discovery
//!
//! This module adapts runtime-owned rendezvous descriptor snapshots into the
//! peer-discovery view used by sync. Instead of custom flooding, peer
//! descriptors are propagated as `RendezvousFact` through the journal sync
//! mechanism and cached by the runtime.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────┐     ┌───────────────────────┐
//! │ Runtime Descriptor     │     │       PeerManager     │
//! │ Registry (aura-agent)  │────▶│   (sync peer tracking)│
//! └───────────────────────┘     └───────────────────────┘
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
//! use aura_rendezvous::facts::RendezvousDescriptor;
//!
//! // Create adapter from the local authority identity.
//! let adapter = RendezvousAdapter::new(local_authority);
//!
//! // Discover peers from a runtime-provided descriptor snapshot.
//! let peers = adapter.discover_context_peers(&descriptors, context_id, 1000);
//! ```

use aura_core::service::{HoldRetrievalRequest, ServiceFamily};
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::{LinkEndpoint, ServiceDescriptor};
use aura_rendezvous::RendezvousDescriptor;
use std::collections::HashMap;

/// Adapter for integrating runtime-owned rendezvous descriptor snapshots with
/// sync peer management.
///
/// This adapter does not own or mutate the descriptor cache. The runtime owns
/// the long-lived registry and passes descriptor snapshots into these helpers.
pub struct RendezvousAdapter {
    /// Local authority so self-descriptors can be filtered out of discovery.
    local_authority: AuthorityId,
}

/// Sync-blended retrieval request scheduled into an ordinary sync window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBlendedRetrieval {
    pub request: HoldRetrievalRequest,
    pub deadline_ms: u64,
}

/// Sync-blended accountability reply scheduled into the same window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBlendedReply {
    pub scope: ContextId,
    pub token: [u8; 32],
    pub deadline_ms: u64,
}

/// Combined retrieval and accountability work for a single sync turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncBlendedHoldWindow {
    pub retrievals: Vec<SyncBlendedRetrieval>,
    pub replies: Vec<SyncBlendedReply>,
}

impl RendezvousAdapter {
    fn deadline_window<T: Clone>(
        entries: &[T],
        now_ms: u64,
        max_batch: usize,
        deadline: impl Fn(&T) -> u64,
    ) -> Vec<T> {
        let mut entries = entries
            .iter()
            .filter(|entry| deadline(entry) >= now_ms)
            .cloned()
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| deadline(entry));
        entries.truncate(max_batch);
        entries
    }

    fn valid_descriptor<'a>(
        &self,
        descriptor: &'a RendezvousDescriptor,
        context_id: ContextId,
        now_ms: u64,
    ) -> Option<&'a RendezvousDescriptor> {
        (descriptor.context_id == context_id
            && descriptor.is_valid(now_ms)
            && descriptor.authority_id != self.local_authority)
            .then_some(descriptor)
    }

    fn descriptor_peer(&self, descriptor: &RendezvousDescriptor) -> DiscoveredPeer {
        DiscoveredPeer {
            authority_id: descriptor.authority_id,
            link_endpoints: descriptor.advertised_link_endpoints(),
            service_descriptors: descriptor.advertised_service_descriptors(),
            valid_until: descriptor.valid_until,
            context_id: descriptor.context_id,
        }
    }

    /// Create a new rendezvous adapter
    pub fn new(local_authority: AuthorityId) -> Self {
        Self { local_authority }
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
    /// A map of AuthorityId to DiscoveredPeer containing split connectivity and
    /// service-surface advertisements.
    pub fn discover_context_peers(
        &self,
        descriptors: &[RendezvousDescriptor],
        context_id: ContextId,
        now_ms: u64,
    ) -> HashMap<AuthorityId, DiscoveredPeer> {
        descriptors
            .iter()
            .filter_map(|descriptor| self.valid_descriptor(descriptor, context_id, now_ms))
            .map(|d| {
                let peer = self.descriptor_peer(d);
                (d.authority_id, peer)
            })
            .collect()
    }

    /// Discover peers in a context that advertise at least one hold-family surface.
    pub fn discover_hold_peers(
        &self,
        descriptors: &[RendezvousDescriptor],
        context_id: ContextId,
        now_ms: u64,
    ) -> HashMap<AuthorityId, DiscoveredPeer> {
        self.discover_context_peers(descriptors, context_id, now_ms)
            .into_iter()
            .filter(|(_, peer)| peer.supports_hold())
            .collect()
    }

    /// Check if a specific peer is reachable in a context
    ///
    /// Queries the descriptor cache for a specific peer's descriptor.
    pub fn get_peer_info(
        &self,
        descriptors: &[RendezvousDescriptor],
        context_id: ContextId,
        peer: AuthorityId,
        now_ms: u64,
    ) -> Option<DiscoveredPeer> {
        descriptors
            .iter()
            .find(|descriptor| {
                descriptor.context_id == context_id
                    && descriptor.authority_id == peer
                    && descriptor.is_valid(now_ms)
            })
            .map(|descriptor| self.descriptor_peer(descriptor))
    }

    /// Get peers that need descriptor refresh
    ///
    /// Returns authorities whose descriptors will expire within the refresh window.
    pub fn peers_needing_refresh(
        &self,
        descriptors: &[RendezvousDescriptor],
        context_id: ContextId,
        now_ms: u64,
    ) -> Vec<AuthorityId> {
        descriptors
            .iter()
            .filter(|descriptor| descriptor.context_id == context_id)
            .filter(|descriptor| descriptor.authority_id != self.local_authority)
            .filter(|descriptor| descriptor.is_valid(now_ms) && descriptor.needs_refresh(now_ms))
            .map(|descriptor| descriptor.authority_id)
            .collect()
    }

    /// Check if our own descriptor needs refresh
    pub fn needs_own_refresh(
        &self,
        descriptors: &[RendezvousDescriptor],
        context_id: ContextId,
        now_ms: u64,
        refresh_window_ms: u64,
    ) -> bool {
        match descriptors.iter().find(|descriptor| {
            descriptor.context_id == context_id && descriptor.authority_id == self.local_authority
        }) {
            None => true,
            Some(descriptor) if !descriptor.is_valid(now_ms) => true,
            Some(descriptor) => {
                let time_until_expiry = descriptor.valid_until.saturating_sub(now_ms);
                time_until_expiry <= refresh_window_ms
            }
        }
    }

    /// Batch selector retrievals and compatible replies into one sync-blended window.
    pub fn plan_sync_blended_hold_window(
        &self,
        retrievals: &[SyncBlendedRetrieval],
        replies: &[SyncBlendedReply],
        now_ms: u64,
        max_batch: usize,
    ) -> SyncBlendedHoldWindow {
        SyncBlendedHoldWindow {
            retrievals: Self::deadline_window(retrievals, now_ms, max_batch, |entry| {
                entry.deadline_ms
            }),
            replies: Self::deadline_window(replies, now_ms, max_batch, |entry| entry.deadline_ms),
        }
    }
}

/// A discovered peer with transport information
#[derive(Debug, Clone)]
pub struct DiscoveredPeer {
    /// The peer's authority ID
    pub authority_id: AuthorityId,
    /// Connectivity endpoints decoupled from service-family policy.
    pub link_endpoints: Vec<LinkEndpoint>,
    /// Family-specific service-surface advertisements.
    pub service_descriptors: Vec<ServiceDescriptor>,
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

    /// Get the preferred connectivity endpoint (first in priority order).
    pub fn preferred_link_endpoint(&self) -> Option<&LinkEndpoint> {
        self.link_endpoints.first()
    }

    /// Check if peer has any direct transport options
    pub fn has_direct_transport(&self) -> bool {
        self.link_endpoints
            .iter()
            .any(|endpoint| endpoint.relay_authority.is_none())
    }

    /// Check if peer requires relay
    pub fn requires_relay(&self) -> bool {
        self.link_endpoints
            .iter()
            .all(|endpoint| endpoint.relay_authority.is_some())
    }

    /// Return whether the peer advertises any hold-family service surface.
    pub fn supports_hold(&self) -> bool {
        self.service_descriptors
            .iter()
            .any(|descriptor| descriptor.header.family == ServiceFamily::Hold)
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
    use aura_rendezvous::facts::{RendezvousDescriptor, TransportHint};

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
            device_id: None,
            context_id: context,
            transport_hints: vec![TransportHint::tcp_direct("127.0.0.1:8080").unwrap()],
            handshake_psk_commitment: [0u8; 32],
            public_key: [0u8; 32],
            valid_from,
            valid_until,
            nonce: [0u8; 32],
            nickname_suggestion: None,
        }
    }

    #[test]
    fn test_adapter_creation() {
        let alice = test_authority(1);
        let adapter = RendezvousAdapter::new(alice);
        assert_eq!(adapter.local_authority, alice);
    }

    #[test]
    fn test_discovered_peer_validity() {
        let peer = DiscoveredPeer {
            authority_id: test_authority(2),
            link_endpoints: vec![LinkEndpoint::direct(
                aura_core::LinkProtocol::Quic,
                "10.0.0.1:8443",
            )],
            service_descriptors: Vec::new(),
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
            link_endpoints: vec![
                LinkEndpoint::direct(aura_core::LinkProtocol::Quic, "10.0.0.1:8443"),
                LinkEndpoint::relay(test_authority(99)),
            ],
            service_descriptors: Vec::new(),
            valid_until: 10_000,
            context_id: test_context(100),
        };

        assert!(direct_peer.has_direct_transport());
        assert!(!direct_peer.requires_relay());

        let relay_only_peer = DiscoveredPeer {
            authority_id: test_authority(3),
            link_endpoints: vec![LinkEndpoint::relay(test_authority(99))],
            service_descriptors: Vec::new(),
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
        let bob_descriptor = test_descriptor(bob, context, 0, 10_000);
        let descriptors = vec![bob_descriptor];

        let adapter = RendezvousAdapter::new(alice);

        let peer_info = adapter.get_peer_info(&descriptors, context, bob, 5000);
        assert!(peer_info.is_some());
        let peer = peer_info.unwrap();
        assert_eq!(peer.authority_id, bob);
        assert_eq!(peer.valid_until, 10_000);

        let unknown = test_authority(99);
        assert!(adapter
            .get_peer_info(&descriptors, context, unknown, 5000)
            .is_none());

        assert!(adapter
            .get_peer_info(&descriptors, context, bob, 15_000)
            .is_none());
    }

    #[test]
    fn test_discover_context_peers() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let carol = test_authority(3);
        let context = test_context(100);
        let other_context = test_context(200);
        let descriptors = vec![
            test_descriptor(bob, context, 0, 10_000),
            test_descriptor(carol, context, 0, 10_000),
            test_descriptor(test_authority(4), other_context, 0, 10_000),
            test_descriptor(alice, context, 0, 10_000),
        ];
        let adapter = RendezvousAdapter::new(alice);

        let peers = adapter.discover_context_peers(&descriptors, context, 5000);

        assert_eq!(peers.len(), 2);
        assert!(peers.contains_key(&bob));
        assert!(peers.contains_key(&carol));
        assert!(!peers.contains_key(&alice));

        let peers_after_expiry = adapter.discover_context_peers(&descriptors, context, 15_000);
        assert!(peers_after_expiry.is_empty());
    }

    #[test]
    fn test_peers_needing_refresh() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);
        let descriptors = vec![test_descriptor(bob, context, 0, 10_000)];
        let adapter = RendezvousAdapter::new(alice);

        let needing_refresh = adapter.peers_needing_refresh(&descriptors, context, 5000);
        assert!(needing_refresh.is_empty());

        let needing_refresh = adapter.peers_needing_refresh(&descriptors, context, 9500);
        assert_eq!(needing_refresh.len(), 1);
        assert_eq!(needing_refresh[0], bob);
    }

    #[test]
    fn test_needs_own_refresh() {
        let alice = test_authority(1);
        let context = test_context(100);
        let adapter = RendezvousAdapter::new(alice);
        let mut descriptors = Vec::new();

        assert!(adapter.needs_own_refresh(&descriptors, context, 5000, 1000));

        descriptors.push(test_descriptor(alice, context, 0, 10_000));

        assert!(!adapter.needs_own_refresh(&descriptors, context, 5000, 1000));
        assert!(adapter.needs_own_refresh(&descriptors, context, 9500, 1000));
        assert!(adapter.needs_own_refresh(&descriptors, context, 15_000, 1000));
    }

    #[test]
    fn test_discovery_view_does_not_mutate_descriptor_registry() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);
        let descriptors = vec![test_descriptor(bob, context, 0, 10_000)];

        let adapter = RendezvousAdapter::new(alice);
        let _ = adapter.discover_context_peers(&descriptors, context, 5000);
        let _ = adapter.get_peer_info(&descriptors, context, bob, 5000);

        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].authority_id, bob);
    }

    #[test]
    fn discover_hold_peers_filters_to_hold_surfaces() {
        let alice = test_authority(1);
        let bob = test_authority(2);
        let context = test_context(100);
        let descriptors = vec![test_descriptor(bob, context, 0, 10_000)];
        let adapter = RendezvousAdapter::new(alice);

        let peers = adapter.discover_hold_peers(&descriptors, context, 5000);

        assert_eq!(peers.len(), 1);
        assert!(peers
            .get(&bob)
            .unwrap_or_else(|| panic!("bob"))
            .supports_hold());
    }

    #[test]
    fn sync_blended_hold_window_orders_by_deadline_without_poll_loop() {
        use aura_core::service::EstablishedPathRef;

        let adapter = RendezvousAdapter::new(test_authority(1));
        let context = test_context(5);
        let first_path = EstablishedPathRef {
            scope: context,
            path_id: [12; 32],
            valid_until: 1_000,
        };
        let second_path = EstablishedPathRef {
            scope: context,
            path_id: [13; 32],
            valid_until: 1_000,
        };
        let window = adapter.plan_sync_blended_hold_window(
            &[
                SyncBlendedRetrieval {
                    request: HoldRetrievalRequest {
                        profile: aura_core::ServiceProfile::DeferredDeliveryHold,
                        scope: context,
                        selector: [2; 32],
                        reply_path: first_path,
                    },
                    deadline_ms: 200,
                },
                SyncBlendedRetrieval {
                    request: HoldRetrievalRequest {
                        profile: aura_core::ServiceProfile::DeferredDeliveryHold,
                        scope: context,
                        selector: [1; 32],
                        reply_path: second_path,
                    },
                    deadline_ms: 150,
                },
            ],
            &[SyncBlendedReply {
                scope: context,
                token: [9; 32],
                deadline_ms: 175,
            }],
            100,
            4,
        );

        assert_eq!(window.retrievals.len(), 2);
        assert_eq!(window.retrievals[0].request.selector, [1; 32]);
        assert_eq!(window.replies.len(), 1);
    }
}
