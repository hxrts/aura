//! Transport Subsystem
//!
//! Groups transport-related fields from AuraEffectSystem:
//! - `transport_handler`: Core transport operations (send, receive, connect)
//! - `transport_inbox`: Incoming message queue
//! - `shared_transport`: Optional shared transport for simulation mode
//! - `transport_stats`: Transport metrics and statistics
//!
//! ## Lock Usage
//!
//! Uses `parking_lot::RwLock` for inbox and stats because:
//! - These are accessed synchronously for quick reads/writes
//! - Never held across async boundaries
//! - See `runtime/CONCURRENCY.md` for full rationale

#![allow(clippy::disallowed_types)]

use crate::runtime::shared_transport::SharedTransport;
use aura_core::effects::transport::{TransportEnvelope, TransportStats};
use aura_core::AuthorityId;
use parking_lot::RwLock;
use std::sync::Arc;

/// Transport subsystem grouping network transport operations.
///
/// This subsystem encapsulates:
/// - Transport handler for sending/receiving messages
/// - Inbox for incoming envelopes
/// - Shared transport for simulation/demo mode
/// - Transport statistics
pub struct TransportSubsystem {
    /// Core transport handler
    handler: aura_effects::transport::RealTransportHandler,

    /// Incoming message inbox
    ///
    /// Protected by parking_lot::RwLock for concurrent access.
    /// Lock is never held across .await points.
    inbox: Arc<RwLock<Vec<TransportEnvelope>>>,

    /// Optional shared transport for simulation mode
    ///
    /// When set, all agents share a common in-memory transport network.
    shared_transport: Option<SharedTransport>,

    /// Transport statistics (messages sent, received, errors, etc.)
    stats: Arc<RwLock<TransportStats>>,
}

impl TransportSubsystem {
    /// Create a new transport subsystem with local inbox
    pub fn new() -> Self {
        Self {
            handler: aura_effects::transport::RealTransportHandler::default(),
            inbox: Arc::new(RwLock::new(Vec::new())),
            shared_transport: None,
            stats: Arc::new(RwLock::new(TransportStats::default())),
        }
    }

    /// Create a transport subsystem with shared transport for simulation
    #[allow(dead_code)]
    pub fn with_shared_transport(shared: SharedTransport, authority: AuthorityId) -> Self {
        // Register this authority in the shared network
        shared.register(authority);

        Self {
            handler: aura_effects::transport::RealTransportHandler::default(),
            inbox: shared.inbox(),
            shared_transport: Some(shared),
            stats: Arc::new(RwLock::new(TransportStats::default())),
        }
    }

    /// Create from existing components
    pub fn from_parts(
        handler: aura_effects::transport::RealTransportHandler,
        inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
        shared_transport: Option<SharedTransport>,
        stats: Arc<RwLock<TransportStats>>,
    ) -> Self {
        Self {
            handler,
            inbox,
            shared_transport,
            stats,
        }
    }

    /// Get reference to the transport handler
    pub fn handler(&self) -> &aura_effects::transport::RealTransportHandler {
        &self.handler
    }

    /// Get shared inbox reference
    pub fn inbox(&self) -> Arc<RwLock<Vec<TransportEnvelope>>> {
        self.inbox.clone()
    }

    /// Get shared stats reference
    #[allow(dead_code)]
    pub fn stats(&self) -> Arc<RwLock<TransportStats>> {
        self.stats.clone()
    }

    /// Check if using shared transport (simulation mode)
    pub fn is_shared(&self) -> bool {
        self.shared_transport.is_some()
    }

    /// Get shared transport if available
    pub fn shared_transport(&self) -> Option<&SharedTransport> {
        self.shared_transport.as_ref()
    }

    /// Push an envelope to the inbox
    pub fn queue_envelope(&self, envelope: TransportEnvelope) {
        let mut inbox = self.inbox.write();
        inbox.push(envelope);
    }

    /// Drain all envelopes from the inbox
    #[allow(dead_code)]
    pub fn drain_inbox(&self) -> Vec<TransportEnvelope> {
        let mut inbox = self.inbox.write();
        std::mem::take(&mut *inbox)
    }

    /// Get current inbox size
    pub fn inbox_len(&self) -> usize {
        self.inbox.read().len()
    }

    /// Update transport statistics
    pub fn update_stats<F>(&self, f: F)
    where
        F: FnOnce(&mut TransportStats),
    {
        let mut stats = self.stats.write();
        f(&mut stats);
    }

    /// Get a snapshot of current stats
    pub fn stats_snapshot(&self) -> TransportStats {
        self.stats.read().clone()
    }
}

impl Default for TransportSubsystem {
    fn default() -> Self {
        Self::new()
    }
}

// Note: TransportSubsystem is intentionally not Clone because
// RealTransportHandler does not implement Clone. The subsystem
// should be wrapped in Arc when shared. However, the inbox, shared_transport,
// and stats can be shared via their Arc wrappers.

impl std::fmt::Debug for TransportSubsystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransportSubsystem")
            .field("handler", &"<RealTransportHandler>")
            .field("inbox_len", &self.inbox_len())
            .field("is_shared", &self.is_shared())
            .field("stats", &self.stats_snapshot())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::transport::TransportEnvelope;
    use aura_core::identifiers::ContextId;
    use std::collections::HashMap;

    fn test_envelope() -> TransportEnvelope {
        TransportEnvelope {
            destination: AuthorityId::new_from_entropy([2u8; 32]),
            source: AuthorityId::new_from_entropy([1u8; 32]),
            context: ContextId::new_from_entropy([0u8; 32]),
            payload: vec![1, 2, 3],
            metadata: HashMap::new(),
            receipt: None,
        }
    }

    #[test]
    fn test_transport_subsystem_creation() {
        let subsystem = TransportSubsystem::new();
        assert!(!subsystem.is_shared());
        assert_eq!(subsystem.inbox_len(), 0);
    }

    #[test]
    fn test_inbox_operations() {
        let subsystem = TransportSubsystem::new();
        let envelope = test_envelope();

        subsystem.queue_envelope(envelope.clone());
        assert_eq!(subsystem.inbox_len(), 1);

        let drained = subsystem.drain_inbox();
        assert_eq!(drained.len(), 1);
        assert_eq!(subsystem.inbox_len(), 0);
    }

    #[test]
    fn test_stats_update() {
        let subsystem = TransportSubsystem::new();

        subsystem.update_stats(|s| {
            s.envelopes_sent += 5;
            s.envelopes_received += 3;
        });

        let snapshot = subsystem.stats_snapshot();
        assert_eq!(snapshot.envelopes_sent, 5);
        assert_eq!(snapshot.envelopes_received, 3);
    }
}
