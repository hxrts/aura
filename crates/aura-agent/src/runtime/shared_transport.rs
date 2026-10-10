//! Shared in-memory transport wiring for simulations and demos.
//!
//! This is a small shared-state bundle that allows multiple simulated runtimes
//! (e.g., Bob/Alice/Carol) to exchange `TransportEnvelope`s deterministically.
//!
//! IMPORTANT: This is not a transport *implementation* by itself; it is the
//! shared state used by the runtime's `TransportEffects` implementation.
//!
//! # Blocking Lock Usage
//!
//! Uses `parking_lot::RwLock` for synchronous interior mutability because:
//! 1. This is simulation/test infrastructure, not production code paths
//! 2. Operations are O(1) HashSet lookups/inserts (sub-microsecond)
//! 3. Locks are never held across `.await` points
//! 4. Peer count in simulations is small (typically <10)

#![allow(clippy::disallowed_types)]

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use aura_core::effects::transport::TransportEnvelope;
use aura_core::AuthorityId;
use parking_lot::RwLock;
use tokio::sync::Notify;

/// Shared transport state for multi-agent simulations.
///
/// - `inboxes`: per-authority message queues (routing by destination AuthorityId)
/// - `online`: set of authorities currently instantiated in this shared network
#[derive(Clone, Debug)]
pub struct SharedTransport {
    shared: Arc<SharedTransportShared>,
}

#[derive(Debug)]
struct SharedTransportShared {
    state: RwLock<SharedTransportState>,
}

#[derive(Debug, Default)]
struct SharedTransportState {
    inboxes: HashMap<AuthorityId, Arc<RwLock<Vec<TransportEnvelope>>>>,
    inbox_notifiers: HashMap<AuthorityId, Arc<Notify>>,
    online: HashSet<AuthorityId>,
    device_authorities: HashMap<aura_core::DeviceId, AuthorityId>,
    link_faults: HashMap<(AuthorityId, AuthorityId), LinkFault>,
    held: Vec<TransportEnvelope>,
}

/// Directed link fault applied by [`SharedTransport::route_envelope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFault {
    /// Envelopes are lost (a partition the sender cannot observe).
    Drop,
    /// Envelopes are parked until [`SharedTransport::heal_links`] (delay).
    Hold,
}

impl SharedTransportState {
    #[allow(dead_code)] // For use with with_state_mut_validated
    fn validate(&self) -> Result<(), crate::runtime::services::invariant::InvariantViolation> {
        for authority_id in &self.online {
            if !self.inboxes.contains_key(authority_id) {
                return Err(
                    crate::runtime::services::invariant::InvariantViolation::new(
                        "SharedTransport",
                        format!("online authority {:?} missing inbox", authority_id),
                    ),
                );
            }
            if !self.inbox_notifiers.contains_key(authority_id) {
                return Err(
                    crate::runtime::services::invariant::InvariantViolation::new(
                        "SharedTransport",
                        format!("online authority {:?} missing inbox notifier", authority_id),
                    ),
                );
            }
        }
        Ok(())
    }
}

