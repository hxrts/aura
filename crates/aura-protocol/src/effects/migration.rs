//! Migration utilities for transitioning to context-aware effects
//!
//! This module provides tools and patterns for gradually migrating from
//! ambient context to explicit context propagation.

use std::sync::Arc;

use async_trait::async_trait;

use aura_core::{
    AuraResult, AuraError, DeviceId, FlowBudget,
    effects::{NetworkEffects, NetworkError, StorageEffects, StorageError,
             CryptoEffects, TimeEffects, TimeError},
};

use super::context::{EffectContext, thread_local};
use super::contextual::{ContextualNetworkEffects, ContextualStorageEffects,
                        ContextualCryptoEffects, ContextualTimeEffects};

/// Migration adapter that bridges between old and new effect interfaces
pub struct MigrationAdapter<T> {
    inner: T,
    device_id: DeviceId,
}

impl<T> MigrationAdapter<T> {
    /// Create a new migration adapter
    pub fn new(inner: T, device_id: DeviceId) -> Self {
        Self { inner, device_id }
    }

    /// Get or create context for the operation
    fn get_context(&self) -> EffectContext {
        thread_local::current()
            .unwrap_or_else(|| EffectContext::new(self.device_id))
    }
}

/// Implement contextual traits by getting context from thread-local
#[async_trait]
impl<T: NetworkEffects> ContextualNetworkEffects for MigrationAdapter<T> {
    async fn send_to_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        // Update thread-local context
        thread_local::set(ctx.clone());
        
        // Call the non-contextual method
        let result = self.inner.send_to_peer(peer_id, message).await;
        
        // Charge flow if successful
        if result.is_ok() {
            let _ = ctx.charge_flow(10); // Ignore error for compatibility
        }
        
        result
    }

    async fn recv_from_peer(
        &self,
        ctx: &mut EffectContext,
        peer_id: DeviceId,
    ) -> Result<Vec<u8>, NetworkError> {
        thread_local::set(ctx.clone());
        
        let result = self.inner.recv_from_peer(peer_id).await;
        
        if result.is_ok() {
            let _ = ctx.charge_flow(10);
        }
        
        result
    }

    async fn broadcast(
        &self,
        ctx: &mut EffectContext,
        message: Vec<u8>,
    ) -> Result<(), NetworkError> {
        thread_local::set(ctx.clone());
        
        let result = self.inner.broadcast(message).await;
        
        if result.is_ok() {
            let _ = ctx.charge_flow(50);
        }
        
        result
    }
}

/// Dual-mode handler that supports both old and new interfaces
pub struct DualModeHandler<O, N> {
    old_handler: O,
    new_handler: N,
    use_contextual: bool,
}

impl<O, N> DualModeHandler<O, N> {
    /// Create a handler that uses the old interface
    pub fn old(handler: O) -> Self
    where
        N: Default,
    {
        Self {
            old_handler: handler,
            new_handler: N::default(),
            use_contextual: false,
        }
    }

    /// Create a handler that uses the new interface
    pub fn new(handler: N) -> Self
    where
        O: Default,
    {
        Self {
            old_handler: O::default(),
            new_handler: handler,
            use_contextual: true,
        }
    }

    /// Create a handler with both interfaces (for gradual migration)
    pub fn dual(old_handler: O, new_handler: N) -> Self {
        Self {
            old_handler,
            new_handler,
            use_contextual: false, // Default to old for compatibility
        }
    }

    /// Switch to using the contextual interface
    pub fn use_contextual(mut self) -> Self {
        self.use_contextual = true;
        self
    }
}

/// Migration helper macros
#[macro_export]
macro_rules! migrate_effect_call {
    ($effects:expr, $method:ident($($args:expr),*)) => {{
        // Try to get context from thread-local
        if let Some(mut ctx) = $crate::effects::context::thread_local::current() {
            // Use contextual version if available
            $effects.$method(&mut ctx, $($args),*).await
        } else {
            // Fall back to non-contextual version
            $effects.$method($($args),*).await
        }
    }};
}

/// Automated migration tool
pub struct MigrationTool {
    device_id: DeviceId,
}

