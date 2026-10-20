//! Manual physical time whose sleeps wait for actual test-controlled progress.
use aura_core::effects::time::{
    PhysicalSleepRegistration, PhysicalSleepRegistrationObservation,
    PhysicalSleepRegistrationObserver,
};
use aura_core::types::window::{PhysicalMillis, WindowPosition};
use aura_core::{
    effects::{PhysicalTimeEffects, TimeError},
    time::PhysicalTime,
};
use std::collections::BTreeMap;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

// The selected clock's synchronous mutation methods cannot await. This guard
// is held only while changing one atomic reading under timer registration.
#[allow(clippy::disallowed_types)]
type ClockMutationLock = std::sync::Mutex<()>;
// The test-only sender is read synchronously in timer Drop; no await occurs.
#[cfg(test)]
#[allow(clippy::disallowed_types)]
type ClockCancellationProbe = std::sync::Mutex<Option<std::sync::mpsc::Sender<bool>>>;
static NEXT_PROVIDER_ID: AtomicU64 = AtomicU64::new(1);

/// Shared provider for actual runtimes and their timeout owners.
#[derive(Clone, Debug)]
pub struct ManualPhysicalClock {
    state: Arc<State>,
}
#[derive(Debug)]
struct State {
    provider_id: u64,
    now: AtomicU64,
    next_relative_registration: AtomicU64,
    time_change: ClockMutationLock,
    changed: tokio::sync::Notify,
    deadlines: tokio::sync::Mutex<BTreeMap<u64, Vec<Arc<RegisteredTimer>>>>,
    deadlines_changed: tokio::sync::Notify,
    observation_override: tokio::sync::Mutex<Option<Result<PhysicalTime, TimeError>>>,
    #[cfg(not(target_arch = "wasm32"))]
    task_observation_failure: tokio::sync::Mutex<Option<(tokio::task::Id, TimeError)>>,
    sleep_failure: tokio::sync::Mutex<Option<TimeError>>,
    sleep_admission: tokio::sync::Mutex<SleepAdmission>,
    #[cfg(test)]
    drop_attempt: ClockCancellationProbe,
}

type SleepAdmission = Option<(
    Option<u64>,
    tokio::sync::oneshot::Sender<WindowPosition<PhysicalMillis>>,
)>;
#[derive(Debug)]
struct RegisteredTimer {
    active: AtomicBool,
    task: Option<tokio::task::Id>,
    generation: u64,
}

