//! Connection pool management for peer coordination
//!
//! Provides connection pooling and lifecycle management for synchronization
//! connections, integrating with aura-transport and aura-rendezvous.
//!
//! # Architecture
//!
//! The connection pool:
//! - Manages connection lifecycle (establish, reuse, close)
//! - Enforces connection limits per peer and globally
//! - Tracks connection health and metrics
//! - Integrates with aura-transport for actual connection management
//!
//! # Usage
//!

//! ```rust,ignore
//! use aura_sync::infrastructure::{ConnectionPool, PoolConfig};
//! use aura_core::DeviceId;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = PoolConfig::default();
//! let mut pool = ConnectionPool::new(config);
//!
//! let peer_id = DeviceId::from_bytes([1; 32]);
//!
//! // Obtain current time from a clock provider (not shown here for brevity)
//! # let now_ms = 0; // provided timestamp
//!
//! // Acquire connection from pool
//! let conn = pool.acquire(peer_id, now).await?;
//!
//! // Use connection...
//!
//! // Return connection to pool
//! pool.release(peer_id, conn, now)?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::core::{sync_resource_exhausted, sync_session_error, SyncResult};
use aura_core::{hash, DeviceId, SessionId};
static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

fn next_sequence() -> u64 {
    NEXT_CONN_ID.fetch_add(1, Ordering::SeqCst)
}

// =============================================================================
// Transport Integration Types
// =============================================================================

/// Information about an established transport connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConnectionInfo {
    /// Unique connection identifier from transport layer
    pub connection_id: String,
    /// Transport protocol used (quic, tcp, webrtc)
    pub protocol: String,
    /// Remote peer address
    pub remote_address: String,
    /// Peer's public key for encryption
    pub public_key: Vec<u8>,
    /// When the connection was established
    pub established_at: u64,
}

/// Peer endpoint information for connection establishment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEndpoint {
    /// Transport protocol (quic, tcp, webrtc)
    pub protocol: String,
    /// Network address (host:port)
    pub address: String,
    /// Peer's transport public key
    pub public_key: Vec<u8>,
    /// Optional signaling server for WebRTC
    pub signaling_server: Option<String>,
}

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for connection pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum total connections across all peers
    pub max_total_connections: usize,

    /// Maximum connections per peer
    pub max_connections_per_peer: usize,

    /// Idle timeout after which connections are closed
    pub idle_timeout: Duration,

    /// Maximum time to wait for connection acquisition
    pub acquire_timeout: Duration,

    /// Enable connection health checks
    pub health_checks_enabled: bool,

    /// Interval between health checks
    pub health_check_interval: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_total_connections: 100,
            max_connections_per_peer: 10,
            idle_timeout: Duration::from_secs(300), // 5 minutes
            acquire_timeout: Duration::from_secs(10),
            health_checks_enabled: true,
            health_check_interval: Duration::from_secs(60),
        }
    }
}

// =============================================================================
// Connection Metadata
// =============================================================================

/// Metadata about a pooled connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    /// Connection identifier (from aura-transport)
    pub connection_id: String,

    /// Peer device ID
    pub peer_id: DeviceId,

    /// Session ID associated with this connection
    pub session_id: Option<SessionId>,

    /// When connection was established (Unix timestamp in seconds)
    pub established_at: u64,

    /// Last time connection was used (Unix timestamp in seconds)
    pub last_used_at: u64,

    /// Number of times connection has been reused
    pub reuse_count: u64,

    /// Current connection state
    pub state: ConnectionState,

    /// Whether connection passed last health check
    pub healthy: bool,

    /// Transport layer connection information
    pub transport_info: Option<TransportConnectionInfo>,
}

impl ConnectionMetadata {
    /// Create new connection metadata
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider and pass it to this method
    pub fn new(connection_id: String, peer_id: DeviceId, now: u64) -> Self {
        Self {
            connection_id,
            peer_id,
            session_id: None,
            established_at: now,
            last_used_at: now,
            reuse_count: 0,
            state: ConnectionState::Idle,
            healthy: true,
            transport_info: None,
        }
    }

    /// Create new connection metadata with transport information
    pub fn new_with_transport(
        connection_id: String,
        peer_id: DeviceId,
        transport_info: TransportConnectionInfo,
        now: u64,
    ) -> Self {
        Self {
            connection_id,
            peer_id,
            session_id: None,
            established_at: now,
            last_used_at: now,
            reuse_count: 0,
            state: ConnectionState::Idle,
            healthy: true,
            transport_info: Some(transport_info),
        }
    }

    /// Check if connection is idle
    pub fn is_idle(&self) -> bool {
        matches!(self.state, ConnectionState::Idle)
    }

