//! Shared frontend task-owner primitive for Layer 7 shells.
//!
//! Provides bounded cancellation and spawner mechanics that any frontend
//! (Dioxus, iocraft, or otherwise) can instantiate with its own spawn
//! function pointers.

use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn begin_shutdown(&self) -> bool {
        !self.cancelled.swap(true, Ordering::SeqCst)
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
            .await
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
    completion: Arc<TaskCompletion>,
}

const TASK_ADMISSION_CLOSED: usize = 1usize << (usize::BITS - 1);

#[derive(Debug, Default)]
struct TaskCompletion {
    state: AtomicUsize,
    changed: futures::task::AtomicWaker,
    observer: async_lock::Mutex<()>,
}

struct TaskCompletionLease(Arc<TaskCompletion>);

struct TrackedTask<F> {
    future: Option<F>,
    lease: Option<TaskCompletionLease>,
}

impl<F: std::future::Future<Output = ()> + Unpin> std::future::Future for TrackedTask<F> {
    type Output = ();
    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<()> {
        let this = self.get_mut();
        let Some(future) = this.future.as_mut() else {
            return std::task::Poll::Ready(());
        };
        let outcome = std::pin::Pin::new(future).poll(cx);
        if outcome.is_ready() {
            this.future.take();
            this.lease.take();
        }
        outcome
    }
}

impl<F> Drop for TrackedTask<F> {
    fn drop(&mut self) {
        // Completion means the original future is already destroyed, including
        // unpolled tasks and panic/cancellation paths.
        self.future.take();
        self.lease.take();
    }
}

impl Drop for TaskCompletionLease {
    fn drop(&mut self) {
        self.0.state.fetch_sub(1, Ordering::AcqRel);
        self.0.changed.wake();
    }
}

impl TaskCompletion {
    // Keep the same acquire/release update semantics across stable and nightly
    // toolchains, where fetch_update has been renamed to try_update.
    fn update_state(&self, update: impl Fn(usize) -> Option<usize>) -> Result<usize, usize> {
        let mut state = self.state.load(Ordering::Acquire);
        loop {
            let next = update(state).ok_or(state)?;
            match self
                .state
                .compare_exchange_weak(state, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(previous) => return Ok(previous),
                Err(actual) => state = actual,
            }
        }
    }

    fn admit(self: &Arc<Self>) -> Option<TaskCompletionLease> {
        self.update_state(|state| {
            if state < TASK_ADMISSION_CLOSED - 2 {
                Some(state + 1)
            } else {
                None
            }
        })
        .ok()
        .map(|_| TaskCompletionLease(self.clone()))
    }

