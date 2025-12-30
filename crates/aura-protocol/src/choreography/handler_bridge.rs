//! Choreographic Handler Bridge
//!
//! Layer 4: Generic handler traits for choreographic protocols.
//! This provides the interface that choreographic protocols need without
//! depending on concrete runtime implementations.

use aura_core::identifiers::{ContextId, DeviceId};
use aura_core::util::serialization::{from_slice, to_vec};
use aura_guards::LeakageBudget;
use biscuit_auth::Biscuit;
use rumpsteak_aura_choreography::effects::ChoreoHandler;
use rumpsteak_aura_choreography::{ChoreographyError, Label};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::effects::choreographic::{ChoreographicEffects, ChoreographicRole};

type ChoreoResult<T> = Result<T, ChoreographyError>;

fn encode_label(label: Label) -> Vec<u8> {
    label.0.as_bytes().to_vec()
}

fn decode_label(payload: Vec<u8>) -> ChoreoResult<Label> {
    let label = String::from_utf8(payload)
        .map_err(|e| ChoreographyError::Transport(format!("Label decode failed: {e}")))?;
    Ok(Label(Box::leak(label.into_boxed_str())))
}

/// Guard profile for message sending operations
#[derive(Debug, Clone)]
pub struct SendGuardProfile {
    pub authorization_tokens: Vec<Biscuit>,
    pub leakage_budget: LeakageBudget,
    pub delta_facts: Vec<Value>,
    pub flow_cost: u32,
}

impl Default for SendGuardProfile {
    fn default() -> Self {
        Self {
            authorization_tokens: vec![],
            leakage_budget: LeakageBudget::zero(),
            delta_facts: vec![],
            flow_cost: 1,
        }
    }
}

/// Trait for choreographic handler configuration
///
/// This trait provides the interface that choreographic protocols need
/// without depending on concrete runtime implementations.
pub trait ChoreographicHandler: Send + Sync {
    /// Get the device ID
    fn device_id(&self) -> DeviceId;

    /// Add role mapping for choreographic protocols
    fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId);

    /// Set flow context for capability tracking
    fn set_flow_context(&mut self, device_id: DeviceId, context_id: ContextId);

    /// Configure guard profile for message types
    fn configure_guard(&mut self, message_type: &'static str, profile: SendGuardProfile);
}

/// Generic endpoint trait for choreographic protocols
pub trait ChoreographicEndpoint: Send + Sync + Clone {
    /// Create a new endpoint for the given device
    fn new(device_id: DeviceId) -> Self;

    /// Get the device ID
    fn device_id(&self) -> DeviceId;
}

/// Generic choreographic adapter trait
///
/// This combines the choreographic handler interface with rumpsteak's ChoreoHandler
/// to provide a complete interface for choreographic protocols.
pub trait ChoreographicAdapter: ChoreographicHandler + ChoreoHandler + Send + Sync {
    type Endpoint: ChoreographicEndpoint;
}

/// Default implementation of ChoreographicEndpoint
#[derive(Debug, Clone)]
pub struct DefaultEndpoint {
    device_id: DeviceId,
}

impl ChoreographicEndpoint for DefaultEndpoint {
    fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

/// Runtime-backed adapter that bridges ChoreographicEffects to rumpsteak handlers.
#[derive(Clone)]
pub struct EffectsChoreographicAdapter<E> {
    effects: Arc<E>,
    device_id: DeviceId,
    role_mappings: HashMap<String, DeviceId>,
    role_order: Vec<DeviceId>,
    flow_contexts: HashMap<DeviceId, ContextId>,
    guard_profiles: HashMap<&'static str, SendGuardProfile>,
}

impl<E> EffectsChoreographicAdapter<E>
where
    E: ChoreographicEffects + Send + Sync + 'static,
{
    pub fn new(effects: Arc<E>, device_id: DeviceId) -> Self {
        Self {
            effects,
            device_id,
            role_mappings: HashMap::new(),
            role_order: Vec::new(),
            flow_contexts: HashMap::new(),
            guard_profiles: HashMap::new(),
        }
    }

    /// Start a choreography session using the current role mappings.
    pub async fn start_session(
        &mut self,
        session_id: uuid::Uuid,
        roles: Vec<DeviceId>,
    ) -> Result<(), crate::effects::choreographic::ChoreographyError> {
        self.role_order = roles.clone();
        let choreo_roles = roles
            .into_iter()
            .enumerate()
            .map(|(idx, device_id)| ChoreographicRole::new(device_id.0, idx as u32))
            .collect();
        self.effects.start_session(session_id, choreo_roles).await
    }

    fn role_index(&self, device_id: &DeviceId) -> Option<usize> {
        self.role_order.iter().position(|id| id == device_id)
    }

    fn to_choreo_role(&self, device_id: &DeviceId) -> Result<ChoreographicRole, ChoreographyError> {
        let idx = self.role_index(device_id).ok_or_else(|| {
            ChoreographyError::Transport(format!("Unknown role for device {device_id}"))
        })?;
        Ok(ChoreographicRole::new(device_id.0, idx as u32))
    }
}

impl<E> ChoreographicHandler for EffectsChoreographicAdapter<E>
where
    E: ChoreographicEffects + Send + Sync + 'static,
{
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId) {
        self.role_mappings.insert(role_name, device_id);
        if !self.role_order.contains(&device_id) {
            self.role_order.push(device_id);
        }
    }

