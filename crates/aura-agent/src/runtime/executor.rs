//! Effect execution infrastructure
//!
//! Provides execution infrastructure for running effects within the authority-centric
//! runtime, managing execution contexts, and coordinating effect handler invocation.

use aura_core::effects::ExecutionMode;
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;
use futures::future::BoxFuture;
use std::sync::Arc;

// Use the registry module's EffectRegistry (not builder's)
use super::registry::{EffectOperation, EffectRegistry, EffectType};

/// Handler trait for dynamic effect dispatch.
pub trait EffectHandler<T>: Send + Sync {
    fn handle(
        &self,
        context: &super::EffectContext,
        params: T,
    ) -> BoxFuture<'static, Result<EffectResult, AuraError>>;
}

impl<T, F> EffectHandler<T> for F
where
    F: Fn(&super::EffectContext, T) -> BoxFuture<'static, Result<EffectResult, AuraError>>
        + Send
        + Sync,
{
    fn handle(
        &self,
        context: &super::EffectContext,
        params: T,
    ) -> BoxFuture<'static, Result<EffectResult, AuraError>> {
        (self)(context, params)
    }
}

/// Executor for effect operations
#[derive(Debug)]
pub struct EffectExecutor {
    authority_id: AuthorityId,
    execution_mode: ExecutionMode,
    registry: Arc<EffectRegistry>,
}

impl EffectExecutor {
    /// Create a new effect executor
    pub fn new(
        authority_id: AuthorityId,
        execution_mode: ExecutionMode,
        registry: Arc<EffectRegistry>,
    ) -> Self {
        Self {
            authority_id,
            execution_mode,
            registry,
        }
    }

    /// Execute an effect operation
    pub async fn execute<T>(
        &self,
        context: &super::EffectContext,
        effect_type: EffectType,
        operation: EffectOperation,
        params: T,
    ) -> Result<EffectResult, AuraError>
    where
        T: Send + Sync + 'static,
    {
        // Validate context matches executor authority
        if context.authority_id() != self.authority_id {
            return Err(AuraError::invalid("Context authority mismatch".to_string()));
        }

        let handler = self
            .registry
            .get_effect_handler::<T>(effect_type, operation)
            .map_err(|e| AuraError::invalid(e.to_string()))?
            .ok_or_else(|| AuraError::invalid(format!("{}.{}", effect_type, operation)))?;

        handler.handle(context, params).await
    }

    /// Get the authority ID
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get the execution mode
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Get the registry
    pub fn registry(&self) -> Arc<EffectRegistry> {
        self.registry.clone()
    }

    /// Create an execution context
    pub fn create_context(&self, context_id: ContextId) -> super::EffectContext {
        super::EffectContext::new(self.authority_id, context_id, self.execution_mode)
    }

    /// Production constructor
    pub fn production(authority_id: AuthorityId, registry: Arc<EffectRegistry>) -> Self {
        Self::new(authority_id, ExecutionMode::Production, registry)
    }

    /// Testing constructor
    pub fn testing(authority_id: AuthorityId, registry: Arc<EffectRegistry>) -> Self {
        Self::new(authority_id, ExecutionMode::Testing, registry)
    }

    /// Simulation constructor
    pub fn simulation(authority_id: AuthorityId, seed: u64, registry: Arc<EffectRegistry>) -> Self {
        Self::new(authority_id, ExecutionMode::Simulation { seed }, registry)
    }
}

/// Builder for effect executors
#[derive(Debug)]
#[allow(dead_code)] // Part of future effect system API
pub struct EffectExecutorBuilder {
    authority_id: Option<AuthorityId>,
    execution_mode: Option<ExecutionMode>,
    registry: Option<Arc<EffectRegistry>>,
}

impl EffectExecutorBuilder {
    /// Create a new executor builder
    #[allow(dead_code)] // Part of future effect system API
    pub fn new() -> Self {
        Self {
            authority_id: None,
            execution_mode: None,
            registry: None,
        }
    }

    /// Set the authority ID
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_authority(mut self, authority_id: AuthorityId) -> Self {
        self.authority_id = Some(authority_id);
        self
    }

    /// Set the execution mode
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_execution_mode(mut self, mode: ExecutionMode) -> Self {
        self.execution_mode = Some(mode);
        self
    }

    /// Set the effect registry
    #[allow(dead_code)] // Part of future effect system API
    pub fn with_registry(mut self, registry: Arc<EffectRegistry>) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Build the executor
    #[allow(dead_code)] // Part of future effect system API
    pub fn build(self) -> Result<EffectExecutor, AuraError> {
        let authority_id = self
            .authority_id
            .ok_or_else(|| AuraError::invalid("Authority ID required".to_string()))?;

        let execution_mode = self
            .execution_mode
            .ok_or_else(|| AuraError::invalid("Execution mode required".to_string()))?;

        let registry = self
            .registry
            .ok_or_else(|| AuraError::invalid("Registry required".to_string()))?;

        Ok(EffectExecutor::new(authority_id, execution_mode, registry))
    }
}

impl Default for EffectExecutorBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of an effect execution
#[derive(Debug, Clone)]
pub enum EffectResult {
    Success(String),
    Error(String),
    Partial(String),
}

impl EffectResult {
    /// Check if the result represents success
    pub fn is_success(&self) -> bool {
        matches!(self, EffectResult::Success(_))
    }

    /// Check if the result represents an error
    pub fn is_error(&self) -> bool {
        matches!(self, EffectResult::Error(_))
    }

    /// Check if the result represents a partial result
    pub fn is_partial(&self) -> bool {
        matches!(self, EffectResult::Partial(_))
    }

    /// Get the result message
    pub fn message(&self) -> &str {
        match self {
            EffectResult::Success(msg) => msg,
            EffectResult::Error(msg) => msg,
            EffectResult::Partial(msg) => msg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::registry::EffectRegistry;
    use crate::EffectContext;
    use aura_core::effects::ExecutionMode;
    use aura_core::identifiers::{AuthorityId, ContextId};
    use futures::future::BoxFuture;
    use std::sync::Arc;

    fn double_handler(
        _context: &EffectContext,
        value: u64,
    ) -> BoxFuture<'static, Result<EffectResult, AuraError>> {
        Box::pin(async move { Ok(EffectResult::Success(format!("ok:{value}"))) })
    }

    #[tokio::test]
    async fn effect_executor_dispatches_registered_handler() {
        let authority_id = AuthorityId::new_from_entropy([1u8; 32]);
        let context_id = ContextId::new_from_entropy([2u8; 32]);
        let registry = Arc::new(EffectRegistry::new(ExecutionMode::Testing));

        registry
            .register_effect_handler::<u64, _>(
                EffectType::Crypto,
                EffectOperation::from("double"),
                double_handler,
            )
            .expect("register handler");

        let executor = EffectExecutor::testing(authority_id, registry);
        let context = executor.create_context(context_id);

        let result = executor
            .execute(
                &context,
                EffectType::Crypto,
                EffectOperation::from("double"),
                7u64,
            )
            .await
            .expect("execute handler");

        assert!(matches!(result, EffectResult::Success(msg) if msg == "ok:7"));
    }
}
