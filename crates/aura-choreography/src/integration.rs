//! Unified choreography adapters following docs/405_protocol_guide.md
//!
//! This module provides the integration layer between choreographic protocols
//! and Aura's unified effect system, implementing the adapter pattern described
//! in the protocol guide.

pub use crate::runtime::{AuraEndpoint, AuraHandlerAdapter, AuraHandlerAdapterFactory};

/// Create a testing adapter following protocol guide patterns
pub fn create_testing_adapter(device_id: aura_types::DeviceId) -> AuraHandlerAdapter {
    AuraHandlerAdapterFactory::for_testing(device_id)
}

/// Create a production adapter following protocol guide patterns  
pub fn create_production_adapter(device_id: aura_types::DeviceId) -> AuraHandlerAdapter {
    AuraHandlerAdapterFactory::for_production(device_id)
}

/// Create a simulation adapter following protocol guide patterns
pub fn create_simulation_adapter(device_id: aura_types::DeviceId) -> AuraHandlerAdapter {
    AuraHandlerAdapterFactory::for_simulation(device_id)
}

/// Create a choreography endpoint for role-specific execution
pub fn create_choreography_endpoint(device_id: aura_types::DeviceId) -> AuraEndpoint {
    AuraEndpoint::new(device_id)
}
