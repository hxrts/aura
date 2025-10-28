//! Unified transport interface with authenticated channels and connection pooling
//!
//! This module implements the clean transport model from docs/040_storage.md Section 5
//! "Unified Transport Architecture" with clear separation between authentication
//! (transport layer) and authorization (application layer).

use crate::{TransportError, TransportErrorBuilder, TransportResult};
use async_trait::async_trait;
use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Authenticated channel handle
///
/// Represents an authenticated connection to a peer device.
/// Authentication is verified at the transport layer, but authorization
/// decisions happen at the application layer.
#[derive(Debug, Clone)]
pub struct AuthenticatedChannel {
    /// Channel identifier
    pub channel_id: Uuid,
    /// Peer device ID (authenticated)
    pub peer_device_id: Uuid,
    /// Peer address
    pub peer_addr: SocketAddr,
    /// Channel creation timestamp
    pub created_at: u64,
    /// Last activity timestamp for idle detection
    pub last_activity: u64,
}

impl AuthenticatedChannel {
    /// Check if channel is idle
    pub fn is_idle(&self, current_time: u64, idle_timeout: Duration) -> bool {
        let idle_duration = current_time.saturating_sub(self.last_activity);
        idle_duration > idle_timeout.as_millis() as u64
    }

    /// Update last activity timestamp
    pub fn touch(&mut self, current_time: u64) {
        self.last_activity = current_time;
    }
}

/// Device authentication credentials for transport layer
///
/// These credentials prove device identity at the transport layer.
/// They do NOT grant any permissions - that's handled by application-layer capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCredentials {
    /// Device identifier
    pub device_id: Uuid,
    /// Account identifier this device belongs to
    pub account_id: Vec<u8>,
    /// Device public key for signature verification
    pub device_public_key: Vec<u8>,
}

impl DeviceCredentials {
    /// Create new device credentials
    pub fn new(device_id: Uuid, account_id: Vec<u8>, device_public_key: Vec<u8>) -> Self {
        Self {
            device_id,
            account_id,
            device_public_key,
        }
    }

    /// Verify device signature
    pub fn verify_signature(&self, message: &[u8], signature: &[u8]) -> TransportResult<()> {
        let verifying_key =
            VerifyingKey::from_bytes(self.device_public_key.as_slice().try_into().map_err(
                |_| TransportErrorBuilder::not_authorized("Invalid device public key".to_string()),
            )?)
            .map_err(|e| {
                TransportErrorBuilder::not_authorized(format!("Invalid verifying key: {:?}", e))
            })?;

        let sig = Signature::from_slice(signature).map_err(|e| {
            TransportErrorBuilder::not_authorized(format!("Invalid signature format: {:?}", e))
        })?;

        verifying_key.verify(message, &sig).map_err(|e| {
            TransportErrorBuilder::not_authorized(format!("Signature verification failed: {:?}", e))
        })
    }
}

/// Handshake message for establishing authenticated channel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeMessage {
    /// Device credentials
    pub credentials: DeviceCredentials,
    /// Challenge nonce for mutual authentication
    pub challenge: Vec<u8>,
    /// Signature over challenge from peer (empty in initial message)
    pub challenge_response: Vec<u8>,
    /// Protocol version
    pub protocol_version: u32,
}

/// Connection pool statistics
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Total number of active connections
    pub active_connections: usize,
    /// Number of idle connections
    pub idle_connections: usize,
    /// Total bytes sent across all connections
    pub total_bytes_sent: u64,
    /// Total bytes received across all connections
    pub total_bytes_received: u64,
}

/// Unified authenticated transport trait
///
/// This trait provides authenticated channels with connection pooling.
/// Key principles:
/// - Transport only verifies device signatures (authentication)
/// - Application layer handles all authorization decisions
/// - Single connection can serve both chunk transfers and envelope flooding
/// - Connection pool efficiently manages peer connections
#[async_trait]
pub trait AuthenticatedTransport: Send + Sync {
    /// Establish authenticated channel with a peer
    ///
    /// This performs mutual authentication:
    /// 1. Exchange device credentials
    /// 2. Challenge-response to prove key ownership
    /// 3. Verify device signatures
    /// 4. Establish encrypted channel
    ///
    /// Returns an authenticated channel handle. No authorization checks are performed -
    /// that's the responsibility of the application layer using this channel.
    async fn establish_authenticated_channel(
        &self,
        peer_addr: SocketAddr,
        my_credentials: &DeviceCredentials,
        my_signing_key: &SigningKey,
    ) -> TransportResult<AuthenticatedChannel>;

