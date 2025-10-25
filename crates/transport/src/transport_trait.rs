//! Transport trait for unified transport interface
//!
//! Defines common interface for all transport implementations

use crate::{Connection, Result};
use aura_journal::DeviceId;
use async_trait::async_trait;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Unified transport interface for all transport implementations
#[async_trait]
pub trait Transport: Send + Sync {
    /// Transport-specific connection type
    type ConnectionType: Connection + Send + Sync;

    /// Send message to a specific peer
    async fn send_to_peer(&self, peer_id: DeviceId, message: &[u8]) -> Result<()>;

    /// Receive message from any peer with timeout
    async fn receive_message(&self, timeout: Duration) -> Result<Option<(DeviceId, Vec<u8>)>>;

    /// Establish connection to a peer
    async fn connect_to_peer(&self, peer_id: DeviceId) -> Result<Uuid>;

    /// Disconnect from a peer
    async fn disconnect_from_peer(&self, peer_id: DeviceId) -> Result<()>;

    /// Check if peer is reachable
    async fn is_peer_reachable(&self, peer_id: DeviceId) -> bool;

    /// Get all active connections
    fn get_connections(&self) -> Vec<Self::ConnectionType>;

    /// Start the transport (begin listening, polling, etc.)
    async fn start(&mut self, message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>) -> Result<()>;

    /// Stop the transport
    async fn stop(&mut self) -> Result<()>;

    /// Get transport name/type
    fn transport_type(&self) -> &'static str;

    /// Check if transport is healthy/operational
    async fn health_check(&self) -> bool;
}