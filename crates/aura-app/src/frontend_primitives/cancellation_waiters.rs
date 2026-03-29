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

#[cfg(test)]
mod tests {
    use super::FrontendCancellationWaiters;
    use futures::{channel::oneshot, executor::block_on};

    #[test]
    fn register_rejects_waiter_once_cancelled() {
        block_on(async {
            let waiters = FrontendCancellationWaiters::default();
            let (tx, _rx) = oneshot::channel();

            let registered = waiters.register(tx, || true).await;

            assert!(!registered);
            assert!(waiters.drain().await.is_empty());
        });
    }

    #[test]
    fn drain_returns_registered_waiters() {
        block_on(async {
            let waiters = FrontendCancellationWaiters::default();
            let (tx_one, _rx_one) = oneshot::channel();
            let (tx_two, _rx_two) = oneshot::channel();

            assert!(waiters.register(tx_one, || false).await);
            assert!(waiters.register(tx_two, || false).await);

            let drained = waiters.drain().await;
            assert_eq!(drained.len(), 2);
            assert!(waiters.drain().await.is_empty());
        });
    }
}
