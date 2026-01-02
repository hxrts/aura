//! Transport Coordinator - Multi-Party Connection Management
//!
//! **Layer 4 (aura-protocol)**: Stateful multi-party coordination handler.
//!
//! This module was moved from aura-effects (Layer 3) because it violates the Layer 3 principle
//! of "stateless, single-party, context-free" handlers. The TransportCoordinator maintains
//! shared state across multiple peer connections, making it multi-party coordination logic
//! that belongs in the orchestration layer.
//!
//! Key violations that required the move:
//! - Maintains global connection registry (`Arc<RwLock<HashMap<String, ConnectionState>>>`)
//! - Manages connections to multiple peers (multi-party, not single-party)
//! - Coordinates connection lifecycle across the system
//! - Enforces global connection limits and cleanup policies

use aura_core::effects::{NetworkEffects, NetworkError, PhysicalTimeEffects, StorageEffects};
use aura_core::{identifiers::DeviceId, ContextId};
use aura_effects::transport::{TransportConfig, TransportError};
use std::collections::HashMap;

/// Transport coordination configuration
#[derive(Debug, Clone)]
pub struct TransportCoordinationConfig {
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Connection timeout
    pub connection_timeout: std::time::Duration,
    /// Retry attempts for transport operations
    pub max_retries: u32,
    /// Default capability requirements
    pub default_capabilities: Vec<String>,
}

impl Default for TransportCoordinationConfig {
    fn default() -> Self {
        Self {
            max_connections: 100,
            connection_timeout: std::time::Duration::from_secs(30),
            max_retries: 3,
            default_capabilities: vec!["transport_basic".to_string()],
        }
    }
}

/// Transport coordination error types
#[derive(Debug, thiserror::Error)]
pub enum TransportCoordinationError {
    /// Protocol execution failed with error message
    #[error("Protocol execution failed: {0}")]
    ProtocolFailed(String),
    /// Capability check failed with error message
    #[error("Capability check failed: {0}")]
    CapabilityCheckFailed(String),
    /// Flow budget exceeded with error message
    #[error("Flow budget exceeded: {0}")]
    FlowBudgetExceeded(String),
    /// Transport layer error
    #[error("Transport error: {0}")]
    Transport(#[from] TransportError),
    /// Effect system error
    #[error("Effect error: {0}")]
    Effect(String),
}

impl aura_core::ProtocolErrorCode for TransportCoordinationError {
    fn code(&self) -> &'static str {
        match self {
            TransportCoordinationError::ProtocolFailed(_) => "transport_protocol_failed",
            TransportCoordinationError::CapabilityCheckFailed(_) => "transport_capability_check",
            TransportCoordinationError::FlowBudgetExceeded(_) => "transport_flow_budget_exceeded",
            TransportCoordinationError::Transport(_) => "transport_layer_error",
            TransportCoordinationError::Effect(_) => "transport_effect_error",
        }
    }
}

/// Result type for transport coordination operations
pub type CoordinationResult<T> = Result<T, TransportCoordinationError>;

/// Simple transport manager for Layer 4 coordination
#[derive(Debug, Clone)]
pub struct RetryingTransportManager {
    config: TransportConfig,
    max_retries: u32,
}

impl RetryingTransportManager {
    /// Create new retrying transport manager
    pub fn new(config: TransportConfig, max_retries: u32) -> Self {
        Self {
            config,
            max_retries,
        }
    }

    async fn connect_with_retry(
        &self,
        network: &(impl NetworkEffects + ?Sized),
        address: &str,
    ) -> Result<ConnectionInfo, TransportCoordinationError> {
        let mut last_error: Option<NetworkError> = None;

        for attempt in 1..=self.max_retries {
            match network.open(address).await {
                Ok(connection_id) => return Ok(ConnectionInfo { connection_id }),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < self.max_retries {
                        continue;
                    }
                }
            }
        }

        let final_error = last_error
            .map(|e| TransportCoordinationError::Effect(format!("Network open failed: {e}")))
            .unwrap_or_else(|| {
                TransportCoordinationError::Transport(TransportError::ConnectionFailed(
                    "All retry attempts exhausted".to_string(),
                ))
            });

        Err(final_error)
    }

    async fn send_data(
        &self,
        network: &(impl NetworkEffects + ?Sized),
        connection_id: &str,
        data: Vec<u8>,
    ) -> Result<(), NetworkError> {
        network.send(connection_id, data).await
    }

    async fn disconnect(
        &self,
        network: &(impl NetworkEffects + ?Sized),
        connection_id: &str,
    ) -> Result<(), NetworkError> {
        network.close(connection_id).await
    }
}

