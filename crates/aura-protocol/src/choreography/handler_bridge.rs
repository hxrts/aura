//! Choreographic Handler Bridge
//!
//! Layer 4: Generic handler traits for choreographic protocols.
//! This provides the interface that choreographic protocols need without
//! depending on concrete runtime implementations.

use crate::guards::LeakageBudget;
use aura_core::identifiers::ContextId;
use aura_core::identifiers::DeviceId;
use biscuit_auth::Biscuit;
// use aura_wot::Capability; // Legacy capability removed - use Biscuit tokens instead
use rumpsteak_aura_choreography::effects::ChoreoHandler;
use serde_json::Value;

#[cfg(test)]
use {
    rumpsteak_aura_choreography::{ChoreographyError, Label},
    serde::{de::DeserializeOwned, Serialize},
};

#[cfg(test)]
type ChoreoResult<T> = Result<T, ChoreographyError>;

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