    /// Check if connection has been idle for too long
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider
    pub fn is_expired(&self, timeout: Duration, now: u64) -> bool {
        if !self.is_idle() {
            return false;
        }
        let elapsed_secs = now.saturating_sub(self.last_used_at);
        Duration::from_secs(elapsed_secs) > timeout
    }

    /// Mark connection as acquired
    ///
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn acquire(&mut self, session_id: SessionId, now: u64) {
        self.state = ConnectionState::Active;
        self.session_id = Some(session_id);
        self.last_used_at = now;
        self.reuse_count += 1;
    }

    /// Mark connection as released
    ///
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn release(&mut self, now: u64) {
        self.state = ConnectionState::Idle;
        self.session_id = None;
        self.last_used_at = now;
    }

    /// Mark connection as closed
    pub fn close(&mut self) {
        self.state = ConnectionState::Closed;
    }
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Connection idle in pool
    Idle,

    /// Connection actively being used
    Active,

    /// Connection being established
    Connecting,

    /// Connection closed
    Closed,

    /// Connection failed health check
    Failed,
}

// =============================================================================
// Connection Handle
// =============================================================================

/// Handle to a pooled connection
///
/// This is what users receive when acquiring a connection from the pool.
/// The actual connection details are managed by aura-transport.
#[derive(Debug, Clone)]
pub struct ConnectionHandle {
    /// Connection ID
    pub id: String,

    /// Associated peer
    pub peer_id: DeviceId,

    /// Session using this connection
    pub session_id: SessionId,

    /// When connection was acquired (Unix timestamp in seconds)
    pub acquired_at: u64,
}

impl ConnectionHandle {
    /// Create a new connection handle
    ///
    /// Note: Callers should obtain `now` via their time provider and pass it to this method
    pub fn new(id: String, peer_id: DeviceId, session_id: SessionId, now: u64) -> Self {
        Self {
            id,
            peer_id,
            session_id,
            acquired_at: now,
        }
    }

    /// Get connection age
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider
    pub fn age(&self, now: u64) -> Duration {
        let elapsed_secs = now.saturating_sub(self.acquired_at);
        Duration::from_secs(elapsed_secs)
    }
}

// =============================================================================
// Connection Pool
// =============================================================================

/// Connection pool for managing peer connections
///
/// Integrates with aura-transport for actual connection management
/// and provides pooling, lifecycle management, and health checking.
pub struct ConnectionPool {
    /// Configuration
    config: PoolConfig,

    /// Active connections by peer
    connections: HashMap<DeviceId, Vec<ConnectionMetadata>>,

    /// Total connection count
    total_connections: usize,

    /// Pool statistics
    stats: PoolStatistics,
}

