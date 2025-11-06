//! Unified session handler adapter for the new AuraEffectSystem
//!
//! This module provides a SessionHandler implementation that uses the unified
//! AuraEffectSystem instead of the legacy fragmented effect handlers.
//!
//! # Architecture
//!
//! ```text
//! LocalSessionType (canonical algebra)
//!     ↓ execute via
//! SessionHandler (polymorphic interface) ← UnifiedSessionAdapter (THIS MODULE)
//!     ↓ delegates to
//! AuraEffectSystem (unified effects)
//!     ↓ routes through
//! MiddlewareStack → Effect Registry → Concrete Handlers
//! ```

use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use aura_protocol::effects::system::AuraEffectSystem;
use aura_types::{
    handlers::{AuraHandler, AuraHandlerError, EffectType, context::AuraContext},
    sessions::LocalSessionType,
    identifiers::DeviceId,
    effects::choreographic::{ChoreographicRole, ChoreographyEvent},
};

/// Temporary Label type until proper session types are implemented
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    value: String,
}

impl Label {
    /// Create a new label
    pub fn new(value: String) -> Self {
        Self { value }
    }
    
    /// Get the label as a string
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// Temporary SessionHandler trait until proper session types are implemented
#[async_trait]
pub trait SessionHandler {
    /// The role type for choreographic participants
    type Role: Send + Sync;
    /// The error type for session operations
    type Error: Send + Sync;
    
    /// Send a message to a specific role
    async fn send<M: Serialize + Send + Sync>(&mut self, to: Self::Role, msg: M) -> Result<(), Self::Error>;
    /// Receive a message from a specific role
    async fn recv<M: DeserializeOwned + Send>(&mut self, from: Self::Role) -> Result<M, Self::Error>;
    /// Select a label for session choice
    async fn select(&mut self, to: Self::Role, label: Label) -> Result<(), Self::Error>;
    /// Offer labels for session choice
    async fn offer(&mut self, from: Self::Role) -> Result<Label, Self::Error>;
}

/// Unified session handler adapter using AuraEffectSystem
///
/// This adapter implements SessionHandler using the unified AuraEffectSystem,
/// providing access to all effect types through the middleware stack.
///
/// Key features:
/// - Uses unified AuraEffectSystem for all operations
/// - Proper context flow with AuraContext
/// - Full middleware stack support
/// - Centralized metrics and error handling
/// - Support for simulation, testing, and production modes
pub struct UnifiedSessionAdapter {
    /// Unified effect system instance
    effect_system: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this adapter
    device_id: DeviceId,
    /// Shared context for session operations
    context: Arc<RwLock<AuraContext>>,
}

impl UnifiedSessionAdapter {
    /// Create a new unified session adapter
    pub fn new(effect_system: AuraEffectSystem, device_id: DeviceId) -> Self {
        let context = Arc::new(RwLock::new(AuraContext::for_testing(device_id)));
        
        Self {
            effect_system: Arc::new(RwLock::new(effect_system)),
            device_id,
            context,
        }
    }

    /// Create adapter with existing context
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

    /// Execute a session type directly through the unified system
    pub async fn execute_session(
        &mut self,
        session: LocalSessionType,
    ) -> Result<(), SessionHandlerError> {
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_session(session, &mut ctx).await
            .map_err(SessionHandlerError::from)
    }

    /// Log a message through the console effect
    async fn log_info(&self, message: &str) -> Result<(), SessionHandlerError> {
        let log_params = aura_protocol::effects::console::ConsoleLogParams {
            level: aura_protocol::effects::console::LogLevel::Info,
            message: message.to_string(),
            component: Some("session".to_string()),
        };

        // Serialize log parameters
        let param_bytes = bincode::serialize(&log_params)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Console, "log", &param_bytes, &mut ctx).await
            .map_err(SessionHandlerError::from)?;

        Ok(())
    }

    /// Emit a choreography event
    async fn emit_event(&self, event: ChoreographyEvent) -> Result<(), SessionHandlerError> {
        // Serialize event directly
        let param_bytes = bincode::serialize(&event)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Choreographic, "emit_event", &param_bytes, &mut ctx).await
            .map_err(SessionHandlerError::from)?;

        Ok(())
    }
}

/// Error type for unified session handler operations
#[derive(Debug, thiserror::Error)]
pub enum SessionHandlerError {
    /// Serialization failed
    #[error("Serialization failed: {0}")]
    SerializationError(String),

    /// Deserialization failed
    #[error("Deserialization failed: {0}")]
    DeserializationError(String),

