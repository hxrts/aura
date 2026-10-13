//! Host-only supervised required-custody fixtures. No protocol receipts are issued.
//! Cancellation drops only the borrowed execution future; the same boxed owner
//! remains available for cleanup and, after failure, explicit reconciliation.
use std::{
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_lock::Mutex;
use async_trait::async_trait;
use aura_core::effects::task::{
    CancellationToken, RejectedRequiredTask, RequiredTaskControl, RequiredTaskCustody,
    RequiredTaskExit, RequiredTaskSettlement, TaskSpawner,
};
use aura_core::{AuraError, OwnedShutdownToken, OwnedTaskSpawner};
use futures::{
    future::{BoxFuture, LocalBoxFuture},
    FutureExt,
};
use tokio::{sync::watch, task::JoinHandle};

/// Actual execution and resource-settlement observations for one admitted task.
#[derive(Clone, Debug)]
pub struct TestTaskDisposalReport {
    /// Name retained at actual admission.
    pub name: &'static str,
    /// Original execution outcome, independent of domain and cleanup failures.
    pub execution: RequiredTaskExit,
    /// Domain failure returned by successful resource settlement.
    pub domain_failure: Option<AuraError>,
    /// First actual cleanup failure, retained even after reconciliation succeeds.
    pub cleanup_failure: Option<AuraError>,
    /// First actual worker interruption, retained independently from cleanup.
    pub worker_failure: Option<AuraError>,
    /// True only after the original owner's cleanup returned a resolved settlement.
    pub acknowledged: bool,
}

#[derive(Clone)]
struct TestCancellation(watch::Sender<bool>);
impl TestCancellation {
    fn new() -> Self {
        Self(watch::channel(false).0)
    }
    fn cancel(&self) {
        self.0.send_replace(true);
    }
}
#[async_trait]
impl CancellationToken for TestCancellation {
    async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        loop {
            if *receiver.borrow_and_update() {
                return;
            }
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
    fn is_cancelled(&self) -> bool {
        *self.0.borrow()
    }
}

struct Slot {
    owner: Mutex<Option<Box<dyn RequiredTaskCustody>>>,
    report: Mutex<Option<TestTaskDisposalReport>>,
    cancellation: TestCancellation,
    finished: AtomicBool,
    name: &'static str,
}
struct RetainedWorker {
    handle: Option<JoinHandle<()>>,
    outcome: Option<Result<(), tokio::task::JoinError>>,
}
impl RetainedWorker {
    fn new(handle: JoinHandle<()>) -> Self {
        Self {
            handle: Some(handle),
            outcome: None,
        }
    }
    async fn join(&mut self) {
        if let Some(handle) = self.handle.as_mut() {
            let outcome = handle.await;
            self.outcome = Some(outcome);
            self.handle.take();
        }
    }
}
struct Entry {
    slot: Arc<Slot>,
    worker: Mutex<RetainedWorker>,
    abort: tokio::task::AbortHandle,
}
struct Registry {
    closed: bool,
    entries: Vec<Arc<Entry>>,
    units: Vec<Arc<Mutex<RetainedWorker>>>,
}
struct Supervisor {
    registry: Mutex<Registry>,
    shutdown: TestCancellation,
    unit_shutdown: TestCancellation,
    unit_failure: Mutex<Option<AuraError>>,
    paused: watch::Sender<bool>,
}

fn fixture_error(message: &'static str) -> AuraError {
    AuraError::invalid(message)
}
fn panic_error() -> AuraError {
    fixture_error("supervised fixture cleanup panicked")
}

async fn settle(slot: &Slot, exit: RequiredTaskExit) -> Result<(), AuraError> {
    let mut owner = slot.owner.lock().await;
    let actual = owner
        .as_mut()
        .ok_or_else(|| fixture_error("original fixture custody already disposed"))?;
    // Acquire the observation destination before polling cleanup. Its returned
    // settlement and owner release have no intervening cancellation point.
    let mut report = slot.report.lock().await;
    report.get_or_insert_with(|| TestTaskDisposalReport {
        name: slot.name,
        execution: exit.clone(),
        domain_failure: None,
        cleanup_failure: None,
        worker_failure: None,
        acknowledged: false,
    });
    let result = AssertUnwindSafe(actual.cleanup(&exit))
        .catch_unwind()
        .await
        .unwrap_or_else(|_| Err(panic_error()));
    let retained = report.as_mut().expect("original execution report retained");
    match result {
        Ok(settlement) => {
            if let RequiredTaskSettlement::ResolvedWithDomainFailure(error) = settlement {
                retained.domain_failure.get_or_insert(error);
            }
            retained.acknowledged = true;
            // Release the original box only after actual cleanup acknowledgment.
            // Failed cleanup keeps it in this same slot, never in a fresh queue.
            owner.take();
            Ok(())
        }
        Err(error) => {
            retained
                .cleanup_failure
                .get_or_insert_with(|| error.clone());
            Err(error)
        }
    }
}

struct Control(Arc<Slot>);
impl RequiredTaskControl for Control {
    fn request_cancel(&mut self) {
        self.0.cancellation.cancel();
    }
    fn reconcile(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(async move {
            if !self.0.finished.load(Ordering::Acquire) {
                return Err(fixture_error("fixture execution or cleanup still live"));
            }
            let report = self
                .0
                .report
                .lock()
                .await
                .clone()
                .ok_or_else(|| fixture_error("fixture execution or cleanup still live"))?;
            if report.acknowledged {
                return Ok(());
            }
            settle(&self.0, report.execution).await
        })
    }
    fn reconcile_local(&mut self) -> LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(async move { self.reconcile().await })
    }
}

impl Supervisor {
    async fn wait_for_execution(mut paused: watch::Receiver<bool>) {
        loop {
            if !*paused.borrow_and_update() {
                return;
            }
            if paused.changed().await.is_err() {
                return;
            }
        }
    }
    fn record_unit(&self, future: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        let mut registry = self
            .registry
            .try_lock()
            .expect("fixture admission serialized");
        if registry.closed || self.shutdown.is_cancelled() {
            return;
        }
        let shutdown = self.shutdown.clone();
        let units = self.unit_shutdown.clone();
        registry
            .units
            .push(Arc::new(Mutex::new(RetainedWorker::new(tokio::spawn(
                async move {
                    tokio::select! { biased;
                        _ = shutdown.cancelled() => {},
                        _ = units.cancelled() => {},
                        _ = token.cancelled() => {},
                        _ = future => {},
                    }
                },
            )))));
    }
}
impl TaskSpawner for Supervisor {
    fn spawn(&self, future: BoxFuture<'static, ()>) {
        self.record_unit(future, Arc::new(self.shutdown.clone()));
    }
    fn spawn_cancellable(&self, future: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        self.record_unit(future, token);
    }
    fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
        self.spawn_local_cancellable(future, Arc::new(self.shutdown.clone()));
    }
    fn spawn_local_cancellable(
        &self,
        future: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        let mut registry = self
            .registry
            .try_lock()
            .expect("fixture admission serialized");
        if registry.closed || self.shutdown.is_cancelled() {
            return;
        }
        let shutdown = self.shutdown.clone();
        let units = self.unit_shutdown.clone();
        registry.units.push(Arc::new(Mutex::new(RetainedWorker::new(
            tokio::task::spawn_local(async move {
                tokio::select! { biased;
                    _ = shutdown.cancelled() => {},
                    _ = units.cancelled() => {},
                    _ = token.cancelled() => {},
                    _ = future => {},
                }
            }),
        ))));
    }
    fn admit_required_custody(
        &self,
        name: &'static str,
        obligation: Box<dyn RequiredTaskCustody>,
        token: Arc<dyn CancellationToken>,
    ) -> Result<Box<dyn RequiredTaskControl>, RejectedRequiredTask<dyn RequiredTaskCustody>> {
        let Some(mut registry) = self.registry.try_lock() else {
            return Err(RejectedRequiredTask {
                obligation,
                source: fixture_error("fixture admission busy"),
            });
        };
        if registry.closed || self.shutdown.is_cancelled() || token.is_cancelled() {
            return Err(RejectedRequiredTask {
                obligation,
                source: fixture_error("fixture admission closed"),
            });
        }
        let slot = Arc::new(Slot {
            owner: Mutex::new(Some(obligation)),
            report: Mutex::new(None),
            cancellation: TestCancellation::new(),
            finished: AtomicBool::new(false),
            name,
        });
        let original = slot.clone();
        let shutdown = self.shutdown.clone();
        let paused = self.paused.subscribe();
        let worker = tokio::spawn(async move {
            let exit = {
                let mut owner = original.owner.lock().await;
                let actual = owner.as_mut().expect("admitted original fixture custody");
                let execution = async {
                    Self::wait_for_execution(paused).await;
                    AssertUnwindSafe(actual.execute()).catch_unwind().await
                };
                tokio::select! { biased;
                    _ = original.cancellation.cancelled() => RequiredTaskExit::Cancelled,
                    _ = shutdown.cancelled() => RequiredTaskExit::Cancelled,
                    _ = token.cancelled() => RequiredTaskExit::Cancelled,
                    result = execution => match result {
                        Ok(Ok(())) => RequiredTaskExit::Completed,
                        Ok(Err(error)) => RequiredTaskExit::Failed(error),
                        Err(_) => RequiredTaskExit::Panicked,
                    },
                }
            };
            // A failed cleanup remains retained and is observable independently.
            let _retained_failure = settle(&original, exit).await.err();
            original.finished.store(true, Ordering::Release);
        });
        registry.entries.push(Arc::new(Entry {
            slot: slot.clone(),
            abort: worker.abort_handle(),
            worker: Mutex::new(RetainedWorker::new(worker)),
        }));
        Ok(Box::new(Control(slot)))
    }
    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(self.shutdown.clone())
    }
}

