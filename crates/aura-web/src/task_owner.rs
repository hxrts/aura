use std::cell::RefCell;

use aura_ui::task_owner::{FrontendTaskOwner, FrontendTaskRuntime};
use wasm_bindgen_futures::spawn_local;
pub type WebTaskOwner = FrontendTaskOwner;

fn spawn_boxed(fut: futures::future::BoxFuture<'static, ()>) {
    spawn_local(async move {
        fut.await;
    });
}

fn spawn_local_boxed(fut: futures::future::LocalBoxFuture<'static, ()>) {
    spawn_local(fut);
}

thread_local! {
    static SHARED_WEB_TASK_OWNER: RefCell<Option<WebTaskOwner>> = const { RefCell::new(None) };
}

#[must_use]
pub fn shared_web_task_owner() -> WebTaskOwner {
    SHARED_WEB_TASK_OWNER.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.get_or_insert_with(|| {
            WebTaskOwner::new(FrontendTaskRuntime::new(spawn_boxed, spawn_local_boxed))
        })
        .clone()
    })
}
