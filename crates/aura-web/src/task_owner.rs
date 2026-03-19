use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::{OwnedShutdownToken, OwnedTaskSpawner};
use futures::{
    channel::oneshot,
    future::{BoxFuture, LocalBoxFuture},
    FutureExt,
};
use wasm_bindgen_futures::spawn_local;

#[derive(Debug, Default)]
struct WebTaskCancellationState {
    cancelled: AtomicBool,
    waiters: Mutex<Vec<oneshot::Sender<()>>>,
}

impl WebTaskCancellationState {
    fn signal_shutdown(&self) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }

        let waiters = {
            let mut guard = self.waiters.lock().expect("web task waiters lock poisoned");
            std::mem::take(&mut *guard)
        };
        for waiter in waiters {
            let _ = waiter.send(());
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct WebTaskCancellationToken {
    state: Arc<WebTaskCancellationState>,
}

#[async_trait]
impl CancellationToken for WebTaskCancellationToken {
    async fn cancelled(&self) {
        if self.state.is_cancelled() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self
                .state
                .waiters
                .lock()
                .expect("web task waiters lock poisoned");
            if self.state.is_cancelled() {
                return;
            }
            waiters.push(tx);
        }
        let _ = rx.await;
    }

    fn is_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }
}

#[derive(Debug)]
struct WebTaskSpawnerImpl {
    cancellation_state: Arc<WebTaskCancellationState>,
}

impl WebTaskSpawnerImpl {
    fn new(cancellation_state: Arc<WebTaskCancellationState>) -> Self {
        Self { cancellation_state }
    }

    fn signal_shutdown(&self) {
        self.cancellation_state.signal_shutdown();
    }
}

impl TaskSpawner for WebTaskSpawnerImpl {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        spawn_local(async move {
            fut.await;
        });
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        spawn_local(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        });
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        spawn_local(fut);
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        spawn_local(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        });
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(WebTaskCancellationToken {
            state: self.cancellation_state.clone(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct WebTaskOwner {
    inner: Arc<WebTaskSpawnerImpl>,
    spawner: OwnedTaskSpawner,
}

impl WebTaskOwner {
    #[must_use]
    pub fn new() -> Self {
        let cancellation_state = Arc::new(WebTaskCancellationState::default());
        let inner = Arc::new(WebTaskSpawnerImpl::new(cancellation_state));
        let shutdown = OwnedShutdownToken::attached(inner.cancellation_token());
        let spawner = OwnedTaskSpawner::new(inner.clone(), shutdown);
        Self { inner, spawner }
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

impl Default for WebTaskOwner {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WebTaskOwner {
    fn drop(&mut self) {
        self.inner.signal_shutdown();
    }
}

thread_local! {
    static SHARED_WEB_TASK_OWNER: RefCell<Option<WebTaskOwner>> = const { RefCell::new(None) };
}

#[must_use]
pub fn shared_web_task_owner() -> WebTaskOwner {
    SHARED_WEB_TASK_OWNER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(WebTaskOwner::new).clone()
    })
}