/// Connection information from transport
#[derive(Debug, Clone)]
struct ConnectionInfo {
    connection_id: String,
}

use async_lock::RwLock;
use std::sync::Arc;

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
    connection_id: String,
    last_activity_ms: u64, // Timestamp in milliseconds from PhysicalTimeEffects
    retry_count: u32,
}

impl<E> TransportCoordinator<E>
where
    E: NetworkEffects + StorageEffects + PhysicalTimeEffects + Clone + Send + Sync,
{
    /// Create new transport coordinator
    pub fn new(config: TransportCoordinationConfig, effects: E) -> Self {
        let transport_config = TransportConfig {
            connect_timeout: config.connection_timeout,
            read_timeout: std::time::Duration::from_secs(60),
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
            .connect_with_retry(&self.effects, address)
            .await?;

        // Store connection state using injected time effects
        let current_time =
            self.effects.physical_time().await.map_err(|e| {
                TransportCoordinationError::Effect(format!("Failed to get time: {e}"))
            })?;
        let now_ms = current_time.ts_ms;

        let connection_state = ConnectionState {
            device_id: peer_id,
            context_id,
            connection_id: connection.connection_id.clone(),
            last_activity_ms: now_ms,
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
        let current_time =
            self.effects.physical_time().await.map_err(|e| {
                TransportCoordinationError::Effect(format!("Failed to get time: {e}"))
            })?;
        let now_ms = current_time.ts_ms;

        {
            let mut connections = self.active_connections.write().await;
            if let Some(connection_state) = connections.get_mut(connection_id) {
                connection_state.last_activity_ms = now_ms;
            } else {
                return Err(TransportCoordinationError::ProtocolFailed(format!(
                    "Connection not found: {connection_id}"
                )));
            }
        }

        // Send data using transport manager
        self.transport_manager
            .send_data(&self.effects, connection_id, data)
            .await
            .map_err(|e| TransportCoordinationError::ProtocolFailed(format!("Send failed: {e}")))?;

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
            .disconnect(&self.effects, connection_id)
            .await
            .map_err(|e| {
                TransportCoordinationError::ProtocolFailed(format!("Disconnect failed: {e}"))
            })?;

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
        let current_time =
            self.effects.physical_time().await.map_err(|e| {
                TransportCoordinationError::Effect(format!("Failed to get time: {e}"))
            })?;
        let now_ms = current_time.ts_ms;
        let max_idle_ms = max_idle.as_millis() as u64;
        let mut to_remove = Vec::new();

        // Find stale connections
        {
            let connections = self.active_connections.read().await;
            for (connection_id, state) in connections.iter() {
                if now_ms.saturating_sub(state.last_activity_ms) > max_idle_ms {
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

        // Get current time for calculating ages
        let current_time_ms = self.effects.physical_time().await.ok().map(|t| t.ts_ms);

        let mut connection_count_by_context = HashMap::new();
        let mut oldest_connection_ms = None;
        let mut newest_connection_ms = None;

        for state in connections.values() {
            *connection_count_by_context
                .entry(state.context_id)
                .or_insert(0) += 1;

            if oldest_connection_ms.is_none()
                || state.last_activity_ms < oldest_connection_ms.unwrap()
            {
                oldest_connection_ms = Some(state.last_activity_ms);
            }

            if newest_connection_ms.is_none()
                || state.last_activity_ms > newest_connection_ms.unwrap()
            {
                newest_connection_ms = Some(state.last_activity_ms);
            }
        }

        let oldest_connection_age = current_time_ms.and_then(|now_ms| {
            oldest_connection_ms
                .map(|oldest_ms| std::time::Duration::from_millis(now_ms.saturating_sub(oldest_ms)))
        });

        let newest_connection_age = current_time_ms.and_then(|now_ms| {
            newest_connection_ms
                .map(|newest_ms| std::time::Duration::from_millis(now_ms.saturating_sub(newest_ms)))
        });

        CoordinationStats {
            total_connections: connections.len(),
            connection_count_by_context,
            oldest_connection_age,
            newest_connection_age,
        }
    }
}

/// Coordination statistics
#[derive(Debug, Clone)]
pub struct CoordinationStats {
    /// Total number of active connections
    pub total_connections: usize,
    /// Number of connections grouped by context
    pub connection_count_by_context: HashMap<ContextId, usize>,
    /// Age of the oldest connection if any
    pub oldest_connection_age: Option<std::time::Duration>,
    /// Age of the newest connection if any
    pub newest_connection_age: Option<std::time::Duration>,
}
