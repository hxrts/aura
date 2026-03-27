use async_lock::Mutex;
use futures::channel::oneshot;

#[derive(Debug, Default)]
pub(crate) struct FrontendCancellationWaiters {
    waiters: Mutex<Vec<oneshot::Sender<()>>>,
}

impl FrontendCancellationWaiters {
    pub(crate) async fn drain(&self) -> Vec<oneshot::Sender<()>> {
        let mut guard = self.waiters.lock().await;
        std::mem::take(&mut *guard)
    }

    pub(crate) async fn register(
        &self,
        waiter: oneshot::Sender<()>,
        is_cancelled: impl FnOnce() -> bool,
    ) -> bool {
        let mut guard = self.waiters.lock().await;
        if is_cancelled() {
            return false;
        }
        guard.push(waiter);
        true
    }
}
