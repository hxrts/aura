//! Simulated time handler for deterministic testing
//!
//! Provides controlled time advancement for reproducible tests.

use crate::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use async_trait::async_trait;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};
use uuid::Uuid;

/// Simulated time handler for deterministic testing
pub struct SimulatedTimeHandler {
    /// Current simulated time in milliseconds
    current_time: Arc<RwLock<u64>>,
    /// Registered contexts waiting for events
    contexts: Arc<RwLock<HashMap<Uuid, ContextInfo>>>,
    /// Scheduled timeouts
    timeouts: Arc<Mutex<BTreeMap<u64, Vec<TimeoutHandle>>>>, // epoch -> handles
    /// Watch for time changes
    time_sender: Arc<Mutex<watch::Sender<u64>>>,
    time_receiver: watch::Receiver<u64>,
}

#[derive(Debug)]
struct ContextInfo {
    registered_at: u64,
    last_activity: u64,
    waiting_condition: Option<WakeCondition>,
}

impl SimulatedTimeHandler {
    /// Create a new simulated time handler starting at epoch 0
    pub fn new() -> Self {
        Self::new_at_time(0)
    }

    /// Create a new simulated time handler starting at a specific time
    pub fn new_at_time(start_time: u64) -> Self {
        let (sender, receiver) = watch::channel(start_time);

        Self {
            current_time: Arc::new(RwLock::new(start_time)),
            contexts: Arc::new(RwLock::new(HashMap::new())),
            timeouts: Arc::new(Mutex::new(BTreeMap::new())),
            time_sender: Arc::new(Mutex::new(sender)),
            time_receiver: receiver,
        }
    }

    /// Advance simulated time to a specific epoch
    pub async fn advance_to(&self, target_time: u64) -> Result<(), TimeError> {
        let mut current = self.current_time.write().await;
        if target_time < *current {
            return Err(TimeError::InvalidEpoch { epoch: target_time });
        }

        *current = target_time;

        // Notify all watchers of time change
        if let Ok(sender) = self.time_sender.try_lock() {
            let _ = sender.send(target_time);
        }

        // Process expired timeouts
        self.process_expired_timeouts(target_time).await;

        Ok(())
    }

    /// Advance simulated time by a duration
    pub async fn advance_by(&self, duration_ms: u64) -> Result<(), TimeError> {
        let current = *self.current_time.read().await;
        self.advance_to(current + duration_ms).await
    }

    /// Get current simulated time
    pub async fn current_time(&self) -> u64 {
        *self.current_time.read().await
    }

    /// Process timeouts that have expired at the given time
    async fn process_expired_timeouts(&self, current_time: u64) {
        let mut timeouts = self.timeouts.lock().await;

        // Find all expired timeouts
        let expired_times: Vec<u64> = timeouts
            .range(..=current_time)
            .map(|(time, _)| *time)
            .collect();

        // Remove expired timeouts
        for expired_time in expired_times {
            timeouts.remove(&expired_time);
            // TODO fix - In a real implementation, this would trigger timeout callbacks
        }
    }

    /// Set a wake condition for a context
    pub async fn set_wake_condition(&self, context_id: Uuid, condition: WakeCondition) {
        let mut contexts = self.contexts.write().await;
        if let Some(context) = contexts.get_mut(&context_id) {
            context.waiting_condition = Some(condition);
        }
    }

