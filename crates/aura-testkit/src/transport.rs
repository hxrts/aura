//! Test Transport Utilities
//!
//! Provides transport utilities and mock transport implementations for testing.
//! Uses the new middleware-based transport system from aura-transport.

use async_trait::async_trait;
use aura_core::{AuraResult, DeviceId};
// Note: TransportMiddlewareStack and TransportStackBuilder were removed in Week 11 cleanup
// use aura_transport::{TransportMiddlewareStack, TransportStackBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Type alias for complex message queue structure
type MessageQueue = Arc<RwLock<HashMap<DeviceId, Vec<(DeviceId, Vec<u8>)>>>>;

/// Simple transport interface for testing
///
/// This provides a TODO fix - Simplified transport interface that test code can use
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send a message to another device
    async fn send_message(&self, to: DeviceId, message: &[u8]) -> AuraResult<()>;
    /// Receive a message if one is available
    async fn receive_message(&self) -> AuraResult<Option<(DeviceId, Vec<u8>)>>;
    /// Get this transport's device ID
    fn device_id(&self) -> DeviceId;
}

/// In-memory transport implementation for testing
///
/// Provides a simple in-memory transport that can be used for local testing
/// without network dependencies.
#[derive(Debug)]
pub struct MemoryTransport {
    device_id: DeviceId,
    // Shared message queue for all memory transports
    messages: MessageQueue,
}

impl MemoryTransport {
    /// Create a new memory transport for the given device
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            messages: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a connected pair of memory transports
    ///
    /// Both transports share the same message queue so they can communicate
    pub fn create_pair() -> (Self, Self) {
        let shared_messages = Arc::new(RwLock::new(HashMap::new()));
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        let transport1 = Self {
            device_id: device1,
            messages: Arc::clone(&shared_messages),
        };

        let transport2 = Self {
            device_id: device2,
            messages: Arc::clone(&shared_messages),
        };

        (transport1, transport2)
    }

    /// Get the number of pending messages for this device
    pub async fn pending_message_count(&self) -> usize {
        let messages = self.messages.read().await;
        messages.get(&self.device_id).map_or(0, |queue| queue.len())
    }

    /// Clear all pending messages for this device
    pub async fn clear_messages(&self) {
        let mut messages = self.messages.write().await;
        messages.insert(self.device_id, vec![]);
    }
}

#[async_trait]
impl Transport for MemoryTransport {
    async fn send_message(&self, to: DeviceId, message: &[u8]) -> AuraResult<()> {
        let mut messages = self.messages.write().await;
        let queue = messages.entry(to).or_insert_with(Vec::new);
        queue.push((self.device_id, message.to_vec()));
        Ok(())
    }

    async fn receive_message(&self) -> AuraResult<Option<(DeviceId, Vec<u8>)>> {
        let mut messages = self.messages.write().await;
        let queue = messages.entry(self.device_id).or_insert_with(Vec::new);
        Ok(queue.pop())
    }

    fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Create a default memory transport for testing
///
/// Standard pattern for creating transport in tests.
pub fn test_memory_transport() -> MemoryTransport {
    MemoryTransport::new(DeviceId::new())
}

// Create a transport middleware stack for testing
//
// Creates a middleware stack suitable for testing scenarios
// Note: Disabled due to Week 11 cleanup - middleware was removed
// TODO fix - Re-implement when transport middleware is needed
/*
pub fn test_transport_stack(_device_id: DeviceId) -> TransportMiddlewareStack {
    use aura_core::AuraResult;
    use aura_transport::{NetworkAddress, TransportHandler, TransportOperation, TransportResult};

    /// Simple test transport handler
    struct TestTransportHandler;

    impl TransportHandler for TestTransportHandler {
        fn execute(&mut self, operation: TransportOperation) -> AuraResult<TransportResult> {
            // Simple test implementation - just echo back some data
            match operation {
                TransportOperation::Send {
                    destination, data, ..
                } => {
                    // For testing, just return success
                    Ok(TransportResult::Sent {
                        destination,
                        bytes_sent: data.len(),
                    })
                }
                TransportOperation::Receive { source, .. } => {
                    // For testing, return no data available
                    let source = source.unwrap_or(NetworkAddress::Memory("test".to_string()));
                    Ok(TransportResult::Received {
                        source,
                        data: vec![],
                        metadata: std::collections::HashMap::new(),
                    })
                }
                _ => {
                    // For other operations, return status
                    Ok(TransportResult::Status {
                        connections: vec![],
                    })
                }
            }
        }
    }

    TransportStackBuilder::new()
        // Add minimal middleware for testing
        .build(Box::new(TestTransportHandler))
}
*/

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_transport_creation() {
        let transport = test_memory_transport();
        // Basic smoke test - just verify we can create it
        assert!(!transport.device_id().0.is_nil());
    }

    #[tokio::test]
    async fn test_memory_transport_communication() {
        let (transport1, transport2) = MemoryTransport::create_pair();

        // Send message from transport1 to transport2
        let message = b"hello world";
        transport1
            .send_message(transport2.device_id(), message)
            .await
            .unwrap();

        // Receive message at transport2
        let received = transport2.receive_message().await.unwrap();
        assert!(received.is_some());

        let (sender, msg) = received.unwrap();
        assert_eq!(sender, transport1.device_id());
        assert_eq!(msg, message);
    }

    #[tokio::test]
    async fn test_message_queue() {
        let transport = test_memory_transport();

        assert_eq!(transport.pending_message_count().await, 0);

        // Send message to self
        transport
            .send_message(transport.device_id(), b"test")
            .await
            .unwrap();
        assert_eq!(transport.pending_message_count().await, 1);

        // Clear messages
        transport.clear_messages().await;
        assert_eq!(transport.pending_message_count().await, 0);
    }

    /* TODO fix - Re-enable when transport middleware is re-implemented
    #[test]
    fn test_transport_stack_creation() {
        let device_id = DeviceId::new();
        let _stack = test_transport_stack(device_id);
        // Basic smoke test - just verify we can create it
    }
    */
}
