//! Shared frontend task-owner primitive for Layer 7 shells.
//!
//! The core `FrontendTaskOwner` and `FrontendTaskRuntime` types are defined
//! in `aura-app::frontend_primitives` and re-exported here. This module adds
//! the Dioxus-specific default spawn wiring and the thread-local singleton.

use std::cell::OnceCell;

use dioxus::prelude::spawn;
use futures::future::{BoxFuture, LocalBoxFuture};

pub use aura_app::frontend_primitives::{FrontendTaskOwner, FrontendTaskRuntime};

fn spawn_boxed(fut: BoxFuture<'static, ()>) {
    spawn(async move {
        fut.await;
    });
}

fn spawn_local_boxed(fut: LocalBoxFuture<'static, ()>) {
    spawn(async move {
        fut.await;
    });
}

thread_local! {
    static SHARED_UI_TASK_OWNER: OnceCell<FrontendTaskOwner> = const { OnceCell::new() };
}

fn shared_ui_task_owner() -> FrontendTaskOwner {
    SHARED_UI_TASK_OWNER.with(|slot| {
        slot.get_or_init(|| {
            FrontendTaskOwner::new(FrontendTaskRuntime::new(spawn_boxed, spawn_local_boxed))
        })
        .clone()
    })
}

pub(crate) fn spawn_ui<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    shared_ui_task_owner().spawn_local(fut);
}
