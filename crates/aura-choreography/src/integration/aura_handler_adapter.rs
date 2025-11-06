//! Unified AuraEffectSystem adapter for choreographic programming
//!
//! This module provides the new unified integration between choreographic protocols
//! and the Aura effect system, replacing the legacy fragmented handler approach.
//!
//! # Architecture
//!
//! ```text
//! Choreographic Protocol (rumpsteak)
//!     ↓ implements
//! ChoreoHandler trait ← AuraHandlerAdapter (THIS MODULE)
//!     ↓ delegates to
//! AuraEffectSystem (unified effects)
//!     ↓ routes through
//! MiddlewareStack → Effect Registry → Concrete Handlers
//! ```
//!
//! # Key Benefits
//!
//! - **Unified**: Single effect system for all operations
//! - **Middleware**: Full middleware stack support for choreographies
//! - **Context**: Proper AuraContext flow through choreographic operations
//! - **Monitoring**: Centralized metrics and observability
//! - **Configuration**: Consistent factory patterns

use async_trait::async_trait;
use rumpsteak_choreography::{ChoreoHandler, ChoreographyError, Label};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_types::{
    handlers::{AuraHandler, AuraHandlerError, EffectType, context::AuraContext},
    identifiers::DeviceId,
    effects::choreographic::{ChoreographicRole, ChoreographyEvent},
};

/// Endpoint for unified Aura effect system choreography
///
/// Maintains choreographic session state and provides access to the unified context.
pub struct AuraEffectEndpoint {
    /// Device ID of this endpoint
    pub device_id: DeviceId,
    /// Active role in the choreography
    pub my_role: ChoreographicRole,
    /// Session context for this choreography
    pub context: Arc<RwLock<AuraContext>>,
}

impl AuraEffectEndpoint {
    /// Create a new Aura effect endpoint
    pub fn new(device_id: DeviceId, my_role: ChoreographicRole, context: Arc<RwLock<AuraContext>>) -> Self {
        Self {
            device_id,
            my_role,
            context,
        }
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get the choreographic role
    pub fn my_role(&self) -> ChoreographicRole {
        self.my_role
    }

    /// Get read access to the context
    pub async fn context(&self) -> tokio::sync::RwLockReadGuard<'_, AuraContext> {
        self.context.read().await
    }

    /// Get write access to the context
    pub async fn context_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, AuraContext> {
        self.context.write().await
    }
}

/// Unified Aura handler adapter for choreographic programming
///
/// This adapter implements `ChoreoHandler` using the unified `AuraEffectSystem`,
/// providing access to all effect types through the middleware stack and
/// ensuring proper context flow throughout choreographic operations.
///
/// Key features:
/// - Single unified effect system
/// - Full middleware stack support
/// - Proper context management
/// - Centralized metrics and observability
/// - Error handling and resilience
pub struct AuraHandlerAdapter {
    /// Unified effect system instance
    effect_system: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this adapter
    device_id: DeviceId,
    /// Shared context for choreographic operations
    context: Arc<RwLock<AuraContext>>,
}

impl AuraHandlerAdapter {
    /// Create a new unified Aura handler adapter
    pub fn new(effect_system: AuraEffectSystem, device_id: DeviceId) -> Self {
        let context = Arc::new(RwLock::new(AuraContext::for_testing(device_id)));
        
        Self {
            effect_system: Arc::new(RwLock::new(effect_system)),
            device_id,
            context,
        }
    }

    /// Create adapter from existing effect system and context
    pub fn with_context(
        effect_system: AuraEffectSystem,
        device_id: DeviceId,
        context: AuraContext,
    ) -> Self {
        Self {
            effect_system: Arc::new(RwLock::new(effect_system)),
            device_id,
            context: Arc::new(RwLock::new(context)),
        }
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get read access to the effect system
    pub async fn effect_system(&self) -> tokio::sync::RwLockReadGuard<'_, AuraEffectSystem> {
        self.effect_system.read().await
    }

    /// Get write access to the effect system
    pub async fn effect_system_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, AuraEffectSystem> {
        self.effect_system.write().await
    }

