//! Core Aura Effect System Implementation
//!
//! This module provides the main `AuraEffectSystem` implementation that serves
//! as the unified handler for all effect execution and session type interpretation
//! in the Aura platform.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::handlers::{
    AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode, MiddlewareStack,
};
use aura_types::identifiers::DeviceId;
use aura_types::sessions::LocalSessionType;

// Note: The local middleware implementations are incompatible with the new unified architecture
// They will be removed or refactored to use the ErasedHandler interface

/// Main implementation of the Aura Effect System
///
/// This is the primary entry point for all effect execution in Aura. It uses
/// the unified handler architecture from aura-types.
///
/// # Architecture
///
/// ```text
/// AuraEffectSystem
/// ├── MiddlewareStack (unified handler from aura-types)
/// └── AuraContext (unified context flow)
/// ```
pub struct AuraEffectSystem {
    /// The unified middleware stack that handles all operations
    middleware_stack: Arc<RwLock<MiddlewareStack>>,
    /// Current execution context
    context: Arc<RwLock<AuraContext>>,
    /// Device ID for this system
    device_id: DeviceId,
    /// Execution mode
    execution_mode: ExecutionMode,
}

impl AuraEffectSystem {
    /// Create a new effect system with the given device ID and execution mode
    pub fn new(device_id: DeviceId, execution_mode: ExecutionMode) -> Self {
        // Create base context
        let context = match execution_mode {
            ExecutionMode::Testing => AuraContext::for_testing(device_id),
            ExecutionMode::Production => AuraContext::for_production(device_id),
            ExecutionMode::Simulation { seed } => AuraContext::for_simulation(device_id, seed),
        };

        // Build the system
        Self {
            middleware_stack: Arc::new(RwLock::new(MiddlewareStack::new(
                device_id,
                execution_mode,
            ))),
            context: Arc::new(RwLock::new(context)),
            device_id,
            execution_mode,
        }
    }

    /// Get the current execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if this system supports a specific effect type
    pub async fn supports_effect(&self, effect_type: EffectType) -> bool {
        let middleware_stack = self.middleware_stack.read().await;
        middleware_stack.supports_effect(effect_type)
    }

    /// Get all supported effect types
    pub async fn supported_effects(&self) -> Vec<EffectType> {
        let middleware_stack = self.middleware_stack.read().await;
        middleware_stack.supported_effects()
    }

    /// Get the current context (cloned for safety)
    pub async fn context(&self) -> AuraContext {
        let context = self.context.read().await;
        context.clone()
    }

    /// Update the context
    pub async fn update_context<F>(&self, updater: F) -> Result<(), AuraHandlerError>
    where
        F: FnOnce(&mut AuraContext) -> Result<(), AuraHandlerError> + Send,
    {
        let mut context = self.context.write().await;
        updater(&mut *context)
    }

    /// Execute an effect with a custom context
    pub async fn execute_effect_with_context(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        context: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        let mut middleware_stack = self.middleware_stack.write().await;
        middleware_stack
            .execute_effect(effect_type, operation, parameters, context)
            .await
    }

    /// Execute a session type with a custom context
    pub async fn execute_session_with_context(
        &self,
        session: LocalSessionType,
        context: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        let mut middleware_stack = self.middleware_stack.write().await;
        middleware_stack.execute_session(session, context).await
    }

    /// Create a new session context
    pub async fn create_session_context(&self) -> AuraContext {
        let base_context = self.context().await;
        // Create a new session context based on the base
        base_context.clone()
    }

    /// Get system statistics
    pub async fn statistics(&self) -> AuraEffectSystemStats {
        let middleware_stack = self.middleware_stack.read().await;

        AuraEffectSystemStats {
            execution_mode: self.execution_mode,
            device_id: self.device_id,
            registered_effects: middleware_stack.supported_effects().len(),
            total_operations: 0, // TODO: Count operations when implemented
            middleware_count: middleware_stack.middleware_count(),
        }
    }
}

