//! Enhanced time handler for production use with advanced scheduling capabilities.

use crate::runtime::services::state::with_state_mut_validated;
use aura_core::effects::{
    PhysicalTimeEffects, RandomExtendedEffects, TimeoutHandle, WakeCondition,
};
use aura_core::{AuraError, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// Preserve the actual effect error through higher-level scheduling APIs.
fn required_time_failure(error: aura_core::effects::TimeError) -> AuraError {
    AuraError::Internal {
        message: format!("required time effect failed: {error}"),
        source: Some(Arc::new(error)),
    }
}

/// Timeout task information
#[derive(Debug, Clone)]
struct TimeoutTask {
    expires_at_ms: u64,
    completed: bool,
}

struct TimeHandlerShared {
    state: RwLock<TimeHandlerState>,
}

/// Enhanced time handler with scheduling, timeouts, and event-driven waking
#[derive(Clone)]
pub struct EnhancedTimeHandler {
    /// In-memory time handler state (contexts, timeouts, stats)
    shared: Arc<TimeHandlerShared>,
    /// Underlying time provider for deterministic/testing overrides
    provider: Arc<dyn PhysicalTimeEffects>,
    /// Random provider for generating unique IDs
    random_provider: Arc<dyn RandomExtendedEffects>,
    /// Whether the handler is using a simulated/virtual time source
    simulated: bool,
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

#[derive(Debug, Default)]
struct TimeHandlerState {
    contexts: HashMap<Uuid, ()>,
    timeouts: HashMap<TimeoutHandle, TimeoutTask>,
    event_count: u32,
    stats: TimeHandlerStats,
}

impl TimeHandlerState {
    fn validate(
        &self,
    ) -> std::result::Result<(), crate::runtime::services::invariant::InvariantViolation> {
        if self.stats.active_contexts != self.contexts.len() as u64 {
            return Err(
                crate::runtime::services::invariant::InvariantViolation::new(
                    "TimeHandler",
                    format!(
                        "active_contexts {} does not match context count {}",
                        self.stats.active_contexts,
                        self.contexts.len()
                    ),
                ),
            );
        }
        if self.stats.active_timeouts > self.timeouts.len() as u64 {
            return Err(
                crate::runtime::services::invariant::InvariantViolation::new(
                    "TimeHandler",
                    format!(
                        "active_timeouts {} exceeds timeout count {}",
                        self.stats.active_timeouts,
                        self.timeouts.len()
                    ),
                ),
            );
        }
        Ok(())
    }
}

impl EnhancedTimeHandler {
    /// Create a new enhanced time handler
    pub fn new() -> Self {
        Self::with_providers(
            Arc::new(aura_effects::time::PhysicalTimeHandler),
            Arc::new(aura_effects::random::RealRandomHandler),
            false,
        )
    }

    /// Check if this is a simulated time handler (for testing)
    pub fn is_simulated(&self) -> bool {
        self.simulated
    }

    /// Sleep for a given number of milliseconds
    pub async fn sleep_ms(
        &self,
        ms: u64,
    ) -> std::result::Result<(), aura_core::effects::TimeError> {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.stats.total_sleeps += 1;
            },
            |state| state.validate(),
        )
        .await;

        // Delegate to the underlying provider
        self.provider.sleep_ms(ms).await
    }

    /// Set a timeout and return a handle
    pub async fn set_timeout(&self, timeout_ms: u64) -> Result<TimeoutHandle> {
        let timeout_id = self.random_provider.random_uuid().await;
        let current_ms = self.current_timestamp().await?;
        let interval = aura_core::types::window::WindowInterval::<
            aura_core::types::window::PhysicalMillis,
        >::new(
            aura_core::types::window::WindowPosition::new(current_ms),
            timeout_ms,
        )
        .map_err(|source| AuraError::Invalid {
            message: "required timer window endpoint is not representable".into(),
            source: Some(Arc::new(source)),
        })?;
        let expires_at_ms = interval.end().value();

        let timeout_task = TimeoutTask {
            expires_at_ms,
            completed: false,
        };

        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.timeouts.insert(timeout_id, timeout_task);
                state.stats.total_timeouts_set += 1;
                state.stats.active_timeouts += 1;
            },
            |state| state.validate(),
        )
        .await;

        Ok(timeout_id)
    }

    /// Wait for an owned fixed deadline without registering a detached timer.
    pub(crate) async fn wait_owned_timeout(&self, budget: &aura_core::TimeoutBudget) -> Result<()> {
        match aura_core::execute_with_timeout_budget(self, budget, || {
            std::future::pending::<
                std::result::Result<std::convert::Infallible, std::convert::Infallible>,
            >()
        })
        .await
        {
            Err(aura_core::TimeoutRunError::Timeout(
                aura_core::TimeoutBudgetError::DeadlineExceeded { .. },
            )) => Ok(()),
            Err(aura_core::TimeoutRunError::Timeout(error)) => Err(AuraError::from(error)),
            Ok(never) | Err(aura_core::TimeoutRunError::Operation(never)) => match never {},
        }
    }

    /// Cancel a registered legacy timeout.
    pub async fn cancel_timeout(&self, timeout_handle: TimeoutHandle) -> Result<()> {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                if state.timeouts.remove(&timeout_handle).is_some() {
                    state.stats.total_timeouts_cancelled += 1;
                    state.stats.active_timeouts = state.stats.active_timeouts.saturating_sub(1);
                    Ok(())
                } else {
                    Err(AuraError::not_found("Timeout not found"))
                }
            },
            |state| state.validate(),
        )
        .await
    }

    /// Register a context for time notifications
    pub async fn register_context(&self, context_id: uuid::Uuid) {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                if state.contexts.insert(context_id, ()).is_none() {
                    state.stats.active_contexts += 1;
                }
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Unregister a context
    pub async fn unregister_context(&self, context_id: uuid::Uuid) {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                if state.contexts.remove(&context_id).is_some() {
                    state.stats.active_contexts = state.stats.active_contexts.saturating_sub(1);
                }
            },
            |state| state.validate(),
        )
        .await;
    }

    async fn timeout_expiration(&self, timeout_id: &TimeoutHandle) -> Result<u64> {
        self.shared
            .state
            .read()
            .await
            .timeouts
            .get(timeout_id)
            .map(|task| task.expires_at_ms)
            .ok_or_else(|| AuraError::not_found("Timeout not found"))
    }

    async fn complete_timeout(&self, timeout_id: &TimeoutHandle) -> Result<()> {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                if state.timeouts.remove(timeout_id).is_some() {
                    state.stats.active_timeouts = state.stats.active_timeouts.saturating_sub(1);
                    Ok(())
                } else {
                    Err(AuraError::not_found("Timeout not found"))
                }
            },
            |state| state.validate(),
        )
        .await
    }

    /// Yield until a wake condition is met
    pub async fn yield_until(&self, condition: WakeCondition) -> Result<()> {
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.stats.total_yield_operations += 1;
            },
            |state| state.validate(),
        )
        .await;

        match condition {
            WakeCondition::Immediate => Ok(()),
            WakeCondition::TimeoutAt(target_timestamp) => {
                let current = self.current_timestamp().await?;
                if current >= target_timestamp {
                    return Ok(());
                }

                let sleep_duration = target_timestamp.saturating_sub(current);
                self.sleep_ms(sleep_duration)
                    .await
                    .map_err(required_time_failure)?;
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
                        self.sleep_ms(10).await.map_err(required_time_failure)?;
                    }
                };

                futures::pin_mut!(timeout_future, wait_future);
                match futures::future::select(wait_future, timeout_future).await {
                    futures::future::Either::Left((res, _)) => res,
                    futures::future::Either::Right((result, _)) => {
                        result.map_err(required_time_failure)?;
                        Err(AuraError::invalid(
                            "Threshold events not reached within timeout",
                        ))
                    }
                }
            }
            WakeCondition::TimeoutExpired { timeout_id } => {
                let expires_at_ms = self.timeout_expiration(&timeout_id).await?;
                let current = self.current_timestamp().await?;
                if current < expires_at_ms {
                    self.sleep_ms(expires_at_ms.saturating_sub(current))
                        .await
                        .map_err(required_time_failure)?;
                }
                self.complete_timeout(&timeout_id).await
            }
            _ => {
                // For other conditions, check if satisfied immediately
                if self.check_wake_condition(&condition).await? {
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
    pub async fn current_timestamp(&self) -> Result<u64> {
        self.provider
            .physical_time()
            .await
            .map(|p| p.ts_ms)
            .map_err(required_time_failure)
    }

    /// Current epoch in milliseconds (alias)
    pub async fn current_epoch(&self) -> Result<u64> {
        self.current_timestamp().await
    }

    /// Millisecond precision helper
    pub async fn current_timestamp_millis(&self) -> Result<u64> {
        self.current_timestamp().await
    }

    /// Create a new enhanced time handler with explicit time provider
    pub fn with_provider(provider: Arc<dyn PhysicalTimeEffects>) -> Self {
        Self::with_providers(
            provider,
            Arc::new(aura_effects::random::RealRandomHandler),
            false,
        )
    }

    pub fn with_providers(
        provider: Arc<dyn PhysicalTimeEffects>,
        random_provider: Arc<dyn RandomExtendedEffects>,
        simulated: bool,
    ) -> Self {
        Self {
            shared: Arc::new(TimeHandlerShared {
                state: RwLock::new(TimeHandlerState::default()),
            }),
            provider,
            random_provider,
            simulated,
        }
    }

    /// Get current time handler statistics
    pub async fn get_statistics(&self) -> TimeHandlerStats {
        let state = self.shared.state.read().await;
        let mut stats = state.stats.clone();
        stats.active_contexts = state.contexts.len() as u64;
        stats.active_timeouts = state.timeouts.len() as u64;
        stats
    }

    /// Cleanup expired timeouts
    pub async fn cleanup_expired_timeouts(&self) -> Result<()> {
        let now_ms = self.current_timestamp().await?;
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                let expired_ids: Vec<TimeoutHandle> = state
                    .timeouts
                    .iter()
                    .filter(|(_, task)| task.expires_at_ms <= now_ms)
                    .map(|(id, _)| *id)
                    .collect();
                if !expired_ids.is_empty() {
                    for id in &expired_ids {
                        state.timeouts.remove(id);
                    }
                    state.stats.active_timeouts = state
                        .stats
                        .active_timeouts
                        .saturating_sub(expired_ids.len() as u64);
                }
            },
            |state| state.validate(),
        )
        .await;
        Ok(())
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
        with_state_mut_validated(
            &self.shared.state,
            |state| {
                state.event_count = state.event_count.saturating_add(1);
            },
            |state| state.validate(),
        )
        .await;
    }

    /// Get current event count
    async fn get_event_count(&self) -> u32 {
        self.shared.state.read().await.event_count
    }

    /// Process wake condition and return whether it's satisfied
    async fn check_wake_condition(&self, condition: &WakeCondition) -> Result<bool> {
        let satisfied = match condition {
            WakeCondition::Immediate => true,
            WakeCondition::NewEvents => {
                // For simplicity, always true since we can't track "new" events
                // In a real implementation, this would track event timestamps
                true
            }
            WakeCondition::EpochReached { target } => {
                let current_epoch = self.current_epoch().await?;
                current_epoch >= *target
            }
            WakeCondition::TimeoutAt(target_timestamp) => {
                let current_timestamp = self.current_timestamp().await?;
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
            WakeCondition::TimeoutExpired { timeout_id } => {
                let current = self.current_timestamp().await?;
                with_state_mut_validated(
                    &self.shared.state,
                    |state| -> Result<bool> {
                        if let Some(task) = state.timeouts.get_mut(timeout_id) {
                            if !task.completed && current >= task.expires_at_ms {
                                task.completed = true;
                                state.stats.active_timeouts =
                                    state.stats.active_timeouts.saturating_sub(1);
                                return Ok(true);
                            }
                        }
                        Ok(false)
                    },
                    |state| state.validate(),
                )
                .await?
            }
            WakeCondition::Custom(_) => {
                // Custom conditions would be handled by external logic
                false
            }
        };

        Ok(satisfied)
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
        EnhancedTimeHandler::sleep_ms(self, ms).await
    }

    async fn wait_until_physical_deadline(
        &self,
        deadline: aura_core::types::window::WindowPosition<
            aura_core::types::window::PhysicalMillis,
        >,
    ) -> std::result::Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
        self.provider.wait_until_physical_deadline(deadline).await
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
    use aura_core::effects::TimeError;
    use aura_core::time::PhysicalTime;

    #[tokio::test]
    async fn required_absolute_deadline_keeps_actual_configured_provider_and_native_failure() {
        use futures::FutureExt;
        use std::error::Error;
        let clock = Arc::new(aura_testkit::time::ManualPhysicalClock::new(100));
        let handler = EnhancedTimeHandler::with_provider(clock.clone());
        let deadline = aura_core::types::window::WindowPosition::new(500);
        let wait = handler.wait_until_physical_deadline(deadline);
        tokio::pin!(wait);
        assert!(wait.as_mut().now_or_never().is_none());
        clock.set_time(499);
        assert!(wait.as_mut().now_or_never().is_none());
        clock
            .fail_next_sleep(TimeError::ProviderFailure {
                operation: aura_core::effects::time::TimeProviderOperation::WaitTimer,
                source: Some(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "actual configured timer outage",
                ))),
            })
            .await;
        let failure = wait
            .await
            .expect_err("selected provider failure cannot choose another clock");
        assert_eq!(
            failure
                .source()
                .and_then(|cause| cause.downcast_ref::<std::io::Error>())
                .expect("actual native provider cause")
                .kind(),
            std::io::ErrorKind::Interrupted
        );
        clock.set_time(500);
        assert_eq!(
            handler
                .wait_until_physical_deadline(deadline)
                .await
                .expect("selected provider recovered")
                .ts_ms,
            500
        );
    }

    struct FixedTimeProvider {
        ts_ms: u64,
    }

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for FixedTimeProvider {
        async fn physical_time(&self) -> std::result::Result<PhysicalTime, TimeError> {
            Ok(PhysicalTime {
                ts_ms: self.ts_ms,
                uncertainty: Some(0),
            })
        }

        async fn sleep_ms(&self, _ms: u64) -> std::result::Result<(), TimeError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_enhanced_time_handler_creation() {
        let handler = EnhancedTimeHandler::default();
        let stats = handler.get_statistics().await;

        assert_eq!(stats.total_sleeps, 0);
        assert_eq!(stats.active_contexts, 0);
        assert!(!handler.is_simulated());
    }

    #[tokio::test]
    async fn required_timer_window_rejects_overflow_before_registration() {
        use std::error::Error;
        let time =
            EnhancedTimeHandler::with_provider(Arc::new(FixedTimeProvider { ts_ms: u64::MAX }));
        let error = time
            .set_timeout(1)
            .await
            .expect_err("overflow cannot mint timeout evidence");
        assert_eq!(
            error
                .source()
                .expect("native interval cause")
                .downcast_ref::<aura_core::types::window::WindowIntervalError>(),
            Some(&aura_core::types::window::WindowIntervalError::EndpointOverflow)
        );
        assert!(time.shared.state.read().await.timeouts.is_empty());
    }

    #[tokio::test]
    async fn test_time_operations() {
        let fixed_ts = 1_772_576_325_465;
        let handler =
            EnhancedTimeHandler::with_provider(Arc::new(FixedTimeProvider { ts_ms: fixed_ts }));

        let current_epoch = handler.current_epoch().await.unwrap();
        let current_timestamp = handler.current_timestamp().await.unwrap();
        let current_timestamp_millis = handler.current_timestamp_millis().await.unwrap();

        assert!(current_epoch > 0);
        assert!(current_timestamp > 0);
        assert!(current_timestamp_millis > 0);
        assert_eq!(current_timestamp, fixed_ts);
        assert_eq!(current_epoch, current_timestamp_millis);
    }

    #[tokio::test]
    async fn test_sleep_operations() {
        let handler = EnhancedTimeHandler::default();

        let start_time = handler.current_timestamp().await.unwrap();
        handler.sleep_ms(1).await.expect("required test sleep");
        let end_time = handler.current_timestamp().await.unwrap();

        let stats = handler.get_statistics().await;
        assert_eq!(stats.total_sleeps, 1);
        assert!(end_time >= start_time);
    }

    #[tokio::test]
    async fn test_timeout_operations() {
        let handler = EnhancedTimeHandler::default();

        let timeout_handle = handler.set_timeout(1).await.unwrap();

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

        let context_id = Uuid::from_bytes([1u8; 16]);

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

        let current_time = handler.current_timestamp().await.unwrap();
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

        let timeout_handle = handler.set_timeout(1).await.unwrap();

        handler.sleep_ms(10).await.expect("required test sleep");

        handler.cleanup_expired_timeouts().await.unwrap();

        let result = handler.cancel_timeout(timeout_handle).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_yield_until_timeout_expired_consumes_issued_handle() {
        let handler = EnhancedTimeHandler::default();

        let timeout_handle = handler.set_timeout(1).await.unwrap();
        handler
            .yield_until(WakeCondition::TimeoutExpired {
                timeout_id: timeout_handle,
            })
            .await
            .unwrap();

        let result = handler.cancel_timeout(timeout_handle).await;
        assert!(
            result.is_err(),
            "timeout handle should be consumed after expiry"
        );
    }
}

#[cfg(test)]
mod required_provider_failure_tests {
    use super::*;
    use aura_core::effects::TimeError;
    use aura_core::time::PhysicalTime;
    use std::error::Error;

    struct FailedProvider;
    #[async_trait::async_trait]
    impl PhysicalTimeEffects for FailedProvider {
        async fn physical_time(&self) -> std::result::Result<PhysicalTime, TimeError> {
            Err(TimeError::OperationFailed {
                reason: "injected required clock query".into(),
            })
        }
        async fn sleep_ms(&self, _: u64) -> std::result::Result<(), TimeError> {
            Err(TimeError::OperationFailed {
                reason: "injected required provider sleep".into(),
            })
        }
    }

    #[tokio::test]
    async fn required_sleep_and_timestamp_preserve_provider_errors() {
        let handler = EnhancedTimeHandler::with_provider(Arc::new(FailedProvider));
        assert!(matches!(
            handler.sleep_ms(1).await,
            Err(TimeError::OperationFailed { .. })
        ));
        assert!(matches!(
            PhysicalTimeEffects::sleep_ms(&handler, 1).await,
            Err(TimeError::OperationFailed { .. })
        ));
        let error = handler
            .current_timestamp()
            .await
            .expect_err("required provider query fails");
        assert!(error
            .source()
            .expect("actual time effect cause")
            .is::<TimeError>());
    }

    #[tokio::test]
    async fn threshold_wait_does_not_convert_failed_sleep_to_expiration() {
        let handler = EnhancedTimeHandler::with_provider(Arc::new(FailedProvider));
        let error = handler
            .yield_until(WakeCondition::ThresholdEvents {
                threshold: 1,
                timeout_ms: 10,
            })
            .await
            .expect_err("either required sleep branch fails");
        assert!(error
            .source()
            .expect("actual sleep cause")
            .is::<TimeError>());
        assert!(matches!(error, AuraError::Internal { .. }));
    }
}
