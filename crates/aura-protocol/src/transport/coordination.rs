//! Transport Coordination - Effect Composition
//!
//! Layer 4: Local orchestration logic for transport effects.
//! NO choreography - direct effect composition only.
//! Target: <200 lines, minimal abstractions.

use super::{CoordinationResult, TransportCoordinationConfig, TransportCoordinationError};
use aura_core::effects::{NetworkEffects, StorageEffects, TimeEffects};
use aura_core::{ContextId, DeviceId};
use aura_effects::transport::{RetryingTransportManager, TransportManager};
use aura_transport::TransportConfig;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Local transport coordinator - NO choreography
/// Composes transport effects for single-device operations
#[derive(Debug)]
pub struct TransportCoordinator<E> {
    config: TransportCoordinationConfig,
    transport_manager: RetryingTransportManager,
    active_connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
    effects: E,
}

/// Local connection state tracking
#[derive(Debug, Clone)]
struct ConnectionState {
    device_id: DeviceId,
    context_id: ContextId,
    connection_info: aura_effects::transport::TransportConnection,
    last_activity: std::time::Instant,
    retry_count: u32,
}

impl<E> TransportCoordinator<E>
where
    E: NetworkEffects + StorageEffects + TimeEffects + Clone + Send + Sync,
{
    /// Create new transport coordinator
    pub fn new(config: TransportCoordinationConfig, effects: E) -> Self {
        let transport_config = TransportConfig {
            connect_timeout: config.connection_timeout,
            read_timeout: std::time::Duration::from_secs(30),
            write_timeout: std::time::Duration::from_secs(30),
            buffer_size: 64 * 1024,
        };

        let transport_manager = RetryingTransportManager::new(transport_config, config.max_retries);

        Self {
            config,
            transport_manager,
            active_connections: Arc::new(RwLock::new(HashMap::new())),
            effects,
        }
    }

    /// Establish connection to peer - NO choreography
    pub async fn connect_to_peer(
        &self,
        peer_id: DeviceId,
        address: &str,
        context_id: ContextId,
    ) -> CoordinationResult<String> {
        // Check connection limit
        {
            let connections = self.active_connections.read().await;
            if connections.len() >= self.config.max_connections {
                return Err(TransportCoordinationError::ProtocolFailed(
                    "Maximum connections exceeded".to_string(),
                ));
            }
        }

        // Attempt connection with retry logic
        let connection = self
            .transport_manager
            .connect_with_retry(address)
            .await
            .map_err(|e| TransportCoordinationError::Transport(e))?;

        // Store connection state
        let connection_state = ConnectionState {
            device_id: peer_id,
            context_id,
            connection_info: connection.clone(),
            last_activity: std::time::Instant::now(),
            retry_count: 0,
        };

        {
            let mut connections = self.active_connections.write().await;
            connections.insert(connection.connection_id.clone(), connection_state);
        }

        Ok(connection.connection_id)
    }

    /// Send data to connected peer - NO choreography
    pub async fn send_data(&self, connection_id: &str, data: Vec<u8>) -> CoordinationResult<()> {
        // Update activity timestamp
        {
            let mut connections = self.active_connections.write().await;
            if let Some(connection_state) = connections.get_mut(connection_id) {
                connection_state.last_activity = std::time::Instant::now();
            } else {
                return Err(TransportCoordinationError::ProtocolFailed(format!(
                    "Connection not found: {}",
                    connection_id
                )));
            }
        }

        // Send data using transport manager
        self.transport_manager
            .inner
            .send_data(connection_id, data)
            .await
            .map_err(|e| TransportCoordinationError::Transport(e))?;

        Ok(())
    }

    /// Disconnect from peer - NO choreography
    pub async fn disconnect_peer(&self, connection_id: &str) -> CoordinationResult<()> {
        // Remove from active connections
        {
            let mut connections = self.active_connections.write().await;
            connections.remove(connection_id);
        }

        // Clean up transport resources
        self.transport_manager
            .inner
            .disconnect(connection_id)
            .await
            .map_err(|e| TransportCoordinationError::Transport(e))?;

        Ok(())
    }

    /// Get connection information
    pub async fn get_connection_info(&self, connection_id: &str) -> Option<DeviceId> {
        let connections = self.active_connections.read().await;
        connections.get(connection_id).map(|state| state.device_id)
    }

    /// List all active connections
    pub async fn list_connections(&self) -> Vec<String> {
        let connections = self.active_connections.read().await;
        connections.keys().cloned().collect()
    }

    /// Clean up stale connections
    pub async fn cleanup_stale_connections(
        &self,
        max_idle: std::time::Duration,
    ) -> CoordinationResult<usize> {
        let now = std::time::Instant::now();
        let mut to_remove = Vec::new();

        // Find stale connections
        {
            let connections = self.active_connections.read().await;
            for (connection_id, state) in connections.iter() {
                if now.duration_since(state.last_activity) > max_idle {
                    to_remove.push(connection_id.clone());
                }
            }
        }

        // Remove stale connections
        let mut removed_count = 0;
        for connection_id in to_remove {
            if self.disconnect_peer(&connection_id).await.is_ok() {
                removed_count += 1;
            }
        }

        Ok(removed_count)
    }

    /// Get coordination statistics
    pub async fn get_stats(&self) -> CoordinationStats {
        let connections = self.active_connections.read().await;

        let mut connection_count_by_context = HashMap::new();
        let mut oldest_connection = None;
        let mut newest_connection = None;

        for state in connections.values() {
            *connection_count_by_context
                .entry(state.context_id.clone())
                .or_insert(0) += 1;

            if oldest_connection.is_none() || state.last_activity < oldest_connection.unwrap() {
                oldest_connection = Some(state.last_activity);
            }

            if newest_connection.is_none() || state.last_activity > newest_connection.unwrap() {
                newest_connection = Some(state.last_activity);
            }
        }

        CoordinationStats {
            total_connections: connections.len(),
            connection_count_by_context,
            oldest_connection_age: oldest_connection.map(|t| t.elapsed()),
            newest_connection_age: newest_connection.map(|t| t.elapsed()),
        }
    }
}

/// Coordination statistics
#[derive(Debug, Clone)]
pub struct CoordinationStats {
    pub total_connections: usize,
    pub connection_count_by_context: HashMap<ContextId, usize>,
    pub oldest_connection_age: Option<std::time::Duration>,
    pub newest_connection_age: Option<std::time::Duration>,
}