impl ConnectionPool {
    /// Create a new connection pool
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            connections: HashMap::new(),
            total_connections: 0,
            stats: PoolStatistics::default(),
        }
    }

    fn derive_session_id(&self, peer_id: DeviceId, now: u64) -> SessionId {
        let mut material = Vec::with_capacity(41);
        material.extend_from_slice(b"aura-sync-session");
        material.extend_from_slice(
            &peer_id
                .to_bytes()
                .unwrap_or([0u8; 32]),
        );
        material.extend_from_slice(&now.to_le_bytes());
        material.extend_from_slice(&self.total_connections.to_le_bytes());
        SessionId::new_from_entropy(hash::hash(&material))
    }

    /// Acquire a connection to a peer
    ///
    /// Tries to reuse an idle connection first, otherwise creates a new one.
    /// Returns error if pool limits are exceeded or timeout occurs.
    pub async fn acquire(&mut self, peer_id: DeviceId, now: u64) -> SyncResult<ConnectionHandle> {
        let session_id = self.derive_session_id(peer_id, now);

        if let Some(connections) = self.connections.get_mut(&peer_id) {
            if let Some(idle_conn) = connections.iter_mut().find(|c| c.is_idle() && c.healthy) {
                idle_conn.acquire(session_id, now);
                self.stats.connections_reused += 1;

                return Ok(ConnectionHandle::new(
                    idle_conn.connection_id.clone(),
                    peer_id,
                    session_id,
                    now,
                ));
            }
        }

        // Check limits before creating new connection
        if self.total_connections >= self.config.max_total_connections {
            self.stats.connection_limit_hits += 1;
            return Err(sync_resource_exhausted(
                "connections",
                "Connection pool limit reached",
            ));
        }

        // Check peer connection limit before creating new connection
        {
            let peer_connections = self.connections.entry(peer_id).or_default();
            if peer_connections.len() >= self.config.max_connections_per_peer {
                self.stats.connection_limit_hits += 1;
                return Err(sync_resource_exhausted(
                    "connections",
                    format!("Per-peer connection limit reached for {:?}", peer_id),
                ));
            }
        } // Drop mutable borrow

        // Create new connection via aura-transport
        let transport_connection = self.establish_transport_connection(peer_id).await?;
        let connection_id = transport_connection.connection_id.clone();
        let mut metadata = ConnectionMetadata::new_with_transport(
            connection_id.clone(),
            peer_id,
            transport_connection,
            now,
        );
        metadata.acquire(session_id, now);

        // Get peer connections again after transport connection creation
        let peer_connections = self.connections.entry(peer_id).or_default();
        peer_connections.push(metadata);
        self.total_connections += 1;
        self.stats.connections_created += 1;

        Ok(ConnectionHandle::new(
            connection_id,
            peer_id,
            session_id,
            now,
        ))
    }

    /// Release a connection back to the pool
    pub fn release(
        &mut self,
        peer_id: DeviceId,
        handle: ConnectionHandle,
        now: u64,
    ) -> SyncResult<()> {
        let connections = self
            .connections
            .get_mut(&peer_id)
            .ok_or_else(|| sync_session_error("No connections for peer"))?;

        let conn = connections
            .iter_mut()
            .find(|c| c.connection_id == handle.id)
            .ok_or_else(|| sync_session_error("Connection not found in pool"))?;

        conn.release(now);
        self.stats.connections_released += 1;

        Ok(())
    }

    /// Close a connection
    pub async fn close(&mut self, peer_id: DeviceId, connection_id: &str) -> SyncResult<()> {
        let connections = self
            .connections
            .get_mut(&peer_id)
            .ok_or_else(|| sync_session_error("No connections for peer"))?;

        if let Some(pos) = connections
            .iter()
            .position(|c| c.connection_id == connection_id)
        {
            let removed = connections.remove(pos);
            if self.total_connections > 0 {
                self.total_connections -= 1;
            }
            self.stats.connections_closed += 1;

            // Close connection via aura-transport
            if let Err(e) = self.close_transport_connection(&removed).await {
                tracing::warn!(
                    "Failed to close transport connection {}: {}",
                    removed.connection_id,
                    e
                );
            }
            Ok(())
        } else {
            Err(sync_session_error("Connection not found"))
        }
    }

    /// Remove expired idle connections
    ///
    /// Note: Callers should obtain `now` as Unix timestamp via their time provider
    pub async fn evict_expired(&mut self, now: u64) -> usize {
        let mut evicted = 0;
        let idle_timeout = self.config.idle_timeout;

        // Step 1: Collect all expired connections to close
        let mut all_connections_to_remove = Vec::new();

        for (_peer_id, connections) in self.connections.iter_mut() {
            let before = connections.len();

            let mut peer_connections_to_remove = Vec::new();
            connections.retain(|conn| {
                let expired = conn.is_expired(idle_timeout, now);
                if expired {
                    peer_connections_to_remove.push(conn.clone());
                }
                !expired
            });

            all_connections_to_remove.extend(peer_connections_to_remove);

            let removed = before - connections.len();
            evicted += removed;
            self.total_connections = self.total_connections.saturating_sub(removed);
        }

        // Step 2: Close expired connections via aura-transport (no borrowing conflicts)
        for conn in all_connections_to_remove {
            if let Err(e) = self.close_transport_connection(&conn).await {
                tracing::warn!(
                    "Failed to close expired transport connection {}: {}",
                    conn.connection_id,
                    e
                );
            }
        }

        // Remove peer entries with no connections
        self.connections.retain(|_, v| !v.is_empty());

        self.stats.connections_evicted += evicted;
        evicted
    }

    /// Get connection metadata
    pub fn get_connection_metadata(
        &self,
        peer_id: &DeviceId,
        connection_id: &str,
    ) -> Option<&ConnectionMetadata> {
        self.connections
            .get(peer_id)?
            .iter()
            .find(|c| c.connection_id == connection_id)
    }

    /// Get all connections for a peer
    pub fn get_peer_connections(&self, peer_id: &DeviceId) -> Option<&[ConnectionMetadata]> {
        self.connections.get(peer_id).map(|v| v.as_slice())
    }

    /// Get pool statistics
    pub fn statistics(&self) -> &PoolStatistics {
        &self.stats
    }

    /// Get current total connection count
    pub fn total_connections(&self) -> usize {
        self.total_connections
    }

    /// Get number of peers with connections
    pub fn peer_count(&self) -> usize {
        self.connections.len()
    }

    /// Establish transport connection to peer via aura-transport
    #[allow(clippy::disallowed_methods, clippy::unwrap_used)]
    async fn establish_transport_connection(
        &self,
        peer_id: DeviceId,
    ) -> SyncResult<TransportConnectionInfo> {
        tracing::debug!("Establishing transport connection to peer {}", peer_id);

        // Synthetic connection record until transport wiring is attached via NetworkEffects.
        let connection_id = format!("transport_{}_{}", peer_id, next_sequence());

        tracing::info!(
            "Established transport connection {} to peer {}",
            connection_id,
            peer_id
        );

        Ok(TransportConnectionInfo {
            connection_id,
            protocol: "quic".to_string(), // Default to QUIC
            remote_address: format!("peer_{}.local:8080", peer_id),
            public_key: hash::hash(peer_id.to_string().as_bytes()).to_vec(),
            established_at: next_sequence(),
        })
    }

    /// Close transport connection via aura-transport
    async fn close_transport_connection(&self, metadata: &ConnectionMetadata) -> SyncResult<()> {
        tracing::debug!("Closing transport connection {}", metadata.connection_id);

        // In a full implementation, this would:
        // 1. Use TransportEffects to close the actual transport connection
        // 2. Clean up any associated resources
        // 3. Send connection close notifications if needed

        if let Some(transport_info) = &metadata.transport_info {
            tracing::info!(
                "Closing {} connection {} to peer {} at {}",
                transport_info.protocol,
                metadata.connection_id,
                metadata.peer_id,
                transport_info.remote_address
            );

            // In a full implementation, this would call effects.close_connection(&metadata.connection_id).await?;

            tracing::debug!(
                "Transport connection {} closed successfully",
                metadata.connection_id
            );
        } else {
            tracing::warn!(
                "No transport info for connection {}, skipping transport close",
                metadata.connection_id
            );
        }

        Ok(())
    }
}

