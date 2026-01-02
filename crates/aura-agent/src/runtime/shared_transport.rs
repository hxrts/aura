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

/// Shared transport state for multi-agent simulations.
///
/// - `inboxes`: per-authority message queues (routing by destination AuthorityId)
/// - `online`: set of authorities currently instantiated in this shared network
#[derive(Clone, Debug)]
pub struct SharedTransport {
    state: Arc<RwLock<SharedTransportState>>,
}

#[derive(Debug, Default)]
struct SharedTransportState {
    inboxes: HashMap<AuthorityId, Arc<RwLock<Vec<TransportEnvelope>>>>,
    online: HashSet<AuthorityId>,
}

impl SharedTransportState {
    fn validate(&self) -> Result<(), String> {
        for authority_id in &self.online {
            if !self.inboxes.contains_key(authority_id) {
                return Err(format!("online authority {:?} missing inbox", authority_id));
            }
        }
        Ok(())
    }
}

impl SharedTransport {
    /// Create a new empty shared transport network.
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(SharedTransportState::default())),
        }
    }

    /// Wrap an existing per-authority inbox (legacy simulation wiring).
    pub fn from_inbox(
        authority_id: AuthorityId,
        inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    ) -> Self {
        let mut inboxes = HashMap::new();
        inboxes.insert(authority_id, inbox);
        let mut online = HashSet::new();
        online.insert(authority_id);
        Self {
            state: Arc::new(RwLock::new(SharedTransportState { inboxes, online })),
        }
    }

    fn with_state<R>(&self, op: impl FnOnce(&SharedTransportState) -> R) -> R {
        let guard = self.state.read();
        op(&guard)
    }

    fn with_state_mut<R>(&self, op: impl FnOnce(&mut SharedTransportState) -> R) -> R {
        let mut guard = self.state.write();
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
                .inboxes
                .entry(authority_id)
                .or_insert_with(|| Arc::new(RwLock::new(Vec::new())))
                .clone()
        })
    }

    /// Access the inbox for a specific authority.
    pub fn inbox_for(&self, authority_id: AuthorityId) -> Arc<RwLock<Vec<TransportEnvelope>>> {
        self.ensure_inbox(authority_id)
    }

    /// Route an envelope into the destination authority inbox.
    pub fn route_envelope(&self, envelope: TransportEnvelope) {
        let inbox = self.ensure_inbox(envelope.destination);
        inbox.write().push(envelope);
    }

    /// Register an authority as "online" in this shared network.
    pub fn register(&self, authority_id: AuthorityId) {
        self.with_state_mut(|state| {
            state.online.insert(authority_id);
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

    /// Check whether a peer authority is online in this shared network.
    pub fn is_peer_online(&self, peer: AuthorityId) -> bool {
        self.with_state(|state| state.online.contains(&peer))
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
    use aura_core::identifiers::ContextId;
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
}
