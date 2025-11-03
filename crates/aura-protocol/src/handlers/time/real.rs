//! Real time handler using system time
//!
//! Provides actual system time operations for production use.

use crate::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration, Instant};
use uuid::Uuid;

/// Real time handler for production use
pub struct RealTimeHandler {
    contexts: Arc<RwLock<HashMap<Uuid, ContextInfo>>>,
    timeouts: Arc<Mutex<HashMap<TimeoutHandle, tokio::task::JoinHandle<()>>>>,
}

#[derive(Debug)]
struct ContextInfo {
    registered_at: Instant,
    last_activity: Instant,
}

impl RealTimeHandler {
    /// Create a new real time handler
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            timeouts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get current system time as epoch milliseconds
    fn current_time_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for RealTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimeEffects for RealTimeHandler {
    async fn current_epoch(&self) -> u64 {
        Self::current_time_ms()
    }

    async fn sleep_ms(&self, ms: u64) {
        sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        let current = Self::current_time_ms();
        if epoch > current {
            let sleep_duration = epoch - current;
            self.sleep_ms(sleep_duration).await;
        }
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::NewEvents => {
                // In a real implementation, this would wait for actual events
                // For now, just yield briefly
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::EpochReached(target_epoch) => {
                self.sleep_until(target_epoch).await;
                Ok(())
            }
            WakeCondition::TimeoutAt(timeout_epoch) => {
                let current = Self::current_time_ms();
                if timeout_epoch <= current {
                    Err(TimeError::Timeout {
                        timeout_ms: timeout_epoch.saturating_sub(current),
                    })
                } else {
                    self.sleep_until(timeout_epoch).await;
                    Ok(())
                }
            }
            WakeCondition::Custom(_) => {
                // Custom conditions would need specific implementations
                tokio::task::yield_now().await;
                Ok(())
            }
            WakeCondition::Immediate => {
                // Return immediately
                Ok(())
            }
        }
    }

    async fn set_timeout(&self, _timeout_ms: u64) -> TimeoutHandle {
        let handle = TimeoutHandle::new();
        let _handle_clone = handle.clone();

        let task = tokio::spawn(async move {
            // TODO: sleep(Duration::from_millis(_timeout_ms)).await;
            // Timeout expired - in a real implementation, this would trigger callbacks
        });

        self.timeouts.lock().await.insert(handle.clone(), task);
        handle
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut timeouts = self.timeouts.lock().await;
        if let Some(task) = timeouts.remove(&handle) {
            task.abort();
            Ok(())
        } else {
            Err(TimeError::TimeoutNotFound { handle })
        }
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, context_id: Uuid) {
        let contexts = self.contexts.clone();
        tokio::spawn(async move {
            let mut contexts = contexts.write().await;
            contexts.insert(
                context_id,
                ContextInfo {
                    registered_at: Instant::now(),
                    last_activity: Instant::now(),
                },
            );
        });
    }

    fn unregister_context(&self, context_id: Uuid) {
        let contexts = self.contexts.clone();
        tokio::spawn(async move {
            let mut contexts = contexts.write().await;
            contexts.remove(&context_id);
        });
    }

    async fn notify_events_available(&self) {
        // Update last activity for all contexts
        let mut contexts = self.contexts.write().await;
        let now = Instant::now();
        for context in contexts.values_mut() {
            context.last_activity = now;
        }
        // In a real implementation, this would wake up waiting contexts
    }

    fn resolution_ms(&self) -> u64 {
        1 // System time has millisecond resolution
    }
}
