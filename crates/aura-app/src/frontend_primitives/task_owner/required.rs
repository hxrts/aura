//! Required obligations retained by the existing frontend task root.
use super::{FrontendTaskSpawnerImpl, TrackedTask};
use async_lock::Mutex;
use aura_core::effects::task::{
    CancellationToken, RejectedRequiredTask, RequiredTaskControl, RequiredTaskCustody,
    RequiredTaskExit, RequiredTaskSettlement,
};
use aura_core::AuraError;
use futures::{
    future::{BoxFuture, LocalBoxFuture},
    FutureExt,
};
use std::{
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
};

/// Supervision evidence only; no protocol, storage or publication receipt is issued.
#[derive(Clone, Debug)]
pub struct FrontendRequiredTaskReport {
    pub name: &'static str,
    pub execution: Option<RequiredTaskExit>,
    pub domain_failure: Option<AuraError>,
    pub cleanup_failure: Option<AuraError>,
    pub worker_interrupted: bool,
    pub acknowledged: bool,
}

#[derive(Default)]
struct RequiredCancellation {
    cancelled: AtomicBool,
    changed: futures::task::AtomicWaker,
}
impl RequiredCancellation {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.changed.wake();
    }
    async fn cancelled(&self) {
        futures::future::poll_fn(|cx| {
            self.changed.register(cx.waker());
            if self.cancelled.load(Ordering::Acquire) {
                std::task::Poll::Ready(())
            } else {
                std::task::Poll::Pending
            }
        })
        .await;
    }
}
struct RequiredSlot {
    owner: Mutex<Option<Box<dyn RequiredTaskCustody>>>,
    report: Mutex<FrontendRequiredTaskReport>,
    execution: OnceLock<RequiredTaskExit>,
    cancellation: RequiredCancellation,
    finished: AtomicBool,
    interrupted: AtomicBool,
    resolved: AtomicBool,
}
impl RequiredSlot {
    async fn settle(&self) -> Result<(), AuraError> {
        let mut owner = self.owner.lock().await;
        let Some(actual) = owner.as_mut() else {
            return Ok(());
        };
        let exit = self
            .execution
            .get()
            .ok_or_else(|| AuraError::invalid("required frontend execution is still active"))?;
        // Retain the observation destination across cleanup. A Ready settlement
        // and release of the original owner have no intervening await.
        let mut report = self.report.lock().await;
        match AssertUnwindSafe(actual.cleanup(exit)).catch_unwind().await {
            Ok(Ok(settlement)) => {
                if let RequiredTaskSettlement::ResolvedWithDomainFailure(source) = settlement {
                    report.domain_failure.get_or_insert(source);
                }
                report.acknowledged = true;
                owner.take();
                self.resolved.store(true, Ordering::Release);
                Ok(())
            }
            outcome => {
                let source = match outcome {
                    Ok(Err(source)) => source,
                    Err(_) => AuraError::invalid("required frontend cleanup panicked"),
                    Ok(Ok(_)) => unreachable!(),
                };
                report.cleanup_failure.get_or_insert_with(|| source.clone());
                Err(source)
            }
        }
    }
    async fn snapshot(&self) -> FrontendRequiredTaskReport {
        let mut report = self.report.lock().await.clone();
        report.execution = self.execution.get().cloned();
        report.worker_interrupted = self.interrupted.load(Ordering::Acquire);
        report
    }
}
struct RequiredWorker {
    slot: Arc<RequiredSlot>,
    complete: bool,
}
impl RequiredWorker {
    fn finish(&mut self) {
        self.complete = true;
    }
}
impl Drop for RequiredWorker {
    fn drop(&mut self) {
        if !self.complete {
            // The actual borrowed worker future was destroyed. This is not a
            // domain outcome or a cleanup acknowledgment.
            self.slot
                .execution
                .get_or_init(|| RequiredTaskExit::Cancelled);
            self.slot.interrupted.store(true, Ordering::Release);
        }
        self.slot.finished.store(true, Ordering::Release);
    }
}
struct RequiredControl(Arc<RequiredSlot>);
impl RequiredTaskControl for RequiredControl {
    fn request_cancel(&mut self) {
        self.0.cancellation.cancel();
    }
    fn reconcile(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(async {
            if !self.0.finished.load(Ordering::Acquire) {
                return Err(AuraError::invalid(
                    "required frontend worker has not finished",
                ));
            }
            self.0.settle().await
        })
    }
    fn reconcile_local(&mut self) -> LocalBoxFuture<'_, Result<(), AuraError>> {
        self.reconcile()
    }
}

