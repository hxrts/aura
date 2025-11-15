//! Context propagation middleware and utilities
//!
//! This module provides middleware for automatic context propagation
//! through async call chains, including integration with tokio tasks
//! and distributed tracing.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::task::JoinHandle;
use tracing::{Instrument, Span};

use super::context::{EffectContext, WithContext};

/// Task-local storage for effect context
tokio::task_local! {
    static TASK_CONTEXT: EffectContext;
}

/// Get the current task-local context
pub async fn current_context() -> Option<EffectContext> {
    TASK_CONTEXT.try_with(|ctx| ctx.clone()).ok()
}

/// Set the task-local context
pub async fn set_context(context: EffectContext) -> Result<(), EffectContext> {
    TASK_CONTEXT.set(context)
}

/// Run a future with a specific context
pub async fn with_context<F, T>(context: EffectContext, future: F) -> T
where
    F: Future<Output = T>,
{
    TASK_CONTEXT
        .scope(context.clone(), async move {
            let span = context.span();
            future.instrument(span).await
        })
        .await
}

/// Spawn a task with context propagation
pub fn spawn_with_context<F>(context: EffectContext, future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    let span = context.span();

    tokio::spawn(async move { TASK_CONTEXT.scope(context, future.instrument(span)).await })
}

/// Context propagation wrapper for futures
pub struct PropagatingFuture<F> {
    inner: Pin<Box<F>>,
    context: Option<EffectContext>,
}

impl<F> PropagatingFuture<F> {
    /// Create a new propagating future
    pub fn new(future: F) -> Self
    where
        F: Future,
    {
        Self {
            inner: Box::pin(future),
            context: None,
        }
    }

    /// Create with explicit context
    pub fn with_context(future: F, context: EffectContext) -> Self
    where
        F: Future,
    {
        Self {
            inner: Box::pin(future),
            context: Some(context),
        }
    }
}

impl<F> Future for PropagatingFuture<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Get context from task-local or use stored context
        let context = self
            .context
            .clone()
            .or_else(|| TASK_CONTEXT.try_with(|ctx| ctx.clone()).ok());

        if let Some(ctx) = context {
            // Enter the context span for this poll
            let _guard = ctx.span().entered();
            self.inner.as_mut().poll(cx)
        } else {
            // No context available, poll directly
            self.inner.as_mut().poll(cx)
        }
    }
}

/// Extension trait for automatic context propagation
pub trait PropagateContext: Future + Sized {
    /// Propagate the current context through this future
    fn propagate_context(self) -> PropagatingFuture<Self> {
        PropagatingFuture::new(self)
    }

    /// Propagate a specific context through this future
    fn with_propagated_context(self, context: EffectContext) -> PropagatingFuture<Self> {
        PropagatingFuture::with_context(self, context)
    }
}

impl<F: Future> PropagateContext for F {}

/// Context propagation guard for synchronous code blocks
pub struct ContextGuard {
    previous: Option<EffectContext>,
    span_guard: Option<tracing::span::Entered<'static>>,
}

impl ContextGuard {
    /// Enter a context for the duration of this guard
    pub fn enter(context: EffectContext) -> Self {
        let previous = super::context::thread_local::current();
        super::context::thread_local::set(context.clone());

        // Create and enter the tracing span
        let span = context.span();
        let span_guard = Some(span.entered());

        Self {
            previous,
            span_guard,
        }
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        // Exit the span
        drop(self.span_guard.take());

        // Restore previous context
        match &self.previous {
            Some(ctx) => super::context::thread_local::set(ctx.clone()),
            None => super::context::thread_local::clear(),
        }
    }
}

// Middleware pattern removed - migrated to explicit context propagation
// 
// **MIGRATION NOTE**: ContextMiddleware wrapper pattern has been removed in favor of
// explicit context propagation using the context utilities in this module.
//
// Instead of wrapping handlers, use:
// - `with_context()` to run operations with explicit context
// - `spawn_with_context()` to spawn tasks with context propagation
// - Direct calls to context utilities for explicit control
//
// This provides cleaner Layer 4 orchestration without hidden wrapper patterns.

/// Batch context for executing multiple operations
pub struct BatchContext {
    contexts: Vec<EffectContext>,
    results: Vec<Box<dyn std::any::Any + Send>>,
}

impl BatchContext {
    /// Create a new batch context
    pub fn new() -> Self {
        Self {
            contexts: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Add an operation to the batch
    pub fn add(&mut self, context: EffectContext) {
        self.contexts.push(context);
    }

    /// Execute all operations in parallel with their contexts
    pub async fn execute_all<F, T>(&mut self, operations: Vec<F>) -> Vec<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let mut handles = Vec::new();

        for (op, ctx) in operations.into_iter().zip(&self.contexts) {
            let handle = spawn_with_context(ctx.clone(), op);
            handles.push(handle);
        }

        let mut results = Vec::new();
        for handle in handles {
            if let Ok(result) = handle.await {
                results.push(result);
            }
        }

        results
    }
}

/// Create a context scope for testing
#[cfg(test)]
pub async fn test_context_scope<F, T>(context: EffectContext, f: F) -> T
where
    F: Future<Output = T>,
{
    with_context(context, f).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{AuraResult, DeviceId};
    use aura_testkit::{aura_test, TestFixture};

    #[aura_test]
    async fn test_context_propagation() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let device_id = fixture.device_id();
        let context = EffectContext::new(device_id).with_metadata("test", "value");

        let result = with_context(context.clone(), async {
            // Context should be available here
            let current = current_context().await;
            assert!(current.is_some());

            let ctx = current.unwrap();
            assert_eq!(ctx.device_id, device_id);
            assert_eq!(ctx.metadata.get("test"), Some(&"value".to_string()));

            42
        })
        .await;

        assert_eq!(result, 42);
        Ok(())
    }

    #[aura_test]
    async fn test_spawn_with_context() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let context = EffectContext::new(fixture.device_id());

        let handle = spawn_with_context(context.clone(), async {
            // Context should be propagated to spawned task
            let current = current_context().await;
            assert!(current.is_some());
            "spawned"
        });

        let result = handle.await.unwrap();
        assert_eq!(result, "spawned");
        Ok(())
    }

    #[aura_test]
    async fn test_propagating_future() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let context = EffectContext::new(fixture.device_id());

        let future = async { current_context().await };

        let propagated = future.with_propagated_context(context.clone());
        let result = propagated.await;

        assert!(result.is_some());
        Ok(())
    }

    #[test]
    fn test_context_guard() {
        let context = EffectContext::new(DeviceId::new());

        {
            let _guard = ContextGuard::enter(context.clone());
            let current = super::super::context::thread_local::current();
            assert!(current.is_some());
        }

        // Context should be cleared after guard drops
        let current = super::super::context::thread_local::current();
        assert!(current.is_none());
    }

    #[aura_test]
    async fn test_batch_context() -> AuraResult<()> {
        let fixture = TestFixture::new().await?;
        let mut batch = BatchContext::new();

        for i in 0..3 {
            let context = EffectContext::new(fixture.create_device_id())
                .with_metadata("index", i.to_string());
            batch.add(context);
        }

        let operations = vec![async { 1 }, async { 2 }, async { 3 }];

        let results = batch.execute_all(operations).await;
        assert_eq!(results, vec![1, 2, 3]);
        Ok(())
    }
}
