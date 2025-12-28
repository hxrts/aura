//! Runtime task registry for agent background work.
//!
//! Tracks spawned tasks and supports cooperative shutdown.

use std::future::Future;
use std::sync::Mutex;

use aura_core::effects::task::{CancellationToken, TaskSpawner};
use futures::future::BoxFuture;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct RuntimeTaskRegistry {
    shutdown_tx: watch::Sender<bool>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl RuntimeTaskRegistry {
    pub fn new() -> Self {
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        Self {
            shutdown_tx,
            handles: Mutex::new(Vec::new()),
        }
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let handle = tokio::spawn(fut);
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    pub fn spawn_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx.changed() => {}
                _ = fut => {}
            }
        });
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }

    pub fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(RuntimeCancellationToken {
            shutdown_rx: self.shutdown_tx.subscribe(),
        })
    }

    pub fn spawn_interval_until<F, Fut>(&self, interval: Duration, mut f: F)
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    _ = ticker.tick() => {
                        if !f().await {
                            break;
                        }
                    }
                }
            }
        });
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }
}

impl Default for RuntimeTaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for RuntimeTaskRegistry {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

#[derive(Debug)]
struct RuntimeCancellationToken {
    shutdown_rx: watch::Receiver<bool>,
}

#[async_trait::async_trait]
impl CancellationToken for RuntimeCancellationToken {
    async fn cancelled(&self) {
        let mut shutdown_rx = self.shutdown_rx.clone();
        loop {
            if *shutdown_rx.borrow() {
                return;
            }
            if shutdown_rx.changed().await.is_err() {
                return;
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        *self.shutdown_rx.borrow()
    }
}

impl TaskSpawner for RuntimeTaskRegistry {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        RuntimeTaskRegistry::spawn(self, fut);
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown_rx.changed() => {}
                _ = token.cancelled() => {}
                _ = fut => {}
            }
        });
        if let Ok(mut handles) = self.handles.lock() {
            handles.push(handle);
        }
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        RuntimeTaskRegistry::cancellation_token(self)
    }
}
