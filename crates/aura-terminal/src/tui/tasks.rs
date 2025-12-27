//! Task registry for TUI async work.
//!
//! Tracks spawned tasks and supports cooperative shutdown.

use std::future::Future;
use std::sync::Mutex;

use tokio::sync::watch;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct UiTaskRegistry {
    shutdown_tx: watch::Sender<bool>,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl UiTaskRegistry {
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
}

impl Default for UiTaskRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for UiTaskRegistry {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Ok(mut handles) = self.handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}