/// Read-only reason an observed scheduler timer cannot drive this exact clock.
/// The result observes one instant; only `try_advance_sole_registered_deadline`
/// makes a synchronized time decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisteredDeadlineRefusal {
    Eligible,
    NotFuture,
    NoLiveTimer,
    EarlierTimer,
    ForeignTask,
    DuplicateTimer,
    ObservationBusy,
}
impl ManualPhysicalClock {
    /// Construct the original physical observation.
    pub fn new(now: u64) -> Self {
        let provider_id = NEXT_PROVIDER_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                old.checked_add(1)
            })
            .expect("manual physical provider identity exhausted");
        Self {
            state: Arc::new(State {
                provider_id,
                now: AtomicU64::new(now),
                next_relative_registration: AtomicU64::new(0),
                time_change: ClockMutationLock::new(()),
                changed: tokio::sync::Notify::new(),
                deadlines: tokio::sync::Mutex::new(BTreeMap::new()),
                deadlines_changed: tokio::sync::Notify::new(),
                observation_override: tokio::sync::Mutex::new(None),
                #[cfg(not(target_arch = "wasm32"))]
                task_observation_failure: tokio::sync::Mutex::new(None),
                sleep_failure: tokio::sync::Mutex::new(None),
                sleep_admission: tokio::sync::Mutex::new(None),
                #[cfg(test)]
                drop_attempt: ClockCancellationProbe::new(None),
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
        deadlines.retain(|_, timers| {
            timers.retain(|timer| timer.active.load(Ordering::Acquire));
            !timers.is_empty()
        });
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
        let timer = Arc::new(RegisteredTimer {
            active: AtomicBool::new(true),
            task: tokio::task::try_id(),
            generation: 0,
        });
        {
            let mut deadlines = self.state.deadlines.lock().await;
            deadlines.retain(|_, timers| {
                timers.retain(|registered| registered.active.load(Ordering::Acquire));
                !timers.is_empty()
            });
            deadlines.entry(deadline).or_default().push(timer.clone());
        }
        self.state.deadlines_changed.notify_waiters();
        RegisteredManualDeadline {
            state: Arc::clone(&self.state),
            timer,
        }
    }
    async fn register_relative_deadline(
        &self,
        duration: u64,
    ) -> Result<(u64, RegisteredManualDeadline), TimeError> {
        let mut deadlines = self.state.deadlines.lock().await;
        // No await follows acquisition: observation, relative endpoint and
        // registration are atomic with the conditional selected-time step.
        let _time_change = self.state.time_change.lock().expect("manual clock owner");
        let target = self
            .state
            .now
            .load(Ordering::SeqCst)
            .checked_add(duration)
            .ok_or_else(|| TimeError::OperationFailed {
                reason: "manual sleep deadline overflow".into(),
            })?;
        deadlines.retain(|_, timers| {
            timers.retain(|timer| timer.active.load(Ordering::Acquire));
            !timers.is_empty()
        });
        let generation = self
            .state
            .next_relative_registration
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |old| {
                old.checked_add(1)
            })
            .map_err(|_| TimeError::OperationFailed {
                reason: "selected sleep registration generation exhausted".into(),
            })?
            + 1;
        let timer = Arc::new(RegisteredTimer {
            active: AtomicBool::new(true),
            task: tokio::task::try_id(),
            generation,
        });
        deadlines.entry(target).or_default().push(timer.clone());
        drop(deadlines);
        self.state.deadlines_changed.notify_waiters();
        Ok((
            target,
            RegisteredManualDeadline {
                state: Arc::clone(&self.state),
                timer,
            },
        ))
    }
    /// Move time forward by `by_ms`, waking sleeps whose deadline it reaches.
    pub fn advance(&self, by_ms: u64) {
        let _time_change = self.state.time_change.lock().expect("manual clock owner");
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
        let _time_change = self.state.time_change.lock().expect("manual clock owner");
        self.state.now.store(now, Ordering::SeqCst);
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
    }

    /// Advance only a single already-polled selected timer. The caller must
    /// additionally hold the originating task's current observation guard;
    /// an equal deadline by itself never identifies the timer's owner.
    pub fn try_advance_sole_registered_deadline(
        &self,
        deadline: u64,
        task: tokio::task::Id,
    ) -> bool {
        let Ok(_time_change) = self.state.time_change.try_lock() else {
            return false;
        };
        let Ok(deadlines) = self.state.deadlines.try_lock() else {
            return false;
        };
        let now = self.state.now.load(Ordering::SeqCst);
        if deadline <= now {
            return false;
        }
        let mut live = deadlines.iter().flat_map(|(at, timers)| {
            timers
                .iter()
                .filter(|timer| timer.active.load(Ordering::Acquire))
                .map(move |timer| (*at, timer))
        });
        let Some((first_deadline, first_timer)) = live.next() else {
            return false;
        };
        if first_deadline != deadline || first_timer.task.as_ref() != Some(&task) {
            return false;
        }
        if live.next().is_some_and(|(at, _)| at == deadline) {
            return false;
        }
        self.state.now.store(deadline, Ordering::SeqCst);
        drop(deadlines);
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
        true
    }

    /// Advance one selected endpoint only when the complete earliest timer
    /// set belongs to these distinct, still-live original task witnesses.
    /// Caller-owned scheduler generation guards must remain held throughout
    /// this synchronous step. Any earlier or same-valued foreign timer refuses.
    pub fn try_advance_exact_registered_deadline_set(
        &self,
        deadline: u64,
        tasks: &[tokio::task::Id],
    ) -> bool {
        if tasks.is_empty()
            || tasks
                .iter()
                .enumerate()
                .any(|(index, task)| tasks[..index].contains(task))
        {
            return false;
        }
        let Ok(_time_change) = self.state.time_change.try_lock() else {
            return false;
        };
        let Ok(deadlines) = self.state.deadlines.try_lock() else {
            return false;
        };
        if deadline <= self.state.now.load(Ordering::SeqCst) {
            return false;
        }
        let mut found = Vec::new();
        for (at, timers) in deadlines.iter() {
            for timer in timers
                .iter()
                .filter(|timer| timer.active.load(Ordering::Acquire))
            {
                if *at < deadline {
                    return false;
                }
                if *at > deadline {
                    break;
                }
                let Some(task) = timer.task.as_ref() else {
                    return false;
                };
                if !tasks.contains(task) || found.contains(task) {
                    return false;
                }
                found.push(*task);
            }
            if *at > deadline {
                break;
            }
        }
        if found.len() != tasks.len() {
            return false;
        }
        self.state.now.store(deadline, Ordering::SeqCst);
        drop(deadlines);
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
        true
    }

    /// Diagnose the selected timer without granting authority to advance it.
    /// A later step must recheck under the original synchronized mutation gate.
    pub fn inspect_registered_deadline(
        &self,
        deadline: u64,
        task: tokio::task::Id,
    ) -> RegisteredDeadlineRefusal {
        let Ok(deadlines) = self.state.deadlines.try_lock() else {
            return RegisteredDeadlineRefusal::ObservationBusy;
        };
        let now = self.state.now.load(Ordering::SeqCst);
        if deadline <= now {
            return RegisteredDeadlineRefusal::NotFuture;
        }
        let mut live = deadlines.iter().flat_map(|(at, timers)| {
            timers
                .iter()
                .filter(|timer| timer.active.load(Ordering::Acquire))
                .map(move |timer| (*at, timer))
        });
        let Some((first_deadline, first_timer)) = live.next() else {
            return RegisteredDeadlineRefusal::NoLiveTimer;
        };
        if first_deadline < deadline {
            return RegisteredDeadlineRefusal::EarlierTimer;
        }
        if first_deadline != deadline {
            return RegisteredDeadlineRefusal::NoLiveTimer;
        }
        if first_timer.task.as_ref() != Some(&task) {
            return RegisteredDeadlineRefusal::ForeignTask;
        }
        if live.next().is_some_and(|(at, _)| at == deadline) {
            return RegisteredDeadlineRefusal::DuplicateTimer;
        }
        RegisteredDeadlineRefusal::Eligible
    }
}
/// Lexical custody removes a cancelled/completed wait from the observed schedule.
struct RegisteredManualDeadline {
    state: Arc<State>,
    timer: Arc<RegisteredTimer>,
}
impl Drop for RegisteredManualDeadline {
    fn drop(&mut self) {
        #[cfg(test)]
        let attempt = self
            .state
            .drop_attempt
            .lock()
            .expect("manual cancellation probe")
            .take();
        // Cancellation and every conditional selected-time step linearize on
        // the same short gate. No await or deadline-map lock is taken here.
        let _time_change = match self.state.time_change.try_lock() {
            Ok(guard) => {
                #[cfg(test)]
                if let Some(attempt) = attempt {
                    let _ = attempt.send(false);
                }
                guard
            }
            Err(std::sync::TryLockError::WouldBlock) => {
                #[cfg(test)]
                if let Some(attempt) = attempt {
                    let _ = attempt.send(true);
                }
                self.state.time_change.lock().expect("manual clock owner")
            }
            Err(std::sync::TryLockError::Poisoned(_)) => panic!("manual clock owner poisoned"),
        };
        self.timer.active.store(false, Ordering::Release);
        self.state.deadlines_changed.notify_waiters();
    }
}

