//! Runtime-agnostic task spawning traits.

use async_trait::async_trait;
use futures::future::{BoxFuture, LocalBoxFuture};
use std::sync::Arc;

/// Cooperative cancellation token.
#[async_trait]
pub trait CancellationToken: Send + Sync {
    /// Resolves when cancellation is requested.
    async fn cancelled(&self);

    /// Non-blocking cancellation check.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A spawner cannot silently convert required work into a detached unit task.
#[derive(Debug, thiserror::Error)]
pub enum TaskSpawnError {
    /// This adapter has no retained required-task outcome owner.
    #[error("spawner does not support supervised required task {name}")]
    UnsupportedFallible {
        /// Stable name of the rejected required task.
        name: &'static str,
    },
}

/// Task spawning contract for runtime implementations.
pub trait TaskSpawner: Send + Sync {
    /// Spawn a background task.
    fn spawn(&self, fut: BoxFuture<'static, ()>);

    /// Spawn a background task tied to a cancellation token.
    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>);

    /// Spawn a background task that may remain thread-local.
    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>);

    /// Spawn a thread-local background task tied to a cancellation token.
    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    );

    /// Admit required work whose actual failure must remain supervised.
    /// Unsupported adapters drop the supplied future and return a typed error.
    fn spawn_fallible_cancellable(
        &self,
        name: &'static str,
        fut: BoxFuture<'static, Result<(), crate::AuraError>>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<(), crate::AuraError> {
        drop(fut);
        Err(crate::AuraError::Internal {
            message: "required task supervision is unavailable".into(),
            source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
        })
    }

    /// Admit thread-local required work with retained failure supervision.
    fn spawn_local_fallible_cancellable(
        &self,
        name: &'static str,
        fut: LocalBoxFuture<'static, Result<(), crate::AuraError>>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<(), crate::AuraError> {
        drop(fut);
        Err(crate::AuraError::Internal {
            message: "required local task supervision is unavailable".into(),
            source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
        })
    }

    /// Return a cancellation token associated with this spawner.
    fn cancellation_token(&self) -> Arc<dyn CancellationToken>;
}

/// Cancellation token that never triggers.
pub struct NeverCancel;

#[async_trait]
impl CancellationToken for NeverCancel {
    async fn cancelled(&self) {
        futures::future::pending::<()>().await;
    }
}

#[cfg(test)]
mod required_task_contract_tests {
    use super::*;
    use std::error::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct UnitOnlySpawner;
    impl TaskSpawner for UnitOnlySpawner {
        fn spawn(&self, future: BoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_cancellable(&self, future: BoxFuture<'static, ()>, _: Arc<dyn CancellationToken>) {
            drop(future);
        }
        fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_local_cancellable(
            &self,
            future: LocalBoxFuture<'static, ()>,
            _: Arc<dyn CancellationToken>,
        ) {
            drop(future);
        }
        fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
            Arc::new(NeverCancel)
        }
    }
    struct DropEvidence(Arc<AtomicBool>);
    impl Drop for DropEvidence {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[test]
    fn unsupported_required_adapter_drops_future_and_retains_structural_source() {
        let dropped = Arc::new(AtomicBool::new(false));
        let evidence = DropEvidence(dropped.clone());
        let future = Box::pin(async move {
            drop(evidence);
            Ok(())
        });
        let source = UnitOnlySpawner
            .spawn_fallible_cancellable("required", future, Arc::new(NeverCancel))
            .expect_err("unit-only adapter cannot claim required supervision");
        assert!(dropped.load(Ordering::Acquire));
        assert!(matches!(
            source
                .source()
                .and_then(|source| source.downcast_ref::<TaskSpawnError>()),
            Some(TaskSpawnError::UnsupportedFallible { name: "required" })
        ));
    }
}