    /// Get read access to the context
    pub async fn context(&self) -> tokio::sync::RwLockReadGuard<'_, AuraContext> {
        self.context.read().await
    }

    /// Get write access to the context
    pub async fn context_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, AuraContext> {
        self.context.write().await
    }

    /// Execute a network send effect
    async fn execute_send_effect<M: Serialize + Send + Sync>(
        &mut self,
        to: ChoreographicRole,
        msg: &M,
    ) -> Result<(), ChoreographyError> {
        // Serialize the message
        let serialized = bincode::serialize(msg).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Message serialization failed: {}", e))
        })?;

        // Create network send effect
        let send_params = aura_protocol::effects::network::NetworkSendParams {
            peer_id: to.device_id,
            data: serialized,
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize parameters
        let param_bytes = bincode::serialize(&send_params)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Parameter serialization failed: {}", e)))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Network, "send", &param_bytes, &mut ctx).await
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Network send failed: {}", e)))?;

        // Emit choreography event
        self.emit_choreography_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(self.device_id.into(), 0),
            to: ChoreographicRole::new(to.device_id, 0),
            message_type: std::any::type_name::<M>().to_string(),
        }, &mut ctx, &mut system).await?;

        Ok(())
    }

    /// Execute a network receive effect
    async fn execute_receive_effect<M: DeserializeOwned + Send>(
        &mut self,
        from: ChoreographicRole,
    ) -> Result<M, ChoreographyError> {
        // Create network receive effect
        let receive_params = aura_protocol::effects::network::NetworkReceiveParams {
            expected_peer: Some(from.device_id),
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize parameters
        let param_bytes = bincode::serialize(&receive_params)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Parameter serialization failed: {}", e)))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        let result_bytes = system.execute_effect(EffectType::Network, "receive", &param_bytes, &mut ctx).await
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Network receive failed: {}", e)))?;
        
        let result: aura_protocol::effects::network::NetworkReceiveResult = bincode::deserialize(&result_bytes)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Result deserialization failed: {}", e)))?;

        // Verify sender matches expected
        if result.sender_id != from.device_id {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Expected message from {:?}, got {:?}",
                from.device_id, result.sender_id
            )));
        }

        // Deserialize the message
        let msg = bincode::deserialize(&result.data).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Message deserialization failed: {}", e))
        })?;

        // Emit choreography event for received message
        self.emit_choreography_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(from.device_id, 0),
            to: ChoreographicRole::new(self.device_id.into(), 0),
            message_type: format!("received_{}", std::any::type_name::<M>()),
        }, &mut ctx, &mut system).await?;

        Ok(msg)
    }

    /// Execute a network broadcast effect
    async fn execute_broadcast_effect(&mut self, data: Vec<u8>) -> Result<(), ChoreographyError> {
        // Create network broadcast effect
        let broadcast_params = aura_protocol::effects::network::NetworkBroadcastParams {
            data,
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize parameters
        let param_bytes = bincode::serialize(&broadcast_params)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Parameter serialization failed: {}", e)))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Network, "broadcast", &param_bytes, &mut ctx).await
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Network broadcast failed: {}", e)))?;

        // Emit choreography event for broadcast
        self.emit_choreography_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(self.device_id.into(), 0),
            to: ChoreographicRole::new(uuid::Uuid::nil(), 999), // Special broadcast role
            message_type: "broadcast".to_string(),
        }, &mut ctx, &mut system).await?;

        Ok(())
    }

    /// Emit a choreography event through the effect system
    async fn emit_choreography_event(
        &self,
        event: ChoreographyEvent,
        ctx: &mut AuraContext,
        system: &mut AuraEffectSystem,
    ) -> Result<(), ChoreographyError> {
        // Serialize event directly
        let param_bytes = bincode::serialize(&event)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Event parameter serialization failed: {}", e)))?;

        system.execute_effect(EffectType::Choreographic, "emit_event", &param_bytes, ctx).await
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Event emission failed: {}", e)))?;

        Ok(())
    }

    /// Log a message through the console effect
    async fn log_info(&mut self, message: &str) -> Result<(), ChoreographyError> {
        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: message.to_string(),
            component: Some("choreography".to_string()),
        };

        // Serialize log parameters
        let param_bytes = bincode::serialize(&log_params)
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Log parameter serialization failed: {}", e)))?;

        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Console, "log", &param_bytes, &mut ctx).await
            .map_err(|e| ChoreographyError::ProtocolViolation(format!("Logging failed: {}", e)))?;

        Ok(())
    }
}