impl ManualPhysicalClock {
    async fn sleep_registered(
        &self,
        duration: u64,
        observer: Option<PhysicalSleepRegistrationObserver>,
    ) -> Result<(), TimeError> {
        let (target, registration) = self.register_relative_deadline(duration).await?;
        if let Some(observer) = observer {
            observer(PhysicalSleepRegistrationObservation::Registered(
                PhysicalSleepRegistration {
                    provider_id: self.state.provider_id,
                    generation: registration.timer.generation,
                    deadline: WindowPosition::new(target),
                },
            ));
        }
        let mut selected_observer = self.state.sleep_admission.lock().await;
        if selected_observer
            .as_ref()
            .is_some_and(|(selected_duration, _)| {
                selected_duration.is_none_or(|selected| selected == duration)
            })
        {
            let (_, sender) = selected_observer
                .take()
                .expect("matching admitted sleep observer");
            let _ = sender.send(WindowPosition::new(target));
        }
        drop(selected_observer);
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

    /// Advance only the exact still-live timer registration issued by this
    /// selected provider. A same-task foreign timer, replacement, or duplicate
    /// cannot satisfy a stale registration observation.
    pub fn try_advance_registered_timer(
        &self,
        registration: PhysicalSleepRegistration,
        task: tokio::task::Id,
    ) -> bool {
        let Ok(_time_change) = self.state.time_change.try_lock() else {
            return false;
        };
        let Ok(deadlines) = self.state.deadlines.try_lock() else {
            return false;
        };
        let deadline = registration.deadline.value();
        if registration.provider_id != self.state.provider_id
            || deadline <= self.state.now.load(Ordering::SeqCst)
        {
            return false;
        }
        let mut live = deadlines.iter().flat_map(|(at, timers)| {
            timers
                .iter()
                .filter(|timer| timer.active.load(Ordering::Acquire))
                .map(move |timer| (*at, timer))
        });
        let Some((first_at, first)) = live.next() else {
            return false;
        };
        if first_at != deadline
            || first.generation != registration.generation
            || first.task != Some(task)
        {
            return false;
        }
        if live.next().is_some_and(|(at, _)| at == deadline) {
            return false;
        }
        self.state.now.store(deadline, Ordering::SeqCst);
        drop(deadlines);
        self.state.changed.notify_waiters();
        self.state.deadlines_changed.notify_waiters();
        true
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
        self.sleep_registered(duration, None).await
    }

    async fn sleep_ms_with_registration_observer(
        &self,
        duration: u64,
        observer: PhysicalSleepRegistrationObserver,
    ) -> Result<(), TimeError> {
        self.sleep_registered(duration, Some(observer)).await
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

    struct UnobservedTime(ManualPhysicalClock);

    #[async_trait::async_trait]
    impl PhysicalTimeEffects for UnobservedTime {
        async fn physical_time(&self) -> Result<PhysicalTime, TimeError> {
            self.0.physical_time().await
        }

        async fn sleep_ms(&self, duration: u64) -> Result<(), TimeError> {
            self.0.sleep_ms(duration).await
        }
    }

    #[tokio::test]
    async fn unsupported_registration_observation_preserves_ordinary_sleep() {
        let clock = ManualPhysicalClock::new(5_000);
        let provider = UnobservedTime(clock.clone());
        let (sender, observed) = tokio::sync::oneshot::channel();
        let mut wait = Box::pin(provider.sleep_ms_with_registration_observer(
            5,
            Box::new(move |observation| {
                let _ = sender.send(observation);
            }),
        ));
        assert!(wait.as_mut().now_or_never().is_none());
        assert_eq!(
            observed.await.expect("one explicit observation"),
            PhysicalSleepRegistrationObservation::Unsupported
        );
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_005))
        );
        clock.advance(5);
        wait.await.expect("ordinary selected sleep completes");
        let (sender, observed) = tokio::sync::oneshot::channel();
        let mut cancelled = Box::pin(provider.sleep_ms_with_registration_observer(
            5,
            Box::new(move |observation| {
                let _ = sender.send(observation);
            }),
        ));
        assert!(cancelled.as_mut().now_or_never().is_none());
        assert_eq!(
            observed.await.expect("one cancellation observation"),
            PhysicalSleepRegistrationObservation::Unsupported
        );
        drop(cancelled);
        assert_eq!(clock.next_registered_deadline().await, None);
    }

    #[tokio::test]
    async fn provider_issued_registration_refuses_same_task_foreign_and_replaced_timer() {
        tokio::spawn(async {
            let clock = ManualPhysicalClock::new(5_000);
            let task = tokio::task::try_id().expect("selected owner task");
            let (sender, observed) = tokio::sync::oneshot::channel();
            let mut actual = Box::pin(clock.sleep_ms_with_registration_observer(
                100,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(actual.as_mut().now_or_never().is_none());
            let registration = match observed.await.expect("provider observation") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            let foreign_clock = ManualPhysicalClock::new(5_000);
            let (sender, foreign_observed) = tokio::sync::oneshot::channel();
            let mut foreign = Box::pin(foreign_clock.sleep_ms_with_registration_observer(
                100,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(foreign.as_mut().now_or_never().is_none());
            let foreign_registration = match foreign_observed.await.expect("foreign observation") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            assert_ne!(registration.provider_id, foreign_registration.provider_id);
            assert_eq!(registration.generation, foreign_registration.generation);
            assert_eq!(registration.deadline, foreign_registration.deadline);
            assert!(!clock.try_advance_registered_timer(foreign_registration, task));
            drop(foreign);
            let mut same_task_foreign = Box::pin(clock.sleep_ms(100));
            assert!(same_task_foreign.as_mut().now_or_never().is_none());
            assert!(!clock.try_advance_registered_timer(registration, task));
            drop(same_task_foreign);
            assert_eq!(clock.now_ms(), 5_000);
            drop(actual);

            let (sender, observed) = tokio::sync::oneshot::channel();
            let mut replaced = Box::pin(clock.sleep_ms_with_registration_observer(
                100,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(replaced.as_mut().now_or_never().is_none());
            let replacement = match observed.await.expect("replacement observation") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            assert_ne!(registration.generation, replacement.generation);
            assert!(!clock.try_advance_registered_timer(registration, task));
            assert!(clock.try_advance_registered_timer(replacement, task));
            replaced.await.expect("replacement selected wait");
            assert!(!clock.try_advance_registered_timer(replacement, task));
        })
        .await
        .expect("selected owner completes");
    }

    #[tokio::test]
    async fn cancellation_and_registered_advance_preserve_both_gate_orders() {
        tokio::spawn(async {
            let clock = ManualPhysicalClock::new(5_000);
            let task = tokio::task::try_id().expect("selected owner task");
            let (sender, observed) = tokio::sync::oneshot::channel();
            let mut cancelled = Box::pin(clock.sleep_ms_with_registration_observer(
                5,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(cancelled.as_mut().now_or_never().is_none());
            let cancelled_token = match observed.await.expect("actual provider observation") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            drop(cancelled);
            assert!(!clock.try_advance_registered_timer(cancelled_token, task));
            assert_eq!(clock.now_ms(), 5_000);

            let (sender, observed) = tokio::sync::oneshot::channel();
            let mut claimed = Box::pin(clock.sleep_ms_with_registration_observer(
                5,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(claimed.as_mut().now_or_never().is_none());
            let claimed_token = match observed.await.expect("replacement provider observation") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            assert!(clock.try_advance_registered_timer(claimed_token, task));
            claimed.await.expect("claimed selected wait");
            assert_eq!(clock.now_ms(), 5_005);
            assert!(!clock.try_advance_registered_timer(claimed_token, task));
        })
        .await
        .expect("selected owner completes");
    }

    #[tokio::test]
    async fn cancellation_cannot_cross_a_claimed_selected_time_gate() {
        tokio::spawn(async {
            let clock = ManualPhysicalClock::new(5_000);
            let task = tokio::task::try_id().expect("selected owner task");
            let (sender, observed) = tokio::sync::oneshot::channel();
            let mut wait = Box::pin(clock.sleep_ms_with_registration_observer(
                5,
                Box::new(move |observation| {
                    let _ = sender.send(observation);
                }),
            ));
            assert!(wait.as_mut().now_or_never().is_none());
            let registration = match observed.await.expect("actual timer admission") {
                PhysicalSleepRegistrationObservation::Registered(registration) => registration,
                PhysicalSleepRegistrationObservation::Unsupported => panic!("manual provider"),
            };
            let (attempt_tx, attempt_rx) = std::sync::mpsc::channel();
            *clock
                .state
                .drop_attempt
                .lock()
                .expect("manual cancellation probe") = Some(attempt_tx);
            std::thread::scope(|scope| {
                let gate = clock.state.time_change.lock().expect("selected time gate");
                let (completion_tx, completion_rx) = std::sync::mpsc::channel();
                scope.spawn(move || {
                    drop(wait);
                    let _ = completion_tx.send(());
                });
                let observed_contention = attempt_rx
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .expect("real cancellation reached the selected gate");
                let deadlines = clock
                    .state
                    .deadlines
                    .try_lock()
                    .expect("timer map available");
                let active_before_release = deadlines
                    .get(&registration.deadline.value())
                    .is_some_and(|timers| {
                        timers.iter().any(|timer| {
                            timer.generation == registration.generation
                                && timer.active.load(Ordering::Acquire)
                        })
                    });
                drop(deadlines);
                let completed_under_gate = completion_rx.try_recv();
                drop(gate);
                if completed_under_gate.is_err() {
                    completion_rx
                        .recv_timeout(std::time::Duration::from_secs(2))
                        .expect("cancellation commits after gate release");
                }
                assert!(
                    observed_contention,
                    "cancellation did not contend on selected-time gate"
                );
                assert!(
                    active_before_release,
                    "cancellation crossed held selected-time gate"
                );
                assert!(matches!(
                    completed_under_gate,
                    Err(std::sync::mpsc::TryRecvError::Empty)
                ));
            });
            assert!(!clock.try_advance_registered_timer(registration, task));
            assert_eq!(clock.now_ms(), 5_000);
        })
        .await
        .expect("selected owner completes");
    }
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
    async fn sole_registered_advance_refuses_earlier_duplicate_and_cancelled_timers() {
        tokio::spawn(async {
            let task = tokio::task::try_id().expect("owned timer task");
            let clock = ManualPhysicalClock::new(5_000);
            let mut batch = Box::pin(clock.sleep_ms(5));
            let mut earlier = Box::pin(clock.sleep_ms(3));
            let mut duplicate = Box::pin(clock.sleep_ms(5));
            assert!(batch.as_mut().now_or_never().is_none());
            let foreign = tokio::spawn(async { tokio::task::try_id().expect("foreign task") })
                .await
                .expect("foreign task join");
            assert_eq!(
                clock.inspect_registered_deadline(5_005, foreign),
                RegisteredDeadlineRefusal::ForeignTask
            );
            assert!(!clock.try_advance_sole_registered_deadline(5_005, foreign));
            assert!(earlier.as_mut().now_or_never().is_none());
            assert_eq!(
                clock.inspect_registered_deadline(5_005, task),
                RegisteredDeadlineRefusal::EarlierTimer
            );
            assert!(!clock.try_advance_sole_registered_deadline(5_005, task));
            drop(earlier);
            assert!(duplicate.as_mut().now_or_never().is_none());
            assert_eq!(
                clock.inspect_registered_deadline(5_005, task),
                RegisteredDeadlineRefusal::DuplicateTimer
            );
            assert!(!clock.try_advance_sole_registered_deadline(5_005, task));
            drop(duplicate);
            assert_eq!(
                clock.inspect_registered_deadline(5_005, task),
                RegisteredDeadlineRefusal::Eligible
            );
            assert!(clock.try_advance_sole_registered_deadline(5_005, task));
            assert_eq!(clock.now_ms(), 5_005);
            batch.await.expect("original selected timer");
            assert!(!clock.try_advance_sole_registered_deadline(5_005, task));
        })
        .await
        .expect("original task completion");
    }

    #[tokio::test]
    async fn exact_two_owner_advance_refuses_extra_earlier_and_foreign_timers() {
        async fn pending_timer(
            clock: Arc<ManualPhysicalClock>,
            duration: u64,
        ) -> (
            tokio::task::JoinHandle<Result<(), TimeError>>,
            tokio::task::Id,
        ) {
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let handle = tokio::spawn(async move {
                let owner = tokio::task::try_id().expect("actual timer task");
                let mut wait = Box::pin(clock.sleep_ms(duration));
                assert!(wait.as_mut().now_or_never().is_none());
                sender.send(owner).expect("selected timer owner observed");
                wait.await
            });
            let owner = receiver.await.expect("original timer admitted");
            (handle, owner)
        }

        let clock = Arc::new(ManualPhysicalClock::new(5_000));
        let (first, issuer) = pending_timer(clock.clone(), 5).await;
        let (second, recipient) = pending_timer(clock.clone(), 5).await;
        assert!(!clock.try_advance_sole_registered_deadline(5_005, issuer));
        assert!(!clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, issuer]));
        let (foreign, foreign_task) = pending_timer(clock.clone(), 5).await;
        assert!(!clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, recipient]));
        foreign.abort();
        assert!(foreign
            .await
            .expect_err("foreign timer cancelled")
            .is_cancelled());
        assert!(!clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, foreign_task]));
        let mut earlier = Box::pin(clock.sleep_ms(3));
        assert!(earlier.as_mut().now_or_never().is_none());
        assert!(!clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, recipient]));
        drop(earlier);
        assert!(clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, recipient]));
        first
            .await
            .expect("issuer task retained")
            .expect("issuer original sleep");
        second
            .await
            .expect("recipient task retained")
            .expect("recipient original sleep");
        assert!(!clock.try_advance_exact_registered_deadline_set(5_005, &[issuer, recipient]));
    }

    #[tokio::test]
    async fn pending_relative_registration_uses_selected_now_without_shifting_absolute_deadline() {
        let clock = ManualPhysicalClock::new(5_000);
        let mut absolute = Box::pin(clock.wait_until_physical_deadline(WindowPosition::new(5_200)));
        assert!(absolute.as_mut().now_or_never().is_none());
        let registration_gate = clock.state.deadlines.lock().await;
        let mut relative = Box::pin(clock.sleep_ms(5));
        assert!(relative.as_mut().now_or_never().is_none());
        clock.set_time(5_010);
        drop(registration_gate);
        assert!(relative.as_mut().now_or_never().is_none());
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_015)),
            "relative target is selected atomically with actual registration"
        );
        clock.set_time(5_015);
        relative.await.expect("original relative wait");
        assert_eq!(
            clock.next_registered_deadline().await,
            Some(WindowPosition::new(5_200)),
            "preexisting absolute endpoint was not moved"
        );
        clock.set_time(5_200);
        assert_eq!(absolute.await.expect("original absolute wait").ts_ms, 5_200);
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