    /// Check if any contexts should wake up due to the given condition
    pub async fn check_wake_conditions(&self, condition: &WakeCondition) -> Vec<Uuid> {
        let contexts = self.contexts.read().await;
        contexts
            .iter()
            .filter_map(|(id, info)| {
                if let Some(waiting) = &info.waiting_condition {
                    if self.conditions_match(waiting, condition) {
                        Some(*id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    /// Check if two conditions match
    fn conditions_match(&self, waiting: &WakeCondition, occurred: &WakeCondition) -> bool {
        match (waiting, occurred) {
            (WakeCondition::NewEvents, WakeCondition::NewEvents) => true,
            (
                WakeCondition::EpochReached { target },
                WakeCondition::EpochReached { target: current },
            ) => current >= target,
            (WakeCondition::TimeoutAt(target), WakeCondition::TimeoutAt(current)) => {
                *current >= *target
            }
            (WakeCondition::Custom(expected), WakeCondition::Custom(actual)) => expected == actual,
            (
                WakeCondition::TimeoutExpired {
                    timeout_id: expected_id,
                },
                WakeCondition::TimeoutExpired {
                    timeout_id: actual_id,
                },
            ) => expected_id == actual_id,
            (WakeCondition::Immediate, _) => true,
            _ => false,
        }
    }
}

impl Default for SimulatedTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimeEffects for SimulatedTimeHandler {
    async fn current_epoch(&self) -> u64 {
        *self.current_time.read().await
    }

    async fn current_timestamp(&self) -> u64 {
        (*self.current_time.read().await) / 1000 // Convert ms to seconds
    }

    async fn current_timestamp_millis(&self) -> u64 {
        *self.current_time.read().await
    }

    async fn sleep_ms(&self, ms: u64) {
        let current = *self.current_time.read().await;
        let target = current + ms;

        // In simulation, sleeping means waiting for time to advance to target
        let mut receiver = self.time_receiver.clone();
        loop {
            let current_time = *receiver.borrow();
            if current_time >= target {
                break;
            }

            // Wait for time change
            if receiver.changed().await.is_err() {
                break; // Time sender was dropped
            }
        }
    }

    async fn delay(&self, duration: std::time::Duration) {
        self.sleep_ms(duration.as_millis() as u64).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), aura_core::AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn sleep_until(&self, epoch: u64) {
        let mut receiver = self.time_receiver.clone();
        loop {
            let current_time = *receiver.borrow();
            if current_time >= epoch {
                break;
            }

            // Wait for time change
            if receiver.changed().await.is_err() {
                break; // Time sender was dropped
            }
        }
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        match condition {
            WakeCondition::EpochReached { target } => {
                self.sleep_until(target).await;
                Ok(())
            }
            WakeCondition::TimeoutAt(timeout_epoch) => {
                let current = self.current_epoch().await;
                if timeout_epoch <= current {
                    Err(TimeError::Timeout {
                        timeout_ms: timeout_epoch.saturating_sub(current),
                    })
                } else {
                    self.sleep_until(timeout_epoch).await;
                    Ok(())
                }
            }
            WakeCondition::Immediate => Ok(()),
            _ => {
                // For other conditions, just yield once
                tokio::task::yield_now().await;
                Ok(())
            }
        }
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), aura_core::AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            aura_core::AuraError::internal(format!("System time error: wait_until failed: {}", e))
        })
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let handle = TimeoutHandle::new_v4();
        let current = self.current_epoch().await;
        let timeout_time = current + timeout_ms;

        let mut timeouts = self.timeouts.lock().await;
        timeouts
            .entry(timeout_time)
            .or_default()
            .push(handle.clone());

        handle
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        let mut timeouts = self.timeouts.lock().await;

        // Find and remove the timeout handle
        for (_, handles) in timeouts.iter_mut() {
            if let Some(pos) = handles.iter().position(|h| *h == handle) {
                handles.remove(pos);
                return Ok(());
            }
        }

        Err(TimeError::TimeoutNotFound {
            handle: handle.to_string(),
        })
    }

    fn is_simulated(&self) -> bool {
        true
    }

    fn register_context(&self, context_id: Uuid) {
        let contexts = self.contexts.clone();
        let current_time = self.current_time.clone();

        tokio::spawn(async move {
            let time = *current_time.read().await;
            let mut contexts = contexts.write().await;
            contexts.insert(
                context_id,
                ContextInfo {
                    registered_at: time,
                    last_activity: time,
                    waiting_condition: None,
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
        let current = self.current_epoch().await;
        let mut contexts = self.contexts.write().await;
        for context in contexts.values_mut() {
            context.last_activity = current;
        }

        // Wake up contexts waiting for new events
        let waiting_contexts: Vec<Uuid> = contexts
            .iter()
            .filter_map(|(id, info)| {
                if matches!(info.waiting_condition, Some(WakeCondition::NewEvents)) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect();

        // Clear waiting conditions for contexts that should wake up
        for context_id in waiting_contexts {
            if let Some(context) = contexts.get_mut(&context_id) {
                context.waiting_condition = None;
            }
        }
    }

    fn resolution_ms(&self) -> u64 {
        1 // Simulated time can have any resolution, default to 1ms
    }
}
