//! Transport effect interface for network operations

use crate::{AuraError, DeviceId};
use async_trait::async_trait;

/// Pure trait for transport/network operations
#[async_trait]
pub trait TransportEffects {
    /// Send message to a specific device
    async fn send(&self, target: DeviceId, message: Vec<u8>) -> Result<(), AuraError>;

    /// Receive next message from any device
    async fn recv(&self) -> Result<(DeviceId, Vec<u8>), AuraError>;

    /// Connect to network/relay
    async fn connect(&self) -> Result<(), AuraError>;

    /// Disconnect from network
    async fn disconnect(&self) -> Result<(), AuraError>;
}
