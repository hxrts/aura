//! Enhanced time handler for production use with advanced scheduling capabilities

use crate::effects::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};
use async_trait::async_trait;
use aura_core::AuraError;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Notify, RwLock};
use tokio::time::{sleep, sleep_until, Instant};
use tracing::{debug, info, warn};
use uuid::Uuid;

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
            let mut receiver_opt = timeout_rx.write().await.take();

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
        let now = Instant::now();
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
        // TODO fix - For now, tasks are removed when they complete
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
                // TODO fix - In a real implementation, this would track event timestamps
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
                // TODO fix - Simplified: return true after short delay
                // Real implementation would check actual event matching
                true
            }
            WakeCondition::ThresholdEvents { threshold, .. } => {
                let event_count = self.get_event_count().await;
                event_count >= *threshold
            }
            WakeCondition::TimeoutExpired { .. } => {
                // TODO fix - For now, always false since we can't track expired timeouts
                false
            }
            WakeCondition::Custom(_) => {
                // Custom conditions would be handled by external logic
                false
            }
        }
    }
}

impl Default for EnhancedTimeHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TimeEffects for EnhancedTimeHandler {
    async fn current_epoch(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    async fn current_timestamp(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    async fn current_timestamp_millis(&self) -> u64 {
        self.current_epoch().await
    }

    async fn sleep_ms(&self, ms: u64) {
        debug!("Sleeping for {} milliseconds", ms);

        let mut stats = self.stats.write().await;
        stats.total_sleeps += 1;
        drop(stats);

        sleep(Duration::from_millis(ms)).await;
    }

    async fn sleep_until(&self, epoch: u64) {
        debug!("Sleeping until epoch: {}", epoch);

        let current = self.current_epoch().await;
        if epoch <= current {
            return; // Already past target time
        }

        let sleep_duration = epoch - current;
        self.sleep_ms(sleep_duration).await;
    }

    async fn delay(&self, duration: Duration) {
        debug!("Delaying for {:?}", duration);

        let mut stats = self.stats.write().await;
        stats.total_sleeps += 1;
        drop(stats);

        sleep(duration).await;
    }

    async fn sleep(&self, duration_ms: u64) -> Result<(), AuraError> {
        self.sleep_ms(duration_ms).await;
        Ok(())
    }

    async fn yield_until(&self, condition: WakeCondition) -> Result<(), TimeError> {
        debug!("Yielding until condition: {:?}", condition);

        let mut stats = self.stats.write().await;
        stats.total_yield_operations += 1;
        drop(stats);

        // Handle timeout-based conditions immediately
        match &condition {
            WakeCondition::TimeoutAt(target_timestamp) => {
                let current = self.current_timestamp().await;
                if *target_timestamp > current {
                    let delay_seconds = target_timestamp - current;
                    sleep(Duration::from_secs(delay_seconds)).await;
                }
                return Ok(());
            }
            WakeCondition::ThresholdEvents { timeout_ms, .. } => {
                let timeout_duration = Duration::from_millis(*timeout_ms);
                let start_time = Instant::now();

                loop {
                    if self.check_wake_condition(&condition).await {
                        return Ok(());
                    }

                    if start_time.elapsed() >= timeout_duration {
                        return Err(TimeError::Timeout {
                            timeout_ms: *timeout_ms,
                        });
                    }

                    sleep(Duration::from_millis(10)).await; // Poll every 10ms
                }
            }
            _ => {}
        }

        // For other conditions, create a scheduled task
        let task_id = Uuid::new_v4();
        let notifier = Arc::new(Notify::new());

        let task = ScheduledTask {
            id: task_id,
            wake_condition: condition.clone(),
            created_at: Instant::now(),
            context_id: None,
            notifier: notifier.clone(),
        };

        {
            let mut tasks = self.scheduled_tasks.write().await;
            tasks.insert(task_id, task);
        }

        // Wait for notification or condition to be satisfied
        loop {
            if self.check_wake_condition(&condition).await {
                break;
            }

            // Wait for notification with timeout
            tokio::select! {
                _ = notifier.notified() => {
                    // Check condition again after notification
                    continue;
                }
                _ = sleep(Duration::from_millis(100)) => {
                    // Periodic check
                    continue;
                }
            }
        }

        // Remove completed task
        {
            let mut tasks = self.scheduled_tasks.write().await;
            tasks.remove(&task_id);
        }

        Ok(())
    }

    async fn wait_until(&self, condition: WakeCondition) -> Result<(), AuraError> {
        self.yield_until(condition).await.map_err(|e| {
            AuraError::internal(format!("System time error: wait_until failed: {}", e))
        })
    }

    async fn set_timeout(&self, timeout_ms: u64) -> TimeoutHandle {
        debug!("Setting timeout for {} milliseconds", timeout_ms);

        let handle = Uuid::new_v4();
        let expires_at = Instant::now() + Duration::from_millis(timeout_ms);

        let timeout_task = TimeoutTask {
            id: handle,
            expires_at,
            completed: false,
        };

        {
            let mut timeouts = self.timeouts.write().await;
            timeouts.insert(handle, timeout_task);
        }

        let mut stats = self.stats.write().await;
        stats.total_timeouts_set += 1;
        stats.active_timeouts += 1;
        drop(stats);

        // Schedule timeout completion
        let timeout_tx = self.timeout_tx.clone();
        tokio::spawn(async move {
            sleep(Duration::from_millis(timeout_ms)).await;

            if let Some(ref sender) = *timeout_tx.read().await {
                let _ = sender.send(handle);
            }
        });

        handle
    }

    async fn cancel_timeout(&self, handle: TimeoutHandle) -> Result<(), TimeError> {
        debug!("Cancelling timeout: {}", handle);

        let mut timeouts = self.timeouts.write().await;
        if timeouts.remove(&handle).is_some() {
            let mut stats = self.stats.write().await;
            stats.total_timeouts_cancelled += 1;
            stats.active_timeouts = stats.active_timeouts.saturating_sub(1);

            info!("Timeout cancelled: {}", handle);
            Ok(())
        } else {
            warn!("Attempted to cancel non-existent timeout: {}", handle);
            Err(TimeError::ServiceUnavailable)
        }
    }

    fn is_simulated(&self) -> bool {
        false
    }

    fn register_context(&self, context_id: Uuid) {
        debug!("Registering context: {}", context_id);

        let contexts = self.contexts.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            let notifier = Arc::new(Notify::new());
            contexts.write().await.insert(context_id, notifier);

            let mut stats_guard = stats.write().await;
            stats_guard.active_contexts += 1;
        });
    }

