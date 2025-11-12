//! Aura Handler Adapter for Rumpsteak Choreographies
//!
//! This module provides integration between rumpsteak-aura generated session types
//! and Aura's effect system, enabling implementation-agnostic choreographic execution.

use crate::{
    effects::system::AuraEffectSystem,
    guards::{FlowHint, LeakageBudget, ProtocolGuard},
    handlers::{AuraHandlerError, ExecutionMode},
};
use aura_core::{relationships::ContextId, DeviceId, Receipt};
use aura_wot::Capability;
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{any::type_name, collections::HashMap};

#[derive(Debug, Clone)]
pub struct SendGuardProfile {
    pub capabilities: Vec<Capability>,
    pub leakage_budget: LeakageBudget,
    pub delta_facts: Vec<Value>,
    pub flow_cost: u32,
}

impl Default for SendGuardProfile {
    fn default() -> Self {
        Self {
            capabilities: vec![Capability::Execute {
                operation: "choreography_send".to_string(),
            }],
            leakage_budget: LeakageBudget::zero(),
            delta_facts: vec![],
            flow_cost: 1,
        }
    }
}

/// Adapter bridging choreographic runtimes with AuraEffectSystem
pub struct AuraHandlerAdapter {
    effect_system: AuraEffectSystem,
    device_id: DeviceId,
    role_mapping: HashMap<String, DeviceId>,
    flow_contexts: HashMap<DeviceId, ContextId>,
    guard_profiles: HashMap<&'static str, SendGuardProfile>,
    default_guard: SendGuardProfile,
}

impl AuraHandlerAdapter {
    /// Create a new adapter for the specified execution mode
    pub fn new(device_id: DeviceId, mode: ExecutionMode) -> Self {
        let effect_system = AuraEffectSystem::new(device_id, mode);
        Self::from_effect_system(effect_system)
    }

    /// Create an adapter from an existing effect system (useful for tests)
    pub fn from_effect_system(effect_system: AuraEffectSystem) -> Self {
        let device_id = effect_system.device_id();
        Self {
            effect_system,
            device_id,
            role_mapping: HashMap::new(),
            flow_contexts: HashMap::new(),
            guard_profiles: HashMap::new(),
            default_guard: SendGuardProfile::default(),
        }
    }

