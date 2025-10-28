//! Protocol Transport Abstraction
//!
//! Provides an abstract interface for protocol-level message transport,
//! enabling dependency injection and testing.

use async_trait::async_trait;
use aura_types::Result;
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

/// Message sent between protocol participants
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMessage {
    /// Source device ID
    pub from: DeviceId,
    /// Destination device ID
    pub to: DeviceId,
    /// Message payload
    pub payload: Vec<u8>,
    /// Optional session ID for routing
    pub session_id: Option<uuid::Uuid>,
}

/// Transport abstraction for protocol-level communication
///
/// This trait provides a higher-level interface than raw transport,
/// focused on protocol participant communication needs.
#[async_trait]
pub trait ProtocolTransport: Send + Sync {
    /// Send a message to a specific participant
    async fn send(&self, message: ProtocolMessage) -> Result<()>;

    /// Broadcast a message to all participants
    async fn broadcast(
        &self,
        from: DeviceId,
        payload: Vec<u8>,
        session_id: Option<uuid::Uuid>,
    ) -> Result<()>;

    /// Receive the next message (blocking)
    async fn receive(&self) -> Result<ProtocolMessage>;

    /// Check if a peer is reachable
    async fn is_reachable(&self, device_id: DeviceId) -> bool;

    /// Get list of connected peers
    async fn connected_peers(&self) -> Vec<DeviceId>;
}

/// Extension trait for protocol transport operations
#[async_trait]
pub trait ProtocolTransportExt: ProtocolTransport {
    /// Send a message and wait for acknowledgment
    async fn send_reliable(&self, message: ProtocolMessage) -> Result<()> {
        // Default implementation just sends
        // Concrete implementations can add reliability
        self.send(message).await
    }

    /// Send to multiple recipients
    async fn multicast(
        &self,
        from: DeviceId,
        recipients: &[DeviceId],
        payload: Vec<u8>,
        session_id: Option<uuid::Uuid>,
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

/// Automatically implement ProtocolTransportExt for all ProtocolTransport implementations
impl<T: ProtocolTransport + ?Sized> ProtocolTransportExt for T {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Mock implementation for testing
    struct MockTransport {
        messages: Arc<Mutex<Vec<ProtocolMessage>>>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                messages: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl ProtocolTransport for MockTransport {
        async fn send(&self, message: ProtocolMessage) -> Result<()> {
            self.messages.lock().await.push(message);
            Ok(())
        }

        async fn broadcast(
            &self,
            from: DeviceId,
            payload: Vec<u8>,
            session_id: Option<uuid::Uuid>,
        ) -> Result<()> {
            let message = ProtocolMessage {
                from,
                to: DeviceId(uuid::Uuid::new_v4()),
                payload,
                session_id,
            };
            self.messages.lock().await.push(message);
            Ok(())
        }

        async fn receive(&self) -> Result<ProtocolMessage> {
            let mut messages = self.messages.lock().await;
            messages
                .pop()
                .ok_or_else(|| aura_types::AuraError::transport_failed("No messages available"))
        }

        async fn is_reachable(&self, _device_id: DeviceId) -> bool {
            true
        }

        async fn connected_peers(&self) -> Vec<DeviceId> {
            vec![]
        }
    }

    #[tokio::test]
    async fn test_protocol_transport() {
        let transport = MockTransport::new();
        let from = DeviceId(uuid::Uuid::new_v4());
        let to = DeviceId(uuid::Uuid::new_v4());

        let message = ProtocolMessage {
            from,
            to,
            payload: vec![1, 2, 3],
            session_id: None,
        };

        transport.send(message).await.unwrap();
        assert_eq!(transport.messages.lock().await.len(), 1);
    }

    #[tokio::test]
    async fn test_multicast() {
        let transport = MockTransport::new();
        let from = DeviceId(uuid::Uuid::new_v4());
        let recipients = vec![
            DeviceId(uuid::Uuid::new_v4()),
            DeviceId(uuid::Uuid::new_v4()),
        ];

        transport
            .multicast(from, &recipients, vec![1, 2, 3], None)
            .await
            .unwrap();
        assert_eq!(transport.messages.lock().await.len(), 2);
    }
}
