use futures::channel::oneshot;
use parking_lot::Mutex;

#[derive(Debug, Default)]
pub(crate) struct FrontendCancellationWaiters {
    waiters: Mutex<Vec<oneshot::Sender<()>>>,
}

impl FrontendCancellationWaiters {
    pub(crate) fn drain(&self) -> Vec<oneshot::Sender<()>> {
        let mut guard = self.waiters.lock();
        std::mem::take(&mut *guard)
    }

    pub(crate) fn register(
        &self,
        waiter: oneshot::Sender<()>,
        is_cancelled: impl FnOnce() -> bool,
    ) -> bool {
        let mut guard = self.waiters.lock();
        if is_cancelled() {
            return false;
        }
        guard.push(waiter);
        true
    }
}
