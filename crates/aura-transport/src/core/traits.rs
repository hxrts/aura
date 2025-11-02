//! Core transport trait definitions

use crate::{TransportEnvelope, TransportResult};
use async_trait::async_trait;
use aura_types::DeviceId;
use std::time::Duration;

/// Core transport interface for all transport implementations
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send message envelope to a specific peer
    async fn send(&self, envelope: TransportEnvelope) -> TransportResult<()>;

    /// Receive message envelope with timeout
    async fn receive(&self, timeout: Duration) -> TransportResult<Option<TransportEnvelope>>;

    /// Establish connection to a peer
    async fn connect(&self, peer_id: DeviceId) -> TransportResult<()>;

    /// Disconnect from a peer
    async fn disconnect(&self, peer_id: DeviceId) -> TransportResult<()>;

    /// Check if peer is reachable
    async fn is_reachable(&self, peer_id: DeviceId) -> bool;

    /// Start the transport
    async fn start(&mut self) -> TransportResult<()>;

    /// Stop the transport
    async fn stop(&mut self) -> TransportResult<()>;

    /// Get transport type identifier
    fn transport_type(&self) -> &'static str;
}
