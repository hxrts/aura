//! Timeout Coordinator - Multi-Context Timeout Management
//!
//! **Layer 4 (aura-protocol)**: Stateful multi-context coordination handler.
//!
//! This module was extracted from aura-effects/src/time.rs RealTimeHandler because it violates
//! the Layer 3 principle of "stateless, single-party, context-free" handlers. The timeout
//! coordination logic maintains shared state across multiple contexts, making it multi-party
//! coordination logic that belongs in the orchestration layer.
//!
//! Key violations that required the extraction:
//! - Maintains global timeout registry (`Arc<RwLock<HashMap<Uuid, JoinHandle>>>`)
//! - Maintains global context registry (`Arc<RwLock<HashMap<Uuid, broadcast::Sender>>>`)
//! - Manages timeouts across multiple contexts (multi-party coordination)
//! - Broadcasts events to all registered contexts
//! - Tracks timeout tasks globally for cancellation

use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

/// Context registry for managing time contexts and timeouts
#[derive(Debug, Default)]
struct ContextRegistry {
    contexts: HashMap<Uuid, broadcast::Sender<()>>,
    timeouts: HashMap<Uuid, tokio::task::JoinHandle<()>>,
}

/// Timeout coordinator that adds multi-context coordination to a base TimeEffects handler
#[derive(Debug, Clone)]
pub struct TimeoutCoordinator<T> {
    /// Base time handler for stateless operations
    inner: T,
    /// Shared registry for coordinating timeouts and contexts
    registry: Arc<RwLock<ContextRegistry>>,
}

impl<T: TimeEffects + Clone> TimeoutCoordinator<T> {
    /// Create a new timeout coordinator wrapping a base time handler
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            registry: Arc::new(RwLock::new(ContextRegistry::default())),
        }
    }
}

#[async_trait]
impl<T: TimeEffects + Clone + Send + Sync> TimeEffects for TimeoutCoordinator<T> {
    // Delegate stateless operations to inner handler

    async fn current_epoch(&self) -> u64 {
        self.inner.current_epoch().await
    }

    async fn current_timestamp(&self) -> u64 {
        self.inner.current_timestamp().await
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.inner.current_timestamp_millis().await
    }

    async fn sleep_ms(&self, ms: u64) {
        self.inner.sleep_ms(ms).await
    }

    async fn sleep_until(&self, epoch: u64) {
        self.inner.sleep_until(epoch).await
    }

    async fn delay(&self, duration: Duration) {
        self.inner.delay(duration).await
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.inner.sleep(duration_ms).await
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        self.inner.yield_until(condition).await
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.inner.wait_until(condition).await
    }

    // Coordination methods (Layer 4)

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let handle = Uuid::new_v4();
        let registry = Arc::clone(&self.registry);
        let handle_clone = handle;
        let timeout_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
            let mut reg = registry.write().await;
            reg.timeouts.remove(&handle_clone);
        });
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let mut reg = registry.write().await;
            reg.timeouts.insert(handle, timeout_task);
        });
        handle
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut registry = self.registry.write().await;
        if let Some(task) = registry.timeouts.remove(&handle) {
            task.abort();
            Ok(())
        } else {
            Err(TimeError::TimeoutNotFound {
                handle: handle.to_string(),
            })
        }
    }

    fn is_simulated(&self) -> bool {
        self.inner.is_simulated()
    }

    fn register_context(&self, context_id: Uuid) {
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let mut reg = registry.write().await;
            let (tx, _) = broadcast::channel(100);
            reg.contexts.insert(context_id, tx);
        });
    }

    fn unregister_context(&self, context_id: Uuid) {
        let registry = Arc::clone(&self.registry);
        tokio::spawn(async move {
            let mut reg = registry.write().await;
            reg.contexts.remove(&context_id);
        });
    }

    async fn notify_events_available(&self) {
        let registry = self.registry.read().await;
        for (_, sender) in registry.contexts.iter() {
            let _ = sender.send(());
        }
    }

    fn resolution_ms(&self) -> u64 {
        self.inner.resolution_ms()
    }
}
