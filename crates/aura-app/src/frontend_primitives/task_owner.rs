//! Shared frontend task-owner primitive for Layer 7 shells.
//!
//! Provides bounded cancellation and spawner mechanics that any frontend
//! (Dioxus, iocraft, or otherwise) can instantiate with its own spawn
//! function pointers.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use async_trait::async_trait;
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::{OwnedShutdownToken, OwnedTaskSpawner};
use futures::{
    channel::oneshot,
    future::{BoxFuture, LocalBoxFuture},
    FutureExt,
};

use super::cancellation_waiters::FrontendCancellationWaiters;

#[derive(Debug, Default)]
struct FrontendTaskCancellationState {
    cancelled: AtomicBool,
    waiters: FrontendCancellationWaiters,
}

impl FrontendTaskCancellationState {
    fn signal_shutdown(&self) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }

        for waiter in self.waiters.drain() {
            let _ = waiter.send(());
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct FrontendTaskCancellationToken {
    state: Arc<FrontendTaskCancellationState>,
}

#[async_trait]
impl CancellationToken for FrontendTaskCancellationToken {
    async fn cancelled(&self) {
        if self.state.is_cancelled() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        if !self
            .state
            .waiters
            .register(tx, || self.state.is_cancelled())
        {
            return;
        }
        let _ = rx.await;
    }

    fn is_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }
}

/// Function pointers for platform-specific task spawning.
///
/// Each frontend shell provides its own spawn implementation (e.g.
/// `dioxus::prelude::spawn` or `wasm_bindgen_futures::spawn_local`).
#[derive(Clone, Copy, Debug)]
pub struct FrontendTaskRuntime {
    spawn: fn(BoxFuture<'static, ()>),
    spawn_local: fn(LocalBoxFuture<'static, ()>),
}

impl FrontendTaskRuntime {
    #[must_use]
    pub const fn new(
        spawn: fn(BoxFuture<'static, ()>),
        spawn_local: fn(LocalBoxFuture<'static, ()>),
    ) -> Self {
        Self { spawn, spawn_local }
    }
}

#[derive(Debug)]
struct FrontendTaskSpawnerImpl {
    cancellation_state: Arc<FrontendTaskCancellationState>,
    runtime: FrontendTaskRuntime,
}

impl FrontendTaskSpawnerImpl {
    fn new(
        cancellation_state: Arc<FrontendTaskCancellationState>,
        runtime: FrontendTaskRuntime,
    ) -> Self {
        Self {
            cancellation_state,
            runtime,
        }
    }

    fn signal_shutdown(&self) {
        self.cancellation_state.signal_shutdown();
    }
}

impl TaskSpawner for FrontendTaskSpawnerImpl {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        (self.runtime.spawn)(fut);
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        (self.runtime.spawn)(Box::pin(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        }));
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        (self.runtime.spawn_local)(fut);
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        (self.runtime.spawn_local)(Box::pin(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        }));
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(FrontendTaskCancellationToken {
            state: self.cancellation_state.clone(),
        })
    }
}

/// Bounded task-owner primitive for Layer 7 frontend shells.
///
/// Owns a cancellation state and a platform-specific spawn runtime.
/// On drop (or explicit shutdown), all cancellable tasks are signalled.
#[derive(Clone, Debug)]
pub struct FrontendTaskOwner {
    inner: Arc<FrontendTaskSpawnerImpl>,
    owner_liveness: Arc<()>,
    spawner: OwnedTaskSpawner,
}

impl FrontendTaskOwner {
    #[must_use]
    pub fn new(runtime: FrontendTaskRuntime) -> Self {
        let cancellation_state = Arc::new(FrontendTaskCancellationState::default());
        let inner = Arc::new(FrontendTaskSpawnerImpl::new(cancellation_state, runtime));
        let shutdown = OwnedShutdownToken::attached(inner.cancellation_token());
        let spawner = OwnedTaskSpawner::new(inner.clone(), shutdown);
        Self {
            inner,
            owner_liveness: Arc::new(()),
            spawner,
        }
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawner.spawn(Box::pin(fut));
    }

    pub fn spawn_cancellable<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.spawner.spawn_cancellable(Box::pin(fut));
    }

    pub fn spawn_local<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        self.spawner.spawn_local(Box::pin(fut));
    }

    pub fn spawn_local_cancellable<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        self.spawner.spawn_local_cancellable(Box::pin(fut));
    }

    #[must_use]
    pub fn owned_spawner(&self) -> OwnedTaskSpawner {
        self.spawner.clone()
    }

    pub fn shutdown(&self) {
        self.inner.signal_shutdown();
    }
}

impl Drop for FrontendTaskOwner {
    fn drop(&mut self) {
        if Arc::strong_count(&self.owner_liveness) == 1 {
            self.inner.signal_shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrontendTaskOwner, FrontendTaskRuntime};

    fn noop_spawn_boxed(_: futures::future::BoxFuture<'static, ()>) {}

    fn noop_spawn_local_boxed(_: futures::future::LocalBoxFuture<'static, ()>) {}

    #[test]
    fn shared_frontend_task_owner_shutdown_marks_owned_spawner_cancelled() {
        let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(
            noop_spawn_boxed,
            noop_spawn_local_boxed,
        ));
        let spawner = owner.owned_spawner();
        assert!(!spawner.shutdown_token().is_cancelled());

        owner.shutdown();

        assert!(spawner.shutdown_token().is_cancelled());
    }

    #[test]
    fn shared_frontend_task_owner_drop_marks_owned_spawner_cancelled() {
        let spawner = {
            let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(
                noop_spawn_boxed,
                noop_spawn_local_boxed,
            ));
            let spawner = owner.owned_spawner();
            assert!(!spawner.shutdown_token().is_cancelled());
            spawner
        };

        assert!(spawner.shutdown_token().is_cancelled());
    }

    #[test]
    fn dropping_temporary_owner_clone_does_not_shutdown_shared_owner() {
        let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(
            noop_spawn_boxed,
            noop_spawn_local_boxed,
        ));
        let spawner = owner.owned_spawner();
        assert!(!spawner.shutdown_token().is_cancelled());

        {
            let temporary = owner.clone();
            drop(temporary);
        }

        assert!(!spawner.shutdown_token().is_cancelled());
        owner.shutdown();
        assert!(spawner.shutdown_token().is_cancelled());
    }
}