impl MigrationTool {
    /// Create a new migration tool
    pub fn new(device_id: DeviceId) -> Self {
        Self { device_id }
    }

    /// Wrap a handler with migration adapter
    pub fn wrap<T>(&self, handler: T) -> MigrationAdapter<T> {
        MigrationAdapter::new(handler, self.device_id)
    }

    /// Create a context for migration
    pub fn create_context(&self) -> EffectContext {
        EffectContext::new(self.device_id)
            .with_metadata("migration", "true")
    }

    /// Run an operation with migration context
    pub async fn run_with_context<F, T>(&self, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let context = self.create_context();
        super::propagation::with_context(context, f).await
    }
}

/// Migration statistics for tracking progress
#[derive(Default)]
pub struct MigrationStats {
    pub total_calls: u64,
    pub contextual_calls: u64,
    pub non_contextual_calls: u64,
    pub migration_errors: u64,
}

impl MigrationStats {
    /// Record a contextual call
    pub fn record_contextual(&mut self) {
        self.total_calls += 1;
        self.contextual_calls += 1;
    }

    /// Record a non-contextual call
    pub fn record_non_contextual(&mut self) {
        self.total_calls += 1;
        self.non_contextual_calls += 1;
    }

    /// Record a migration error
    pub fn record_error(&mut self) {
        self.migration_errors += 1;
    }

    /// Get the migration completion percentage
    pub fn completion_percentage(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            (self.contextual_calls as f64 / self.total_calls as f64) * 100.0
        }
    }
}

/// Migration guide generator
pub struct MigrationGuide {
    recommendations: Vec<String>,
}

impl MigrationGuide {
    /// Analyze code and generate migration recommendations
    pub fn analyze(stats: &MigrationStats) -> Self {
        let mut recommendations = Vec::new();

        if stats.completion_percentage() < 50.0 {
            recommendations.push(
                "Consider using MigrationAdapter for automatic context propagation".to_string()
            );
        }

        if stats.migration_errors > 0 {
            recommendations.push(
                "Review migration errors - context may be missing in some paths".to_string()
            );
        }

        if stats.non_contextual_calls > stats.contextual_calls {
            recommendations.push(
                "Prioritize migrating high-frequency call sites first".to_string()
            );
        }

        Self { recommendations }
    }

    /// Get migration recommendations
    pub fn recommendations(&self) -> &[String] {
        &self.recommendations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::effects::NetworkEffects;

    struct MockNetworkEffects;

    #[async_trait]
    impl NetworkEffects for MockNetworkEffects {
        async fn send_to_peer(
            &self,
            _peer_id: DeviceId,
            _message: Vec<u8>,
        ) -> Result<(), NetworkError> {
            Ok(())
        }

        async fn recv_from_peer(
            &self,
            _peer_id: DeviceId,
        ) -> Result<Vec<u8>, NetworkError> {
            Ok(vec![])
        }

        async fn broadcast(&self, _message: Vec<u8>) -> Result<(), NetworkError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_migration_adapter() {
        let device_id = DeviceId::new();
        let mock = MockNetworkEffects;
        let adapter = MigrationAdapter::new(mock, device_id);

        let mut ctx = EffectContext::new(device_id)
            .with_flow_budget(FlowBudget::new(100));

        // Should work with contextual interface
        assert!(adapter.send_to_peer(&mut ctx, device_id, vec![]).await.is_ok());
        assert_eq!(ctx.flow_budget.remaining(), 90);
    }

    #[test]
    fn test_migration_stats() {
        let mut stats = MigrationStats::default();
        
        stats.record_contextual();
        stats.record_contextual();
        stats.record_non_contextual();
        
        assert_eq!(stats.total_calls, 3);
        assert_eq!(stats.contextual_calls, 2);
        assert_eq!(stats.completion_percentage(), 66.66666666666667);
    }

    #[test]
    fn test_migration_guide() {
        let mut stats = MigrationStats::default();
        stats.non_contextual_calls = 10;
        stats.contextual_calls = 2;
        stats.total_calls = 12;
        
        let guide = MigrationGuide::analyze(&stats);
        assert!(!guide.recommendations().is_empty());
    }
}