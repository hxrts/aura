//! Enhanced time handler for production use with advanced scheduling capabilities.

#![allow(clippy::disallowed_methods)]

use crate::effects::{TimeoutHandle, WakeCondition};
use async_lock::RwLock;
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::{AuraError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Timeout task information
#[derive(Debug, Clone)]
struct TimeoutTask {
    id: TimeoutHandle,
    expires_at_ms: u64,
    completed: bool,
}

/// Enhanced time handler with scheduling, timeouts, and event-driven waking
pub struct EnhancedTimeHandler {
    /// Registered contexts for time events
    contexts: Arc<RwLock<HashMap<Uuid, ()>>>,
    /// Active timeout operations
    timeouts: Arc<RwLock<HashMap<TimeoutHandle, TimeoutTask>>>,
    /// Event counter for threshold-based waking
    event_count: Arc<RwLock<usize>>,
    /// Time handler statistics
    stats: Arc<RwLock<TimeHandlerStats>>,
    /// Underlying time provider for deterministic/testing overrides
    provider: Arc<dyn PhysicalTimeEffects>,
    /// Random provider for generating unique IDs
    random_provider: Arc<dyn RandomEffects>,
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
        Self::with_providers(
            Arc::new(aura_effects::time::PhysicalTimeHandler),
            Arc::new(aura_effects::random::RealRandomHandler),
        )
    }

    /// Check if this is a simulated time handler (for testing)
    pub fn is_simulated(&self) -> bool {
        // In practice, this would check if provider is a mock/simulated implementation
        // For now, return false for production handlers
        false
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
        let timeout_id = self.random_provider.random_uuid().await;
        let current_ms = self.current_timestamp().await;
        let expires_at_ms = current_ms.saturating_add(timeout_ms);

        let timeout_task = TimeoutTask {
            id: timeout_id,
            expires_at_ms,
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
    pub async fn register_context(&self, context_id: uuid::Uuid) {
        let mut contexts_guard = self.contexts.write().await;
        contexts_guard.insert(context_id, ());
    }

    /// Unregister a context
    pub async fn unregister_context(&self, context_id: uuid::Uuid) {
        let mut contexts_guard = self.contexts.write().await;
        contexts_guard.remove(&context_id);
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
                let target_count = threshold;

                let timeout_future = self.sleep_ms(timeout_ms);
                let wait_future = async {
                    loop {
                        let current_count = self.get_event_count().await;
                        if current_count >= target_count {
                            return Ok(());
                        }

                        // Polling loop yields via sleep to remain simulator-controllable
                        let _ = self.sleep_ms(10).await;
                    }
                };

                futures::pin_mut!(timeout_future, wait_future);
                match futures::future::select(wait_future, timeout_future).await {
                    futures::future::Either::Left((res, _)) => res,
                    futures::future::Either::Right((_, _)) => Err(AuraError::invalid(
                        "Threshold events not reached within timeout",
                    )),
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
        Self::with_providers(provider, Arc::new(aura_effects::random::RealRandomHandler))
    }

    pub fn with_providers(
        provider: Arc<dyn PhysicalTimeEffects>,
        random_provider: Arc<dyn RandomEffects>,
    ) -> Self {
        Self {
            contexts: Arc::new(RwLock::new(HashMap::new())),
            timeouts: Arc::new(RwLock::new(HashMap::new())),
            event_count: Arc::new(RwLock::new(0)),
            stats: Arc::new(RwLock::new(TimeHandlerStats::default())),
            provider,
            random_provider,
        }
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
        let now_ms = self.current_timestamp().await;
        let mut timeouts = self.timeouts.write().await;

        let expired_ids: Vec<TimeoutHandle> = timeouts
            .iter()
            .filter(|(_, task)| task.expires_at_ms <= now_ms)
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
#[allow(clippy::disallowed_methods)] // Test code uses Uuid::new_v4() for test data generation
mod tests {
    use super::*;

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
        handler.sleep_ms(1).await;
        let end_time = handler.current_timestamp().await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_sleeps, 1);
        assert!(end_time >= start_time);
    }

    #[tokio::test]
    async fn test_timeout_operations() {
        let handler = EnhancedTimeHandler::default();

        let timeout_handle = handler.set_timeout(1).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_timeouts_set, 1);
        assert_eq!(stats.active_timeouts, 1);

        handler.cancel_timeout(timeout_handle).await.unwrap();

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_timeouts_cancelled, 1);
    }

    #[tokio::test]
    async fn test_context_management() {
        let handler = EnhancedTimeHandler::default();

        let context_id = Uuid::new_v4();

        handler.register_context(context_id).await;

        let stats = handler.get_statistics().await;
        assert_eq!(stats.active_contexts, 1);

        handler.unregister_context(context_id).await;

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
        let future_time = current_time + 1; // 1ms in future

        let result = handler
            .yield_until(WakeCondition::TimeoutAt(future_time))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_yield_until_threshold_events() {
        let handler = Arc::new(EnhancedTimeHandler::default());

        handler.simulate_event().await;
        handler.simulate_event().await;
        handler.simulate_event().await;

        let result = handler
            .yield_until(WakeCondition::ThresholdEvents {
                threshold: 3,
                timeout_ms: 10,
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_notification() {
        let handler = EnhancedTimeHandler::default();

        let initial_count = handler.get_event_count().await;

        handler.notify_events_available().await;

        let new_count = handler.get_event_count().await;
        assert_eq!(new_count, initial_count + 1);
    }

    #[tokio::test]
    async fn test_cleanup_expired_timeouts() {
        let handler = EnhancedTimeHandler::default();

        let timeout_handle = handler.set_timeout(1).await;

        handler.sleep_ms(10).await;

        handler.cleanup_expired_timeouts().await;

        let result = handler.cancel_timeout(timeout_handle).await;
        assert!(result.is_err());
    }
}
