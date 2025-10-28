//! Protocol transport abstractions.

use async_trait::async_trait;
use aura_types::Result;
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Message exchanged between protocol participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Source participant.
    pub from: DeviceId,
    /// Destination participant.
    pub to: DeviceId,
    /// Opaque payload.
    pub payload: Vec<u8>,
    /// Optional session identifier.
    pub session_id: Option<Uuid>,
}

/// Transport abstraction used by protocols.
#[async_trait]
pub trait ProtocolTransport: Send + Sync {
    /// Send a message to a participant.
    async fn send(&self, message: ProtocolMessage) -> Result<()>;
    /// Broadcast a payload to all connected peers.
    async fn broadcast(
        &self,
        from: DeviceId,
        payload: Vec<u8>,
        session_id: Option<Uuid>,
    ) -> Result<()>;
    /// Receive next inbound message (awaits if necessary).
    async fn receive(&self) -> Result<ProtocolMessage>;
    /// Determine whether a peer is reachable.
    async fn is_reachable(&self, device_id: DeviceId) -> bool;
    /// Return list of connected peers.
    async fn connected_peers(&self) -> Vec<DeviceId>;
}

/// Extension helpers for additional semantics.
#[async_trait]
pub trait ProtocolTransportExt: ProtocolTransport {
    /// Send and await acknowledgement (default: best-effort).
    async fn send_reliable(&self, message: ProtocolMessage) -> Result<()> {
        self.send(message).await
    }

    /// Multicast to a set of recipients.
    async fn multicast(
        &self,
        from: DeviceId,
        recipients: &[DeviceId],
        payload: Vec<u8>,
        session_id: Option<Uuid>,
    ) -> Result<()> {
        for recipient in recipients {
            let message = ProtocolMessage {
                from,
                to: *recipient,
                payload: payload.clone(),
                session_id,
            };
            self.send(message).await?;
        }
        Ok(())
    }
}

impl<T: ProtocolTransport + ?Sized> ProtocolTransportExt for T {}
