//! Time effect handlers
//!
//! This module provides standard implementations of the `TimeEffects` trait
//! defined in `aura-core`. These handlers can be used by choreographic applications
//! and other Aura components.

use aura_macros::aura_effect_handlers;
use aura_core::{effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition}, AuraError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, RwLock};
use tokio::time;
use uuid::Uuid;

// Generate both real and simulated time handlers using the macro
aura_effect_handlers! {
    trait_name: TimeEffects,
    mock: {
        struct_name: SimulatedTimeHandler,
        state: {
            current_time: Arc<Mutex<u64>>,
            time_scale: f64,
        },
        features: {
            async_trait: true,
            deterministic: true,
        },
        methods: {
            current_epoch() -> u64 => {
                *self.current_time.lock().unwrap()
            },
            current_timestamp() -> u64 => {
                *self.current_time.lock().unwrap() / 1000
            },
            current_timestamp_millis() -> u64 => {
                *self.current_time.lock().unwrap()
            },
            sleep_ms(ms: u64) => {
                let scaled_duration = Duration::from_millis((ms as f64 / self.time_scale) as u64);
                if self.time_scale > 100.0 {
                    self.advance_time(ms);
                } else {
                    tokio::time::sleep(scaled_duration).await;
                    self.advance_time(ms);
                }
            },
            sleep_until(epoch: u64) => {
                let current = self.current_timestamp_millis().await;
                if epoch > current {
                    let wait_time = epoch - current;
                    self.sleep_ms(wait_time).await;
                }
            },
            delay(duration: Duration) => {
                self.sleep_ms(duration.as_millis() as u64).await;
            },
            sleep(duration_ms: u64) -> Result<(), AuraError> => {
                self.sleep_ms(duration_ms).await;
                Ok(())
            },
            yield_until(condition: WakeCondition) -> Result<(), TimeError> => {
                if self.time_scale > 10.0 {
                    self.advance_time(1);
                } else {
                    tokio::task::yield_now().await;
                }
                Ok(())
            },
            wait_until(condition: WakeCondition) -> Result<(), AuraError> => {
                self.yield_until(condition)
                    .await
                    .map_err(|e| AuraError::internal(format!("Wait condition failed: {}", e)))
            },
            set_timeout(timeout_ms: u64) -> TimeoutHandle => {
                Uuid::new_v4()
            },
            cancel_timeout(handle: TimeoutHandle) -> Result<(), TimeError> => {
                Ok(())
            },
            is_simulated() -> bool => {
                true
            },
            register_context(context_id: Uuid) => {
                // In simulation, track registered contexts
            },
            unregister_context(context_id: Uuid) => {
                // In simulation, track unregistered contexts  
            },
            notify_events_available() => {
                self.advance_time(1);
            },
            resolution_ms() -> u64 => {
                1
            },
        },
    },
    real: {
        struct_name: RealTimeHandler,
        state: {
            registry: Arc<RwLock<ContextRegistry>>,
        },
        features: {
            async_trait: true,
            disallowed_methods: true,
        },
        methods: {
            current_epoch() -> u64 => {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64
            },
            current_timestamp() -> u64 => {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_secs()
            },
            current_timestamp_millis() -> u64 => {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or(Duration::ZERO)
                    .as_millis() as u64
            },
            sleep_ms(ms: u64) => {
                time::sleep(Duration::from_millis(ms)).await;
            },
            sleep_until(epoch: u64) => {
                let current = self.current_timestamp_millis().await;
                if epoch > current {
                    let wait_time = epoch - current;
                    time::sleep(Duration::from_millis(wait_time)).await;
                }
            },
            delay(duration: Duration) => {
                time::sleep(duration).await;
            },
            sleep(duration_ms: u64) -> Result<(), AuraError> => {
                time::sleep(Duration::from_millis(duration_ms)).await;
                Ok(())
            },
            yield_until(condition: WakeCondition) -> Result<(), TimeError> => {
                match condition {
                    WakeCondition::NewEvents => {
                        tokio::task::yield_now().await;
                        Ok(())
                    }
                    WakeCondition::EpochReached { target } => {
                        let current_time = self.current_timestamp().await;
                        if target > current_time {
                            let delay = target - current_time;
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                        }
                        Ok(())
                    }
                    WakeCondition::TimeoutAt(target_time) => {
                        let current_time = self.current_timestamp().await;
                        if target_time > current_time {
                            let delay = target_time - current_time;
                            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                        }
                        Ok(())
                    }
                    WakeCondition::EventMatching(_) => {
                        tokio::task::yield_now().await;
                        Ok(())
                    }
                    WakeCondition::ThresholdEvents { threshold: _, timeout_ms } => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms)).await;
                        Ok(())
                    }
                    WakeCondition::Immediate => Ok(()),
                    WakeCondition::Custom(_) => {
                        tokio::task::yield_now().await;
                        Ok(())
                    }
                    WakeCondition::TimeoutExpired { timeout_id: _ } => {
                        tokio::task::yield_now().await;
                        Ok(())
                    }
                }
            },
            wait_until(condition: WakeCondition) -> Result<(), AuraError> => {
                self.yield_until(condition)
                    .await
                    .map_err(|e| AuraError::internal(format!("Wait condition failed: {}", e)))
            },
            set_timeout(timeout_ms: u64) -> TimeoutHandle => {
                let handle = Uuid::new_v4();
                let registry = Arc::clone(&self.registry);
                let handle_clone = handle;
                let timeout_task = tokio::spawn(async move {
                    time::sleep(Duration::from_millis(timeout_ms)).await;
                    let mut reg = registry.write().await;
                    reg.timeouts.remove(&handle_clone);
                });
                let registry = Arc::clone(&self.registry);
                tokio::spawn(async move {
                    let mut reg = registry.write().await;
                    reg.timeouts.insert(handle, timeout_task);
                });
                handle
            },
            cancel_timeout(handle: TimeoutHandle) -> Result<(), TimeError> => {
                let mut registry = self.registry.write().await;
                if let Some(task) = registry.timeouts.remove(&handle) {
                    task.abort();
                    Ok(())
                } else {
                    Err(TimeError::TimeoutNotFound {
                        handle: handle.to_string(),
                    })
                }
            },
            is_simulated() -> bool => {
                false
            },
            register_context(context_id: Uuid) => {
                let registry = Arc::clone(&self.registry);
                tokio::spawn(async move {
                    let mut reg = registry.write().await;
                    let (tx, _) = broadcast::channel(100);
                    reg.contexts.insert(context_id, tx);
                });
            },
            unregister_context(context_id: Uuid) => {
                let registry = Arc::clone(&self.registry);
                tokio::spawn(async move {
                    let mut reg = registry.write().await;
                    reg.contexts.remove(&context_id);
                });
            },
            notify_events_available() => {
                let registry = self.registry.read().await;
                for (_, sender) in registry.contexts.iter() {
                    let _ = sender.send(());
                }
            },
            resolution_ms() -> u64 => {
                1
            },
        },
    },
}