    /// Effect creation failed
    #[error("Effect creation failed: {0}")]
    EffectCreationFailed(String),

    /// Effect execution failed
    #[error("Effect execution failed: {0}")]
    EffectExecutionFailed(String),

    /// Network operation failed
    #[error("Network operation failed: {0}")]
    NetworkError(String),

    /// Invalid role or label
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    /// Aura handler error
    #[error("Aura handler error: {0}")]
    AuraHandlerError(#[from] AuraHandlerError),
}

impl From<bincode::Error> for SessionHandlerError {
    fn from(e: bincode::Error) -> Self {
        Self::SerializationError(e.to_string())
    }
}

#[async_trait]
impl SessionHandler for UnifiedSessionAdapter {
    type Role = ChoreographicRole;
    type Error = SessionHandlerError;

    async fn send<M: Serialize + Send + Sync>(
        &mut self,
        to: Self::Role,
        msg: M,
    ) -> Result<(), Self::Error> {
        // Log the send operation
        self.log_info(&format!(
            "Sending message from {} to {}",
            self.device_id, to.device_id
        )).await?;

        // Serialize message to bytes
        let bytes = bincode::serialize(&msg)?;

        // Create network send effect
        let send_params = aura_protocol::effects::network::NetworkSendParams {
            peer_id: to.device_id,
            data: bytes,
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize send parameters
        let param_bytes = bincode::serialize(&send_params)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Network, "send", &param_bytes, &mut ctx).await
            .map_err(|e| SessionHandlerError::NetworkError(e.to_string()))?;

        // Emit choreography event
        self.emit_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(self.device_id.into(), 0),
            to: ChoreographicRole::new(to.device_id, 0),
            message_type: std::any::type_name::<M>().to_string(),
        }).await?;

        Ok(())
    }

    async fn recv<M: DeserializeOwned + Send>(
        &mut self,
        from: Self::Role,
    ) -> Result<M, Self::Error> {
        // Log the receive operation
        self.log_info(&format!(
            "Receiving message at {} from {}",
            self.device_id, from.device_id
        )).await?;

        // Create network receive effect
        let receive_params = aura_protocol::effects::network::NetworkReceiveParams {
            expected_peer: Some(from.device_id),
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize receive parameters
        let param_bytes = bincode::serialize(&receive_params)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        let result_bytes = system.execute_effect(EffectType::Network, "receive", &param_bytes, &mut ctx).await
            .map_err(|e| SessionHandlerError::NetworkError(e.to_string()))?;
        
        let result: aura_protocol::effects::network::NetworkReceiveResult = bincode::deserialize(&result_bytes)
            .map_err(|e| SessionHandlerError::DeserializationError(e.to_string()))?;

        // Verify sender matches expected
        if result.sender_id != from.device_id {
            return Err(SessionHandlerError::InvalidOperation(format!(
                "Expected message from {:?}, got {:?}",
                from.device_id, result.sender_id
            )));
        }

        // Deserialize the message
        let msg: M = bincode::deserialize(&result.data)
            .map_err(|e| SessionHandlerError::DeserializationError(e.to_string()))?;

        // Emit choreography event for received message
        self.emit_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(from.device_id, 0),
            to: ChoreographicRole::new(self.device_id.into(), 0),
            message_type: format!("received_{}", std::any::type_name::<M>()),
        }).await?;

        Ok(msg)
    }

    async fn select(&mut self, to: Self::Role, label: Label) -> Result<(), Self::Error> {
        // Log the select operation
        self.log_info(&format!(
            "Selecting label '{}' to role {}",
            label.as_str(), to.device_id
        )).await?;

        // Send label as a string message
        let label_bytes = label.as_str().as_bytes().to_vec();

        // Create network send effect for label
        let send_params = aura_protocol::effects::network::NetworkSendParams {
            peer_id: to.device_id,
            data: label_bytes,
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize send parameters for label
        let param_bytes = bincode::serialize(&send_params)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        system.execute_effect(EffectType::Network, "send", &param_bytes, &mut ctx).await
            .map_err(|e| SessionHandlerError::NetworkError(e.to_string()))?;

        // Emit choreography event for label selection
        self.emit_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(self.device_id.into(), 0),
            to: ChoreographicRole::new(to.device_id, 0),
            message_type: format!("label_selected:{}", label.as_str()),
        }).await?;

        Ok(())
    }

    async fn offer(&mut self, from: Self::Role) -> Result<Label, Self::Error> {
        // Log waiting for offer
        self.log_info(&format!("Waiting for label from {}", from.device_id)).await?;

        // Create network receive effect
        let receive_params = aura_protocol::effects::network::NetworkReceiveParams {
            expected_peer: Some(from.device_id),
            timeout: Some(Duration::from_secs(30)),
        };

        // Serialize receive parameters for label
        let param_bytes = bincode::serialize(&receive_params)
            .map_err(|e| SessionHandlerError::EffectCreationFailed(e.to_string()))?;

        // Execute through unified effect system
        let mut system = self.effect_system.write().await;
        let mut ctx = self.context.write().await;
        
        let result_bytes = system.execute_effect(EffectType::Network, "receive", &param_bytes, &mut ctx).await
            .map_err(|e| SessionHandlerError::NetworkError(e.to_string()))?;
        
        let result: aura_protocol::effects::network::NetworkReceiveResult = bincode::deserialize(&result_bytes)
            .map_err(|e| SessionHandlerError::DeserializationError(e.to_string()))?;

        // Verify sender matches expected
        if result.sender_id != from.device_id {
            return Err(SessionHandlerError::InvalidOperation(format!(
                "Expected label from {:?}, got {:?}",
                from.device_id, result.sender_id
            )));
        }

        // Convert bytes to label string
        let label_str = String::from_utf8(result.data)
            .map_err(|e| SessionHandlerError::DeserializationError(e.to_string()))?;

        let label = Label::new(label_str.clone());

        // Emit choreography event for received label
        self.emit_event(ChoreographyEvent::MessageSent {
            from: ChoreographicRole::new(from.device_id, 0),
            to: ChoreographicRole::new(self.device_id.into(), 0),
            message_type: format!("label_received:{}", label_str),
        }).await?;

        Ok(label)
    }
}

