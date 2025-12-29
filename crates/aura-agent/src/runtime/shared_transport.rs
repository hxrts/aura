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
//! Uses `parking_lot::RwLock` because:
//! 1. This is Layer 6 runtime code explicitly allowed per clippy.toml
//! 2. Locks protect in-memory simulation state with brief, sync-only access
//! 3. No lock is held across .await points

use std::collections::HashSet;
use std::sync::Arc;

use aura_core::effects::transport::TransportEnvelope;
use aura_core::AuthorityId;
#[allow(clippy::disallowed_types)]
use parking_lot::RwLock;

/// Shared transport state for multi-agent simulations.
///
/// - `inbox`: shared message store (routing is done by destination AuthorityId)
/// - `online`: set of authorities currently instantiated in this shared network
#[derive(Clone, Debug)]
pub struct SharedTransport {
    inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
    online: Arc<RwLock<HashSet<AuthorityId>>>,
}

impl SharedTransport {
    /// Create a new empty shared transport network.
    pub fn new() -> Self {
        Self {
            inbox: Arc::new(RwLock::new(Vec::new())),
            online: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Wrap an existing shared inbox (legacy simulation wiring).
    ///
    /// This preserves the historical `Arc<RwLock<Vec<TransportEnvelope>>>` sharing model
    /// while allowing newer code to track an explicit online set.
    pub fn from_inbox(inbox: Arc<RwLock<Vec<TransportEnvelope>>>) -> Self {
        Self {
            inbox,
            online: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Access the shared inbox used for routing envelopes.
    pub fn inbox(&self) -> Arc<RwLock<Vec<TransportEnvelope>>> {
        self.inbox.clone()
    }

    /// Register an authority as "online" in this shared network.
    pub fn register(&self, authority_id: AuthorityId) {
        self.online.write().insert(authority_id);
    }

    /// Count other authorities currently registered as online.
    pub fn connected_peer_count(&self, self_authority: AuthorityId) -> usize {
        let online = self.online.read();
        online.iter().filter(|id| **id != self_authority).count()
    }

    /// Check whether a peer authority is online in this shared network.
    pub fn is_peer_online(&self, peer: AuthorityId) -> bool {
        let online = self.online.read();
        online.contains(&peer)
    }
}

impl Default for SharedTransport {
    fn default() -> Self {
        Self::new()
    }
}