    /// Get existing authenticated channel from pool
    ///
    /// Returns cached channel if one exists and is still valid.
    /// This enables connection reuse across multiple use cases.
    async fn get_channel(&self, peer_device_id: Uuid) -> Option<AuthenticatedChannel>;

    /// Send data over authenticated channel
    ///
    /// The channel must have been established via establish_authenticated_channel.
    /// Authorization checks are the caller's responsibility.
    async fn send(&self, channel: &AuthenticatedChannel, data: &[u8]) -> TransportResult<()>;

    /// Receive data from authenticated channel
    ///
    /// Returns data received from the peer, or None on timeout.
    /// Authorization checks are the caller's responsibility.
    async fn receive(
        &self,
        channel: &AuthenticatedChannel,
        timeout: Duration,
    ) -> TransportResult<Option<Vec<u8>>>;

    /// Close authenticated channel
    ///
    /// Removes channel from pool and closes underlying connection.
    async fn close_channel(&self, channel: &AuthenticatedChannel) -> TransportResult<()>;

    /// Get connection pool statistics
    async fn pool_stats(&self) -> PoolStats;

    /// Prune idle connections from pool
    ///
    /// Removes connections that have been idle longer than idle_timeout.
    async fn prune_idle_connections(&self, idle_timeout: Duration) -> TransportResult<usize>;
}

/// Connection pool for managing authenticated channels
///
/// Implements connection reuse across different use cases (storage, SSB, etc).
pub struct ConnectionPool {
    /// Active channels indexed by peer device ID
    channels: Arc<RwLock<BTreeMap<Uuid, AuthenticatedChannel>>>,
    /// Statistics
    stats: Arc<RwLock<PoolStats>>,
}