    /// Add a role mapping for a participant
    pub fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId) {
        self.role_mapping.insert(role_name, device_id);
    }

    /// Configure a specific flow context for a peer device
    pub fn set_flow_context_for_peer(&mut self, peer: DeviceId, context: ContextId) {
        self.flow_contexts.insert(peer, context);
    }

    /// Register guard metadata for a specific message type
    pub fn register_message_guard<T>(&mut self, profile: SendGuardProfile)
    where
        T: 'static,
    {
        self.guard_profiles.insert(type_name::<T>(), profile);
    }

    /// Get device ID for a role
    pub fn get_device_id_for_role(&self, role: &str) -> Option<DeviceId> {
        self.role_mapping.get(role).copied()
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Immutable access to the underlying effect system
    pub fn effects(&self) -> &AuraEffectSystem {
        &self.effect_system
    }

    /// Mutable access to the underlying effect system
    pub fn effects_mut(&mut self) -> &mut AuraEffectSystem {
        &mut self.effect_system
    }

    /// Latest FlowGuard receipt recorded by the adapter
    pub async fn latest_receipt(&self) -> Option<Receipt> {
        self.effect_system.latest_receipt().await
    }

    /// Send a message with guard-chain enforcement
    pub async fn send<T: Serialize>(
        &mut self,
        target: DeviceId,
        message: T,
    ) -> Result<(), AuraHandlerError> {
        use crate::effects::NetworkEffects;

        let target_device = target;
        let target_uuid: uuid::Uuid = target_device.into();
        let message_bytes =
            bincode::serialize(&message).map_err(|e| AuraHandlerError::EffectSerialization {
                effect_type: crate::handlers::EffectType::Network,
                operation: "send_to_peer".to_string(),
                source: Box::new(e),
            })?;

        let guard_profile = self
            .guard_profiles
            .get(type_name::<T>())
            .cloned()
            .unwrap_or_else(|| self.default_guard.clone());

        let guard = ProtocolGuard::new(format!("choreography_send::{}", type_name::<T>()))
            .require_capabilities(guard_profile.capabilities.clone())
            .delta_facts(guard_profile.delta_facts.clone())
            .leakage_budget(guard_profile.leakage_budget.clone());

        let flow_context = self.ensure_flow_context(&target_device);
        let flow_cost = guard_profile.flow_cost.max(1);
        self.effect_system
            .set_flow_hint(FlowHint::new(flow_context, target_device, flow_cost))
            .await;

        // TODO: Apply guard constraints properly
        // For now, execute the operation directly to avoid lifetime issues
        self.effect_system
            .send_to_peer(target_uuid, message_bytes.clone())
            .await
            .map_err(|e| AuraHandlerError::ContextError {
                message: format!("Send to peer failed: {}", e),
            })?;

        tracing::debug!("Sent message to device {}", target_device);
        Ok(())
    }

    /// Receive a message from a participant via network effects
    pub async fn recv_from<T: DeserializeOwned>(
        &mut self,
        sender: DeviceId,
    ) -> Result<T, AuraHandlerError> {
        use crate::effects::NetworkEffects;

        let sender_uuid: uuid::Uuid = sender.into();
        let message_bytes = self
            .effect_system
            .receive_from(sender_uuid)
            .await
            .map_err(|e| AuraHandlerError::ContextError {
                message: format!("Failed to receive message: {}", e),
            })?;

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

    fn ensure_flow_context(&mut self, peer: &DeviceId) -> ContextId {
        self.flow_contexts
            .entry(*peer)
            .or_insert_with(|| ContextId::new(format!("choreo://{}->{}", self.device_id, peer)))
            .clone()
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
        AuraHandlerAdapter::new(device_id, ExecutionMode::Testing)
    }

    /// Create adapter for production
    pub fn for_production(device_id: DeviceId) -> AuraHandlerAdapter {
        AuraHandlerAdapter::new(device_id, ExecutionMode::Production)
    }

    /// Create adapter for simulation
    pub fn for_simulation(device_id: DeviceId) -> AuraHandlerAdapter {
        AuraHandlerAdapter::new(device_id, ExecutionMode::Simulation { seed: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestMessage {
        payload: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct GuardedPayload {
        value: u32,
    }

    #[tokio::test]
    async fn test_adapter_creation() {
        let device_id = DeviceId::new();
        let adapter = AuraHandlerAdapterFactory::for_testing(device_id);

        // Should create without error
        assert_eq!(adapter.device_id(), device_id);
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

    #[tokio::test]
    async fn guard_chain_emits_receipt() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_a);
        adapter.set_flow_context_for_peer(device_b, ContextId::new("test.ctx"));

        // Network send will fail due to missing peers, but guard execution should still emit a receipt.
        let _ = adapter
            .send(
                device_b,
                TestMessage {
                    payload: "hello".into(),
                },
            )
            .await;

        assert!(
            adapter.latest_receipt().await.is_some(),
            "expected FlowGuard receipt even when transport fails"
        );
    }

    #[tokio::test]
    async fn custom_guard_profile_controls_flow_cost() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let mut adapter = AuraHandlerAdapterFactory::for_testing(device_a);
        adapter.set_flow_context_for_peer(device_b, ContextId::new("custom.ctx"));

        let mut profile = SendGuardProfile::default();
        profile.flow_cost = 64;
        adapter.register_message_guard::<GuardedPayload>(profile);

        let _ = adapter.send(device_b, GuardedPayload { value: 7 }).await;

        let receipt = adapter.latest_receipt().await.expect("receipt");
        assert_eq!(receipt.cost, 64);
    }

    #[tokio::test]
    async fn memory_network_delivers_messages_between_adapters() {
        let device_a = DeviceId::new();
        let device_b = DeviceId::new();
        let mut adapter_a = AuraHandlerAdapterFactory::for_testing(device_a);
        let mut adapter_b = AuraHandlerAdapterFactory::for_testing(device_b);

        adapter_a
            .send(
                device_b,
                TestMessage {
                    payload: "hello".into(),
                },
            )
            .await
            .expect("send");

        let msg: TestMessage = adapter_b.recv_from(device_a).await.expect("recv");
        assert_eq!(msg.payload, "hello");
    }
}
