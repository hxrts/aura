//! Runtime-agnostic task spawning traits.

use async_trait::async_trait;
use futures::future::BoxFuture;
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

/// Task spawning contract for runtime implementations.
pub trait TaskSpawner: Send + Sync {
    /// Spawn a background task.
    fn spawn(&self, fut: BoxFuture<'static, ()>);

    /// Spawn a background task tied to a cancellation token.
    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>);

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