/// Host-only test owner for actual required execution, cancellation and disposal.
/// Drop requests shutdown; tests must explicitly drain to prove acknowledgment.
pub struct SupervisedTaskTestkit {
    supervisor: Arc<Supervisor>,
}
impl Default for SupervisedTaskTestkit {
    fn default() -> Self {
        Self::new()
    }
}
impl SupervisedTaskTestkit {
    /// Create an open supervisor with execution enabled and no timers.
    pub fn new() -> Self {
        Self {
            supervisor: Arc::new(Supervisor {
                registry: Mutex::new(Registry {
                    closed: false,
                    entries: Vec::new(),
                    units: Vec::new(),
                }),
                shutdown: TestCancellation::new(),
                unit_shutdown: TestCancellation::new(),
                unit_failure: Mutex::new(None),
                paused: watch::channel(false).0,
            }),
        }
    }
    /// Canonical owned spawner injected before runtime/app assembly.
    pub fn owned_spawner(&self) -> OwnedTaskSpawner {
        OwnedTaskSpawner::new(
            self.supervisor.clone(),
            OwnedShutdownToken::attached(self.supervisor.cancellation_token()),
        )
    }
    /// Hold execution before its first poll; cancellation still reaches cleanup.
    pub fn pause_execution(&self) {
        self.supervisor.paused.send_replace(true);
    }
    /// Release the authoritative execution gate without advancing time.
    pub fn resume_execution(&self) {
        self.supervisor.paused.send_replace(false);
    }
    /// Request cancellation of all owned work and close future admissions.
    pub fn shutdown(&self) {
        self.supervisor.shutdown.cancel();
    }
    /// Inject actual worker interruption without pretending required cleanup ran.
    /// The original box and native JoinError remain supervised for reconciliation.
    pub async fn abort_workers(&self) {
        let entries = self.supervisor.registry.lock().await.entries.clone();
        for entry in entries {
            entry.abort.abort();
        }
    }
    /// Seal admission, join actual workers, and inspect their independent cleanup outcomes.
    /// Unresolved cleanup keeps its original box and may be reconciled via its control.
    pub async fn drain(&self) -> Result<Vec<TestTaskDisposalReport>, AuraError> {
        let (entries, units) = {
            let mut registry = self.supervisor.registry.lock().await;
            registry.closed = true;
            (registry.entries.clone(), registry.units.clone())
        };
        self.supervisor.unit_shutdown.cancel();
        for entry in &entries {
            // Await by mutable reference in the original slot. Cancelling this
            // drain drops only the lock guard, never the retained join handle.
            let mut worker = entry.worker.lock().await;
            worker.join().await;
            let mut report = entry.slot.report.lock().await;
            if let Some(Err(source)) = worker.outcome.take() {
                let exit = if source.is_cancelled() {
                    RequiredTaskExit::Cancelled
                } else {
                    RequiredTaskExit::Panicked
                };
                let failure = AuraError::Internal {
                    message: "fixture required worker interrupted before disposal".into(),
                    source: Some(Arc::new(source)),
                };
                let retained = report.get_or_insert_with(|| TestTaskDisposalReport {
                    name: entry.slot.name,
                    execution: exit,
                    domain_failure: None,
                    cleanup_failure: None,
                    worker_failure: None,
                    acknowledged: false,
                });
                retained.worker_failure.get_or_insert(failure);
                retained.acknowledged = false;
                entry.slot.finished.store(true, Ordering::Release);
            }
        }
        for unit in units {
            let mut worker = unit.lock().await;
            worker.join().await;
            let mut failure = self.supervisor.unit_failure.lock().await;
            if let Some(Err(source)) = worker.outcome.take() {
                failure.get_or_insert_with(|| AuraError::Internal {
                    message: "fixture unit worker failed".into(),
                    source: Some(Arc::new(source)),
                });
            }
        }
        let mut reports = Vec::new();
        for entry in entries {
            reports.push(
                entry
                    .slot
                    .report
                    .lock()
                    .await
                    .clone()
                    .ok_or_else(|| fixture_error("fixture worker lacks disposal outcome"))?,
            );
        }
        if let Some(source) = self.supervisor.unit_failure.lock().await.clone() {
            return Err(source);
        }
        Ok(reports)
    }
}
impl Drop for SupervisedTaskTestkit {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct OriginalCustody {
        entered: Arc<AtomicBool>,
        cleanups: Arc<AtomicUsize>,
        failure: Option<AuraError>,
        cleanup_failure: Option<AuraError>,
        cleanup_gate: Option<tokio::sync::oneshot::Sender<()>>,
    }
    impl RequiredTaskCustody for OriginalCustody {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async move {
                self.entered.store(true, Ordering::Release);
                match self.failure.clone() {
                    Some(error) => Err(error),
                    None => futures::future::pending().await,
                }
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                self.cleanups.fetch_add(1, Ordering::AcqRel);
                if let Some(entered) = self.cleanup_gate.take() {
                    entered.send(()).expect("actual cleanup entered");
                    futures::future::pending::<()>().await;
                }
                match self.cleanup_failure.take() {
                    Some(error) => Err(error),
                    None => Ok(RequiredTaskSettlement::Resolved),
                }
            })
        }
    }
    fn custody() -> OriginalCustody {
        OriginalCustody {
            entered: Arc::new(AtomicBool::new(false)),
            cleanups: Arc::new(AtomicUsize::new(0)),
            failure: None,
            cleanup_failure: None,
            cleanup_gate: None,
        }
    }

    #[tokio::test]
    async fn cancellation_before_execute_retains_original_cleanup_and_acknowledges_it() {
        let kit = SupervisedTaskTestkit::new();
        kit.pause_execution();
        let owner = custody();
        let entered = owner.entered.clone();
        let cleanups = owner.cleanups.clone();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(owner))
            .unwrap_or_else(|_| panic!("actual original admission"));
        control.request_cancel();
        let reports = kit.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(!entered.load(Ordering::Acquire));
        assert_eq!(cleanups.load(Ordering::Acquire), 1);
        assert!(matches!(reports[0].execution, RequiredTaskExit::Cancelled));
        assert!(reports[0].acknowledged);
        assert!(reports[0].cleanup_failure.is_none());
    }

    #[tokio::test]
    async fn failed_cleanup_retains_same_box_native_causes_and_reconciliation() {
        use std::error::Error;
        let kit = SupervisedTaskTestkit::new();
        let mut owner = custody();
        let execution = Arc::new(std::io::Error::other("original execution"));
        let cleanup = Arc::new(std::io::Error::other("original cleanup"));
        owner.failure = Some(AuraError::Internal {
            message: "execution".into(),
            source: Some(execution.clone()),
        });
        owner.cleanup_failure = Some(AuraError::Internal {
            message: "cleanup".into(),
            source: Some(cleanup.clone()),
        });
        let cleanups = owner.cleanups.clone();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(owner))
            .unwrap_or_else(|_| panic!("actual original admission"));
        let reports = kit.drain().await.unwrap();
        assert!(!reports[0].acknowledged);
        let RequiredTaskExit::Failed(original) = &reports[0].execution else {
            panic!("actual execution failure lost");
        };
        assert!(std::ptr::eq(
            original
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap(),
            execution.as_ref()
        ));
        assert!(std::ptr::eq(
            reports[0]
                .cleanup_failure
                .as_ref()
                .unwrap()
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap(),
            cleanup.as_ref()
        ));
        control.reconcile().await.unwrap();
        let reports = kit.drain().await.unwrap();
        assert!(reports[0].acknowledged);
        assert!(reports[0].cleanup_failure.is_some());
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn cancelled_drain_keeps_original_worker_handle_for_later_acknowledgment() {
        let kit = SupervisedTaskTestkit::new();
        kit.pause_execution();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(custody()))
            .unwrap_or_else(|_| panic!("actual original admission"));
        assert!(kit.drain().now_or_never().is_none());
        control.request_cancel();
        let reports = kit.drain().await.unwrap();
        assert_eq!(reports.len(), 1);
        assert!(reports[0].acknowledged);
        assert!(matches!(reports[0].execution, RequiredTaskExit::Cancelled));
    }
    #[tokio::test]
    async fn interrupted_cleanup_retains_native_execution_and_original_reconciliation() {
        use std::error::Error;
        let kit = SupervisedTaskTestkit::new();
        let mut owner = custody();
        let original = Arc::new(std::io::Error::other(
            "original execution before interrupted cleanup",
        ));
        owner.failure = Some(AuraError::Internal {
            message: "execution".into(),
            source: Some(original.clone()),
        });
        let (entered, ready) = tokio::sync::oneshot::channel();
        owner.cleanup_gate = Some(entered);
        let cleanups = owner.cleanups.clone();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(owner))
            .unwrap_or_else(|_| panic!("actual original admission"));
        ready.await.expect("original cleanup entered");
        let entry = kit.supervisor.registry.lock().await.entries[0].clone();
        assert!(
            entry.slot.report.try_lock().is_none(),
            "cleanup retains its acknowledgment destination until settlement"
        );
        assert!(
            control.reconcile().await.is_err(),
            "live cleanup cannot be reconciled"
        );
        kit.abort_workers().await;
        let reports = kit.drain().await.unwrap();
        assert!(!reports[0].acknowledged);
        assert!(reports[0].cleanup_failure.is_none());
        assert!(reports[0]
            .worker_failure
            .as_ref()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<tokio::task::JoinError>()
            .unwrap()
            .is_cancelled());
        let RequiredTaskExit::Failed(error) = &reports[0].execution else {
            panic!("first native execution failure lost");
        };
        assert!(std::ptr::eq(
            error
                .source()
                .unwrap()
                .downcast_ref::<std::io::Error>()
                .unwrap(),
            original.as_ref()
        ));
        control.reconcile().await.unwrap();
        let reports = kit.drain().await.unwrap();
        assert!(reports[0].acknowledged);
        assert!(reports[0].worker_failure.is_some());
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
    }

    #[tokio::test]
    async fn repeated_drain_preserves_aborted_worker_native_failure_without_repolling() {
        use std::error::Error;
        let kit = SupervisedTaskTestkit::new();
        kit.pause_execution();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(custody()))
            .unwrap_or_else(|_| panic!("actual original admission"));
        kit.abort_workers().await;
        let first = kit.drain().await.unwrap();
        let repeated = kit.drain().await.unwrap();
        assert!(!first[0].acknowledged && !repeated[0].acknowledged);
        let a = first[0]
            .worker_failure
            .as_ref()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<tokio::task::JoinError>()
            .unwrap();
        let b = repeated[0]
            .worker_failure
            .as_ref()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<tokio::task::JoinError>()
            .unwrap();
        assert!(a.is_cancelled() && std::ptr::eq(a, b));
        assert!(matches!(repeated[0].execution, RequiredTaskExit::Cancelled));
        control.reconcile().await.unwrap();
        assert!(kit.drain().await.unwrap()[0].acknowledged);
    }
    #[tokio::test]
    async fn cancelled_drain_after_join_retains_original_native_failure() {
        use std::error::Error;
        let kit = SupervisedTaskTestkit::new();
        kit.pause_execution();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(custody()))
            .unwrap_or_else(|_| panic!("actual original admission"));
        let entry = kit.supervisor.registry.lock().await.entries[0].clone();
        kit.abort_workers().await;
        entry.worker.lock().await.join().await;
        let held_report = entry.slot.report.lock().await;
        {
            let drain = kit.drain();
            futures::pin_mut!(drain);
            assert!(futures::poll!(drain.as_mut()).is_pending());
        }
        assert!(entry.worker.lock().await.outcome.is_some());
        drop(held_report);
        let report = kit.drain().await.unwrap();
        assert!(!report[0].acknowledged);
        assert!(report[0]
            .worker_failure
            .as_ref()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<tokio::task::JoinError>()
            .unwrap()
            .is_cancelled());
        control.reconcile().await.unwrap();
        assert!(kit.drain().await.unwrap()[0].acknowledged);
    }
    #[tokio::test]
    async fn cancelled_settlement_wait_never_reexecutes_acknowledged_cleanup() {
        let kit = SupervisedTaskTestkit::new();
        let mut owner = custody();
        owner.failure = Some(fixture_error("execution"));
        owner.cleanup_failure = Some(fixture_error("first cleanup"));
        let cleanups = owner.cleanups.clone();
        let mut control = kit
            .owned_spawner()
            .spawn_required_custody("original", Box::new(owner))
            .unwrap_or_else(|_| panic!("actual original admission"));
        assert!(!kit.drain().await.unwrap()[0].acknowledged);
        assert_eq!(cleanups.load(Ordering::Acquire), 1);
        let entry = kit.supervisor.registry.lock().await.entries[0].clone();
        let held_report = entry.slot.report.lock().await;
        {
            // Exercise the settlement path itself, after control's initial read,
            // with the destination deliberately contended before cleanup polls.
            let settlement = settle(&entry.slot, RequiredTaskExit::Cancelled);
            futures::pin_mut!(settlement);
            assert!(futures::poll!(settlement.as_mut()).is_pending());
        }
        assert_eq!(cleanups.load(Ordering::Acquire), 1);
        drop(held_report);
        control.reconcile().await.unwrap();
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
        assert!(kit.drain().await.unwrap()[0].acknowledged);
        control.reconcile().await.unwrap();
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
    }
}