#[async_trait]
impl ChoreoHandler for AuraHandlerAdapter {
    type Role = ChoreographicRole;
    type Endpoint = AuraEffectEndpoint;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        ep: &mut Self::Endpoint,
        to: Self::Role,
        msg: &M,
    ) -> Result<(), ChoreographyError> {
        // Log the send operation
        self.log_info(&format!(
            "Sending message from {} to {}",
            ep.my_role.device_id, to.device_id
        )).await?;

        // Execute the send through unified effect system
        self.execute_send_effect(to, msg).await
    }

    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<M, ChoreographyError> {
        // Log the receive operation
        self.log_info(&format!(
            "Receiving message at {} from {}",
            ep.my_role.device_id, from.device_id
        )).await?;

        // Execute the receive through unified effect system
        self.execute_receive_effect(from).await
    }

    async fn choose(
        &mut self,
        ep: &mut Self::Endpoint,
        who: Self::Role,
        label: Label,
    ) -> Result<(), ChoreographyError> {
        // Log the choice
        self.log_info(&format!(
            "Role {} choosing label: {}",
            who.device_id, label.0
        )).await?;

        // Broadcast the label as bytes
        let label_data = label.0.as_bytes().to_vec();
        self.execute_broadcast_effect(label_data).await
    }

    async fn offer(
        &mut self,
        ep: &mut Self::Endpoint,
        from: Self::Role,
    ) -> Result<Label, ChoreographyError> {
        // Log waiting for offer
        self.log_info(&format!("Waiting for label from {}", from.device_id)).await?;

        // Receive the label data
        let result: aura_protocol::effects::network::NetworkReceiveResult = self.execute_receive_effect(from).await?;

        // Verify sender
        if result.sender_id != from.device_id {
            return Err(ChoreographyError::ProtocolViolation(format!(
                "Expected label from {:?}, got {:?}",
                from.device_id, result.sender_id
            )));
        }

        // Convert bytes to label string
        let label_str = String::from_utf8(result.data).map_err(|e| {
            ChoreographyError::ProtocolViolation(format!("Invalid label UTF-8: {}", e))
        })?;

        // Leak the string to get a 'static str (required by Label)
        let static_label: &'static str = Box::leak(label_str.into_boxed_str());
        Ok(Label(static_label))
    }

    async fn with_timeout<F, T>(
        &mut self,
        _ep: &mut Self::Endpoint,
        _at: Self::Role,
        dur: Duration,
        body: F,
    ) -> Result<T, ChoreographyError>
    where
        F: std::future::Future<Output = Result<T, ChoreographyError>> + Send,
    {
        // Use tokio's timeout for proper timeout implementation
        tokio::time::timeout(dur, body)
            .await
            .map_err(|_| ChoreographyError::ProtocolViolation("Operation timed out".to_string()))?
    }
}

/// Convert AuraHandlerError to ChoreographyError - using function instead of orphan impl
fn aura_error_to_choreography_error(error: AuraHandlerError) -> ChoreographyError {
    ChoreographyError::ProtocolViolation(format!("Aura handler error: {}", error))
}

/// Factory functions for creating choreography adapters

/// Create a choreography adapter for production use
pub fn create_production_adapter(device_id: DeviceId) -> AuraHandlerAdapter {
    let effect_system = AuraEffectSystem::for_production(device_id);
    AuraHandlerAdapter::new(effect_system, device_id)
}

/// Create a choreography adapter for testing
pub fn create_testing_adapter(device_id: DeviceId) -> AuraHandlerAdapter {
    let effect_system = AuraEffectSystem::for_testing(device_id);
    AuraHandlerAdapter::new(effect_system, device_id)
}

/// Create a choreography adapter for simulation
pub fn create_simulation_adapter(device_id: DeviceId, seed: u64) -> AuraHandlerAdapter {
    let effect_system = AuraEffectSystem::for_simulation(device_id, seed);
    AuraHandlerAdapter::new(effect_system, device_id)
}

/// Create an endpoint for choreography execution
pub fn create_choreography_endpoint(
    device_id: DeviceId,
    role: ChoreographicRole,
    context: Arc<RwLock<AuraContext>>,
) -> AuraEffectEndpoint {
    AuraEffectEndpoint::new(device_id, role, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_adapter_creation() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let adapter = create_testing_adapter(device_id);
        
        assert_eq!(adapter.device_id(), device_id);
    }

    #[tokio::test]
    async fn test_endpoint_creation() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let role = ChoreographicRole::new(device_id, 0);
        let context = Arc::new(RwLock::new(AuraContext::for_testing(device_id)));
        
        let endpoint = create_choreography_endpoint(device_id, role, context);
        
        assert_eq!(endpoint.device_id(), device_id);
        assert_eq!(endpoint.my_role(), role);
    }

    #[tokio::test]
    async fn test_context_access() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let adapter = create_testing_adapter(device_id);
        
        // Test read access
        {
            let ctx = adapter.context().await;
            assert_eq!(ctx.device_id, device_id);
        }
        
        // Test write access
        {
            let mut ctx = adapter.context_mut().await;
            ctx.session_id = Some(uuid::Uuid::new_v4().into());
        }
    }
}