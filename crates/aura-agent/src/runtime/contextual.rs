//! Context-aware versions of effect traits
//!
//! This module provides effect traits that accept explicit context parameters,
//! enabling proper request tracing and context propagation without relying
//! on ambient state.

use async_trait::async_trait;

use aura_core::{
    effects::{NetworkError, StorageError, TimeError},
    AuraResult, DeviceId, AuraError,
};

use super::context::EffectContext;

/// Context-aware network effects
#[async_trait]
pub trait ContextualNetworkEffects: Send + Sync {
    /// Send a message to a peer with context
    async fn send_to_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), NetworkError>;

    /// Receive a message from a peer with context
    async fn recv_from_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
    ) -> Result<Vec<u8>, NetworkError>;

    /// Broadcast a message to all peers with context
    async fn broadcast(
        &self,
        ctx: &mut EffectContext,
        message: Vec<u8>,
    ) -> Result<(), NetworkError>;
}

/// Context-aware storage effects
#[async_trait]
pub trait ContextualStorageEffects: Send + Sync {
    /// Store data with context
    async fn store(
        &self,
        ctx: &mut EffectContext,
        key: &str,
        value: Vec<u8>,
        encrypted: bool,
    ) -> Result<(), StorageError>;

    /// Retrieve data with context
    async fn retrieve(
        &self,
        ctx: &mut EffectContext,
        key: &str,
    ) -> Result<Option<Vec<u8>>, StorageError>;

    /// Delete data with context
    async fn delete(&self, ctx: &mut EffectContext, key: &str) -> Result<(), StorageError>;

    /// List keys with context
    async fn list_keys(
        &self,
        ctx: &mut EffectContext,
        prefix: &str,
    ) -> Result<Vec<String>, StorageError>;
}

/// Context-aware crypto effects
#[async_trait]
pub trait ContextualCryptoEffects: Send + Sync {
    /// Sign a message with context
    async fn ed25519_sign(
        &self,
        ctx: &mut EffectContext,
        message: &[u8],
        private_key: &[u8],
    ) -> Result<Vec<u8>, AuraError>;

    /// Verify a signature with context
    async fn ed25519_verify(
        &self,
        ctx: &mut EffectContext,
        message: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> Result<bool, AuraError>;

    /// Generate random bytes with context
    async fn secure_random(&self, ctx: &mut EffectContext, len: usize) -> Vec<u8>;
}

/// Context-aware time effects
#[async_trait]
pub trait ContextualTimeEffects: Send + Sync {
    /// Get current epoch with context
    async fn current_epoch(&self, ctx: &EffectContext) -> u64;

    /// Get current timestamp with context
    async fn current_timestamp(&self, ctx: &EffectContext) -> u64;

    /// Sleep for a duration with context
    async fn sleep(
        &self,
        ctx: &mut EffectContext,
        duration: std::time::Duration,
    ) -> Result<(), TimeError>;

    /// Set a timeout with context
    async fn timeout<F, T>(
        &self,
        ctx: &mut EffectContext,
        duration: std::time::Duration,
        future: F,
    ) -> Result<T, TimeError>
    where
        F: std::future::Future<Output = T> + Send,
        T: Send;
}

/// Adapter to bridge between contextual and non-contextual traits
pub struct ContextAdapter<T> {
    inner: T,
    default_context: EffectContext,
}

impl<T> ContextAdapter<T> {
    /// Create a new context adapter
    pub fn new(inner: T, device_id: DeviceId) -> Self {
        Self {
            inner,
            default_context: EffectContext::new(device_id),
        }
    }

    /// Get the inner handler
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// Get a mutable reference to the inner handler
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

/// Migration helper to convert non-contextual calls to contextual
pub trait ContextualMigration<T> {
    /// Wrap with a default context
    fn with_default_context(self, device_id: DeviceId) -> ContextAdapter<Self>
    where
        Self: Sized,
    {
        ContextAdapter::new(self, device_id)
    }
}

// Implement for all types
impl<T> ContextualMigration<T> for T {}

/// Context propagation middleware
pub struct ContextPropagator {
    /// Context provider for storing/retrieving context
    provider: Box<dyn super::context::ContextProvider>,
}

impl ContextPropagator {
    /// Create a new context propagator
    pub fn new(provider: Box<dyn super::context::ContextProvider>) -> Self {
        Self { provider }
    }