/// Factory functions for creating unified session adapters

/// Create a session adapter for production use
pub fn create_production_session_adapter(device_id: DeviceId) -> UnifiedSessionAdapter {
    let effect_system = AuraEffectSystem::for_production(device_id);
    UnifiedSessionAdapter::new(effect_system, device_id)
}

/// Create a session adapter for testing
pub fn create_testing_session_adapter(device_id: DeviceId) -> UnifiedSessionAdapter {
    let effect_system = AuraEffectSystem::for_testing(device_id);
    UnifiedSessionAdapter::new(effect_system, device_id)
}

/// Create a session adapter for simulation
pub fn create_simulation_session_adapter(device_id: DeviceId, seed: u64) -> UnifiedSessionAdapter {
    let effect_system = AuraEffectSystem::for_simulation(device_id, seed);
    UnifiedSessionAdapter::new(effect_system, device_id)
}

/// Create a session adapter from an existing AuraEffectSystem
pub fn create_session_adapter_from_system(
    effect_system: AuraEffectSystem,
    device_id: DeviceId,
) -> UnifiedSessionAdapter {
    UnifiedSessionAdapter::new(effect_system, device_id)
}

/// Create a session adapter with custom context
pub fn create_session_adapter_with_context(
    effect_system: AuraEffectSystem,
    device_id: DeviceId,
    context: AuraContext,
) -> UnifiedSessionAdapter {
    UnifiedSessionAdapter::with_context(effect_system, device_id, context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_unified_adapter_creation() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let adapter = create_testing_session_adapter(device_id);
        
        assert_eq!(adapter.device_id(), device_id);
    }

    #[tokio::test]
    async fn test_context_access() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let adapter = create_testing_session_adapter(device_id);
        
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

    #[tokio::test]
    async fn test_factory_functions() {
        let device_id = DeviceId::from(Uuid::new_v4());
        
        // Test testing adapter
        let test_adapter = create_testing_session_adapter(device_id);
        assert_eq!(test_adapter.device_id(), device_id);
        
        // Test simulation adapter
        let sim_adapter = create_simulation_session_adapter(device_id, 42);
        assert_eq!(sim_adapter.device_id(), device_id);
        
        // Test custom system adapter
        let system = AuraEffectSystem::for_testing(device_id);
        let custom_adapter = create_session_adapter_from_system(system, device_id);
        assert_eq!(custom_adapter.device_id(), device_id);
    }

    #[test]
    fn test_error_conversions() {
        let bincode_error = bincode::Error::from(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "test error"
        ));
        let session_error = SessionHandlerError::from(bincode_error);
        
        match session_error {
            SessionHandlerError::SerializationError(_) => {},
            _ => panic!("Expected SerializationError"),
        }
    }
}