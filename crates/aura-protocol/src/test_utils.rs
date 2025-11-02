//! Test utilities for protocol testing
//!
//! This module provides test-specific implementations and utilities
//! that should not be used in production code.

use crate::context::SimpleTransport;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Simple in-memory transport for testing choreographic protocols
#[derive(Debug, Default, Clone)]
pub struct MemoryTransport {
    #[allow(clippy::type_complexity)]
    messages: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl MemoryTransport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_messages(&self) -> Vec<(String, Vec<u8>)> {
        self.messages.lock().unwrap().clone()
    }

    pub fn clear_messages(&self) {
        self.messages.lock().unwrap().clear()
    }
}

#[async_trait]
impl SimpleTransport for MemoryTransport {
    async fn send_message(&self, peer_id: &str, message: &[u8]) -> Result<(), String> {
        self.messages
            .lock()
            .unwrap()
            .push((peer_id.to_string(), message.to_vec()));
        Ok(())
    }

    async fn broadcast_message(&self, message: &[u8]) -> Result<(), String> {
        self.messages
            .lock()
            .unwrap()
            .push(("broadcast".to_string(), message.to_vec()));
        Ok(())
    }

    async fn is_peer_reachable(&self, _peer_id: &str) -> bool {
        true // Always reachable in memory transport
    }
}

/// Generate a deterministic test UUID for non-production use
pub fn generate_test_uuid() -> uuid::Uuid {
    // Use UUID v4 with a fixed seed for deterministic tests
    uuid::Uuid::from_bytes([
        0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd,
        0xef,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_transport() {
        let transport = MemoryTransport::new();

        // Test send
        transport.send_message("peer1", b"hello").await.unwrap();
        let messages = transport.get_messages();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, "peer1");
        assert_eq!(messages[0].1, b"hello");

        // Test broadcast
        transport.broadcast_message(b"world").await.unwrap();
        let messages = transport.get_messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].0, "broadcast");
        assert_eq!(messages[1].1, b"world");

        // Test clear
        transport.clear_messages();
        assert!(transport.get_messages().is_empty());

        // Test reachability
        assert!(transport.is_peer_reachable("anyone").await);
    }
}
