//! Aura Handler Adapter for Rumpsteak Choreographies
//!
//! This module provides integration between rumpsteak-aura generated session types
//! and Aura's effect system, enabling implementation-agnostic choreographic execution.

use crate::types::ChoreographicRole;
use aura_protocol::{handlers::CompositeHandler, AuraHandlerError, effects::choreographic::ChoreographicEffects};
use aura_types::DeviceId;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError as RumpsteakError, Label};

use async_trait::async_trait;
use futures::channel::mpsc::{self, UnboundedReceiver, UnboundedSender};
use rumpsteak_aura::channel::Bidirectional;
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Adapter that bridges rumpsteak `ChoreoHandler` to aura's effect system
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
    fn get_device_id_for_role(&self, role: &str) -> Option<DeviceId> {
        self.role_mapping.get(role).copied()
    }

    /// Convert rumpsteak error to choreography error
    fn convert_error(err: AuraHandlerError) -> RumpsteakError {
        RumpsteakError::Transport(format!("Aura handler error: {}", err))
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
}

#[async_trait]
impl ChoreoHandler for AuraHandlerAdapter {
    type Role = ChoreographicRole;
    type Endpoint = AuraEndpoint;

    async fn send<M>(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> Result<(), RumpsteakError>
    where
        M: Serialize + Send + Sync,
    {
        // Serialize the message
        let msg_bytes =
            bincode::serialize(msg).map_err(|e| RumpsteakError::Serialization(e.to_string()))?;

        // Convert role to aura choreographic role
        let aura_role = aura_protocol::effects::ChoreographicRole {
            device_id: to.device_id().unwrap_or(self.device_id).into(),
            role_index: match to {
                ChoreographicRole::Participant(idx) => idx,
                ChoreographicRole::Coordinator => 0,
                ChoreographicRole::Device(_) => 0,
            },
        };

        // Delegate to choreographic effects
        use aura_protocol::effects::choreographic::ChoreographicEffects;
        self.effects
            .send_to_role_bytes(aura_role, msg_bytes)
            .await
            .map_err(|e| RumpsteakError::Transport(e.to_string()))
    }

    async fn recv<M>(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<M, RumpsteakError>
    where
        M: DeserializeOwned + Send,
    {
        // Convert role to aura choreographic role
        let aura_role = aura_protocol::effects::ChoreographicRole {
            device_id: from.device_id().unwrap_or(self.device_id).into(),
            role_index: match from {
                ChoreographicRole::Participant(idx) => idx,
                ChoreographicRole::Coordinator => 0,
                ChoreographicRole::Device(_) => 0,
            },
        };

        // Delegate to choreographic effects
        use aura_protocol::effects::choreographic::ChoreographicEffects;
        let msg_bytes = self
            .effects
            .receive_from_role_bytes(aura_role)
            .await
            .map_err(|e| RumpsteakError::Transport(e.to_string()))?;

        // Deserialize the message
        bincode::deserialize(&msg_bytes).map_err(|e| RumpsteakError::Serialization(e.to_string()))
    }

    async fn choose(
        &mut self,
        _ep: &mut Self::Endpoint,
        _role: Self::Role,
        _label: Label,
    ) -> Result<(), RumpsteakError> {
        // For now, just emit a generic event - can be enhanced later
        use aura_protocol::effects::choreographic::ChoreographicEffects;
        self.effects
            .emit_choreo_event(
                aura_protocol::effects::choreographic::ChoreographyEvent::MessageSent {
                    from: aura_protocol::effects::choreographic::ChoreographicRole {
                        device_id: self.device_id.into(),
                        role_index: 0,
                    },
                    to: aura_protocol::effects::choreographic::ChoreographicRole {
                        device_id: self.device_id.into(),
                        role_index: 0,
                    },
                    message_type: format!("choice:{:?}", _label),
                },
            )
            .await
            .map_err(|e| RumpsteakError::Transport(e.to_string()))
    }

    async fn offer(
        &mut self,
        _ep: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> Result<Label, RumpsteakError> {
        // For now, just return a dummy label - can be enhanced later
        Ok(Label("default"))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _from: Self::Role,
        _timeout: std::time::Duration,
        future: F,
    ) -> Result<T, RumpsteakError>
    where
        F: std::future::Future<Output = Result<T, RumpsteakError>> + Send,
    {
        // For now, just execute the future without timeout - can be enhanced later
        future.await
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
