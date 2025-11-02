//! Transport factory for creating unified transport implementations

use crate::{
    core::{
        implementations::{
            HttpsRelayTransport, MemoryTransport, SimulationTransport, TcpTransport,
        },
        traits::Transport,
    },
    TransportConfig, TransportResult, TransportType,
};
use std::sync::Arc;

/// Transport factory for creating configured transport instances
pub struct TransportFactory;

impl TransportFactory {
    /// Create a transport from configuration
    pub fn create_transport(_config: &TransportConfig) -> TransportResult<Arc<dyn Transport>> {
        match &_config.transport_type {
            TransportType::Memory => {
                let transport = MemoryTransport::new(_config.device_id);
                Ok(Arc::new(transport))
            }
            TransportType::Tcp { address, port } => {
                let transport = TcpTransport::new(_config.device_id, address.clone(), *port);
                Ok(Arc::new(transport))
            }
            TransportType::HttpsRelay { url } => {
                let transport = HttpsRelayTransport::new(_config.device_id, url.clone());
                Ok(Arc::new(transport))
            }
            TransportType::Simulation => {
                let transport = SimulationTransport::new(_config.device_id);
                Ok(Arc::new(transport))
            }
        }
    }
}
