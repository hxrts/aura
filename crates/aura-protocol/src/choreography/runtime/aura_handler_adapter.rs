//! Aura Handler Adapter for Rumpsteak Choreographies
//!
//! This module provides integration between rumpsteak-aura generated session types
//! and Aura's effect system, enabling implementation-agnostic choreographic execution.

use crate::choreography::types::ChoreographicRole;
use crate::effects::choreographic::ChoreographicEffects;
use crate::handlers::{AuraHandlerError, CompositeHandler};
use aura_core::DeviceId;
use std::collections::HashMap;

/// TODO fix - Simplified adapter for potential future rumpsteak integration
pub struct AuraHandlerAdapter {
    /// The underlying aura effect system
    effects: CompositeHandler,
    /// Current device ID
    device_id: DeviceId,
    /// Role mapping for participants
    role_mapping: HashMap<String, DeviceId>,
}

impl AuraHandlerAdapter {
    /// Create a new adapter with the given effect system
    pub fn new(effects: CompositeHandler, device_id: DeviceId) -> Self {
        Self {
            effects,
            device_id,
            role_mapping: HashMap::new(),
        }
    }

    /// Add a role mapping for a participant
    pub fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId) {
        self.role_mapping.insert(role_name, device_id);
    }

    /// Get device ID for a role
    pub fn get_device_id_for_role(&self, role: &str) -> Option<DeviceId> {
        self.role_mapping.get(role).copied()
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get access to the effects system
    pub fn effects(&self) -> &CompositeHandler {
        &self.effects
    }

    /// Get mutable access to the effects system
    pub fn effects_mut(&mut self) -> &mut CompositeHandler {
        &mut self.effects
    }

    /// Send a message to a participant via network effects
    pub async fn send<T: serde::Serialize>(
        &mut self,
        target: DeviceId,
        message: T,
    ) -> Result<(), AuraHandlerError> {
        use crate::effects::NetworkEffects;

        // Serialize message to bytes
        let message_bytes =
            bincode::serialize(&message).map_err(|e| AuraHandlerError::EffectSerialization {
                effect_type: crate::handlers::EffectType::Network,
                operation: "send_to_peer".to_string(),
                source: Box::new(e),
            })?;

        // Convert DeviceId to UUID for network layer
        let target_uuid: uuid::Uuid = target.into();

        // Send via network effects
        self.effects
            .send_to_peer(target_uuid, message_bytes)
            .await
            .map_err(|e| AuraHandlerError::ContextError {
                message: format!("Failed to send message: {}", e),
            })?;

        tracing::debug!("Sent message to device {}", target);
        Ok(())
    }

    /// Receive a message from a participant via network effects
    pub async fn recv_from<T: serde::de::DeserializeOwned>(
        &mut self,
        sender: DeviceId,
    ) -> Result<T, AuraHandlerError> {
        use crate::effects::NetworkEffects;

        // Convert DeviceId to UUID for network layer
        let sender_uuid: uuid::Uuid = sender.into();

        // Receive via network effects
        let message_bytes = self.effects.receive_from(sender_uuid).await.map_err(|e| {
            AuraHandlerError::ContextError {
                message: format!("Failed to receive message: {}", e),
            }
        })?;

        // Deserialize message from bytes
        let message: T = bincode::deserialize(&message_bytes).map_err(|e| {
            AuraHandlerError::EffectDeserialization {
                effect_type: crate::handlers::EffectType::Network,
                operation: "receive_from".to_string(),
                source: Box::new(e),
            }
        })?;

        tracing::debug!("Received message from device {}", sender);
        Ok(message)
    }
}

/// Endpoint for choreographic communication
pub struct AuraEndpoint {
    device_id: DeviceId,
}

impl AuraEndpoint {
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Factory for creating AuraHandlerAdapter instances
pub struct AuraHandlerAdapterFactory;

impl AuraHandlerAdapterFactory {
    /// Create adapter for testing
    pub fn for_testing(device_id: DeviceId) -> AuraHandlerAdapter {
        let effects = CompositeHandler::for_testing(device_id.into());
        AuraHandlerAdapter::new(effects, device_id)
    }

    /// Create adapter for production
    pub fn for_production(device_id: DeviceId) -> AuraHandlerAdapter {
        let effects = CompositeHandler::for_production(device_id.into());
        AuraHandlerAdapter::new(effects, device_id)
    }

    /// Create adapter for simulation
    pub fn for_simulation(device_id: DeviceId) -> AuraHandlerAdapter {
        let effects = CompositeHandler::for_simulation(device_id.into());
        AuraHandlerAdapter::new(effects, device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_adapter_creation() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        // Should create without error
        assert_eq!(adapter.device_id, device_id);
    }

    #[tokio::test]
    async fn test_role_mapping() {
        let device_id = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        let peer_device = DeviceId::new();
        adapter.add_role_mapping("alice".to_string(), peer_device);

        assert_eq!(adapter.get_device_id_for_role("alice"), Some(peer_device));
        assert_eq!(adapter.get_device_id_for_role("bob"), None);
    }
}