    async fn drained(&self) {
        let _observer = self.observer.lock().await;
        futures::future::poll_fn(|cx| {
            self.changed.register(cx.waker());
            if self.state.load(Ordering::Acquire) == TASK_ADMISSION_CLOSED {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
    }
}

impl FrontendTaskSpawnerImpl {
    fn new(
        cancellation_state: Arc<FrontendTaskCancellationState>,
        runtime: FrontendTaskRuntime,
    ) -> Self {
        Self {
            cancellation_state,
            runtime,
            completion: Arc::new(TaskCompletion::default()),
        }
    }

    fn signal_shutdown(&self) {
        // Reserve the final cancellation-dispatch future while atomically
        // closing public admission. Its own destruction is part of drainage.
        if self
            .completion
            .update_state(|state| {
                if state & TASK_ADMISSION_CLOSED == 0 {
                    Some((state | TASK_ADMISSION_CLOSED) + 1)
                } else {
                    None
                }
            })
            .is_err()
        {
            return;
        }
        self.cancellation_state.begin_shutdown();
        let cancellation_state = self.cancellation_state.clone();
        let dispatch: BoxFuture<'static, ()> = Box::pin(async move {
            for waiter in cancellation_state.waiters.drain().await {
                let _ = waiter.send(());
            }
        });
        (self.runtime.spawn)(Box::pin(TrackedTask {
            future: Some(dispatch),
            lease: Some(TaskCompletionLease(self.completion.clone())),
        }));
    }
}

impl TaskSpawner for FrontendTaskSpawnerImpl {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        let Some(lease) = self.completion.admit() else {
            return;
        };
        (self.runtime.spawn)(Box::pin(TrackedTask {
            future: Some(fut),
            lease: Some(lease),
        }));
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        self.spawn(Box::pin(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        }));
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        let Some(lease) = self.completion.admit() else {
            return;
        };
        (self.runtime.spawn_local)(Box::pin(TrackedTask {
            future: Some(fut),
            lease: Some(lease),
        }));
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        self.spawn_local(Box::pin(async move {
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
#[aura_macros::actor_root(
    owner = "shared_frontend_task_manager",
    domain = "frontend_task_runtime",
    supervision = "frontend_task_root",
    category = "actor_owned"
)]
pub struct FrontendTaskManager {
    inner: Arc<FrontendTaskSpawnerImpl>,
    owner_liveness: Arc<()>,
    spawner: OwnedTaskSpawner,
}

pub type FrontendTaskOwner = FrontendTaskManager;

impl FrontendTaskManager {
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

    #[must_use]
    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        self.spawner.shutdown_token()
    }

    pub fn shutdown(&self) {
        self.inner.signal_shutdown();
    }

    /// Acknowledge actual destruction/completion of all admitted tasks after
    /// shutdown. The caller supplies its original bounded observation window.
    pub async fn wait_drained(&self) {
        self.inner.completion.drained().await;
    }
}

impl Drop for FrontendTaskManager {
    fn drop(&mut self) {
        if Arc::strong_count(&self.owner_liveness) == 1 {
            self.inner.signal_shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FrontendTaskOwner, FrontendTaskRuntime};
    use futures::FutureExt;

    #[tokio::test]
    async fn drain_waits_for_admitted_escaped_spawner_and_rejects_late_work() {
        fn spawn(future: futures::future::BoxFuture<'static, ()>) {
            tokio::spawn(future);
        }
        fn spawn_local(future: futures::future::LocalBoxFuture<'static, ()>) {
            tokio::task::spawn_local(future);
        }
        let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(spawn, spawn_local));
        let spawner = owner.owned_spawner();
        let (release, admitted) = futures::channel::oneshot::channel::<()>();
        spawner.spawn(Box::pin(async move {
            let _ = admitted.await;
        }));
        owner.shutdown();
        assert!(owner.wait_drained().now_or_never().is_none());
        release.send(()).unwrap();
        owner.wait_drained().await;
        let ran = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let late_ran = ran.clone();
        spawner.spawn(Box::pin(async move {
            late_ran.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        owner.wait_drained().await;
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn unpolled_future_destruction_precedes_completion_acknowledgment() {
        struct OriginalFuture(std::sync::Arc<super::TaskCompletion>);
        impl std::future::Future for OriginalFuture {
            type Output = ();
            fn poll(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
            ) -> std::task::Poll<()> {
                std::task::Poll::Pending
            }
        }
        impl Drop for OriginalFuture {
            fn drop(&mut self) {
                assert_eq!(
                    self.0.state.load(std::sync::atomic::Ordering::Acquire),
                    super::TASK_ADMISSION_CLOSED + 1
                );
            }
        }
        let completion = std::sync::Arc::new(super::TaskCompletion::default());
        let lease = completion.admit().unwrap();
        let tracked = super::TrackedTask {
            future: Some(OriginalFuture(completion.clone())),
            lease: Some(lease),
        };
        completion.state.fetch_or(
            super::TASK_ADMISSION_CLOSED,
            std::sync::atomic::Ordering::AcqRel,
        );
        drop(tracked);
        assert_eq!(
            completion.state.load(std::sync::atomic::Ordering::Acquire),
            super::TASK_ADMISSION_CLOSED
        );
    }

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
    fn dropping_ephemeral_owner_clone_does_not_shutdown_shared_owner() {
        let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(
            noop_spawn_boxed,
            noop_spawn_local_boxed,
        ));
        let spawner = owner.owned_spawner();
        assert!(!spawner.shutdown_token().is_cancelled());

        {
            let ephemeral = owner.clone();
            drop(ephemeral);
        }

        assert!(!spawner.shutdown_token().is_cancelled());
        owner.shutdown();
        assert!(spawner.shutdown_token().is_cancelled());
    }
}
