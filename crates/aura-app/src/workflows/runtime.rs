//! Runtime access helpers for workflows.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_lock::RwLock;

use crate::runtime_bridge::RuntimeBridge;
use crate::AppCore;
use aura_core::AuraError;

/// Get the runtime bridge or return a consistent error.
pub async fn require_runtime(
    app_core: &Arc<RwLock<AppCore>>,
) -> Result<Arc<dyn RuntimeBridge>, AuraError> {
    let core = app_core.read().await;
    core.runtime()
        .cloned()
        .ok_or_else(|| AuraError::agent("Runtime bridge not available"))
}

/// Yield to the scheduler once without binding workflows to a runtime crate.
pub async fn cooperative_yield() {
    struct YieldOnce(bool);

    impl Future for YieldOnce {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
            if self.0 {
                Poll::Ready(())
            } else {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    YieldOnce(false).await;
}
