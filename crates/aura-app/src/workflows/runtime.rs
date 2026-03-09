//! Runtime access helpers for workflows.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_lock::RwLock;

use crate::runtime_bridge::RuntimeBridge;
use crate::AppCore;
use aura_core::AuraError;

const DEFAULT_HARNESS_CONVERGENCE_ROUNDS: usize = 8;
const DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS: u64 = 150;

fn harness_mode_enabled() -> bool {
    std::env::var_os("AURA_HARNESS_MODE").is_some()
}

fn harness_convergence_rounds() -> usize {
    std::env::var("AURA_HARNESS_CONVERGENCE_ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(DEFAULT_HARNESS_CONVERGENCE_ROUNDS)
}

fn harness_convergence_backoff_ms() -> u64 {
    std::env::var("AURA_HARNESS_CONVERGENCE_BACKOFF_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_HARNESS_CONVERGENCE_BACKOFF_MS)
}

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

/// Ask the runtime to perform a bounded convergence pass suitable for harness-mode real-runtime
/// execution. The runtime bridge owns the actual harness profile policy.
pub async fn converge_runtime(runtime: &Arc<dyn RuntimeBridge>) {
    let rounds = if harness_mode_enabled() {
        harness_convergence_rounds()
    } else {
        1
    };
    let backoff_ms = harness_convergence_backoff_ms();

    for round in 0..rounds {
        if harness_mode_enabled() {
            let _ = runtime.trigger_discovery().await;
            let _ = runtime.process_ceremony_messages().await;
        }
        let _ = runtime.trigger_sync().await;
        cooperative_yield().await;

        if round + 1 < rounds && harness_mode_enabled() && backoff_ms > 0 {
            runtime.sleep_ms(backoff_ms).await;
        }
    }
}

/// Validate that the runtime has at least one viable connectivity path before a
/// shared-flow operation relies on remote convergence.
pub async fn ensure_runtime_peer_connectivity(
    runtime: &Arc<dyn RuntimeBridge>,
    flow: &str,
) -> Result<(), AuraError> {
    let sync_peers = runtime.get_sync_peers().await;
    let discovered_peers = runtime.get_discovered_peers().await;
    let lan_peers = runtime.get_lan_peers().await;

    if sync_peers.is_empty() && discovered_peers.is_empty() && lan_peers.is_empty() {
        return Err(AuraError::agent(format!(
            "Missing connectivity prerequisite for {flow}: sync_peers=0 discovered_peers=0 lan_peers=0"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_runtime_peer_connectivity;
    use crate::runtime_bridge::{OfflineRuntimeBridge, RuntimeBridge};
    use aura_core::identifiers::AuthorityId;
    use std::sync::Arc;

    #[tokio::test]
    async fn connectivity_check_fails_when_no_peers_exist() {
        let runtime: Arc<dyn RuntimeBridge> = Arc::new(OfflineRuntimeBridge::new(
            AuthorityId::new_from_entropy([7_u8; 32]),
        ));

        let error = ensure_runtime_peer_connectivity(&runtime, "test_flow")
            .await
            .expect_err("offline runtime should not satisfy peer connectivity");

        let message = error.to_string();
        assert!(message.contains("Missing connectivity prerequisite"));
        assert!(message.contains("test_flow"));
    }
}