#[async_trait]
impl AuraHandler for AuraEffectSystem {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        self.execute_effect_with_context(effect_type, operation, parameters, ctx)
            .await
    }

    async fn execute_session(
        &mut self,
        session: LocalSessionType,
        ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        self.execute_session_with_context(session, ctx).await
    }

    fn supports_effect(&self, _effect_type: EffectType) -> bool {
        // In practice, we would check our middleware stack capabilities
        // For now, return false for stub implementation
        false
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Factory implementation for creating AuraEffectSystem instances
pub struct AuraEffectSystemFactory;

impl AuraEffectSystemFactory {
    /// Create a handler for the given execution mode
    pub fn create_handler(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
    ) -> Box<dyn AuraHandler> {
        let system = AuraEffectSystem::new(device_id, execution_mode);
        Box::new(system)
    }

    /// Get the supported effect types
    pub fn supported_effect_types() -> Vec<EffectType> {
        vec![
            EffectType::Crypto,
            EffectType::Network,
            EffectType::Storage,
            EffectType::Time,
            EffectType::Console,
            EffectType::Random,
            EffectType::Ledger,
            EffectType::Journal,
            EffectType::Choreographic,
        ]
    }
}

/// Statistics about the effect system
#[derive(Debug, Clone)]
pub struct AuraEffectSystemStats {
    /// Current execution mode
    pub execution_mode: ExecutionMode,
    /// Device ID
    pub device_id: DeviceId,
    /// Number of registered effect types
    pub registered_effects: usize,
    /// Total number of operations across all effects
    pub total_operations: usize,
    /// Number of middleware in the stack
    pub middleware_count: usize,
}

impl AuraEffectSystemStats {
    /// Check if the system is in a deterministic mode
    pub fn is_deterministic(&self) -> bool {
        self.execution_mode.is_deterministic()
    }

    /// Check if the system is in production mode
    pub fn is_production(&self) -> bool {
        self.execution_mode.is_production()
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "AuraEffectSystem({:?}, {} effects, {} ops, {} middleware)",
            self.execution_mode,
            self.registered_effects,
            self.total_operations,
            self.middleware_count
        )
    }
}

/// Convenience functions for creating common effect system configurations
impl AuraEffectSystem {
    /// Create an effect system for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Testing)
    }

    /// Create an effect system for production
    pub fn for_production(device_id: DeviceId) -> Self {
        Self::new(device_id, ExecutionMode::Production)
    }

    /// Create an effect system for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self::new(device_id, ExecutionMode::Simulation { seed })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_effect_system_creation() {
        let device_id = DeviceId::from(Uuid::new_v4());

        // Test testing mode
        let testing_system = AuraEffectSystem::for_testing(device_id);
        assert_eq!(testing_system.execution_mode(), ExecutionMode::Testing);
        assert_eq!(testing_system.device_id(), device_id);

        // Test production mode
        let production_system = AuraEffectSystem::for_production(device_id);
        assert_eq!(
            production_system.execution_mode(),
            ExecutionMode::Production
        );

        // Test simulation mode
        let simulation_system = AuraEffectSystem::for_simulation(device_id, 42);
        assert_eq!(
            simulation_system.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_context_management() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        // Test context retrieval
        let context = system.context().await;
        assert_eq!(context.device_id, device_id);
        assert_eq!(context.execution_mode, ExecutionMode::Testing);

        // Test session context creation
        let session_context = system.create_session_context().await;
        assert_eq!(session_context.device_id, device_id);
    }

    #[tokio::test]
    async fn test_statistics() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        let stats = system.statistics().await;
        assert_eq!(stats.device_id, device_id);
        assert!(stats.is_deterministic());
        assert!(!stats.is_production());

        let summary = stats.summary();
        assert!(summary.contains("AuraEffectSystem"));
        assert!(summary.contains("Testing"));
    }

    #[test]
    fn test_factory() {
        let device_id = DeviceId::from(Uuid::new_v4());

        let handler = AuraEffectSystemFactory::create_handler(device_id, ExecutionMode::Testing);
        assert_eq!(handler.execution_mode(), ExecutionMode::Testing);

        let supported_effects = AuraEffectSystemFactory::supported_effect_types();
        assert!(supported_effects.contains(&EffectType::Crypto));
        assert!(supported_effects.contains(&EffectType::Network));
        assert!(supported_effects.contains(&EffectType::Choreographic));
    }

    #[tokio::test]
    async fn test_context_updates() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let system = AuraEffectSystem::for_testing(device_id);

        // Test context update
        system
            .update_context(|_ctx| {
                // Simplified test - just check that the updater can be called
                Ok(())
            })
            .await
            .unwrap();

        let updated_context = system.context().await;
        assert_eq!(updated_context.device_id, device_id);
    }
}