    fn set_flow_context(&mut self, device_id: DeviceId, context_id: ContextId) {
        self.flow_contexts.insert(device_id, context_id);
    }

    fn configure_guard(&mut self, message_type: &'static str, profile: SendGuardProfile) {
        self.guard_profiles.insert(message_type, profile);
    }
}

#[async_trait::async_trait]
impl<E> ChoreoHandler for EffectsChoreographicAdapter<E>
where
    E: ChoreographicEffects + Send + Sync + 'static,
{
    type Role = DeviceId;
    type Endpoint = DefaultEndpoint;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> ChoreoResult<()> {
        let role = self.to_choreo_role(&to)?;
        let payload = to_vec(msg).map_err(|e| {
            ChoreographyError::Transport(format!("Choreography encode failed: {e}"))
        })?;
        self.effects
            .send_to_role_bytes(role, payload)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))?;
        Ok(())
    }

    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<M> {
        let role = self.to_choreo_role(&from)?;
        let payload = self
            .effects
            .receive_from_role_bytes(role)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))?;
        let message = from_slice(&payload).map_err(|e| {
            ChoreographyError::Transport(format!("Choreography decode failed: {e}"))
        })?;
        Ok(message)
    }

    async fn choose(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        who: Self::Role,
        label: Label,
    ) -> ChoreoResult<()> {
        let role = self.to_choreo_role(&who)?;
        let payload = encode_label(label);
        self.effects
            .send_to_role_bytes(role, payload)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))?;
        Ok(())
    }

    async fn offer(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        from: Self::Role,
    ) -> ChoreoResult<Label> {
        let role = self.to_choreo_role(&from)?;
        let payload = self
            .effects
            .receive_from_role_bytes(role)
            .await
            .map_err(|e| ChoreographyError::Transport(e.to_string()))?;
        decode_label(payload)
    }

    async fn with_timeout<F, T>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _at: Self::Role,
        dur: std::time::Duration,
        body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        self.effects.set_timeout(dur.as_millis() as u64).await;
        body.await
    }
}

impl<E> ChoreographicAdapter for EffectsChoreographicAdapter<E>
where
    E: ChoreographicEffects + Send + Sync + 'static,
{
    type Endpoint = DefaultEndpoint;
}

/// Test-only mock implementation
#[cfg(test)]
pub struct MockChoreographicAdapter {
    device_id: DeviceId,
    role_mappings: std::collections::HashMap<String, DeviceId>,
}

#[cfg(test)]
impl MockChoreographicAdapter {
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            role_mappings: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
impl ChoreographicHandler for MockChoreographicAdapter {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn add_role_mapping(&mut self, role_name: String, device_id: DeviceId) {
        self.role_mappings.insert(role_name, device_id);
    }

    fn set_flow_context(&mut self, _device_id: DeviceId, _context_id: ContextId) {
        // Mock implementation - no-op
    }

    fn configure_guard(&mut self, _message_type: &'static str, _profile: SendGuardProfile) {
        // Mock implementation - no-op
    }
}

#[cfg(test)]
#[async_trait::async_trait]
impl ChoreoHandler for MockChoreographicAdapter {
    type Role = DeviceId;
    type Endpoint = DefaultEndpoint;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _to: Self::Role,
        _msg: &M,
    ) -> ChoreoResult<()> {
        // Mock implementation - always succeeds
        Ok(())
    }

    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> ChoreoResult<M> {
        Err(ChoreographyError::Transport(
            "Mock adapter - recv not implemented".to_string(),
        ))
    }

    async fn choose(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _who: Self::Role,
        _label: Label,
    ) -> ChoreoResult<()> {
        // Mock implementation - always succeeds
        Ok(())
    }

    async fn offer(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _from: Self::Role,
    ) -> ChoreoResult<Label> {
        Err(ChoreographyError::Transport(
            "Mock adapter - offer not implemented".to_string(),
        ))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _endpoint: &mut Self::Endpoint,
        _at: Self::Role,
        _dur: std::time::Duration,
        _body: F,
    ) -> ChoreoResult<T>
    where
        F: std::future::Future<Output = ChoreoResult<T>> + Send,
    {
        // Mock implementation - just execute without timeout
        _body.await
    }
}

#[cfg(test)]
impl ChoreographicAdapter for MockChoreographicAdapter {
    type Endpoint = DefaultEndpoint;
}
