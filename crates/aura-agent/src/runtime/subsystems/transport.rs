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
use std::sync::{
    atomic::{AtomicU32, AtomicU64, Ordering},
    Arc,
};

#[derive(Debug, Default)]
struct TransportStatsCounters {
    envelopes_sent: AtomicU64,
    envelopes_received: AtomicU64,
    send_failures: AtomicU64,
    receive_failures: AtomicU64,
    active_channels: AtomicU32,
    total_payload_bytes: AtomicU64,
}

impl TransportStatsCounters {
    fn record_send(&self, payload_len: usize) {
        self.envelopes_sent.fetch_add(1, Ordering::Relaxed);
        self.total_payload_bytes
            .fetch_add(payload_len as u64, Ordering::Relaxed);
    }

    fn record_receive(&self) {
        self.envelopes_received.fetch_add(1, Ordering::Relaxed);
    }

    fn record_send_failure(&self) {
        self.send_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn record_receive_failure(&self) {
        self.receive_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn set_active_channels(&self, active: u32) {
        self.active_channels.store(active, Ordering::Relaxed);
    }

    fn snapshot(&self) -> TransportStats {
        let sent = self.envelopes_sent.load(Ordering::Relaxed);
        let total_bytes = self.total_payload_bytes.load(Ordering::Relaxed);
        let avg = if sent > 0 {
            (total_bytes / sent) as u32
        } else {
            0
        };

        TransportStats {
            envelopes_sent: sent,
            envelopes_received: self.envelopes_received.load(Ordering::Relaxed),
            send_failures: self.send_failures.load(Ordering::Relaxed),
            receive_failures: self.receive_failures.load(Ordering::Relaxed),
            active_channels: self.active_channels.load(Ordering::Relaxed),
            avg_envelope_size: avg,
        }
    }
}

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
    stats: Arc<TransportStatsCounters>,
}

impl TransportSubsystem {
    /// Create a new transport subsystem with local inbox
    pub fn new() -> Self {
        Self {
            handler: aura_effects::transport::RealTransportHandler::default(),
            inbox: Arc::new(RwLock::new(Vec::new())),
            shared_transport: None,
            stats: Arc::new(TransportStatsCounters::default()),
        }
    }

    /// Create a transport subsystem with shared transport for simulation
    #[allow(dead_code)]
    pub fn with_shared_transport(shared: SharedTransport, authority: AuthorityId) -> Self {
        // Register this authority in the shared network
        shared.register(authority);

        Self {
            handler: aura_effects::transport::RealTransportHandler::default(),
            inbox: shared.inbox_for(authority),
            shared_transport: Some(shared),
            stats: Arc::new(TransportStatsCounters::default()),
        }
    }

    /// Create from existing components
    ///
    /// Stats are created internally since `TransportStatsCounters` is a private type.
    pub fn from_parts(
        handler: aura_effects::transport::RealTransportHandler,
        inbox: Arc<RwLock<Vec<TransportEnvelope>>>,
        shared_transport: Option<SharedTransport>,
    ) -> Self {
        Self {
            handler,
            inbox,
            shared_transport,
            stats: Arc::new(TransportStatsCounters::default()),
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
    pub fn stats(&self) -> Arc<TransportStatsCounters> {
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
        if let Some(shared) = self.shared_transport.as_ref() {
            shared.route_envelope(envelope);
        } else {
            let mut inbox = self.inbox.write();
            inbox.push(envelope);
        }
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

    /// Record a successful send and payload size
    pub fn record_send(&self, payload_len: usize) {
        self.stats.record_send(payload_len);
    }

    /// Record a successful receive
    pub fn record_receive(&self) {
        self.stats.record_receive();
    }

    /// Record a send failure
    pub fn record_send_failure(&self) {
        self.stats.record_send_failure();
    }

    /// Record a receive failure
    pub fn record_receive_failure(&self) {
        self.stats.record_receive_failure();
    }

    /// Set active channel count
    pub fn set_active_channels(&self, active: u32) {
        self.stats.set_active_channels(active);
    }

    /// Get a snapshot of current stats
    pub fn stats_snapshot(&self) -> TransportStats {
        self.stats.snapshot()
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

        for _ in 0..5 {
            subsystem.record_send(0);
        }
        for _ in 0..3 {
            subsystem.record_receive();
        }

        let snapshot = subsystem.stats_snapshot();
        assert_eq!(snapshot.envelopes_sent, 5);
        assert_eq!(snapshot.envelopes_received, 3);
    }
}
