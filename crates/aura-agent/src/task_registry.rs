//! Shared task registry for agent background work.
//!
//! Centralizes task tracking for runtime services and reactive pipelines.

#![allow(clippy::disallowed_types)]

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::effects::PhysicalTimeEffects;
use futures::future::BoxFuture;
use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct TaskRegistry {
    shutdown_tx: watch::Sender<bool>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl TaskRegistry {
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
        Arc::new(TaskRegistryCancellationToken {
            shutdown_rx: self.shutdown_tx.subscribe(),
        })
    }

    pub fn spawn_interval_until<F, Fut>(
        &self,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle = tokio::spawn(async move {
            let interval_ms = interval.as_millis().try_into().unwrap_or(u64::MAX);
            loop {
                if *shutdown_rx.borrow() {
                    break;
                }

                if !f().await {
                    break;
                }

                tokio::select! {
                    _ = shutdown_rx.changed() => break,
                    result = time_effects.sleep_ms(interval_ms) => {
                        if result.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        self.handles.lock().push(handle);
    }
}

impl Default for TaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TaskRegistry {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        for handle in self.handles.lock().drain(..) {
            handle.abort();
        }
    }
}

#[derive(Debug)]
struct TaskRegistryCancellationToken {
    shutdown_rx: watch::Receiver<bool>,
}

#[async_trait::async_trait]
impl CancellationToken for TaskRegistryCancellationToken {
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

impl TaskSpawner for TaskRegistry {
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        TaskRegistry::spawn(self, fut);
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
        TaskRegistry::cancellation_token(self)
    }
}
