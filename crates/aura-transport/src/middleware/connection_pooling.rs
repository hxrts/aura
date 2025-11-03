//! Connection Pooling Middleware

use super::stack::TransportMiddleware;
use super::handler::{TransportHandler, TransportOperation, TransportResult, NetworkAddress, ConnectionInfo, ConnectionState};
use aura_types::effects::AuraEffects;
use aura_types::{MiddlewareContext, MiddlewareResult};
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_connections_per_host: u32,
    pub idle_timeout_ms: u64,
    pub connection_timeout_ms: u64,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections_per_host: 10,
            idle_timeout_ms: 30000, // 30 seconds
            connection_timeout_ms: 5000, // 5 seconds
        }
    }
}

pub struct ConnectionPoolingMiddleware {
    config: PoolConfig,
    pools: HashMap<NetworkAddress, VecDeque<String>>, // Available connection IDs
    active_connections: HashMap<String, ConnectionInfo>,
}

impl ConnectionPoolingMiddleware {
    pub fn new() -> Self {
        Self {
            config: PoolConfig::default(),
            pools: HashMap::new(),
            active_connections: HashMap::new(),
        }
    }
    
    pub fn with_config(config: PoolConfig) -> Self {
        Self {
            config,
            pools: HashMap::new(),
            active_connections: HashMap::new(),
        }
    }
}

impl Default for ConnectionPoolingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

impl TransportMiddleware for ConnectionPoolingMiddleware {
    fn process(
        &mut self,
        operation: TransportOperation,
        _context: &MiddlewareContext,
        effects: &dyn AuraEffects,
        next: &mut dyn TransportHandler,
    ) -> MiddlewareResult<TransportResult> {
        match operation {
            TransportOperation::Connect { address, options } => {
                // Check if we have an available connection in the pool
                if let Some(pool) = self.pools.get_mut(&address) {
                    if let Some(connection_id) = pool.pop_front() {
                        if let Some(_conn_info) = self.active_connections.get(&connection_id) {
                            // Reuse existing connection
                            return Ok(TransportResult::Connected {
                                address,
                                connection_id,
                            });
                        }
                    }
                }
                
                // No available connection, create new one
                let result = next.execute(TransportOperation::Connect { address: address.clone(), options }, effects)?;
                
                if let TransportResult::Connected { connection_id, .. } = &result {
                    // Track the new connection
                    let conn_info = ConnectionInfo {
                        address: address.clone(),
                        connection_id: connection_id.clone(),
                        state: ConnectionState::Connected,
                        bytes_sent: 0,
                        bytes_received: 0,
                        created_at: effects.current_timestamp(),
                        last_activity: effects.current_timestamp(),
                    };
                    self.active_connections.insert(connection_id.clone(), conn_info);
                }
                
                Ok(result)
            }
            
            TransportOperation::Disconnect { address } => {
                // Instead of actually disconnecting, return connection to pool
                if let Some(pool) = self.pools.get_mut(&address) {
                    if pool.len() < self.config.max_connections_per_host as usize {
                        // Find connection for this address
                        for (conn_id, conn_info) in &self.active_connections {
                            if conn_info.address == address {
                                pool.push_back(conn_id.clone());
                                break;
                            }
                        }
                        return Ok(TransportResult::Disconnected { address });
                    }
                }
                
                // Pool is full, actually disconnect
                next.execute(TransportOperation::Disconnect { address }, effects)
            }
            
            _ => next.execute(operation, effects),
        }
    }
    
    fn middleware_name(&self) -> &'static str {
        "ConnectionPoolingMiddleware"
    }
    
    fn middleware_info(&self) -> HashMap<String, String> {
        let mut info = HashMap::new();
        info.insert("max_connections_per_host".to_string(), self.config.max_connections_per_host.to_string());
        info.insert("idle_timeout_ms".to_string(), self.config.idle_timeout_ms.to_string());
        info.insert("active_pools".to_string(), self.pools.len().to_string());
        info.insert("total_pooled_connections".to_string(), 
                   self.pools.values().map(|pool| pool.len()).sum::<usize>().to_string());
        info
    }
}