impl ConnectionPool {
    /// Create new connection pool
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(BTreeMap::new())),
            stats: Arc::new(RwLock::new(PoolStats {
                active_connections: 0,
                idle_connections: 0,
                total_bytes_sent: 0,
                total_bytes_received: 0,
            })),
        }
    }

    /// Add channel to pool
    pub async fn add_channel(&self, channel: AuthenticatedChannel) {
        let peer_id = channel.peer_device_id;
        let mut channels = self.channels.write().await;
        channels.insert(peer_id, channel);

        let mut stats = self.stats.write().await;
        stats.active_connections = channels.len();
    }

    /// Get channel from pool
    pub async fn get_channel(&self, peer_device_id: Uuid) -> Option<AuthenticatedChannel> {
        let channels = self.channels.read().await;
        channels.get(&peer_device_id).cloned()
    }

    /// Remove channel from pool
    pub async fn remove_channel(&self, peer_device_id: Uuid) -> Option<AuthenticatedChannel> {
        let mut channels = self.channels.write().await;
        let removed = channels.remove(&peer_device_id);

        if removed.is_some() {
            let mut stats = self.stats.write().await;
            stats.active_connections = channels.len();
        }

        removed
    }

    /// Update channel activity
    pub async fn touch_channel(&self, peer_device_id: Uuid, current_time: u64) {
        let mut channels = self.channels.write().await;
        if let Some(channel) = channels.get_mut(&peer_device_id) {
            channel.touch(current_time);
        }
    }

    /// Get pool statistics
    pub async fn stats(&self) -> PoolStats {
        self.stats.read().await.clone()
    }

    /// Prune idle connections
    pub async fn prune_idle(&self, current_time: u64, idle_timeout: Duration) -> usize {
        let mut channels = self.channels.write().await;
        let initial_count = channels.len();

        channels.retain(|_, channel| !channel.is_idle(current_time, idle_timeout));

        let pruned = initial_count - channels.len();

        if pruned > 0 {
            let mut stats = self.stats.write().await;
            stats.active_connections = channels.len();
        }

        pruned
    }

    /// Record bytes sent
    pub async fn record_send(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.total_bytes_sent += bytes;
    }

    /// Record bytes received
    pub async fn record_receive(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.total_bytes_received += bytes;
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_credentials() {
        let device_id = Uuid::new_v4();
        let account_id = b"test_account".to_vec();
        let device_public_key = vec![0u8; 32];

        let creds =
            DeviceCredentials::new(device_id, account_id.clone(), device_public_key.clone());

        assert_eq!(creds.device_id, device_id);
        assert_eq!(creds.account_id, account_id);
        assert_eq!(creds.device_public_key, device_public_key);
    }

    #[test]
    fn test_channel_idle_detection() {
        let channel = AuthenticatedChannel {
            channel_id: Uuid::new_v4(),
            peer_device_id: Uuid::new_v4(),
            peer_addr: "127.0.0.1:8080".parse().unwrap(),
            created_at: 1000,
            last_activity: 1000,
        };

        let idle_timeout = Duration::from_secs(60);

        // Not idle immediately
        assert!(!channel.is_idle(1000, idle_timeout));

        // Not idle after 30 seconds
        assert!(!channel.is_idle(31000, idle_timeout));

        // Idle after 61 seconds
        assert!(channel.is_idle(61001, idle_timeout));
    }

    #[tokio::test]
    async fn test_connection_pool() {
        let pool = ConnectionPool::new();

        let device_id = Uuid::new_v4();
        let channel = AuthenticatedChannel {
            channel_id: Uuid::new_v4(),
            peer_device_id: device_id,
            peer_addr: "127.0.0.1:8080".parse().unwrap(),
            created_at: 1000,
            last_activity: 1000,
        };

        // Add channel
        pool.add_channel(channel.clone()).await;

        // Get channel
        let retrieved = pool.get_channel(device_id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().peer_device_id, device_id);

        // Stats
        let stats = pool.stats().await;
        assert_eq!(stats.active_connections, 1);

        // Remove channel
        let removed = pool.remove_channel(device_id).await;
        assert!(removed.is_some());

        // No longer in pool
        let not_found = pool.get_channel(device_id).await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_idle_pruning() {
        let pool = ConnectionPool::new();

        let device1 = Uuid::new_v4();
        let device2 = Uuid::new_v4();

        let channel1 = AuthenticatedChannel {
            channel_id: Uuid::new_v4(),
            peer_device_id: device1,
            peer_addr: "127.0.0.1:8080".parse().unwrap(),
            created_at: 1000,
            last_activity: 1000, // Will be idle
        };

        let channel2 = AuthenticatedChannel {
            channel_id: Uuid::new_v4(),
            peer_device_id: device2,
            peer_addr: "127.0.0.1:8081".parse().unwrap(),
            created_at: 1000,
            last_activity: 50000, // Will not be idle
        };

        pool.add_channel(channel1).await;
        pool.add_channel(channel2).await;

        // Prune idle connections (60 second timeout, current time 61 seconds)
        let pruned = pool.prune_idle(61001, Duration::from_secs(60)).await;

        // Should have pruned 1 connection
        assert_eq!(pruned, 1);

        // device1 should be gone, device2 should remain
        assert!(pool.get_channel(device1).await.is_none());
        assert!(pool.get_channel(device2).await.is_some());
    }

    #[tokio::test]
    async fn test_activity_tracking() {
        let pool = ConnectionPool::new();

        let device_id = Uuid::new_v4();
        let channel = AuthenticatedChannel {
            channel_id: Uuid::new_v4(),
            peer_device_id: device_id,
            peer_addr: "127.0.0.1:8080".parse().unwrap(),
            created_at: 1000,
            last_activity: 1000,
        };

        pool.add_channel(channel).await;

        // Touch channel
        pool.touch_channel(device_id, 30000).await;

        // Channel should have updated last_activity
        let channel = pool.get_channel(device_id).await.unwrap();
        assert_eq!(channel.last_activity, 30000);
    }

    #[tokio::test]
    async fn test_stats_tracking() {
        let pool = ConnectionPool::new();

        pool.record_send(1024).await;
        pool.record_receive(2048).await;
        pool.record_send(512).await;

        let stats = pool.stats().await;
        assert_eq!(stats.total_bytes_sent, 1536);
        assert_eq!(stats.total_bytes_received, 2048);
    }
}
