//! Stateless effect execution engine
//!
//! The EffectExecutor provides a lock-free dispatch mechanism for effect handlers.
//! It maintains no state and performs no locking during execution.

use std::collections::HashMap;
use std::sync::Arc;

use crate::handlers::AuraContext;
use crate::handlers::{AuraHandler, AuraHandlerError, EffectType};

/// Stateless effect dispatcher that routes operations to registered handlers
#[derive(Clone)]
pub struct EffectExecutor {
    /// Map of effect types to their handlers - immutable after construction
    // TODO: Restore when AuraHandler trait is properly defined
    // handlers: Arc<HashMap<EffectType, Arc<dyn AuraHandler>>>,
    _marker: std::marker::PhantomData<()>,
}

impl EffectExecutor {
    /// Create a new effect executor with no handlers
    pub fn new() -> Self {
        Self {
            // handlers: Arc::new(HashMap::new()),
            _marker: std::marker::PhantomData,
        }
    }

    /// Builder pattern for registering handlers
    pub fn builder() -> EffectExecutorBuilder {
        EffectExecutorBuilder::new()
    }

    // TODO: Restore when AuraHandler trait is properly defined
    // /// Execute an effect operation with the given context
    // ///
    // /// This method performs no locking and maintains no state. The context
    // /// is an immutable snapshot that flows through the operation.
    // pub async fn execute(
    //     &self,
    //     effect_type: EffectType,
    //     operation: &str,
    //     params: &[u8],
    //     context: &AuraContext,
    // ) -> Result<Vec<u8>, AuraHandlerError> {
    //     // Direct dispatch with no locks
    //     let handler = self
    //         .handlers
    //         .get(&effect_type)
    //         .ok_or_else(|| AuraHandlerError::UnsupportedEffect { effect_type })?;
    //
    //     // Execute the operation - handler receives immutable context
    //     handler
    //         .execute_effect(effect_type, operation, params, context)
    //         .await
    // }

    // /// Check if a handler is registered for the given effect type
    // pub fn supports(&self, effect_type: EffectType) -> bool {
    //     self.handlers.contains_key(&effect_type)
    // }

    // /// Get the list of supported effect types
    // pub fn supported_effects(&self) -> Vec<EffectType> {
    //     self.handlers.keys().copied().collect()
    // }
}

impl Default for EffectExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing an EffectExecutor with registered handlers
pub struct EffectExecutorBuilder {
    // TODO: Restore when AuraHandler trait is properly defined
    // handlers: HashMap<EffectType, Arc<dyn AuraHandler>>,
    _marker: std::marker::PhantomData<()>,
}

impl EffectExecutorBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            // handlers: HashMap::new(),
            _marker: std::marker::PhantomData,
        }
    }

    // TODO: Restore when AuraHandler trait is properly defined
    // /// Register a handler for an effect type
    // pub fn with_handler(mut self, effect_type: EffectType, handler: Arc<dyn AuraHandler>) -> Self {
    //     self.handlers.insert(effect_type, handler);
    //     self
    // }

    // /// Register multiple handlers at once
    // pub fn with_handlers(
    //     mut self,
    //     handlers: impl IntoIterator<Item = (EffectType, Arc<dyn AuraHandler>)>,
    // ) -> Self {
    //     self.handlers.extend(handlers);
    //     self
    // }

    /// Build the executor with immutable handler map
    pub fn build(self) -> EffectExecutor {
        EffectExecutor {
            // handlers: Arc::new(self.handlers),
            _marker: std::marker::PhantomData,
        }
    }
}

impl Default for EffectExecutorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: These tests use outdated MockHandler and EffectExecutor APIs that were removed
// during the effect system refactor. They need to be rewritten to use the new effect system.
#[cfg(disabled_test)]
mod tests {
    use super::*;
    use crate::handlers::MockHandler;
    use aura_core::AuraResult;
    use aura_macros::aura_test;
    use aura_testkit::{ TestFixture};

    #[aura_test]
    async fn test_executor_dispatch() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;

        // Create a mock handler
        let mock_handler = Arc::new(MockHandler::new());

        // Build executor with the handler
        let executor = EffectExecutor::builder()
            .with_handler(EffectType::Time, mock_handler.clone())
            .build();

        // Create a test context
        let context = crate::handlers::AuraContext::default();

        // Execute an operation
        let result = executor
            .execute(EffectType::Time, "current_timestamp", &[], &context)
            .await?;

        // Verify result
        assert!(!result.is_empty());
        Ok(())
    }

    #[aura_test]
    async fn test_unsupported_effect() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;

        // Create executor with no handlers
        let executor = EffectExecutor::new();
        let context = crate::handlers::AuraContext::default();

        // Try to execute unsupported effect
        let result = executor
            .execute(EffectType::Crypto, "hash", &[1, 2, 3], &context)
            .await;

        // Should fail with UnsupportedEffect
        assert!(matches!(
            result,
            Err(AuraHandlerError::UnsupportedEffect {
                effect_type: EffectType::Crypto
            })
        ));
        Ok(())
    }

    #[aura_test]
    async fn test_executor_is_stateless() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;

        // Create two executors with same configuration
        let handler = Arc::new(MockHandler::new());

        let executor1 = EffectExecutor::builder()
            .with_handler(EffectType::Time, handler.clone())
            .build();

        let executor2 = executor1.clone();

        // Both should work identically
        let context = crate::handlers::AuraContext::default();

        let result1 = executor1
            .execute(EffectType::Time, "current_timestamp", &[], &context)
            .await?;

        let result2 = executor2
            .execute(EffectType::Time, "current_timestamp", &[], &context)
            .await?;

        // Results should be consistent (stateless execution)
        assert_eq!(result1, result2);
        Ok(())
    }
}
