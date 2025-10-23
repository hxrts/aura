// Stub transport implementation for testing
//
// Reference: 080_architecture_protocol_integration.md - Part 5: Transport Abstraction Design
//
// This is a minimal in-memory transport for unit tests and protocol development.
// It simulates connections and message passing without actual network I/O.
// For production use, implement a real transport (e.g., Noise + HTTPS).

use crate::{BroadcastResult, Connection, PresenceTicket, Transport, TransportError, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// In-memory message queue for a connection
#[derive(Debug, Clone)]
struct MessageQueue {
    messages: Vec<Vec<u8>>,
}

impl MessageQueue {
    fn new() -> Self {
        MessageQueue {
            messages: Vec::new(),
        }
    }
    
    fn push(&mut self, message: Vec<u8>) {
        self.messages.push(message);
    }
    
    fn pop(&mut self) -> Option<Vec<u8>> {
        if self.messages.is_empty() {
            None
        } else {
            Some(self.messages.remove(0))
        }
    }
}

/// Stub transport implementation - in-memory only
///
/// This transport maintains in-memory message queues between peers.
/// It's useful for:
/// - Unit testing protocols without network I/O
/// - Development and debugging
/// - CI/CD environments
///
/// NOT suitable for production use.
#[derive(Clone)]
pub struct StubTransport {
    /// Message queues indexed by connection ID
    queues: Arc<Mutex<HashMap<String, MessageQueue>>>,
    
    /// Active connections
    connections: Arc<Mutex<HashMap<String, Connection>>>,
}

impl StubTransport {
    /// Create a new stub transport
    pub fn new() -> Self {
        StubTransport {
            queues: Arc::new(Mutex::new(HashMap::new())),
            connections: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for StubTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Transport for StubTransport {
    async fn connect(
        &self,
        peer_id: &str,
        _my_ticket: &PresenceTicket,
        _peer_ticket: &PresenceTicket,
    ) -> Result<Connection> {
        // In stub transport, we don't actually verify tickets
        // Real implementation would:
        // 1. Perform handshake
        // 2. Verify both tickets
        // 3. Establish encrypted channel
        
        let conn = crate::ConnectionBuilder::new(peer_id).build();
        
        // Register connection
        {
            let mut connections = self.connections.lock().unwrap();
            connections.insert(conn.id().to_string(), conn.clone());
        }
        
        // Create message queue
        {
            let mut queues = self.queues.lock().unwrap();
            queues.insert(conn.id().to_string(), MessageQueue::new());
        }
        
        Ok(conn)
    }
    
    async fn send(&self, conn: &Connection, message: &[u8]) -> Result<()> {
        let mut queues = self.queues.lock().unwrap();
        
        let queue = queues.get_mut(conn.id())
            .ok_or_else(|| TransportError::ConnectionFailed(
                format!("Connection {} not found", conn.id())
            ))?;
        
        queue.push(message.to_vec());
        
        Ok(())
    }
    
    async fn receive(
        &self,
        conn: &Connection,
        _timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        // In stub transport, we ignore timeout and return immediately
        // Real implementation would block up to timeout
        
        let mut queues = self.queues.lock().unwrap();
        
        let queue = queues.get_mut(conn.id())
            .ok_or_else(|| TransportError::ConnectionFailed(
                format!("Connection {} not found", conn.id())
            ))?;
        
        Ok(queue.pop())
    }
    
    async fn broadcast(
        &self,
        connections: &[Connection],
        message: &[u8],
    ) -> Result<BroadcastResult> {
        let mut succeeded = Vec::new();
        let mut failed = Vec::new();
        
        for conn in connections {
            match self.send(conn, message).await {
                Ok(_) => succeeded.push(conn.peer_id().to_string()),
                Err(_) => failed.push(conn.peer_id().to_string()),
            }
        }
        
        Ok(BroadcastResult { succeeded, failed })
    }
    
    async fn disconnect(&self, conn: &Connection) -> Result<()> {
        {
            let mut connections = self.connections.lock().unwrap();
            connections.remove(conn.id());
        }
        
        {
            let mut queues = self.queues.lock().unwrap();
            queues.remove(conn.id());
        }
        
        Ok(())
    }
    
    async fn is_connected(&self, conn: &Connection) -> bool {
        let connections = self.connections.lock().unwrap();
        connections.contains_key(conn.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    
    fn create_dummy_ticket() -> PresenceTicket {
        PresenceTicket::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            1,
            3600,
        ).unwrap()
    }
    
    #[tokio::test]
    async fn test_connect() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport
            .connect("peer1", &ticket, &ticket)
            .await
            .unwrap();
        
        assert_eq!(conn.peer_id(), "peer1");
        assert!(transport.is_connected(&conn).await);
    }
    
    #[tokio::test]
    async fn test_send_receive() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport
            .connect("peer1", &ticket, &ticket)
            .await
            .unwrap();
        
        // Send a message
        let message = b"hello world";
        transport.send(&conn, message).await.unwrap();
        
        // Receive the message
        let received = transport
            .receive(&conn, Duration::from_secs(1))
            .await
            .unwrap();
        
        assert_eq!(received, Some(message.to_vec()));
    }
    
    #[tokio::test]
    async fn test_receive_empty_queue() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport
            .connect("peer1", &ticket, &ticket)
            .await
            .unwrap();
        
        // Receive from empty queue
        let received = transport
            .receive(&conn, Duration::from_secs(1))
            .await
            .unwrap();
        
        assert_eq!(received, None);
    }
    
    #[tokio::test]
    async fn test_multiple_messages() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport
            .connect("peer1", &ticket, &ticket)
            .await
            .unwrap();
        
        // Send multiple messages
        transport.send(&conn, b"msg1").await.unwrap();
        transport.send(&conn, b"msg2").await.unwrap();
        transport.send(&conn, b"msg3").await.unwrap();
        
        // Receive in order
        let msg1 = transport.receive(&conn, Duration::from_secs(1)).await.unwrap();
        let msg2 = transport.receive(&conn, Duration::from_secs(1)).await.unwrap();
        let msg3 = transport.receive(&conn, Duration::from_secs(1)).await.unwrap();
        
        assert_eq!(msg1, Some(b"msg1".to_vec()));
        assert_eq!(msg2, Some(b"msg2".to_vec()));
        assert_eq!(msg3, Some(b"msg3".to_vec()));
    }
    
    #[tokio::test]
    async fn test_broadcast() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn1 = transport.connect("peer1", &ticket, &ticket).await.unwrap();
        let conn2 = transport.connect("peer2", &ticket, &ticket).await.unwrap();
        let conn3 = transport.connect("peer3", &ticket, &ticket).await.unwrap();
        
        // Broadcast a message
        let message = b"broadcast message";
        let result = transport
            .broadcast(&[conn1.clone(), conn2.clone(), conn3.clone()], message)
            .await
            .unwrap();
        
        assert_eq!(result.succeeded.len(), 3);
        assert_eq!(result.failed.len(), 0);
        
        // All peers should have received the message
        let msg1 = transport.receive(&conn1, Duration::from_secs(1)).await.unwrap();
        let msg2 = transport.receive(&conn2, Duration::from_secs(1)).await.unwrap();
        let msg3 = transport.receive(&conn3, Duration::from_secs(1)).await.unwrap();
        
        assert_eq!(msg1, Some(message.to_vec()));
        assert_eq!(msg2, Some(message.to_vec()));
        assert_eq!(msg3, Some(message.to_vec()));
    }
    
    #[tokio::test]
    async fn test_disconnect() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport.connect("peer1", &ticket, &ticket).await.unwrap();
        
        assert!(transport.is_connected(&conn).await);
        
        transport.disconnect(&conn).await.unwrap();
        
        assert!(!transport.is_connected(&conn).await);
    }
    
    #[tokio::test]
    async fn test_send_after_disconnect() {
        let transport = StubTransport::new();
        let ticket = create_dummy_ticket();
        
        let conn = transport.connect("peer1", &ticket, &ticket).await.unwrap();
        
        transport.disconnect(&conn).await.unwrap();
        
        // Send should fail after disconnect
        let result = transport.send(&conn, b"message").await;
        assert!(result.is_err());
    }
}

