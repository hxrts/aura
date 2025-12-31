//! Runtime task registry for agent background work.
//!
//! Tracks spawned tasks and supports cooperative shutdown.
//!
//! # Blocking Lock Usage
//!
//! Uses `parking_lot::Mutex` for JoinHandle storage because:
//! 1. Operations are O(1) push or O(n) drain (shutdown only)
//! 2. Lock is never held across `.await` points
//! 3. No I/O or async work inside lock scope

#![allow(clippy::disallowed_types)]

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use aura_core::effects::task::{CancellationToken, TaskSpawner};
use futures::future::BoxFuture;
use parking_lot::Mutex;
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
        self.handles.lock().push(handle);
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
        self.handles.lock().push(handle);
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
        for handle in self.handles.lock().drain(..) {
            handle.abort();
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
        self.handles.lock().push(handle);
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
        for handle in self.handles.lock().drain(..) {
            handle.abort();
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
        self.handles.lock().push(handle);
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        RuntimeTaskRegistry::cancellation_token(self)
    }
}
