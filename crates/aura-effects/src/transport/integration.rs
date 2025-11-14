//! Transport Integration Examples
//!
//! Shows how transport handlers integrate with mature libraries.
//! Target: <200 lines, demonstrate Layer 3 patterns.

use super::{
    tcp::TcpTransportHandler,
    websocket::WebSocketTransportHandler,
    memory::InMemoryTransportHandler,
    framing::FramingHandler,
    utils::{AddressResolver, TimeoutHelper, BufferUtils, ConnectionMetrics},
    TransportConfig, TransportError, TransportResult,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use url::Url;

/// Unified transport manager for all transport types
#[derive(Debug, Clone)]
pub struct TransportManager {
    config: TransportConfig,
    tcp_handler: TcpTransportHandler,
    ws_handler: WebSocketTransportHandler,
    memory_handler: InMemoryTransportHandler,
    framing_handler: FramingHandler,
    metrics: Arc<RwLock<HashMap<String, ConnectionMetrics>>>,
}

impl TransportManager {
    /// Create new transport manager
    pub fn new(config: TransportConfig) -> Self {
        let tcp_handler = TcpTransportHandler::new(config.clone());
        let ws_handler = WebSocketTransportHandler::new(config.clone());
        let memory_handler = InMemoryTransportHandler::new(config.clone());
        let framing_handler = FramingHandler::new(config.buffer_size);
        
        Self {
            config,
            tcp_handler,
            ws_handler,
            memory_handler,
            framing_handler,
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Connect using URL scheme detection
    pub async fn connect_by_url(&self, url: &str) -> TransportResult<TransportConnection> {
        let parsed_url = Url::parse(url)
            .map_err(|e| TransportError::Protocol(format!("Invalid URL: {}", e)))?;
        
        match parsed_url.scheme() {
            "ws" | "wss" => {
                let (ws_stream, connection) = self.ws_handler.connect(parsed_url).await?;
                // Store metrics
                let mut metrics_map = self.metrics.write().await;
                metrics_map.insert(connection.connection_id.clone(), ConnectionMetrics::new());
                Ok(connection)
            },
            "tcp" => {
                // Convert URL to socket address
                let addresses = AddressResolver::resolve(
                    parsed_url.host_str().unwrap_or("localhost"),
                    parsed_url.port().unwrap_or(8080)
                ).await?;
                
                let connection = self.tcp_handler.connect(addresses[0]).await?;
                // Store metrics
                let mut metrics_map = self.metrics.write().await;
                metrics_map.insert(connection.connection_id.clone(), ConnectionMetrics::new());
                Ok(connection)
            },
            "memory" => {
                let peer_id = parsed_url.host_str().unwrap_or("default");
                let _receiver = self.memory_handler.register_peer(peer_id).await?;
                
                // Create mock connection info for memory transport
                let connection = super::TransportConnection {
                    connection_id: format!("memory-{}", peer_id),
                    local_addr: "memory://local".to_string(),
                    remote_addr: format!("memory://{}", peer_id),
                    metadata: {
                        let mut meta = HashMap::new();
                        meta.insert("protocol".to_string(), "memory".to_string());
                        meta.insert("peer_id".to_string(), peer_id.to_string());
                        meta
                    },
                };
                
                // Store metrics
                let mut metrics_map = self.metrics.write().await;
                metrics_map.insert(connection.connection_id.clone(), ConnectionMetrics::new());
                Ok(connection)
            },
            other => Err(TransportError::Protocol(format!("Unsupported scheme: {}", other)))
        }
    }
    
    /// Send data with automatic protocol selection
    pub async fn send_data(&self, connection_id: &str, data: Vec<u8>) -> TransportResult<()> {
        // Update metrics
        {
            let mut metrics_map = self.metrics.write().await;
            if let Some(metrics) = metrics_map.get_mut(connection_id) {
                metrics.record_sent(data.len() as u64);
            }
        }
        
        if connection_id.starts_with("memory-") {
            let peer_id = connection_id.strip_prefix("memory-").unwrap();
            self.memory_handler.send_to_peer(peer_id, data).await
        } else if connection_id.starts_with("ws-") {
            // For WebSocket, would need active connection management
            Err(TransportError::Protocol("WebSocket send requires active connection".to_string()))
        } else if connection_id.starts_with("tcp-") {
            // For TCP, would need active connection management  
            Err(TransportError::Protocol("TCP send requires active connection".to_string()))
        } else {
            Err(TransportError::Protocol(format!("Unknown connection type: {}", connection_id)))
        }
    }
    
    /// Get connection metrics
    pub async fn get_metrics(&self, connection_id: &str) -> Option<ConnectionMetrics> {
        let metrics_map = self.metrics.read().await;
        metrics_map.get(connection_id).cloned()
    }
    
    /// List all active connections
    pub async fn list_connections(&self) -> Vec<String> {
        let metrics_map = self.metrics.read().await;
        metrics_map.keys().cloned().collect()
    }
    
    /// Clean up connection
    pub async fn disconnect(&self, connection_id: &str) -> TransportResult<()> {
        // Remove metrics
        {
            let mut metrics_map = self.metrics.write().await;
            metrics_map.remove(connection_id);
        }
        
        if connection_id.starts_with("memory-") {
            let peer_id = connection_id.strip_prefix("memory-").unwrap();
            self.memory_handler.unregister_peer(peer_id).await
        } else {
            // For TCP/WebSocket, would close active connections
            Ok(())
        }
    }
}

/// Transport connection reference for tracking
pub use super::TransportConnection;

/// Demonstration of retry logic with exponential backoff
pub struct RetryingTransportManager {
    inner: TransportManager,
    max_retries: u32,
}

impl RetryingTransportManager {
    pub fn new(config: TransportConfig, max_retries: u32) -> Self {
        Self {
            inner: TransportManager::new(config),
            max_retries,
        }
    }
    
    /// Connect with retry logic
    pub async fn connect_with_retry(&self, url: &str) -> TransportResult<TransportConnection> {
        let mut attempt = 0;
        let mut last_error = None;
        
        while attempt < self.max_retries {
            match self.inner.connect_by_url(url).await {
                Ok(connection) => return Ok(connection),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.max_retries - 1 {
                        let delay = TimeoutHelper::exponential_backoff(
                            attempt,
                            std::time::Duration::from_millis(100),
                            std::time::Duration::from_secs(5),
                        );
                        
                        let jittered_delay = TimeoutHelper::add_jitter(delay, 25);
                        tokio::time::sleep(jittered_delay).await;
                    }
                    attempt += 1;
                }
            }
        }
        
        Err(last_error.unwrap_or(TransportError::ConnectionFailed(
            "Max retries exceeded".to_string()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_memory_transport_integration() {
        let manager = TransportManager::new(TransportConfig::default());
        
        // Connect to memory transport
        let connection = manager.connect_by_url("memory://test-peer").await.unwrap();
        assert!(connection.connection_id.starts_with("memory-"));
        
        // Send data
        manager.send_data(&connection.connection_id, vec![1, 2, 3, 4]).await.unwrap();
        
        // Check metrics
        let metrics = manager.get_metrics(&connection.connection_id).await.unwrap();
        assert_eq!(metrics.messages_sent, 1);
        assert_eq!(metrics.bytes_sent, 4);
        
        // Cleanup
        manager.disconnect(&connection.connection_id).await.unwrap();
    }
}