#[derive(Default)]
pub(super) struct RequiredRegistry(Mutex<Vec<Arc<RequiredSlot>>>);
impl std::fmt::Debug for RequiredRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequiredRegistry").finish_non_exhaustive()
    }
}
impl RequiredRegistry {
    pub(super) async fn reports(&self) -> Vec<FrontendRequiredTaskReport> {
        let slots = self.0.lock().await.clone();
        let mut reports = Vec::with_capacity(slots.len());
        for slot in slots {
            reports.push(slot.snapshot().await);
        }
        reports
    }
    pub(super) async fn require_disposed(&self) -> Result<(), AuraError> {
        for report in self.reports().await {
            if !report.acknowledged {
                return Err(report.cleanup_failure.unwrap_or_else(|| {
                    AuraError::invalid(format!(
                        "required frontend custody remains unresolved: {}",
                        report.name
                    ))
                }));
            }
        }
        Ok(())
    }
}
impl FrontendTaskSpawnerImpl {
    pub(super) fn admit_required(
        &self,
        name: &'static str,
        obligation: Box<dyn RequiredTaskCustody>,
        token: Arc<dyn CancellationToken>,
    ) -> Result<Box<dyn RequiredTaskControl>, RejectedRequiredTask<dyn RequiredTaskCustody>> {
        let Some(mut registry) = self.required.0.try_lock() else {
            return Err(RejectedRequiredTask {
                obligation,
                source: AuraError::invalid("required frontend admission is busy"),
            });
        };
        // Settled resource records no longer require root retention; original
        // opaque controls still retain their own observation. Admission is bounded.
        registry.retain(|slot| !slot.resolved.load(Ordering::Acquire));
        if registry.len() >= 128 {
            return Err(RejectedRequiredTask {
                obligation,
                source: AuraError::invalid("required frontend custody capacity is exhausted"),
            });
        }
        let Some(lease) = self.completion.admit() else {
            return Err(RejectedRequiredTask {
                obligation,
                source: AuraError::invalid("required frontend admission is closed"),
            });
        };
        let slot = Arc::new(RequiredSlot {
            owner: Mutex::new(Some(obligation)),
            report: Mutex::new(FrontendRequiredTaskReport {
                name,
                execution: None,
                domain_failure: None,
                cleanup_failure: None,
                worker_interrupted: false,
                acknowledged: false,
            }),
            execution: OnceLock::new(),
            cancellation: RequiredCancellation::default(),
            finished: AtomicBool::new(false),
            interrupted: AtomicBool::new(false),
            resolved: AtomicBool::new(false),
        });
        registry.push(slot.clone());
        drop(registry);
        // Construct before spawning: dropping even an unpolled worker retains
        // original custody and records actual worker interruption.
        let mut worker = RequiredWorker {
            slot: slot.clone(),
            complete: false,
        };
        let future: BoxFuture<'static, ()> = Box::pin(async move {
            let exit = {
                let mut owner = worker.slot.owner.lock().await;
                if let Some(actual) = owner.as_mut() {
                    let execution = AssertUnwindSafe(actual.execute()).catch_unwind().fuse();
                    futures::pin_mut!(execution);
                    futures::select_biased! {
                    _ = token.cancelled().fuse() => RequiredTaskExit::Cancelled,
                    _ = worker.slot.cancellation.cancelled().fuse() => RequiredTaskExit::Cancelled,
                    outcome = execution => match outcome {
                        Ok(Ok(())) => RequiredTaskExit::Completed,
                        Ok(Err(source)) => RequiredTaskExit::Failed(source),
                        Err(_) => RequiredTaskExit::Panicked,
                    },
                    }
                } else {
                    RequiredTaskExit::Failed(AuraError::invalid(
                        "admitted original frontend owner missing",
                    ))
                }
            };
            worker.slot.execution.get_or_init(|| exit);
            let _ = worker.slot.settle().await;
            worker.finish();
        });
        (self.runtime.spawn)(Box::pin(TrackedTask {
            future: Some(future),
            lease: Some(lease),
        }));
        Ok(Box::new(RequiredControl(slot)))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{FrontendTaskOwner, FrontendTaskRuntime};
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct OriginalOwner {
        dropped: Arc<AtomicUsize>,
        cleanups: Arc<AtomicUsize>,
        fail_cleanup: Arc<AtomicBool>,
        ready: Option<futures::channel::oneshot::Sender<()>>,
        failure: AuraError,
    }
    impl Drop for OriginalOwner {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }
    impl RequiredTaskCustody for OriginalOwner {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async {
                if let Some(ready) = self.ready.take() {
                    let _ = ready.send(());
                }
                futures::future::pending().await
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            exit: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                assert!(matches!(exit, RequiredTaskExit::Cancelled));
                self.cleanups.fetch_add(1, Ordering::AcqRel);
                if self.fail_cleanup.load(Ordering::Acquire) {
                    Err(self.failure.clone())
                } else {
                    Ok(RequiredTaskSettlement::Resolved)
                }
            })
        }
    }
    fn runtime() -> FrontendTaskRuntime {
        fn spawn(future: BoxFuture<'static, ()>) {
            tokio::spawn(future);
        }
        fn local(future: LocalBoxFuture<'static, ()>) {
            tokio::task::spawn_local(future);
        }
        FrontendTaskRuntime::new(spawn, local)
    }
    fn original(fail: bool) -> (OriginalOwner, futures::channel::oneshot::Receiver<()>) {
        let (ready, receive) = futures::channel::oneshot::channel();
        (
            OriginalOwner {
                dropped: Arc::new(AtomicUsize::new(0)),
                cleanups: Arc::new(AtomicUsize::new(0)),
                fail_cleanup: Arc::new(AtomicBool::new(fail)),
                ready: Some(ready),
                failure: AuraError::Storage {
                    message: "configured required cleanup refused".into(),
                    source: Some(Arc::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "original fixture provider refusal",
                    ))),
                },
            },
            receive,
        )
    }

    #[tokio::test]
    async fn required_custody_closed_admission_returns_original_owner() {
        let owner = FrontendTaskOwner::new(runtime());
        owner.shutdown();
        let (original, _) = original(false);
        let dropped = original.dropped.clone();
        let rejection = match owner
            .owned_spawner()
            .spawn_required_custody("closed", Box::new(original))
        {
            Err(rejection) => rejection,
            Ok(_) => panic!("closed root admitted original obligation"),
        };
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert!(rejection.source.to_string().contains("closed"));
        drop(rejection.obligation);
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        owner.wait_required_disposed().await.unwrap();
    }

    #[tokio::test]
    async fn required_custody_cleanup_failure_retains_original_for_reconciliation() {
        let owner = FrontendTaskOwner::new(runtime());
        let (original, ready) = original(true);
        let dropped = original.dropped.clone();
        let cleanups = original.cleanups.clone();
        let fail = original.fail_cleanup.clone();
        let expected_source = match &original.failure {
            AuraError::Storage {
                source: Some(source),
                ..
            } => source.clone(),
            _ => unreachable!(),
        };
        let mut handle = owner
            .owned_spawner()
            .spawn_required_custody("retained", Box::new(original))
            .unwrap();
        ready.await.unwrap();
        owner.shutdown();
        assert!(owner.wait_required_disposed().await.is_err());
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert_eq!(cleanups.load(Ordering::Acquire), 1);
        let report = owner.required_task_reports().await.remove(0);
        assert!(matches!(
            report.execution,
            Some(RequiredTaskExit::Cancelled)
        ));
        assert!(!report.acknowledged);
        assert!(report.cleanup_failure.is_some());
        let observed_source = match report.cleanup_failure.as_ref().unwrap() {
            AuraError::Storage {
                source: Some(source),
                ..
            } => source,
            _ => panic!("original concrete provider cause must survive supervision"),
        };
        assert!(Arc::ptr_eq(&expected_source, observed_source));
        assert!(!report.worker_interrupted);
        fail.store(false, Ordering::Release);
        handle.reconcile().await.unwrap();
        owner.wait_required_disposed().await.unwrap();
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
        let report = owner.required_task_reports().await.remove(0);
        assert!(report.acknowledged);
        assert!(
            report.cleanup_failure.is_some(),
            "first concrete cleanup failure remains"
        );
    }

    #[tokio::test]
    async fn required_custody_observer_drop_does_not_cancel_original_worker() {
        let owner = FrontendTaskOwner::new(runtime());
        let (original, ready) = original(false);
        let dropped = original.dropped.clone();
        let handle = owner
            .owned_spawner()
            .spawn_required_custody("observer", Box::new(original))
            .unwrap();
        ready.await.unwrap();
        drop(owner.required_task_reports());
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        drop(handle);
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        owner.shutdown();
        owner.wait_required_disposed().await.unwrap();
        assert_eq!(dropped.load(Ordering::Acquire), 1);
    }

    #[tokio::test]
    async fn required_custody_cleanup_interruption_preserves_original_failure() {
        // Keep the actual worker in this deterministic executor so its borrowed
        // cleanup can be destroyed at a known Pending boundary, without clocks.
        thread_local! {
            static WORKER: std::cell::RefCell<Option<BoxFuture<'static, ()>>> =
                const { std::cell::RefCell::new(None) };
        }
        fn retain(future: BoxFuture<'static, ()>) {
            WORKER.with(|worker| assert!(worker.borrow_mut().replace(future).is_none()));
        }
        fn local(_: LocalBoxFuture<'static, ()>) {
            panic!("this fixture admits only a Send worker");
        }
        struct InterruptedCleanup {
            original: OriginalOwner,
            cleanup_ready: Option<futures::channel::oneshot::Sender<()>>,
            resume: futures::channel::oneshot::Receiver<()>,
        }
        impl RequiredTaskCustody for InterruptedCleanup {
            fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
                Box::pin(async { Err(self.original.failure.clone()) })
            }
            fn cleanup<'a>(
                &'a mut self,
                exit: &'a RequiredTaskExit,
            ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
                Box::pin(async move {
                    let RequiredTaskExit::Failed(source) = exit else {
                        panic!("cleanup must retain the actual original execution failure");
                    };
                    self.original.cleanups.fetch_add(1, Ordering::AcqRel);
                    if let Some(ready) = self.cleanup_ready.take() {
                        ready.send(()).unwrap();
                    }
                    (&mut self.resume).await.expect("actual cleanup release");
                    Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                        source.clone(),
                    ))
                })
            }
        }
        let root = FrontendTaskOwner::new(FrontendTaskRuntime::new(retain, local));
        let (original, _) = original(false);
        let dropped = original.dropped.clone();
        let cleanups = original.cleanups.clone();
        let expected_source = match &original.failure {
            AuraError::Storage {
                source: Some(source),
                ..
            } => source.clone(),
            _ => unreachable!(),
        };
        let (ready, cleanup_ready) = futures::channel::oneshot::channel();
        let (release, resume) = futures::channel::oneshot::channel();
        let mut handle = root
            .owned_spawner()
            .spawn_required_custody(
                "interrupted-cleanup",
                Box::new(InterruptedCleanup {
                    original,
                    cleanup_ready: Some(ready),
                    resume,
                }),
            )
            .unwrap();
        let mut worker = WORKER.with(|worker| worker.borrow_mut().take().unwrap());
        assert!(futures::poll!(&mut worker).is_pending());
        cleanup_ready.await.unwrap();
        drop(worker);
        root.shutdown();
        // Shutdown dispatch is itself an owned task in the same executor.
        // Drive its real completion before observing root drainage.
        let shutdown_dispatch = WORKER.with(|worker| worker.borrow_mut().take().unwrap());
        shutdown_dispatch.await;
        assert!(root.wait_required_disposed().await.is_err());
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        assert_eq!(cleanups.load(Ordering::Acquire), 1);
        let report = root.required_task_reports().await.remove(0);
        assert!(report.worker_interrupted);
        assert!(!report.acknowledged);
        assert!(
            report.cleanup_failure.is_none(),
            "worker destruction is not a provider refusal"
        );
        let Some(RequiredTaskExit::Failed(AuraError::Storage {
            source: Some(observed),
            ..
        })) = report.execution
        else {
            panic!("worker destruction must not overwrite the original execution failure");
        };
        assert!(Arc::ptr_eq(&expected_source, &observed));
        release.send(()).unwrap();
        handle.reconcile().await.unwrap();
        root.wait_required_disposed().await.unwrap();
        assert_eq!(dropped.load(Ordering::Acquire), 1);
        assert_eq!(cleanups.load(Ordering::Acquire), 2);
        let report = root.required_task_reports().await.remove(0);
        assert!(report.acknowledged);
        assert!(report.worker_interrupted);
        let Some(AuraError::Storage {
            source: Some(observed),
            ..
        }) = report.domain_failure
        else {
            panic!("actual resource settlement must preserve the independent domain failure");
        };
        assert!(Arc::ptr_eq(&expected_source, &observed));
    }

    #[tokio::test]
    async fn required_custody_unpolled_worker_drop_retains_original_owner() {
        fn dispose(_: BoxFuture<'static, ()>) {}
        fn local(_: LocalBoxFuture<'static, ()>) {}
        let owner = FrontendTaskOwner::new(FrontendTaskRuntime::new(dispose, local));
        let (original, _) = original(false);
        let dropped = original.dropped.clone();
        let mut handle = owner
            .owned_spawner()
            .spawn_required_custody("unpolled", Box::new(original))
            .unwrap();
        owner.shutdown();
        assert!(owner.wait_required_disposed().await.is_err());
        assert_eq!(dropped.load(Ordering::Acquire), 0);
        let report = owner.required_task_reports().await.remove(0);
        assert!(report.worker_interrupted);
        assert!(!report.acknowledged);
        handle.reconcile().await.unwrap();
        owner.wait_required_disposed().await.unwrap();
        assert_eq!(dropped.load(Ordering::Acquire), 1);
    }
}
