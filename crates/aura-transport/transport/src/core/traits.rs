//! Core transport trait definitions
//!
//! This module defines the fundamental transport abstractions that all
//! transport implementations must follow.

use crate::TransportResult;
use async_trait::async_trait;
use aura_types::DeviceId;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Core transport interface for all transport implementations
///
/// This trait provides the fundamental operations that any transport
/// must support for peer-to-peer communication in the Aura system.
#[async_trait]
pub trait Transport: Send + Sync {
    /// Transport-specific connection type
    type ConnectionType: Send + Sync;

    /// Send message to a specific peer
    async fn send_to_peer(&self, peer_id: DeviceId, message: &[u8]) -> TransportResult<()>;

    /// Receive message from any peer with timeout
    async fn receive_message(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, Vec<u8>)>>;

    /// Establish connection to a peer
    async fn connect_to_peer(&self, peer_id: DeviceId) -> TransportResult<Uuid>;

    /// Disconnect from a peer
    async fn disconnect_from_peer(&self, peer_id: DeviceId) -> TransportResult<()>;

    /// Check if peer is reachable
    async fn is_peer_reachable(&self, peer_id: DeviceId) -> bool;

    /// Get all active connections
    fn get_connections(&self) -> Vec<Self::ConnectionType>;

    /// Start the transport (begin listening, polling, etc.)
    async fn start(
        &mut self,
        message_sender: mpsc::UnboundedSender<(DeviceId, Vec<u8>)>,
    ) -> TransportResult<()>;

    /// Stop the transport
    async fn stop(&mut self) -> TransportResult<()>;

    /// Get transport name/type
    fn transport_type(&self) -> &'static str;

    /// Check if transport is healthy/operational
    async fn health_check(&self) -> bool;
}
