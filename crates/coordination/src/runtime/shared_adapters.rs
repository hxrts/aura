//! Shared adapters for journal and transport integration.
//!
//! This module provides production-ready adapters that wrap the real ledger
//! (journal) and transport stack for use with the lifecycle scheduler.

use crate::Transport;
use aura_journal::AccountLedger;
use aura_types::DeviceId;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Shared adapter for journal/ledger access
///
/// Provides a standardized interface to the journal while tracking
/// access patterns and metrics for the scheduler.
#[derive(Clone)]
pub struct SharedJournalAdapter {
    ledger: Arc<RwLock<AccountLedger>>,
    access_count: Arc<std::sync::atomic::AtomicU64>,
}

impl SharedJournalAdapter {
    /// Create a new shared journal adapter
    pub fn new(ledger: Arc<RwLock<AccountLedger>>) -> Self {
        info!("Creating shared journal adapter");
        Self {
            ledger,
            access_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get the wrapped ledger for protocol execution
    pub fn ledger(&self) -> Arc<RwLock<AccountLedger>> {
        self.access_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        debug!(
            "Journal access requested (total: {})",
            self.access_count.load(std::sync::atomic::Ordering::Relaxed)
        );
        self.ledger.clone()
    }

    /// Get access statistics
    pub fn access_stats(&self) -> JournalAccessStats {
        JournalAccessStats {
            total_accesses: self.access_count.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
}

/// Shared adapter for transport access
///
/// Provides a standardized interface to the transport layer while tracking
/// connection metrics and peer activity for the scheduler.
#[derive(Clone)]
pub struct SharedTransportAdapter {
    transport: Arc<dyn Transport>,
    connected_peers: Arc<tokio::sync::RwLock<std::collections::HashSet<DeviceId>>>,
    message_count: Arc<std::sync::atomic::AtomicU64>,
    last_activity: Arc<std::sync::atomic::AtomicU64>,
}

impl SharedTransportAdapter {
    /// Create a new shared transport adapter
    pub fn new(transport: Arc<dyn Transport>) -> Self {
        info!("Creating shared transport adapter");
        Self {
            transport,
            connected_peers: Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new())),
            message_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            last_activity: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get the wrapped transport for protocol execution
    pub fn transport(&self) -> Arc<dyn Transport> {
        self.update_activity();
        debug!("Transport access requested");
        self.transport.clone()
    }

    /// Register a peer as connected
    pub async fn register_peer_connected(&self, peer_id: DeviceId) {
        let mut peers = self.connected_peers.write().await;
        if peers.insert(peer_id) {
            info!("Peer {} connected (total peers: {})", peer_id, peers.len());
        }
        self.update_activity();
    }

    /// Register a peer as disconnected
    pub async fn register_peer_disconnected(&self, peer_id: DeviceId) {
        let mut peers = self.connected_peers.write().await;
        if peers.remove(&peer_id) {
            warn!(
                "Peer {} disconnected (remaining peers: {})",
                peer_id,
                peers.len()
            );
        }
        self.update_activity();
    }

    /// Record a message being sent or received
    pub fn record_message_activity(&self) {
        self.message_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.update_activity();
    }

    /// Get transport statistics
    pub async fn transport_stats(&self) -> TransportStats {
        let peer_count = self.connected_peers.read().await.len();
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last_activity = self
            .last_activity
            .load(std::sync::atomic::Ordering::Relaxed);

        TransportStats {
            connected_peers: peer_count,
            total_messages: self
                .message_count
                .load(std::sync::atomic::Ordering::Relaxed),
            last_activity_seconds_ago: if last_activity > 0 {
                current_time.saturating_sub(last_activity)
            } else {
                0
            },
        }
    }

    /// Get list of connected peers
    pub async fn connected_peers(&self) -> Vec<DeviceId> {
        self.connected_peers.read().await.iter().copied().collect()
    }

    fn update_activity(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_activity
            .store(now, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Environment bundle containing both journal and transport adapters
///
/// This provides a convenient way to pass both adapters to the scheduler
/// while ensuring they're properly initialized and configured.
#[derive(Clone)]
pub struct EnvironmentBundle {
    pub journal: SharedJournalAdapter,
    pub transport: SharedTransportAdapter,
}

impl EnvironmentBundle {
    /// Create a new environment bundle
    pub fn new(ledger: Arc<RwLock<AccountLedger>>, transport: Arc<dyn Transport>) -> Self {
        info!("Creating environment bundle for scheduler");
        Self {
            journal: SharedJournalAdapter::new(ledger),
            transport: SharedTransportAdapter::new(transport),
        }
    }

    /// Get combined metrics from both adapters
    pub async fn combined_metrics(&self) -> EnvironmentMetrics {
        let journal_stats = self.journal.access_stats();
        let transport_stats = self.transport.transport_stats().await;

        EnvironmentMetrics {
            journal: journal_stats,
            transport: transport_stats,
        }
    }
}

/// Statistics for journal access patterns
#[derive(Debug, Clone)]
pub struct JournalAccessStats {
    pub total_accesses: u64,
}

/// Statistics for transport activity
#[derive(Debug, Clone)]
pub struct TransportStats {
    pub connected_peers: usize,
    pub total_messages: u64,
    pub last_activity_seconds_ago: u64,
}

/// Combined environment metrics
#[derive(Debug, Clone)]
pub struct EnvironmentMetrics {
    pub journal: JournalAccessStats,
    pub transport: TransportStats,
}
