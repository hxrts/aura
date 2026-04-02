//! Sanctioned frontend task owner for TUI async work.
//!
//! The terminal shell stays an observed frontend layer. Its local async work
//! uses the shared Layer 7 frontend task root from `aura-app` rather than a
//! terminal-local parallel task-owner implementation.

use std::future::Future;

use aura_app::frontend_primitives::{FrontendTaskOwner, FrontendTaskRuntime};
use aura_core::{OwnedShutdownToken, OwnedTaskSpawner};
use aura_macros::actor_root;
use futures::future::{BoxFuture, LocalBoxFuture};

fn spawn_boxed(fut: BoxFuture<'static, ()>) {
    tokio::spawn(fut);
}

fn spawn_local_boxed(fut: LocalBoxFuture<'static, ()>) {
    tokio::task::spawn_local(fut);
}

#[derive(Clone, Debug)]
#[actor_root(
    owner = "terminal_ui_task_manager",
    domain = "terminal_ui_task_runtime",
    supervision = "terminal_ui_task_root",
    category = "actor_owned"
)]
pub struct UiTaskManager {
    inner: FrontendTaskOwner,
}

pub type UiTaskOwner = UiTaskManager;

impl UiTaskManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: FrontendTaskOwner::new(FrontendTaskRuntime::new(spawn_boxed, spawn_local_boxed)),
        }
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.spawn(fut);
    }

    pub fn spawn_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.spawn_cancellable(fut);
    }

    pub fn spawn_local<F>(&self, fut: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.inner.spawn_local(fut);
    }

    pub fn spawn_local_cancellable<F>(&self, fut: F)
    where
        F: Future<Output = ()> + 'static,
    {
        self.inner.spawn_local_cancellable(fut);
    }

    #[must_use]
    pub fn owned_spawner(&self) -> OwnedTaskSpawner {
        self.inner.owned_spawner()
    }

    #[must_use]
    pub fn shutdown_token(&self) -> &OwnedShutdownToken {
        self.inner.shutdown_token()
    }

    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

impl Default for UiTaskManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::UiTaskOwner;
    use std::sync::Arc;
    use tokio::sync::Notify;

    #[tokio::test]
    async fn cancellable_spawn_observes_owner_shutdown() {
        let owner = UiTaskOwner::new();
        let started = Arc::new(Notify::new());
        let started_task = started.clone();

        owner.spawn_cancellable(async move {
            started_task.notify_waiters();
            futures::future::pending::<()>().await;
        });

        started.notified().await;
        owner.shutdown();
        tokio::task::yield_now().await;
        assert!(owner.shutdown_token().is_cancelled());
    }
}