    fn unregister_context(&self, context_id: Uuid) {
        debug!("Unregistering context: {}", context_id);

        let contexts = self.contexts.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            contexts.write().await.remove(&context_id);

            let mut stats_guard = stats.write().await;
            stats_guard.active_contexts = stats_guard.active_contexts.saturating_sub(1);
        });
    }

    async fn notify_events_available(&self) {
        debug!("Notifying all waiters that events are available");

        self.increment_event_count().await;
        self.global_notify.notify_waiters();

        // Notify all registered contexts
        let contexts = self.contexts.read().await;
        for notifier in contexts.values() {
            notifier.notify_waiters();
        }

        // Notify scheduled tasks
        let tasks = self.scheduled_tasks.read().await;
        for task in tasks.values() {
            task.notifier.notify_waiters();
        }
    }

    fn resolution_ms(&self) -> u64 {
        1 // 1 millisecond resolution
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn test_enhanced_time_handler_creation() {
        let handler = EnhancedTimeHandler::new();
        let stats = handler.get_statistics().await;

        assert_eq!(stats.total_sleeps, 0);
        assert_eq!(stats.active_contexts, 0);
        assert!(!handler.is_simulated());
    }

    #[tokio::test]
    async fn test_time_operations() {
        let handler = EnhancedTimeHandler::new();

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
        let handler = EnhancedTimeHandler::new();

        let start = Instant::now();
        handler.sleep_ms(50).await;
        let elapsed = start.elapsed();

        assert!(elapsed >= Duration::from_millis(40)); // Allow some variance
        assert!(elapsed < Duration::from_millis(100));

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_sleeps, 1);
    }

    #[tokio::test]
    async fn test_timeout_operations() {
        let handler = EnhancedTimeHandler::new();

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
        let handler = EnhancedTimeHandler::new();

        let context_id = Uuid::new_v4();

        // Register context
        handler.register_context(context_id);

        // Give some time for async registration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.active_contexts, 1);

        // Unregister context
        handler.unregister_context(context_id);

        // Give some time for async unregistration
        tokio::time::sleep(Duration::from_millis(10)).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.active_contexts, 0);
    }

    #[tokio::test]
    async fn test_yield_until_immediate() {
        let handler = EnhancedTimeHandler::new();

        let result = handler.yield_until(WakeCondition::Immediate).await;
        assert!(result.is_ok());

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_yield_operations, 1);
    }

    #[tokio::test]
    async fn test_yield_until_timeout() {
        let handler = EnhancedTimeHandler::new();

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
        let handler = Arc::new(EnhancedTimeHandler::new());

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
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    handler.simulate_event().await;
                }
            }
        });

        let result = timeout(Duration::from_millis(500), wait_future).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_notification() {
        let handler = EnhancedTimeHandler::new();

        // Start event count
        let initial_count = handler.get_event_count().await;

        // Notify events available
        handler.notify_events_available().await;

        let new_count = handler.get_event_count().await;
        assert_eq!(new_count, initial_count + 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_timeouts() {
        let handler = EnhancedTimeHandler::new();

        // Set a very short timeout
        let timeout_handle = handler.set_timeout(1).await;

        // Wait for it to expire
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Cleanup expired timeouts
        handler.cleanup_expired_timeouts().await;

        // Should no longer be able to cancel (it was cleaned up)
        let result = handler.cancel_timeout(timeout_handle).await;
        assert!(result.is_err());
    }
}
