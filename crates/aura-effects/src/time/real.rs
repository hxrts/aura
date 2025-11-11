//! Real time effect handler for production use

use aura_core::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use aura_core::AuraError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time;
use uuid::Uuid;

/// Context registry for managing time contexts
#[derive(Debug, Default)]
struct ContextRegistry {
    contexts: HashMap<Uuid, broadcast::Sender<()>>,
    timeouts: HashMap<Uuid, tokio::task::JoinHandle<()>>,
}

/// Real time handler for production use
#[derive(Debug, Clone)]
pub struct RealTimeHandler {
    registry: Arc<RwLock<ContextRegistry>>,
}

impl Default for RealTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl RealTimeHandler {
    /// Create a new real time handler
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(ContextRegistry::default())),
        }
    }
}

#[async_trait]
impl TimeEffects for RealTimeHandler {
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    async fn current_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs()
    }

    async fn current_timestamp_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64
    }

    async fn sleep_ms(&self, ms: u64) {
        time::sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = self.current_timestamp_millis().await;
        if epoch > current {
            let wait_time = epoch - current;
            time::sleep(Duration::from_millis(wait_time)).await;
        }
    }

    async fn delay(&self, duration: Duration) {
        time::sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        time::sleep(Duration::from_millis(duration_ms)).await;
        Ok(())
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::NewEvents => {
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::EpochReached { target } => {
                // Wait until target epoch is reached
                let current_time = self.current_timestamp().await;
                if target > current_time {
                    let delay = target - current_time;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                Ok(())
            }
            WakeCondition::TimeoutAt(target_time) => {
                // Wait until target time is reached
                let current_time = self.current_timestamp().await;
                if target_time > current_time {
                    let delay = target_time - current_time;
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }
                Ok(())
            }
            WakeCondition::EventMatching(_) => {
                // Simplified: just yield for event matching
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::ThresholdEvents { threshold: _, timeout_ms } => {
                // Simplified: just wait for the timeout
                tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms)).await;
                Ok(())
            }
            WakeCondition::Immediate => {
                // Wake immediately - no wait
                Ok(())
            }
            WakeCondition::Custom(_) => {
                // Simplified: just yield for custom conditions
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::TimeoutExpired { timeout_id: _ } => {
                // Simplified: just yield for timeout expiration
                tokio::task::yield_now().await;
                Ok(())
            }
        }
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            AuraError::internal(format!("Wait condition failed: {}", e))
        })
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let handle = Uuid::new_v4();
        let registry = Arc::clone(&self.registry);
        let handle_clone = handle;
        
        let timeout_task = tokio::spawn(async move {
            time::sleep(Duration::from_millis(timeout_ms)).await;
            let mut reg = registry.write().await;
            reg.timeouts.remove(&handle_clone);
        });
        
        let mut registry = self.registry.write().await;
        registry.timeouts.insert(handle, timeout_task);
        handle
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut registry = self.registry.write().await;
        
        if let Some(task) = registry.timeouts.remove(&handle) {
            task.abort();
            Ok(())
        } else {
            Err(TimeError::TimeoutNotFound {
                handle: handle.to_string()
            })
        }
    }

    fn is_simulated(&self) -> bool {
        false
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
        1 // 1ms resolution
    }
}