    /// Run an operation with context propagation
    pub async fn with_context<F, R>(&self, context: EffectContext, operation: F) -> R
    where
        F: FnOnce() -> R,
    {
        let previous = self.provider.current_context();
        self.provider.set_context(context);
        let result = operation();
        if let Some(prev) = previous {
            self.provider.set_context(prev);
        }
        result
    }

    /// Get the current context
    pub fn current(&self) -> Option<EffectContext> {
        self.provider.current_context()
    }

    /// Create a child context for a nested operation
    pub fn child_context(&self) -> Option<EffectContext> {
        self.current().map(|ctx| ctx.child())
    }
}

/// Extension trait for adding context to effect operations
pub trait EffectContextExt {
    /// Run this operation with the given context
    fn in_context(self, ctx: &mut EffectContext) -> ContextualOperation<Self>
    where
        Self: Sized,
    {
        ContextualOperation {
            operation: self,
            context: ctx,
        }
    }
}

impl<T> EffectContextExt for T {}

/// Wrapper for contextual operations
pub struct ContextualOperation<'a, T> {
    operation: T,
    context: &'a mut EffectContext,
}

impl<'a, T> ContextualOperation<'a, T> {
    /// Execute the operation with deadline checking
    pub async fn execute_with_deadline<F, R>(self, f: F) -> AuraResult<R>
    where
        F: FnOnce(T, &mut EffectContext) -> R,
    {
        // Check deadline before execution
        if self.context.is_deadline_exceeded() {
            return Err(AuraError::invalid("Operation deadline exceeded"));
        }

        // Execute with context
        Ok(f(self.operation, self.context))
    }

    /// Execute the operation with flow budget checking
    pub async fn execute_with_flow<F, R>(self, cost: u64, f: F) -> AuraResult<R>
    where
        F: FnOnce(T, &mut EffectContext) -> R,
    {
        // Charge flow budget
        self.context.charge_flow(cost)?;

        // Execute with context
        Ok(f(self.operation, self.context))
    }
}

/// Contextual effect system interface
#[async_trait]
pub trait ContextualEffects:
    ContextualNetworkEffects
    + ContextualStorageEffects
    + ContextualCryptoEffects
    + ContextualTimeEffects
    + Send
    + Sync
{
    /// Get the current context
    fn current_context(&self) -> Option<EffectContext>;

    /// Create a root context for a new operation
    fn create_context(&self, device_id: DeviceId) -> EffectContext {
        EffectContext::new(device_id)
    }

    /// Create a child context
    fn child_context(&self, parent: &EffectContext) -> EffectContext {
        parent.child()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_testkit::{aura_test, TestFixture};

    struct MockContextualEffects {
        device_id: DeviceId,
    }

    #[async_trait]
    impl ContextualNetworkEffects for MockContextualEffects {
        async fn send_to_peer(
            &self,
            ctx: &mut EffectContext,
            _peer_id: DeviceId,
            _message: Vec<u8>,
        ) -> Result<(), NetworkError> {
            // Charge flow for network operation
            ctx.charge_flow(10).map_err(|_| NetworkError::Timeout)?;
            Ok(())
        }

        async fn recv_from_peer(
            &self,
            _ctx: &mut EffectContext,
            _peer_id: DeviceId,
        ) -> Result<Vec<u8>, NetworkError> {
            Ok(vec![])
        }

        async fn broadcast(
            &self,
            ctx: &mut EffectContext,
            _message: Vec<u8>,
        ) -> Result<(), NetworkError> {
            // Charge more for broadcast
            ctx.charge_flow(50).map_err(|_| NetworkError::Timeout)?;
            Ok(())
        }
    }

    #[aura_test]
    async fn test_contextual_effects() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let device_id = fixture.device_id();
        let effects = MockContextualEffects { device_id };

        let mut context = EffectContext::new(device_id).with_flow_budget(FlowBudget::new(100));

        // Test flow budget charging
        effects
            .send_to_peer(&mut context, device_id, vec![])
            .await?;
        assert_eq!(context.flow_budget.remaining(), 90);

        effects.broadcast(&mut context, vec![]).await?;
        assert_eq!(context.flow_budget.remaining(), 40);
        Ok(())
    }

    #[test]
    fn test_context_adapter() {
        let device_id = DeviceId::new();
        let mock = MockContextualEffects { device_id };
        let adapter = mock.with_default_context(device_id);

        assert_eq!(adapter.default_context.device_id, device_id);
    }
}
