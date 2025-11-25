//! Enhanced time handler for production use with advanced scheduling capabilities.

#![allow(clippy::disallowed_methods)]

use crate::effects::{TimeoutHandle, WakeCondition};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{AuraError, Result};
use aura_effects::time::monotonic_now;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time::Instant;
use uuid::Uuid;

// Fixed instant for deterministic timing (initialized once)
static EPOCH_INSTANT: OnceLock<std::time::Instant> = OnceLock::new();

/// Scheduled task information
#[derive(Debug, Clone)]
struct ScheduledTask {
    id: Uuid,
    wake_condition: WakeCondition,
    created_at: Instant,
    context_id: Option<Uuid>,
    notifier: Arc<Notify>,
}

/// Timeout task information
#[derive(Debug, Clone)]
struct TimeoutTask {
    id: TimeoutHandle,
    expires_at: Instant,
    completed: bool,
}

/// Enhanced time handler with scheduling, timeouts, and event-driven waking
pub struct EnhancedTimeHandler {
    /// Registered contexts for time events
    contexts: Arc<RwLock<HashMap<Uuid, Arc<Notify>>>>,
    /// Scheduled tasks waiting for conditions
    scheduled_tasks: Arc<RwLock<HashMap<Uuid, ScheduledTask>>>,
    /// Active timeout operations
    timeouts: Arc<RwLock<HashMap<TimeoutHandle, TimeoutTask>>>,
    /// Event counter for threshold-based waking
    event_count: Arc<RwLock<usize>>,
    /// Global notification for events
    global_notify: Arc<Notify>,
    /// Channel for timeout notifications
    timeout_tx: Arc<RwLock<Option<mpsc::UnboundedSender<TimeoutHandle>>>>,
    timeout_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<TimeoutHandle>>>>,
    /// Time handler statistics
    stats: Arc<RwLock<TimeHandlerStats>>,
    /// Underlying time provider for deterministic/testing overrides
    provider: Arc<dyn PhysicalTimeEffects>,
}

/// Statistics for the time handler
#[derive(Debug, Clone, Default)]
pub struct TimeHandlerStats {
    pub total_sleeps: u64,
    pub total_timeouts_set: u64,
    pub total_timeouts_cancelled: u64,
    pub total_yield_operations: u64,
    pub active_contexts: u64,
    pub active_timeouts: u64,
}

impl EnhancedTimeHandler {
    /// Create a new enhanced time handler
    pub fn new() -> Self {
        Self::with_provider(Arc::new(aura_effects::time::PhysicalTimeHandler))
    }

    /// Check if this is a simulated time handler (for testing)
    pub fn is_simulated(&self) -> bool {
        // In practice, this would check if provider is a mock/simulated implementation
        // For now, return false for production handlers
        false
    }

    /// Get current instant (for timing operations)
    /// Note: This method is for performance metrics only, not protocol logic
    /// Returns a synthetic instant based on physical time for deterministic behavior
    pub async fn now_instant(&self) -> tokio::time::Instant {
        // Use physical time converted to instant for deterministic metrics
        let current_ms = self.current_timestamp().await;

        // Initialize fixed epoch once for deterministic behavior
        let epoch_base = EPOCH_INSTANT.get_or_init(monotonic_now);

        // Create synthetic instant based on physical timestamp
        let duration = std::time::Duration::from_millis(current_ms % (24 * 60 * 60 * 1000));
        tokio::time::Instant::from_std(*epoch_base + duration)
    }

    /// Sleep for a given number of milliseconds
    pub async fn sleep_ms(&self, ms: u64) {
        let mut stats = self.stats.write().await;
        stats.total_sleeps += 1;
        drop(stats);

        // Delegate to the underlying provider
        let _ = self.provider.sleep_ms(ms).await;
    }

    /// Set a timeout and return a handle
    pub async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        let timeout_id = uuid::Uuid::new_v4();
        let current_instant = self.now_instant().await;
        let expires_at = current_instant + tokio::time::Duration::from_millis(timeout_ms);

        let timeout_task = TimeoutTask {
            id: timeout_id,
            expires_at,
            completed: false,
        };

