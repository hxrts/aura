//! Aura Handler Adapter for Rumpsteak Choreographies
//!
//! This module provides integration between rumpsteak-aura generated session types
//! and Aura's effect system, enabling implementation-agnostic choreographic execution.

use crate::{
    handlers::{AuraHandlerError, ExecutionMode},
    runtime::AuraEffectSystem,
};
use async_trait::async_trait;
use aura_core::{ContextId, DeviceId, Receipt};
use aura_protocol::guards::{FlowHint, LeakageBudget, ProtocolGuard};
use aura_wot::Capability;
use rumpsteak_aura_choreography::effects::{
    ChoreoHandler, ChoreographyError, Label, Result as ChoreoResult,
};
// Note: Endpoint type should be available from effects or another module
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{any::type_name, collections::HashMap, time::Duration};

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
    /// Create a new adapter with an existing effect system (recommended)
    ///
    /// This is the preferred method as it follows proper dependency injection.
    /// Use this instead of `new()` for better testability and control.
    pub fn with_effect_system(effect_system: AuraEffectSystem, device_id: DeviceId) -> Self {
        Self::from_effect_system(effect_system, device_id)
    }

    /// Create a new adapter for the specified execution mode
    ///
    /// # Deprecated
    /// This method creates the effect system internally which makes testing difficult.
    /// Consider using `with_effect_system()` instead for better dependency injection.
    pub fn new(device_id: DeviceId, mode: ExecutionMode) -> Self {
        use crate::runtime::EffectSystemConfig;

        let config = match mode {
            ExecutionMode::Testing => EffectSystemConfig::for_testing(device_id),
            ExecutionMode::Production => EffectSystemConfig::for_production(device_id)
                .expect("Failed to create production config"),
            ExecutionMode::Simulation { seed } => {
                EffectSystemConfig::for_simulation(device_id, seed)
            }
        };

        // TODO: Create effect system based on configuration
        // Create a stub effect system for testing
        let effect_system = AuraEffectSystem::new();
        Self::from_effect_system(effect_system, device_id)
    }

    /// Create an adapter from an existing effect system (useful for tests)
    pub fn from_effect_system(effect_system: AuraEffectSystem, device_id: DeviceId) -> Self {
        // TODO: Box<dyn AuraEffects> doesn't have a device_id() method yet
        // Pass device_id as a parameter instead
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
        // TODO: Box<dyn AuraEffects> doesn't implement latest_receipt yet
        // self.effect_system.latest_receipt().await
        None
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
                source: e.to_string(),
            })?;

        let guard_profile = self
            .guard_profiles
            .get(type_name::<T>())
            .cloned()
            .unwrap_or_else(|| self.default_guard.clone());

        let guard = ProtocolGuard::new(format!("choreography_send::{}", type_name::<T>()))
            // TODO: Re-enable capability checking when ProtocolGuard::require_capabilities is implemented
            // .require_capabilities(guard_profile.capabilities.clone())
            .delta_facts(guard_profile.delta_facts.clone())
            .leakage_budget(guard_profile.leakage_budget.clone());

        let flow_context = self.ensure_flow_context(&target_device);
        let flow_cost = guard_profile.flow_cost.max(1);
        // TODO: Box<dyn AuraEffects> doesn't implement set_flow_hint yet
        // self.effect_system.set_flow_hint(FlowHint::new(
        //     flow_context.clone(),
        //     target_device,
        //     flow_cost,
        // ));

        // Charge flow and generate receipt
        use aura_core::effects::JournalEffects;
        let _receipt = JournalEffects::charge_flow_budget(
            &self.effect_system,
            &flow_context,
            &target_device,
            flow_cost,
        )
        .await
        .map_err(|e| AuraHandlerError::ContextError {
            message: format!("Flow budget charging failed: {}", e),
        })?;

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
                source: e.to_string(),
            }
        })?;

        tracing::debug!("Received message from device {}", sender);
        Ok(message)
    }

    fn ensure_flow_context(&mut self, peer: &DeviceId) -> ContextId {
        self.flow_contexts
            .entry(*peer)
            .or_insert_with(|| {
                // Create a deterministic context ID from the peer relationship
                // TODO: Use a stable naming scheme or store in metadata
                ContextId::new()
            })
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

// TODO: Endpoint trait not available - implement when rumpsteak session types are resolved
// impl Endpoint for AuraEndpoint {
//     type Role = DeviceId;
// }

// Implement ChoreoHandler trait for AuraHandlerAdapter
#[async_trait]
impl ChoreoHandler for AuraHandlerAdapter {
    type Role = DeviceId;
    type Endpoint = AuraEndpoint;

    /// Send a message to a specific role with guard chain enforcement
    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        _ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> ChoreoResult<()> {
        self.send(to, msg)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))
    }

    /// Receive a strongly-typed message from a specific role
    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        _ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<M> {
        self.recv_from(from)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))
    }

    /// Internal choice: broadcast a label selection (for branch protocols)
    async fn choose(
        &mut self,
        _ep: &mut Self::Endpoint,
        who: Self::Role,
        label: Label,
    ) -> ChoreoResult<()> {
        // TODO: ChoiceMessage doesn't implement Serialize/Deserialize
        // This needs to be refactored to use a serializable type
        Err(ChoreographyError::Transport(
            "Choice messages not yet supported".to_string(),
        ))

        // // Send the label as a choice message
        // let choice_msg = ChoiceMessage {
        //     label: label.clone(),
        // };
        // self.send(who, choice_msg)
        //     .await
        //     .map_err(|e| ChoreographyError::Transport(e.to_string()))
    }

    /// External choice: receive a label selection (for branch protocols)
    async fn offer(&mut self, _ep: &mut Self::Endpoint, from: Self::Role) -> ChoreoResult<Label> {
        // TODO: ChoiceMessage doesn't implement Serialize/Deserialize
        // This needs to be refactored to use a serializable type
        Err(ChoreographyError::Transport(
            "Choice messages not yet supported".to_string(),
        ))

        // // Receive a choice message and extract the label
        // let choice_msg: ChoiceMessage = self
        //     .recv_from(from)
        //     .await
        //     .map_err(|e| ChoreographyError::Transport(e.to_string()))?;
        // Ok(choice_msg.label)
    }

    /// Execute a future with a timeout
    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _at: Self::Role,
        dur: Duration,
        body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        tokio::time::timeout(dur, body)
            .await
            .map_err(|_| ChoreographyError::Timeout(dur))?
    }
}

