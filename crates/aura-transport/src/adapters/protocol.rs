//! Protocol adapter for AuraProtocolHandler interface

use crate::{core::Transport, MessageMetadata, TransportEnvelope, TransportResult};
use aura_types::DeviceId;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

/// Adapter that bridges AuraProtocolHandler to core Transport
pub struct ProtocolAdapter {
    transport: Arc<dyn Transport>,
    device_id: DeviceId,
}

impl ProtocolAdapter {
    /// Create a new protocol adapter with the given transport and device ID
    pub fn new(transport: Arc<dyn Transport>, device_id: DeviceId) -> Self {
        Self {
            transport,
            device_id,
        }
    }

    /// Send protocol message to peer
    pub async fn send_to_peer(
        &self,
        peer_id: DeviceId,
        message: &[u8],
        message_type: String,
    ) -> TransportResult<()> {
        let envelope = TransportEnvelope {
            from: self.device_id,
            to: peer_id,
            #[allow(clippy::disallowed_methods)]
            message_id: Uuid::new_v4(),
            payload: message.to_vec(),
            metadata: MessageMetadata {
                timestamp: aura_types::time_utils::current_unix_timestamp_millis(),
                message_type,
                priority: crate::MessagePriority::Normal,
            },
        };

        self.transport.send(envelope).await
    }

    /// Receive protocol message from any peer
    pub async fn receive_from_peer(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, Vec<u8>, String)>> {
        match self.transport.receive(timeout).await? {
            Some(envelope) => Ok(Some((
                envelope.from,
                envelope.payload,
                envelope.metadata.message_type,
            ))),
            None => Ok(None),
        }
    }

    /// Connect to a peer
    pub async fn connect_to_peer(&self, peer_id: DeviceId) -> TransportResult<()> {
        self.transport.connect(peer_id).await
    }

    /// Disconnect from a peer
    pub async fn disconnect_from_peer(&self, peer_id: DeviceId) -> TransportResult<()> {
        self.transport.disconnect(peer_id).await
    }

    /// Check if peer is reachable
    pub async fn is_peer_reachable(&self, peer_id: DeviceId) -> bool {
        self.transport.is_reachable(peer_id).await
    }
}
