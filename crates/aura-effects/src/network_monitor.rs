//! Debounced network-change monitoring handler.
//!
//! This handler wraps an upstream `NetworkChangeEffects` implementation and
//! suppresses transient unusable states for a configurable debounce window.

use std::time::Duration;

use async_trait::async_trait;
use aura_core::effects::{
    NetworkChange, NetworkChangeEffects, NetworkChangeStream, NetworkError, NetworkUsability,
};

/// Default debounce window for unusable network transitions.
const DEFAULT_UNUSABLE_DEBOUNCE_MS: u64 = 2_000;

/// Network monitor wrapper that debounces unusable transitions.
#[derive(Clone)]
pub struct NetworkMonitorHandler<E> {
    upstream: E,
    unusable_debounce: Duration,
}

impl<E> NetworkMonitorHandler<E> {
    /// Create a monitor with the default unusable debounce duration (2 seconds).
    pub fn new(upstream: E) -> Self {
        Self {
            upstream,
            unusable_debounce: Duration::from_millis(DEFAULT_UNUSABLE_DEBOUNCE_MS),
        }
    }

    /// Create a monitor with a custom unusable debounce duration.
    pub fn with_debounce(upstream: E, unusable_debounce: Duration) -> Self {
        Self {
            upstream,
            unusable_debounce,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl<E> NetworkChangeEffects for NetworkMonitorHandler<E>
where
    E: NetworkChangeEffects + Send + Sync,
{
    async fn subscribe_network_changes(
        &self,
    ) -> Result<Box<dyn NetworkChangeStream>, NetworkError> {
        let stream = self.upstream.subscribe_network_changes().await?;
        Ok(Box::new(DebouncedNetworkChangeStream::new(
            stream,
            self.unusable_debounce,
        )))
    }
}

struct DebouncedNetworkChangeStream {
    inner: Box<dyn NetworkChangeStream>,
    #[cfg(not(target_arch = "wasm32"))]
    unusable_debounce: Duration,
}

impl DebouncedNetworkChangeStream {
    fn new(inner: Box<dyn NetworkChangeStream>, unusable_debounce: Duration) -> Self {
        #[cfg(target_arch = "wasm32")]
        let _ = unusable_debounce;

        Self {
            inner,
            #[cfg(not(target_arch = "wasm32"))]
            unusable_debounce,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl NetworkChangeStream for DebouncedNetworkChangeStream {
    async fn next_change(&mut self) -> Result<Option<NetworkChange>, NetworkError> {
        let Some(change) = self.inner.next_change().await? else {
            return Ok(None);
        };

        if !matches!(change.usability, NetworkUsability::Unusable { .. }) {
            return Ok(Some(change));
        }

        #[cfg(target_arch = "wasm32")]
        {
            // Browser harness/runtime paths do not have a Send-safe debounce timer future here.
            // Deliver unusable transitions immediately on wasm instead of hanging the async_trait future.
            return Ok(Some(change));
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut pending_unusable = change;
            let timer = tokio::time::sleep(self.unusable_debounce);
            tokio::pin!(timer);

            loop {
                tokio::select! {
                    _ = &mut timer => {
                        return Ok(Some(pending_unusable));
                    }
                    next = self.inner.next_change() => {
                        let Some(next_change) = next? else {
                            return Ok(Some(pending_unusable));
                        };

                        match next_change.usability {
                            NetworkUsability::Usable => return Ok(Some(next_change)),
                            NetworkUsability::Unusable { .. } => {
                                pending_unusable = next_change;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Arc};

    use tokio::sync::Mutex as AsyncMutex;

    use super::*;

    #[derive(Clone, Default)]
    struct MemoryNetworkEffects {
        queue: Arc<AsyncMutex<VecDeque<NetworkChange>>>,
    }

    impl MemoryNetworkEffects {
        fn new(changes: Vec<NetworkChange>) -> Self {
            Self {
                queue: Arc::new(AsyncMutex::new(changes.into_iter().collect())),
            }
        }
    }

    struct MemoryStream {
        queue: Arc<AsyncMutex<VecDeque<NetworkChange>>>,
    }

    #[async_trait]
    impl NetworkChangeStream for MemoryStream {
        async fn next_change(&mut self) -> Result<Option<NetworkChange>, NetworkError> {
            Ok(self.queue.lock().await.pop_front())
        }
    }

    #[async_trait]
    impl NetworkChangeEffects for MemoryNetworkEffects {
        async fn subscribe_network_changes(
            &self,
        ) -> Result<Box<dyn NetworkChangeStream>, NetworkError> {
            Ok(Box::new(MemoryStream {
                queue: Arc::clone(&self.queue),
            }))
        }
    }

    fn usable(generation: u64) -> NetworkChange {
        NetworkChange {
            generation,
            usability: NetworkUsability::Usable,
            interfaces: vec![],
        }
    }

    fn unusable(generation: u64, reason: &str) -> NetworkChange {
        NetworkChange {
            generation,
            usability: NetworkUsability::Unusable {
                reason: reason.to_string(),
            },
            interfaces: vec![],
        }
    }

    #[tokio::test]
    async fn suppresses_transient_unusable_when_usable_arrives_before_deadline() {
        let upstream = MemoryNetworkEffects::new(vec![unusable(1, "wifi-down"), usable(2)]);
        let handler = NetworkMonitorHandler::with_debounce(
            upstream,
            Duration::from_millis(DEFAULT_UNUSABLE_DEBOUNCE_MS),
        );

        let mut stream = handler.subscribe_network_changes().await.unwrap();
        let first = stream.next_change().await.unwrap().unwrap();

        assert_eq!(first.generation, 2);
        assert!(matches!(first.usability, NetworkUsability::Usable));
    }

    #[tokio::test]
    async fn emits_unusable_when_no_recovery_arrives() {
        let upstream = MemoryNetworkEffects::new(vec![unusable(10, "carrier-loss")]);
        let handler = NetworkMonitorHandler::with_debounce(upstream, Duration::from_millis(1));

        let mut stream = handler.subscribe_network_changes().await.unwrap();
        let first = stream.next_change().await.unwrap().unwrap();

        assert_eq!(first.generation, 10);
        assert!(matches!(first.usability, NetworkUsability::Unusable { .. }));
    }
}