        {
            let mut timeouts = self.timeouts.write().await;
            timeouts.insert(timeout_id, timeout_task);
        }

        {
            let mut stats = self.stats.write().await;
            stats.total_timeouts_set += 1;
            stats.active_timeouts += 1;
        }

        timeout_id
    }

    /// Cancel a timeout
    pub async fn cancel_timeout(&self, timeout_handle: TimeoutHandle) -> Result<()> {
        let mut timeouts = self.timeouts.write().await;

        if timeouts.remove(&timeout_handle).is_some() {
            let mut stats = self.stats.write().await;
            stats.total_timeouts_cancelled += 1;
            stats.active_timeouts = stats.active_timeouts.saturating_sub(1);
            Ok(())
        } else {
            Err(AuraError::not_found("Timeout not found"))
        }
    }

    /// Register a context for time notifications
    pub fn register_context(&self, context_id: uuid::Uuid) {
        let notifier = Arc::new(tokio::sync::Notify::new());
        let contexts = self.contexts.clone();

        tokio::spawn(async move {
            let mut contexts_guard = contexts.write().await;
            contexts_guard.insert(context_id, notifier);
        });
    }

    /// Unregister a context
    pub fn unregister_context(&self, context_id: uuid::Uuid) {
        let contexts = self.contexts.clone();

        tokio::spawn(async move {
            let mut contexts_guard = contexts.write().await;
            contexts_guard.remove(&context_id);
        });
    }

    /// Yield until a wake condition is met
    pub async fn yield_until(&self, condition: WakeCondition) -> Result<()> {
        let mut stats = self.stats.write().await;
        stats.total_yield_operations += 1;
        drop(stats);

        match condition {
            WakeCondition::Immediate => Ok(()),
            WakeCondition::TimeoutAt(target_timestamp) => {
                let current = self.current_timestamp().await;
                if current >= target_timestamp {
                    return Ok(());
                }

                let sleep_duration = target_timestamp.saturating_sub(current);
                self.sleep_ms(sleep_duration).await;
                Ok(())
            }
            WakeCondition::ThresholdEvents {
                threshold,
                timeout_ms,
            } => {
                let start_count = self.get_event_count().await;
                let target_count = start_count + threshold;

                let timeout_future = self.sleep_ms(timeout_ms);
                let wait_future = async {
                    loop {
                        let current_count = self.get_event_count().await;
                        if current_count >= target_count {
                            return Ok(());
                        }

                        // Wait for global notification
                        self.global_notify.notified().await;
                    }
                };

                tokio::select! {
                    result = wait_future => result,
                    _ = timeout_future => Err(AuraError::invalid("Threshold events not reached within timeout")),
                }
            }
            _ => {
                // For other conditions, check if satisfied immediately
                if self.check_wake_condition(&condition).await {
                    Ok(())
                } else {
                    Err(AuraError::invalid(
                        "Wake condition not supported or not met",
                    ))
                }
            }
        }
    }

    /// Notify that events are available
    pub async fn notify_events_available(&self) {
        self.increment_event_count().await;
        self.global_notify.notify_waiters();
    }

    /// Current timestamp in milliseconds
    pub async fn current_timestamp(&self) -> u64 {
        self.provider
            .physical_time()
            .await
            .map(|p| p.ts_ms)
            .unwrap_or_default()
    }

    /// Current epoch in milliseconds (alias)
    pub async fn current_epoch(&self) -> u64 {
        self.current_timestamp().await
    }

    /// Millisecond precision helper
    pub async fn current_timestamp_millis(&self) -> u64 {
        self.current_timestamp().await
    }

    /// Create a new enhanced time handler with explicit time provider
    pub fn with_provider(provider: Arc<dyn PhysicalTimeEffects>) -> Self {
        let (timeout_tx, timeout_rx) = mpsc::unbounded_channel();

        let handler = Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            scheduled_tasks: Arc::new(RwLock::new(HashMap::new())),
            timeouts: Arc::new(RwLock::new(HashMap::new())),
            event_count: Arc::new(RwLock::new(0)),
            global_notify: Arc::new(Notify::new()),
            timeout_tx: Arc::new(RwLock::new(Some(timeout_tx))),
            timeout_rx: Arc::new(RwLock::new(Some(timeout_rx))),
            stats: Arc::new(RwLock::new(TimeHandlerStats::default())),
            provider,
        };

        // Start the timeout processing background task
        handler.start_timeout_processor();

        handler
    }

    /// Start the background task that processes timeouts
    fn start_timeout_processor(&self) {
        let timeouts = self.timeouts.clone();
        let timeout_rx = self.timeout_rx.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            let receiver_opt = timeout_rx.write().await.take();

            if let Some(mut receiver) = receiver_opt {
                while let Some(timeout_id) = receiver.recv().await {
                    // Process timeout completion
                    let mut timeouts_guard = timeouts.write().await;
                    if let Some(timeout_task) = timeouts_guard.get_mut(&timeout_id) {
                        timeout_task.completed = true;
                    }

                    // Update statistics
                    let mut stats_guard = stats.write().await;
                    stats_guard.active_timeouts = stats_guard.active_timeouts.saturating_sub(1);
                }
            }
        });
    }

    /// Get current time handler statistics
    pub async fn get_statistics(&self) -> TimeHandlerStats {
        let mut stats = self.stats.read().await.clone();
        stats.active_contexts = self.contexts.read().await.len() as u64;
        stats.active_timeouts = self.timeouts.read().await.len() as u64;
        stats
    }

    /// Cleanup expired timeouts
    pub async fn cleanup_expired_timeouts(&self) {
        let now = self.now_instant().await;
        let mut timeouts = self.timeouts.write().await;

        let expired_ids: Vec<TimeoutHandle> = timeouts
            .iter()
            .filter(|(_, task)| task.expires_at <= now)
            .map(|(id, _)| *id)
            .collect();

        for id in expired_ids {
            timeouts.remove(&id);
        }
    }

    /// Cleanup completed scheduled tasks
    pub async fn cleanup_completed_tasks(&self) {
        // This would be called periodically to clean up completed tasks
        // Tasks are removed when they complete; no background cleanup required yet.
    }

    /// Simulate external event arrival (for testing)
    pub async fn simulate_event(&self) {
        self.increment_event_count().await;
        self.global_notify.notify_waiters();
    }

    /// Increment the global event count
    async fn increment_event_count(&self) {
        let mut count = self.event_count.write().await;
        *count += 1;
    }

    /// Get current event count
    async fn get_event_count(&self) -> usize {
        *self.event_count.read().await
    }

    /// Process wake condition and return whether it's satisfied
    async fn check_wake_condition(&self, condition: &WakeCondition) -> bool {
        match condition {
            WakeCondition::Immediate => true,
            WakeCondition::NewEvents => {
                // For simplicity, always true since we can't track "new" events
                // In a real implementation, this would track event timestamps
                true
            }
            WakeCondition::EpochReached { target } => {
                let current_epoch = self.current_epoch().await;
                current_epoch >= *target
            }
            WakeCondition::TimeoutAt(target_timestamp) => {
                let current_timestamp = self.current_timestamp().await;
                current_timestamp >= *target_timestamp
            }
            WakeCondition::EventMatching(_criteria) => {
                // Simplified: return true after short delay
                // Real implementation would check actual event matching
                true
            }
            WakeCondition::ThresholdEvents { threshold, .. } => {
                let event_count = self.get_event_count().await;
                event_count >= *threshold
            }
            WakeCondition::TimeoutExpired { .. } => {
                // For now, always false since we can't track expired timeouts
                false
            }
            WakeCondition::Custom(_) => {
                // Custom conditions would be handled by external logic
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl aura_core::effects::PhysicalTimeEffects for EnhancedTimeHandler {
    async fn physical_time(
        &self,
    ) -> std::result::Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        self.provider.physical_time().await
    }

    async fn sleep_ms(&self, ms: u64) -> std::result::Result<(), aura_core::effects::TimeError> {
        self.sleep_ms(ms).await;
        Ok(())
    }
}

impl Default for EnhancedTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_enhanced_time_handler_creation() {
        let handler = EnhancedTimeHandler::default();
        let stats = handler.get_statistics().await;

        assert_eq!(stats.total_sleeps, 0);
        assert_eq!(stats.active_contexts, 0);
        assert!(!handler.is_simulated());
    }

    #[tokio::test]
    async fn test_time_operations() {
        let handler = EnhancedTimeHandler::default();

        let current_epoch = handler.current_epoch().await;
        let current_timestamp = handler.current_timestamp().await;
        let current_timestamp_millis = handler.current_timestamp_millis().await;

        assert!(current_epoch > 0);
        assert!(current_timestamp > 0);
        assert!(current_timestamp_millis > 0);
        assert_eq!(current_epoch, current_timestamp_millis);
    }

    #[tokio::test]
    async fn test_sleep_operations() {
        let handler = EnhancedTimeHandler::default();

        let start_time = handler.current_timestamp().await;
        handler.sleep_ms(50).await;
        let end_time = handler.current_timestamp().await;

        // Just verify sleep was recorded, not actual timing
        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_sleeps, 1);

        // In a real test environment with actual sleep, end_time would be later
        // But for mocked time, this just verifies the call succeeded
        assert!(end_time >= start_time);
    }

    #[tokio::test]
    async fn test_timeout_operations() {
        let handler = EnhancedTimeHandler::default();

        // Set timeout
        let timeout_handle = handler.set_timeout(100).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_timeouts_set, 1);
        assert_eq!(stats.active_timeouts, 1);

        // Cancel timeout
        handler.cancel_timeout(timeout_handle).await.unwrap();

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_timeouts_cancelled, 1);
    }

    #[tokio::test]
    async fn test_context_management() {
        let handler = EnhancedTimeHandler::default();

        let context_id = Uuid::new_v4();

        // Register context
        handler.register_context(context_id);

        // Give some time for async registration
        handler.sleep_ms(10).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.active_contexts, 1);

        // Unregister context
        handler.unregister_context(context_id);

        // Give some time for async unregistration
        handler.sleep_ms(10).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.active_contexts, 0);
    }

    #[tokio::test]
    async fn test_yield_until_immediate() {
        let handler = EnhancedTimeHandler::default();

        let result = handler.yield_until(WakeCondition::Immediate).await;
        assert!(result.is_ok());

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_yield_operations, 1);
    }

    #[tokio::test]
    async fn test_yield_until_timeout() {
        let handler = EnhancedTimeHandler::default();

        let current_time = handler.current_timestamp().await;
        let future_time = current_time + 100; // 100 seconds in future

        let result = timeout(
            Duration::from_millis(50),
            handler.yield_until(WakeCondition::TimeoutAt(future_time)),
        )
        .await;

        // Should not complete within 50ms timeout
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_yield_until_threshold_events() {
        let handler = Arc::new(EnhancedTimeHandler::default());

        // Start waiting for 3 events with 1 second timeout
        let wait_future = handler.yield_until(WakeCondition::ThresholdEvents {
            threshold: 3,
            timeout_ms: 1000,
        });

        // Simulate events
        tokio::spawn({
            let handler = handler.clone();
            async move {
                for _ in 0..3 {
                    handler.sleep_ms(10).await;
                    handler.simulate_event().await;
                }
            }
        });

        let result = timeout(Duration::from_millis(500), wait_future).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_notification() {
        let handler = EnhancedTimeHandler::default();

        // Start event count
        let initial_count = handler.get_event_count().await;

        // Notify events available
        handler.notify_events_available().await;

        let new_count = handler.get_event_count().await;
        assert_eq!(new_count, initial_count + 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_timeouts() {
        let handler = EnhancedTimeHandler::default();

        // Set a very short timeout
        let timeout_handle = handler.set_timeout(1).await;

        // Wait for it to expire
        handler.sleep_ms(10).await;

        // Cleanup expired timeouts
        handler.cleanup_expired_timeouts().await;

        // Should no longer be able to cancel (it was cleaned up)
        let result = handler.cancel_timeout(timeout_handle).await;
        assert!(result.is_err());
    }
}
