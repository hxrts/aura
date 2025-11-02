//! Example unified protocol handler

use crate::{
    adapters::ProtocolAdapter, core::TransportFactory, TransportConfig, TransportResult,
    TransportType,
};
use aura_types::DeviceId;
use std::time::Duration;

/// Example protocol handler using the unified transport system
pub struct ExampleProtocolHandler {
    adapter: ProtocolAdapter,
}

impl ExampleProtocolHandler {
    /// Create a new example handler with memory transport
    pub fn new_with_memory(device_id: DeviceId) -> TransportResult<Self> {
        let config = TransportConfig {
            transport_type: TransportType::Memory,
            device_id,
            max_message_size: 1024 * 1024,
            connection_timeout_ms: 30000,
        };

        let transport = TransportFactory::create_transport(&config)?;
        let adapter = ProtocolAdapter::new(transport, device_id);

        Ok(Self { adapter })
    }

    /// Send a protocol message
    pub async fn send_message(
        &self,
        peer_id: DeviceId,
        message: &[u8],
        message_type: &str,
    ) -> TransportResult<()> {
        self.adapter
            .send_to_peer(peer_id, message, message_type.to_string())
            .await
    }

    /// Receive a protocol message
    pub async fn receive_message(
        &self,
        timeout: Duration,
    ) -> TransportResult<Option<(DeviceId, Vec<u8>, String)>> {
        self.adapter.receive_from_peer(timeout).await
    }

    /// Connect to a peer
    pub async fn connect_to_peer(&self, peer_id: DeviceId) -> TransportResult<()> {
        self.adapter.connect_to_peer(peer_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::DeviceId;

    #[tokio::test]
    async fn test_unified_transport_system() {
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        let _handler1 = ExampleProtocolHandler::new_with_memory(device1).unwrap();
        let _handler2 = ExampleProtocolHandler::new_with_memory(device2).unwrap();

        // In a real implementation, we'd connect the underlying memory transports
        // This test demonstrates the API structure
    }
}
