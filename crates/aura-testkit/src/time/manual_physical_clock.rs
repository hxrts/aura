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
}
impl ManualPhysicalClock {
    /// Construct the original physical observation.
    pub fn new(now: u64) -> Self {
        Self {
            state: Arc::new(State {
                now: AtomicU64::new(now),
                changed: tokio::sync::Notify::new(),
            }),
        }
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
