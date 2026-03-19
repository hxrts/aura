//! Sanctioned frontend task owner for TUI async work.
//!
//! The TUI remains an observed frontend layer. When it needs local background
//! work for update-loop mechanics or non-authoritative adaptation, it consumes
//! the shared `aura-core` owned task primitives instead of defining a parallel
//! raw Tokio registry model.

use std::future::Future;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::{OwnedShutdownToken, OwnedTaskSpawner};
use futures::future::{BoxFuture, LocalBoxFuture};
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Clone)]
struct UiTaskCancellationToken {
    shutdown_tx: watch::Sender<bool>,
}

#[async_trait]
impl CancellationToken for UiTaskCancellationToken {
    async fn cancelled(&self) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        if *shutdown_rx.borrow() {
            return;
        }

        loop {
            if shutdown_rx.changed().await.is_err() {
                return;
            }
            if *shutdown_rx.borrow() {
                return;
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        *self.shutdown_tx.borrow()
    }
}

#[derive(Debug)]
struct UiTaskSpawnerImpl {
    shutdown_tx: watch::Sender<bool>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl UiTaskSpawnerImpl {
    fn new(shutdown_tx: watch::Sender<bool>) -> Self {
        Self {
            shutdown_tx,
            handles: Mutex::new(Vec::new()),
        }
    }

    fn record(&self, handle: JoinHandle<()>) {
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    fn abort_all(&self) {
        let _ = self.shutdown_tx.send(true);
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

impl TaskSpawner for UiTaskSpawnerImpl {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        self.record(tokio::spawn(fut));
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        self.record(tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => {}
                _ = fut => {}
            }
        }));
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        self.record(tokio::task::spawn_local(fut));
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        self.record(tokio::task::spawn_local(async move {
            tokio::select! {
                _ = token.cancelled() => {}
                _ = fut => {}
            }
        }));
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(UiTaskCancellationToken {
            shutdown_tx: self.shutdown_tx.clone(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct UiTaskOwner {
    inner: Arc<UiTaskSpawnerImpl>,
    spawner: OwnedTaskSpawner,
}

impl UiTaskOwner {
    #[must_use]
    pub fn new() -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let inner = Arc::new(UiTaskSpawnerImpl::new(shutdown_tx));
        let shutdown = OwnedShutdownToken::attached(inner.cancellation_token());
        let spawner = OwnedTaskSpawner::new(inner.clone(), shutdown);
        Self { inner, spawner }
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawner.spawn(Box::pin(fut));
    }

    pub fn spawn_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawner.spawn_cancellable(Box::pin(fut));
    }

    #[must_use]
    pub fn owned_spawner(&self) -> OwnedTaskSpawner {
        self.spawner.clone()
    }

    #[must_use]
    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        self.spawner.shutdown_token()
    }

    pub fn shutdown(&self) {
        self.inner.abort_all();
    }
}

impl Default for UiTaskOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for UiTaskOwner {
    fn drop(&mut self) {
        self.inner.abort_all();
    }
}

#[cfg(test)]
mod tests {
    use super::UiTaskOwner;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn cancellable_spawn_observes_owner_shutdown() {
        let owner = UiTaskOwner::new();
        let started = Arc::new(Notify::new());
        let started_task = started.clone();

        owner.spawn_cancellable(async move {
            started_task.notify_waiters();
            futures::future::pending::<()>().await;
        });

        started.notified().await;
        owner.shutdown();
        tokio::task::yield_now().await;
        assert!(owner.shutdown_token().is_cancelled());
    }
}