/// Connection pool statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PoolStatistics {
    /// Total connections created
    pub connections_created: u64,

    /// Total connections reused
    pub connections_reused: u64,

    /// Total connections released
    pub connections_released: u64,

    /// Total connections closed
    pub connections_closed: u64,

    /// Total connections evicted due to idle timeout
    pub connections_evicted: usize,

    /// Number of times connection limit was hit
    pub connection_limit_hits: u64,
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_acquisition_and_release() {
        let config = PoolConfig::default();
        let mut pool = ConnectionPool::new(config);

        let peer_id = DeviceId::from_bytes([1; 32]);

        let now = 1000000u64; // Unix timestamp

        // Acquire connection
        let handle = pool.acquire(peer_id, now).await.unwrap();
        assert_eq!(pool.total_connections(), 1);

        // Release connection
        pool.release(peer_id, handle, now).unwrap();

        // Connection should be reused
        let _handle2 = pool.acquire(peer_id, now).await.unwrap();
        assert_eq!(pool.total_connections(), 1);
        assert_eq!(pool.statistics().connections_reused, 1);
    }

    #[tokio::test]
    async fn test_connection_pool_limits() {
        let config = PoolConfig {
            max_total_connections: 2,
            ..Default::default()
        };

        let mut pool = ConnectionPool::new(config);

        let peer1 = DeviceId::from_bytes([1; 32]);
        let peer2 = DeviceId::from_bytes([2; 32]);

        let now = 1000000u64; // Unix timestamp

        // Acquire 2 connections
        let _handle1 = pool.acquire(peer1, now).await.unwrap();
        let _handle2 = pool.acquire(peer2, now).await.unwrap();

        // Third should fail
        let result = pool.acquire(peer1, now).await;
        assert!(result.is_err());
        assert_eq!(pool.statistics().connection_limit_hits, 1);
    }

    #[tokio::test]
    async fn test_connection_eviction() {
        let config = PoolConfig {
            idle_timeout: Duration::from_millis(10),
            ..Default::default()
        };

        let mut pool = ConnectionPool::new(config);

        let peer_id = DeviceId::from_bytes([1; 32]);

        let now = 1000000u64; // Unix timestamp

        // Acquire and release connection
        let handle = pool.acquire(peer_id, now).await.unwrap();
        pool.release(peer_id, handle, now).unwrap();

        // Advance time past idle timeout (10ms timeout, we advance 100ms)
        let later = now + 100;
        let evicted = pool.evict_expired(later).await;
        assert_eq!(evicted, 1);
        assert_eq!(pool.total_connections(), 0);
    }

    #[tokio::test]
    async fn test_connection_metadata_tracking() {
        let config = PoolConfig::default();
        let mut pool = ConnectionPool::new(config);

        let peer_id = DeviceId::from_bytes([1; 32]);

        let now = 1000000u64; // Unix timestamp

        // Acquire connection
        let handle = pool.acquire(peer_id, now).await.unwrap();

        // Check metadata
        let metadata = pool.get_connection_metadata(&peer_id, &handle.id).unwrap();
        assert_eq!(metadata.state, ConnectionState::Active);
        assert_eq!(metadata.reuse_count, 1);

        let connection_id = metadata.connection_id.clone();

        // Release and check again
        pool.release(peer_id, handle, now).unwrap();

        let metadata = pool
            .get_connection_metadata(&peer_id, &connection_id)
            .unwrap();
        assert_eq!(metadata.state, ConnectionState::Idle);
    }
}