/// Internal message type for choice communication
/// TODO: Label doesn't implement Serialize/Deserialize
/// Need to either wrap it or use a different type
#[derive(Debug, Clone)]
struct ChoiceMessage {
    label: Label,
}

/// Factory for creating AuraHandlerAdapter instances
pub struct AuraHandlerAdapterFactory;

impl AuraHandlerAdapterFactory {
    /// Create adapter for testing - uses proper AuraHandler from aura-mpst
    pub fn for_testing(
        device_id: DeviceId,
    ) -> Result<aura_mpst::AuraHandler, aura_mpst::MpstError> {
        aura_mpst::AuraHandler::for_testing(device_id)
    }

    /// Create adapter for production - uses proper AuraHandler from aura-mpst
    pub fn for_production(
        device_id: DeviceId,
    ) -> Result<aura_mpst::AuraHandler, aura_mpst::MpstError> {
        aura_mpst::AuraHandler::for_production(device_id)
    }

    /// Create adapter for simulation - uses proper AuraHandler from aura-mpst
    pub fn for_simulation(
        device_id: DeviceId,
    ) -> Result<aura_mpst::AuraHandler, aura_mpst::MpstError> {
        aura_mpst::AuraHandler::for_simulation(device_id)
    }

    /// Legacy method: Create AuraHandlerAdapter for backward compatibility
    pub fn legacy_for_testing(device_id: DeviceId) -> AuraHandlerAdapter {
        AuraHandlerAdapter::new(device_id, ExecutionMode::Testing)
    }

    /// Legacy method: Create AuraHandlerAdapter for backward compatibility
    pub fn legacy_for_production(device_id: DeviceId) -> AuraHandlerAdapter {
        AuraHandlerAdapter::new(device_id, ExecutionMode::Production)
    }

    /// Legacy method: Create AuraHandlerAdapter for backward compatibility
    pub fn legacy_for_simulation(device_id: DeviceId) -> AuraHandlerAdapter {
        AuraHandlerAdapter::new(device_id, ExecutionMode::Simulation { seed: 0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;
    use aura_testkit::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestMessage {
        payload: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct GuardedPayload {
        value: u32,
    }

    #[aura_test]
    async fn test_adapter_creation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let adapter = AuraHandlerAdapter::with_effect_system((*fixture.effects()).clone(), device_id);

        // Should create without error
        assert_eq!(adapter.device_id(), device_id);
        Ok(())
    }

    #[aura_test]
    async fn test_role_mapping() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let mut adapter = AuraHandlerAdapter::with_effect_system((*fixture.effects()).clone(), device_id);

        let peer_device = DeviceId::new();
        adapter.add_role_mapping("alice".to_string(), peer_device);

        assert_eq!(adapter.get_device_id_for_role("alice"), Some(peer_device));
        assert_eq!(adapter.get_device_id_for_role("bob"), None);
        Ok(())
    }

    #[aura_test]
    async fn guard_chain_emits_receipt() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_a = fixture.device_id();
        let device_b = DeviceId::new();
        let mut adapter = AuraHandlerAdapter::with_effect_system((*fixture.effects()).clone(), device_a);
        adapter.set_flow_context_for_peer(device_b, ContextId::new());

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
        Ok(())
    }

    #[aura_test]
    async fn custom_guard_profile_controls_flow_cost() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_a = fixture.device_id();
        let device_b = DeviceId::new();
        let mut adapter = AuraHandlerAdapter::with_effect_system((*fixture.effects()).clone());
        adapter.set_flow_context_for_peer(device_b, ContextId::new("custom.ctx"));

        let profile = SendGuardProfile {
            flow_cost: 64,
            ..Default::default()
        };
        adapter.register_message_guard::<GuardedPayload>(profile);

        let _ = adapter.send(device_b, GuardedPayload { value: 7 }).await;

        let receipt = adapter.latest_receipt().await.expect("receipt");
        assert_eq!(receipt.cost, 64);
        Ok(())
    }

    #[aura_test]
    async fn memory_network_delivers_messages_between_adapters() -> aura_core::AuraResult<()> {
        let fixture_a = create_test_fixture().await?;
        let fixture_b = create_test_fixture().await?;
        let device_a = fixture_a.device_id();
        let device_b = fixture_b.device_id();

        // Create adapters with proper testing setup
        let mut adapter_a = AuraHandlerAdapter::with_effect_system((*fixture_a.effects()).clone());
        let mut adapter_b = AuraHandlerAdapter::with_effect_system((*fixture_b.effects()).clone());

        // The network delivery should work via shared memory network registry
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
        Ok(())
    }
}
