//! Handler Adapters for Protocol-Transport Bridge
//!
//! This module provides adapters and re-exports for transport handlers,
//! maintaining clean separation between protocol logic and transport implementation.
//!
//! ## Architecture
//!
//! The handler system follows a clean separation:
//! - Protocol crate (this crate) defines the `AuraProtocolHandler` trait
//! - Transport crate owns concrete handler implementations
//! - This module provides adapters and convenient re-exports
//!
//! This design ensures protocol logic remains transport-agnostic while
//! allowing both Aura and Rumpsteak protocols to share transport implementations.

use crate::middleware::handler::AuraProtocolHandler;
use std::marker::PhantomData;

/// Re-export transport handlers when the transport crate is available
#[cfg(feature = "transport")]
pub use aura_transport::handlers::{InMemoryHandler, NetworkHandler};

#[cfg(all(feature = "transport", feature = "simulation"))]
pub use aura_transport::handlers::SimulationHandler;

/// Builder pattern for creating protocol handlers with middleware
pub struct HandlerBuilder<H> {
    _phantom: PhantomData<H>,
}

impl<H> Default for HandlerBuilder<H>
where
    H: AuraProtocolHandler,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<H> HandlerBuilder<H>
where
    H: AuraProtocolHandler,
{
    /// Create a new handler builder
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }

    /// Build an in-memory handler for testing
    #[cfg(feature = "transport")]
    pub fn in_memory(
        device_id: H::DeviceId,
    ) -> InMemoryHandler<H::DeviceId, H::SessionId, H::Message> {
        InMemoryHandler::new(device_id)
    }

    /// Build a network handler for production
    #[cfg(feature = "transport")]
    pub fn network(
        device_id: H::DeviceId,
        transport: impl Into<String>,
    ) -> NetworkHandler<H::DeviceId, H::SessionId, H::Message> {
        NetworkHandler::new(device_id, transport)
    }

    /// Build a simulation handler for testing
    #[cfg(all(feature = "transport", feature = "simulation"))]
    pub fn simulation(
        device_id: H::DeviceId,
    ) -> SimulationHandler<H::DeviceId, H::SessionId, H::Message> {
        SimulationHandler::new(device_id)
    }
}

/// Type alias for protocol handlers with standard Aura types
pub type StandardHandler = dyn AuraProtocolHandler<
    DeviceId = aura_types::DeviceId,
    SessionId = uuid::Uuid,
    Message = Vec<u8>,
>;

/// Factory for creating handlers with standard Aura types
pub struct StandardHandlerFactory;

impl StandardHandlerFactory {
    /// Create an in-memory handler for testing
    #[cfg(feature = "transport")]
    pub fn in_memory(
        device_id: aura_types::DeviceId,
    ) -> InMemoryHandler<aura_types::DeviceId, uuid::Uuid, Vec<u8>> {
        InMemoryHandler::new(device_id)
    }

    /// Create a network handler for production
    #[cfg(feature = "transport")]
    pub fn network(
        device_id: aura_types::DeviceId,
        transport_url: &str,
    ) -> NetworkHandler<aura_types::DeviceId, uuid::Uuid, Vec<u8>> {
        NetworkHandler::new(device_id, transport_url)
    }
}

/// Extension methods for handler adapters
pub trait HandlerAdapterExt: AuraProtocolHandler {
    /// Convert this handler to a boxed dynamic handler
    fn boxed(
        self,
    ) -> Box<
        dyn AuraProtocolHandler<
            DeviceId = Self::DeviceId,
            SessionId = Self::SessionId,
            Message = Self::Message,
        >,
    >
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

impl<T: AuraProtocolHandler> HandlerAdapterExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(feature = "transport")]
    fn test_handler_factory() {
        use aura_types::DeviceId;
        use uuid::Uuid;

        let device_id = DeviceId::from(Uuid::new_v4());

        // Test creating handlers via factory
        let _in_memory = StandardHandlerFactory::in_memory(device_id);
        let _network = StandardHandlerFactory::network(device_id, "tcp://localhost:8080");
    }
}
