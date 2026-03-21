use std::cell::RefCell;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::task::{Poll, Waker};

use async_trait::async_trait;
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::{OwnedShutdownToken, OwnedTaskSpawner};
use dioxus::prelude::spawn;
use futures::{
    future::{BoxFuture, LocalBoxFuture},
    FutureExt,
};

#[derive(Debug)]
#[allow(clippy::disallowed_types)]
struct UiTaskCancellationState {
    cancelled: AtomicBool,
    waiters: Mutex<Vec<Waker>>,
}

impl UiTaskCancellationState {
    fn signal_shutdown(&self) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        let waiters = {
            let mut waiters = self
                .waiters
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *waiters)
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn register_waiter(&self, waker: &Waker) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let mut waiters = self
            .waiters
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.is_cancelled() {
            return true;
        }
        waiters.push(waker.clone());
        false
    }
}

#[derive(Clone)]
struct UiTaskCancellationToken {
    state: Arc<UiTaskCancellationState>,
}

#[async_trait]
impl CancellationToken for UiTaskCancellationToken {
    async fn cancelled(&self) {
        futures::future::poll_fn(|cx| {
            if self.state.register_waiter(cx.waker()) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await;
    }

    fn is_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }
}

#[derive(Debug)]
struct UiTaskSpawnerImpl {
    cancellation_state: Arc<UiTaskCancellationState>,
}

impl UiTaskSpawnerImpl {
    fn new(cancellation_state: Arc<UiTaskCancellationState>) -> Self {
        Self { cancellation_state }
    }

    fn signal_shutdown(&self) {
        self.cancellation_state.signal_shutdown();
    }
}

impl TaskSpawner for UiTaskSpawnerImpl {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        spawn(async move {
            fut.await;
        });
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        spawn(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        });
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        spawn(async move {
            fut.await;
        });
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        spawn(async move {
            futures::select! {
                _ = token.cancelled().fuse() => {}
                _ = fut.fuse() => {}
            }
        });
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(UiTaskCancellationToken {
            state: self.cancellation_state.clone(),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UiTaskOwner {
    inner: Arc<UiTaskSpawnerImpl>,
    spawner: OwnedTaskSpawner,
}

impl UiTaskOwner {
    fn new() -> Self {
        let cancellation_state = Arc::new(UiTaskCancellationState {
            cancelled: AtomicBool::new(false),
            waiters: Mutex::new(Vec::new()),
        });
        let inner = Arc::new(UiTaskSpawnerImpl::new(cancellation_state));
        let shutdown = OwnedShutdownToken::attached(inner.cancellation_token());
        let spawner = OwnedTaskSpawner::new(inner.clone(), shutdown);
        Self { inner, spawner }
    }

    fn spawn_local<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + 'static,
    {
        self.spawner.spawn_local(Box::pin(fut));
    }
}

impl Drop for UiTaskOwner {
    fn drop(&mut self) {
        self.inner.signal_shutdown();
    }
}

thread_local! {
    static SHARED_UI_TASK_OWNER: RefCell<Option<UiTaskOwner>> = const { RefCell::new(None) };
}

fn shared_ui_task_owner() -> UiTaskOwner {
    SHARED_UI_TASK_OWNER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(UiTaskOwner::new).clone()
    })
}

pub(crate) fn spawn_ui<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    shared_ui_task_owner().spawn_local(fut);
}