/// Context registry for managing time contexts
#[derive(Debug, Default)]
struct ContextRegistry {
    contexts: HashMap<Uuid, broadcast::Sender<()>>,
    timeouts: HashMap<Uuid, tokio::task::JoinHandle<()>>,
}

#[allow(clippy::disallowed_methods)]
impl SimulatedTimeHandler {
    /// Create a new simulated time handler starting at the given time
    pub fn new_with_time(start_time_ms: u64) -> Self {
        Self::new_deterministic(start_time_ms, 1.0)
    }

    /// Create a simulated time handler starting at Unix epoch
    pub fn new_at_epoch() -> Self {
        Self::new_with_time(0)
    }

    /// Create a simulated time handler with custom time scale
    pub fn with_time_scale(start_time_ms: u64, time_scale: f64) -> Self {
        Self::new_deterministic(start_time_ms, time_scale)
    }

    /// Advance simulated time by the given duration
    pub fn advance_time(&self, duration_ms: u64) {
        let mut time = self.current_time.lock().unwrap();
        *time += duration_ms;
    }

    /// Set the absolute simulated time
    pub fn set_time(&self, time_ms: u64) {
        let mut time = self.current_time.lock().unwrap();
        *time = time_ms;
    }

    /// Get the current simulated time (for testing)
    pub fn get_time(&self) -> u64 {
        *self.current_time.lock().unwrap()
    }

    /// Reset time to epoch (for testing)
    pub fn reset(&self) {
        self.set_time(0);
    }

    /// Set the time scale for simulation speed
    pub fn set_time_scale(&mut self, scale: f64) {
        self.time_scale = scale;
    }
}

#[allow(clippy::disallowed_methods)]
impl RealTimeHandler {
    /// Create a new real time handler
    pub fn new_real() -> Self {
        Self::new()
    }
}
