//! Manual physical time whose sleeps wait for actual test-controlled progress.
use aura_core::{
    effects::{PhysicalTimeEffects, TimeError},
    time::PhysicalTime,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Shared provider for actual runtimes and their timeout owners.
#[derive(Clone, Debug)]
pub struct ManualPhysicalClock {
    state: Arc<State>,
}
#[derive(Debug)]
struct State {
    now: AtomicU64,
    changed: tokio::sync::Notify,
    observation_failure: tokio::sync::Mutex<Option<TimeError>>,
    sleep_failure: tokio::sync::Mutex<Option<TimeError>>,
}
impl ManualPhysicalClock {
    /// Construct the original physical observation.
    pub fn new(now: u64) -> Self {
        Self {
            state: Arc::new(State {
                now: AtomicU64::new(now),
                changed: tokio::sync::Notify::new(),
                observation_failure: tokio::sync::Mutex::new(None),
                sleep_failure: tokio::sync::Mutex::new(None),
            }),
        }
    }
    /// Fail the next actual provider observation with the supplied typed cause.
    pub async fn fail_next_observation(&self, cause: TimeError) {
        *self.state.observation_failure.lock().await = Some(cause);
    }
    /// Fail the next sleep poll, including an already waiting original sleep.
    pub async fn fail_next_sleep(&self, cause: TimeError) {
        *self.state.sleep_failure.lock().await = Some(cause);
        self.state.changed.notify_waiters();
    }
    /// Publish an actual physical observation, including deliberate rollback.
    pub fn set_time(&self, now: u64) {
        self.state.now.store(now, Ordering::SeqCst);
        self.state.changed.notify_waiters();
    }
}
#[async_trait::async_trait]
impl PhysicalTimeEffects for ManualPhysicalClock {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        if let Some(cause) = self.state.observation_failure.lock().await.take() {
            return Err(cause);
        }
        Ok(PhysicalTime {
            ts_ms: self.state.now.load(Ordering::SeqCst),
            uncertainty: None,
        })
    }
    async fn sleep_ms(&self, duration: u64) -> Result<(), TimeError> {
        let target = self
            .state
            .now
            .load(Ordering::SeqCst)
            .checked_add(duration)
            .ok_or_else(|| TimeError::OperationFailed {
                reason: "manual sleep deadline overflow".into(),
            })?;
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(cause) = self.state.sleep_failure.lock().await.take() {
                return Err(cause);
            }
            if self.state.now.load(Ordering::SeqCst) >= target {
                return Ok(());
            }
            changed.await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    #[tokio::test]
    async fn provider_faults_are_one_shot_and_wake_original_waiting_sleep() {
        let clock = ManualPhysicalClock::new(100);
        clock
            .fail_next_observation(TimeError::OperationFailed {
                reason: "observation fixture fault".into(),
            })
            .await;
        assert!(
            matches!(clock.physical_time().await, Err(TimeError::OperationFailed { reason }) if reason == "observation fixture fault")
        );
        assert_eq!(
            clock.physical_time().await.expect("fault consumed").ts_ms,
            100
        );
        let sleep = clock.sleep_ms(20);
        tokio::pin!(sleep);
        assert!(sleep.as_mut().now_or_never().is_none());
        clock
            .fail_next_sleep(TimeError::OperationFailed {
                reason: "sleep fixture fault".into(),
            })
            .await;
        assert!(
            matches!(sleep.await, Err(TimeError::OperationFailed { reason }) if reason == "sleep fixture fault")
        );
        assert_eq!(
            clock
                .physical_time()
                .await
                .expect("sleep does not advance clock")
                .ts_ms,
            100
        );
    }
    #[tokio::test]
    async fn sleep_requires_explicit_observation_and_keeps_rollback_visible() {
        let clock = ManualPhysicalClock::new(5000);
        let sleep = clock.sleep_ms(100);
        tokio::pin!(sleep);
        assert!(sleep.as_mut().now_or_never().is_none());
        clock.set_time(4900);
        assert_eq!(
            clock
                .physical_time()
                .await
                .expect("physical observation")
                .ts_ms,
            4900
        );
        assert!(sleep.as_mut().now_or_never().is_none());
        clock.set_time(5099);
        assert!(sleep.as_mut().now_or_never().is_none());
        clock.set_time(5100);
        sleep.await.expect("original sleep target reached");
    }
}
