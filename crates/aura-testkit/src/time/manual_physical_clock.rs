//! Manual physical time whose sleeps wait for actual test-controlled progress.
use aura_core::types::window::{PhysicalMillis, WindowPosition};
use aura_core::{
    effects::{PhysicalTimeEffects, TimeError},
    time::PhysicalTime,
};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicU64, AtomicUsize, Ordering},
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
    deadlines: tokio::sync::Mutex<BTreeMap<u64, Arc<AtomicUsize>>>,
    deadlines_changed: tokio::sync::Notify,
    observation_override: tokio::sync::Mutex<Option<Result<PhysicalTime, TimeError>>>,
    #[cfg(not(target_arch = "wasm32"))]
    task_observation_failure: tokio::sync::Mutex<Option<(tokio::task::Id, TimeError)>>,
    sleep_failure: tokio::sync::Mutex<Option<TimeError>>,
    sleep_admission: tokio::sync::Mutex<SleepAdmission>,
}

type SleepAdmission = Option<(
    Option<u64>,
    tokio::sync::oneshot::Sender<WindowPosition<PhysicalMillis>>,
)>;
impl ManualPhysicalClock {
    /// Construct the original physical observation.
    pub fn new(now: u64) -> Self {
        Self {
            state: Arc::new(State {
                now: AtomicU64::new(now),
                changed: tokio::sync::Notify::new(),
                deadlines: tokio::sync::Mutex::new(BTreeMap::new()),
                deadlines_changed: tokio::sync::Notify::new(),
                observation_override: tokio::sync::Mutex::new(None),
                #[cfg(not(target_arch = "wasm32"))]
                task_observation_failure: tokio::sync::Mutex::new(None),
                sleep_failure: tokio::sync::Mutex::new(None),
                sleep_admission: tokio::sync::Mutex::new(None),
            }),
        }
    }
    /// Fail the next actual provider observation with the supplied typed cause.
    pub async fn fail_next_observation(&self, cause: TimeError) {
        *self.state.observation_override.lock().await = Some(Err(cause));
    }
    /// Fail the next read by this exact Tokio task. Background runtime reads
    /// of the same provider cannot consume the selected workflow fault.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn fail_next_current_task_observation(
        &self,
        cause: TimeError,
    ) -> Result<(), TimeError> {
        let task = tokio::task::try_id().ok_or_else(|| TimeError::OperationFailed {
            reason: "selected physical fault requires a Tokio task".into(),
        })?;
        let mut slot = self.state.task_observation_failure.lock().await;
        if slot.is_some() {
            return Err(TimeError::OperationFailed {
                reason: "selected physical fault already installed".into(),
            });
        }
        *slot = Some((task, cause));
        Ok(())
    }
    /// Script one actual physical read without changing independently driven
    /// time or the original endpoints registered by absolute waits.
    pub fn script_next_observation(&self, at_ms: u64) -> Result<(), TimeError> {
        let mut slot =
            self.state
                .observation_override
                .try_lock()
                .map_err(|_| TimeError::OperationFailed {
                    reason: "manual observation override is borrowed".into(),
                })?;
        if slot.is_some() {
            return Err(TimeError::OperationFailed {
                reason: "manual observation override already installed".into(),
            });
        }
        *slot = Some(Ok(PhysicalTime {
            ts_ms: at_ms,
            uncertainty: None,
        }));
        Ok(())
    }
    /// Fail the next sleep poll, including an already waiting original sleep.
    pub async fn fail_next_sleep(&self, cause: TimeError) {
        *self.state.sleep_failure.lock().await = Some(cause);
        self.state.changed.notify_waiters();
    }
    /// Observe a polled relative sleep's actual target, without advancing
    /// time or acknowledging its caller. A live observer has exclusive custody.
    pub async fn observe_next_relative_sleep(
        &self,
    ) -> Result<tokio::sync::oneshot::Receiver<WindowPosition<PhysicalMillis>>, TimeError> {
        self.observe_relative_sleep_matching(None).await
    }
    /// Observe an actually polled relative sleep of one duration, leaving
    /// unrelated admitted timers untouched and without satisfying the sleep.
    pub async fn observe_next_relative_sleep_of(
        &self,
        duration_ms: u64,
    ) -> Result<tokio::sync::oneshot::Receiver<WindowPosition<PhysicalMillis>>, TimeError> {
        self.observe_relative_sleep_matching(Some(duration_ms))
            .await
    }
    async fn observe_relative_sleep_matching(
        &self,
        duration_ms: Option<u64>,
    ) -> Result<tokio::sync::oneshot::Receiver<WindowPosition<PhysicalMillis>>, TimeError> {
        let mut slot = self.state.sleep_admission.lock().await;
        if slot.as_ref().is_some_and(|(_, sender)| !sender.is_closed()) {
            return Err(TimeError::OperationFailed {
                reason: "manual relative-sleep observer already installed".into(),
            });
        }
        let (sender, receiver) = tokio::sync::oneshot::channel();
        *slot = Some((duration_ms, sender));
        Ok(receiver)
    }
    /// Current manual observation in milliseconds.
    pub fn now_ms(&self) -> u64 {
        self.state.now.load(Ordering::SeqCst)
    }
    /// Earliest still-future endpoint registered by a polled provider wait.
    /// This observes timers, not executor quiescence or protocol completion.
    pub async fn next_registered_deadline(&self) -> Option<WindowPosition<PhysicalMillis>> {
        let now = self.now_ms();
        let mut deadlines = self.state.deadlines.lock().await;
        deadlines.retain(|_, count| count.load(Ordering::Acquire) != 0);
        deadlines
            .keys()
            .copied()
            .find(|deadline| *deadline > now)
            .map(WindowPosition::new)
    }
    /// Observe actual timer admission without advancing the selected clock.
    /// A scenario driver must separately establish quiescence before advancing.
    pub async fn wait_for_registered_deadline(&self) -> WindowPosition<PhysicalMillis> {
        loop {
            let changed = self.state.deadlines_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(deadline) = self.next_registered_deadline().await {
                return deadline;
            }
            changed.await;
        }
    }
    async fn register_deadline(&self, deadline: u64) -> RegisteredManualDeadline {
        let count = {
            let mut deadlines = self.state.deadlines.lock().await;
            deadlines.retain(|_, count| count.load(Ordering::Acquire) != 0);
            let count = deadlines.entry(deadline).or_default();
            count.fetch_add(1, Ordering::AcqRel);
            count.clone()
        };
        self.state.deadlines_changed.notify_waiters();
        RegisteredManualDeadline {
            state: Arc::clone(&self.state),
            count,
        }
    }
    /// Move time forward by `by_ms`, waking sleeps whose deadline it reaches.
    pub fn advance(&self, by_ms: u64) {
        self.state
            .now
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |now| {
                now.checked_add(by_ms)
            })
            .expect("explicit manual physical advance overflow");
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
    }
    /// Publish an actual physical observation, including deliberate rollback.
    pub fn set_time(&self, now: u64) {
        self.state.now.store(now, Ordering::SeqCst);
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
    }
}
/// Lexical custody removes a cancelled/completed wait from the observed schedule.
struct RegisteredManualDeadline {
    state: Arc<State>,
    count: Arc<AtomicUsize>,
}
impl Drop for RegisteredManualDeadline {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
        self.state.deadlines_changed.notify_waiters();
    }
}
#[async_trait::async_trait]
impl PhysicalTimeEffects for ManualPhysicalClock {
    async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(task) = tokio::task::try_id() {
            let mut selected = self.state.task_observation_failure.lock().await;
            if selected.as_ref().is_some_and(|(owner, _)| *owner == task) {
                return Err(selected.take().expect("matched selected fault").1);
            }
        }
        if let Some(observation) = self.state.observation_override.lock().await.take() {
            return observation;
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
        let _registration = self.register_deadline(target).await;
        let mut observer = self.state.sleep_admission.lock().await;
        if observer.as_ref().is_some_and(|(selected_duration, _)| {
            selected_duration.is_none_or(|selected| selected == duration)
        }) {
            let (_, sender) = observer.take().expect("matching admitted sleep observer");
            let _ = sender.send(WindowPosition::new(target));
        }
        drop(observer);
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

    async fn wait_until_physical_deadline(
        &self,
        deadline: WindowPosition<PhysicalMillis>,
    ) -> Result<PhysicalTime, TimeError> {
        let _registration = self.register_deadline(deadline.value()).await;
        let mut previous = self.state.now.load(Ordering::SeqCst);
        loop {
            let changed = self.state.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if let Some(cause) = self.state.sleep_failure.lock().await.take() {
                return Err(cause);
            }
            let now = self.state.now.load(Ordering::SeqCst);
            if now < previous {
                return Err(TimeError::PhysicalClockRollback {
                    previous_ms: previous,
                    observed_ms: now,
                });
            }
            if now >= deadline.value() {
                return Ok(PhysicalTime {
                    ts_ms: now,
                    uncertainty: None,
                });
            }
            previous = now;
            changed.await;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    #[tokio::test]
    async fn relative_admission_and_timer_inventory_preserve_original_endpoints() {
        let clock = ManualPhysicalClock::new(7_000);
        let absolute = clock.wait_until_physical_deadline(WindowPosition::new(7_100));
        tokio::pin!(absolute);
        assert!(absolute.as_mut().now_or_never().is_none());
        let admission = clock
            .observe_next_relative_sleep()
            .await
            .expect("one live relative observer");
        assert!(clock.observe_next_relative_sleep().await.is_err());
        let relative = clock.sleep_ms(5);
        tokio::pin!(relative);
        assert!(relative.as_mut().now_or_never().is_none());
        assert_eq!(
            admission.await.expect("actual relative timer").value(),
            7_005
        );
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(7_005))
        );
        assert_eq!(clock.now_ms(), 7_000);
        clock
            .script_next_observation(6_500)
            .expect("one transient read");
        assert_eq!(
            clock.physical_time().await.expect("scripted read").ts_ms,
            6_500
        );
        assert_eq!(clock.now_ms(), 7_000);
        assert!(relative.as_mut().now_or_never().is_none());
        clock.set_time(7_005);
        relative.await.expect("explicit batch progress");
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(7_100))
        );
        assert!(absolute.as_mut().now_or_never().is_none());
        clock.set_time(7_100);
        assert_eq!(
            absolute.await.expect("original absolute timer").ts_ms,
            7_100
        );
        assert_eq!(clock.next_registered_deadline().await, None);
    }
    #[tokio::test]
    async fn registered_deadlines_preserve_duplicate_custody_and_cancelled_waits() {
        let clock = ManualPhysicalClock::new(5_000);
        let mut first = Box::pin(clock.wait_until_physical_deadline(WindowPosition::new(5_100)));
        let mut second = Box::pin(clock.wait_until_physical_deadline(WindowPosition::new(5_100)));
        let mut earlier = Box::pin(clock.sleep_ms(25));
        assert!(first.as_mut().now_or_never().is_none());
        assert!(second.as_mut().now_or_never().is_none());
        assert!(earlier.as_mut().now_or_never().is_none());
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_025))
        );
        drop(earlier);
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_100))
        );
        drop(first);
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_100))
        );
        assert_eq!(
            clock.now_ms(),
            5_000,
            "observation and cancellation cannot drive time"
        );
        clock.advance(100);
        assert_eq!(second.await.expect("actual original endpoint").ts_ms, 5_100);
        assert_eq!(clock.next_registered_deadline().await, None);
        assert!(clock.state.deadlines.lock().await.is_empty());
    }

    #[tokio::test]
    async fn registered_deadlines_observe_actual_admission_without_lost_wakeup() {
        let clock = ManualPhysicalClock::new(5_000);
        let mut observation = Box::pin(clock.wait_for_registered_deadline());
        assert!(observation.as_mut().now_or_never().is_none());
        let mut original = Box::pin(clock.wait_until_physical_deadline(WindowPosition::new(5_200)));
        assert!(original.as_mut().now_or_never().is_none());
        assert_eq!(observation.await, WindowPosition::new(5_200));
        assert_eq!(clock.now_ms(), 5_000);
        drop(original);
        assert_eq!(clock.next_registered_deadline().await, None);
        let mut next = Box::pin(clock.sleep_ms(40));
        assert!(next.as_mut().now_or_never().is_none());
        assert_eq!(
            clock.wait_for_registered_deadline().await,
            WindowPosition::new(5_040)
        );
        assert_eq!(clock.now_ms(), 5_000);
    }

    #[tokio::test]
    async fn required_absolute_deadline_preserves_endpoint_after_delayed_registration() {
        let clock = Arc::new(ManualPhysicalClock::new(100));
        let wait = clock.wait_until_physical_deadline(WindowPosition::new(500));
        tokio::pin!(wait);
        clock.set_time(350);
        assert!(wait.as_mut().now_or_never().is_none());
        clock.set_time(499);
        assert!(wait.as_mut().now_or_never().is_none());
        clock.set_time(500);
        assert_eq!(
            wait.await.expect("original fixed endpoint reached").ts_ms,
            500
        );
    }

    #[tokio::test]
    async fn required_absolute_deadline_preserves_rollback_and_native_timer_fault() {
        use std::error::Error;
        let clock = ManualPhysicalClock::new(350);
        let wait = clock.wait_until_physical_deadline(WindowPosition::new(500));
        tokio::pin!(wait);
        assert!(wait.as_mut().now_or_never().is_none());
        clock.set_time(300);
        assert!(matches!(
            wait.await,
            Err(TimeError::PhysicalClockRollback {
                previous_ms: 350,
                observed_ms: 300
            })
        ));

        let wait = clock.wait_until_physical_deadline(WindowPosition::new(500));
        tokio::pin!(wait);
        assert!(wait.as_mut().now_or_never().is_none());
        clock
            .fail_next_sleep(TimeError::ProviderFailure {
                operation: aura_core::effects::time::TimeProviderOperation::WaitTimer,
                source: Some(Arc::new(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "selected manual timer provider fault",
                ))),
            })
            .await;
        let failure = wait
            .await
            .expect_err("native timer error, not invented expiry");
        let cause = failure
            .source()
            .and_then(|source| source.downcast_ref::<std::io::Error>())
            .expect("actual selected IO error");
        assert_eq!(cause.kind(), std::io::ErrorKind::Interrupted);
        assert_eq!(
            clock
                .physical_time()
                .await
                .expect("time remains actual")
                .ts_ms,
            300
        );
    }
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
    async fn selected_task_observation_fault_survives_other_runtime_readers() {
        let clock = ManualPhysicalClock::new(100);
        let selected_clock = clock.clone();
        let (armed, observed) = tokio::sync::oneshot::channel();
        let (resume, continued) = tokio::sync::oneshot::channel();
        let selected = tokio::spawn(async move {
            selected_clock
                .fail_next_current_task_observation(TimeError::OperationFailed {
                    reason: "selected owner read failed".into(),
                })
                .await
                .expect("selected Tokio owner");
            armed.send(()).expect("driver observes armed fault");
            continued.await.expect("driver permits selected read");
            assert!(matches!(
                selected_clock.physical_time().await,
                Err(TimeError::OperationFailed { reason }) if reason == "selected owner read failed"
            ));
            assert_eq!(
                selected_clock
                    .physical_time()
                    .await
                    .expect("one-shot fault")
                    .ts_ms,
                100
            );
        });
        observed.await.expect("selected task armed fault");
        let background = clock.clone();
        assert_eq!(
            tokio::spawn(async move { background.physical_time().await })
                .await
                .expect("background task")
                .expect("unrelated reader remains healthy")
                .ts_ms,
            100
        );
        resume.send(()).expect("selected task still owns fault");
        selected.await.expect("selected owner settled");
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