impl SharedTransport {
    /// Create a new empty shared transport network.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(SharedTransportShared {
                state: RwLock::new(SharedTransportState::default()),
            }),
        }
    }

    fn with_state<R>(&self, op: impl FnOnce(&SharedTransportState) -> R) -> R {
        let guard = self.shared.state.read();
        op(&guard)
    }

    fn with_state_mut<R>(&self, op: impl FnOnce(&mut SharedTransportState) -> R) -> R {
        let mut guard = self.shared.state.write();
        let result = op(&mut guard);
        #[cfg(debug_assertions)]
        {
            if let Err(message) = guard.validate() {
                tracing::error!(%message, "SharedTransport state invariant violated");
                debug_assert!(false, "SharedTransport invariant violated: {}", message);
            }
        }
        result
    }

    fn ensure_inbox(&self, authority_id: AuthorityId) -> Arc<RwLock<Vec<TransportEnvelope>>> {
        self.with_state_mut(|state| {
            state
                .inbox_notifiers
                .entry(authority_id)
                .or_insert_with(|| Arc::new(Notify::new()));
            state
                .inboxes
                .entry(authority_id)
                .or_insert_with(|| Arc::new(RwLock::new(Vec::new())))
                .clone()
        })
    }

    fn inbox_notify_inner(&self, authority_id: AuthorityId) -> Arc<Notify> {
        self.with_state_mut(|state| {
            state
                .inbox_notifiers
                .entry(authority_id)
                .or_insert_with(|| Arc::new(Notify::new()))
                .clone()
        })
    }

    /// Access the inbox for a specific authority.
    pub fn inbox_for(&self, authority_id: AuthorityId) -> Arc<RwLock<Vec<TransportEnvelope>>> {
        self.ensure_inbox(authority_id)
    }

    /// Route an envelope into the destination authority inbox.
    pub fn route_envelope(&self, envelope: TransportEnvelope) {
        let fault = self.with_state(|state| {
            state
                .link_faults
                .get(&(envelope.source, envelope.destination))
                .copied()
        });
        match fault {
            Some(LinkFault::Drop) => return,
            Some(LinkFault::Hold) => {
                self.with_state_mut(|state| state.held.push(envelope));
                return;
            }
            None => {}
        }
        // Device routing selects a physical mailbox, never rewrites identity evidence.
        let mailbox = envelope
            .metadata
            .get("aura-destination-device-id")
            .and_then(|raw| raw.parse::<aura_core::DeviceId>().ok())
            .and_then(|device| self.authority_for_device(device))
            .unwrap_or(envelope.destination);
        let inbox = self.ensure_inbox(mailbox);
        let notify = self.inbox_notify_inner(mailbox);
        inbox.write().push(envelope);
        notify.notify_waiters();
    }

    /// Record which authority owns a simulated device, so device-addressed
    /// peers (e.g. sync) resolve the way LAN discovery resolves them in production.
    pub fn register_device(&self, device_id: aura_core::DeviceId, authority_id: AuthorityId) {
        self.with_state_mut(|state| {
            state.device_authorities.insert(device_id, authority_id);
        });
    }

    /// Authority owning a registered simulated device.
    pub fn authority_for_device(&self, device_id: aura_core::DeviceId) -> Option<AuthorityId> {
        self.with_state(|state| state.device_authorities.get(&device_id).copied())
    }

    /// Register an authority as "online" in this shared network.
    pub fn register(&self, authority_id: AuthorityId) {
        self.with_state_mut(|state| {
            state.online.insert(authority_id);
            state
                .inbox_notifiers
                .entry(authority_id)
                .or_insert_with(|| Arc::new(Notify::new()));
            state
                .inboxes
                .entry(authority_id)
                .or_insert_with(|| Arc::new(RwLock::new(Vec::new())));
        });
    }

    /// Count other authorities currently registered as online.
    pub fn connected_peer_count(&self, self_authority: AuthorityId) -> usize {
        self.with_state(|state| {
            state
                .online
                .iter()
                .filter(|id| **id != self_authority)
                .count()
        })
    }

    /// List all authorities currently registered as online.
    pub fn online_peers(&self) -> Vec<AuthorityId> {
        self.with_state(|state| {
            let mut peers: Vec<AuthorityId> = state.online.iter().copied().collect();
            peers.sort();
            peers
        })
    }

    /// Check whether a peer authority is online in this shared network.
    pub fn is_peer_online(&self, peer: AuthorityId) -> bool {
        self.with_state(|state| state.online.contains(&peer))
    }

    /// Fault both directions of the link between two authorities.
    pub fn fault_link(&self, a: AuthorityId, b: AuthorityId, fault: LinkFault) {
        self.with_state_mut(|state| {
            state.link_faults.insert((a, b), fault);
            state.link_faults.insert((b, a), fault);
        });
    }

    /// Clear every link fault and deliver held envelopes in send order.
    pub fn heal_links(&self) {
        let held = self.with_state_mut(|state| {
            state.link_faults.clear();
            std::mem::take(&mut state.held)
        });
        for envelope in held {
            self.route_envelope(envelope);
        }
    }

    /// Return the authority-scoped inbox notifier used by shared transport delivery.
    pub fn inbox_notify(&self, authority_id: AuthorityId) -> Arc<Notify> {
        self.inbox_notify_inner(authority_id)
    }
}

impl Default for SharedTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::identifiers::ContextId;
    use std::collections::HashMap;

    fn envelope_for(destination: AuthorityId, source: AuthorityId) -> TransportEnvelope {
        TransportEnvelope {
            destination,
            source,
            context: ContextId::new_from_entropy([0u8; 32]),
            payload: vec![1, 2, 3],
            metadata: HashMap::new(),
            receipt: None,
        }
    }

    #[test]
    fn routes_envelopes_to_destination_inbox() {
        let shared = SharedTransport::new();
        let a = AuthorityId::new_from_entropy([1u8; 32]);
        let b = AuthorityId::new_from_entropy([2u8; 32]);

        shared.route_envelope(envelope_for(a, b));
        shared.route_envelope(envelope_for(b, a));

        let inbox_a = shared.inbox_for(a);
        let inbox_b = shared.inbox_for(b);

        let inbox_a = inbox_a.read();
        let inbox_b = inbox_b.read();

        assert_eq!(inbox_a.len(), 1);
        assert_eq!(inbox_a[0].destination, a);
        assert_eq!(inbox_b.len(), 1);
        assert_eq!(inbox_b[0].destination, b);
    }

    #[test]
    fn physical_device_routing_preserves_authenticated_authority_fields() {
        let shared = SharedTransport::new();
        let subject = AuthorityId::new_from_entropy([91; 32]);
        let provisional = AuthorityId::new_from_entropy([92; 32]);
        let source = AuthorityId::new_from_entropy([93; 32]);
        let device = aura_core::DeviceId::new_from_entropy([94; 32]);
        shared.register_device(device, provisional);
        let mut envelope = envelope_for(subject, source);
        envelope
            .metadata
            .insert("aura-destination-device-id".into(), device.to_string());
        shared.route_envelope(envelope);
        assert!(shared.inbox_for(subject).read().is_empty());
        let mailbox = shared.inbox_for(provisional);
        let inbox = mailbox.read();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].destination, subject);
        assert_eq!(inbox[0].source, source);
        assert_eq!(
            inbox[0].metadata["aura-destination-device-id"],
            device.to_string()
        );
    }

    #[tokio::test]
    async fn route_envelope_notifies_destination_waiters() {
        let shared = SharedTransport::new();
        let a = AuthorityId::new_from_entropy([3u8; 32]);
        let b = AuthorityId::new_from_entropy([4u8; 32]);
        let notify = shared.inbox_notify(a);
        let notified = notify.notified();
        let second = notify.notified();
        futures::pin_mut!(notified, second);
        assert!(futures::poll!(notified.as_mut()).is_pending());
        assert!(futures::poll!(second.as_mut()).is_pending());
        shared.route_envelope(envelope_for(a, b));
        notified.await;
        second.await;
        assert_eq!(
            shared.inbox_for(a).read().len(),
            1,
            "both observers wake without consuming or duplicating delivery"
        );
    }
}
