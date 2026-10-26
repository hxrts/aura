//! Original-executor retry custody; sample resources are not protocol ACKs.
use super::*;
use aura_core::effects::task::RequiredTaskControl;

struct ResidentOwner {
    primary: Arc<std::io::Error>,
    cleanup_causes: Vec<Arc<std::io::Error>>,
    gates: std::collections::VecDeque<tokio::sync::oneshot::Receiver<()>>,
    entered: tokio::sync::mpsc::Sender<usize>,
    attempt: usize,
    identity: Option<OwnedRuntimeTaskIdentity>,
    #[cfg(not(target_arch = "wasm32"))]
    executor_task: Option<tokio::task::Id>,
    resource: Option<Arc<()>>,
    panic_first: bool,
}
impl ResidentOwner {
    async fn execute_original(&mut self) -> Result<(), AuraError> {
        self.identity = current_owned_runtime_task();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.executor_task = Some(
                tokio::task::try_id().expect("actual registered original runs in a Tokio task"),
            );
        }
        Err(AuraError::Internal {
            message: "original native operation failed".into(),
            source: Some(self.primary.clone()),
        })
    }
    async fn cleanup_original(
        &mut self,
        original: &RequiredTaskExit,
    ) -> Result<RequiredTaskSettlement, AuraError> {
        assert_eq!(current_owned_runtime_task(), self.identity);
        assert!(matches!(original, RequiredTaskExit::Failed(_)));
        let attempt = self.attempt;
        self.attempt += 1;
        self.entered
            .try_send(attempt)
            .expect("actual cleanup observer remains alive");
        self.gates
            .pop_front()
            .expect("one actual attempt gate")
            .await
            .expect("same original cleanup release");
        assert_eq!(current_owned_runtime_task(), self.identity);
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(
            tokio::task::try_id(),
            self.executor_task,
            "same physical original executor after actual cleanup await"
        );
        if attempt < self.cleanup_causes.len() {
            if self.panic_first && attempt == 0 {
                std::panic::panic_any(self.cleanup_causes[attempt].clone());
            }
            return Err(AuraError::Internal {
                message: "actual original cleanup refusal".into(),
                source: Some(self.cleanup_causes[attempt].clone()),
            });
        }
        drop(self.resource.take());
        Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
            AuraError::Internal {
                message: "same original failed operation".into(),
                source: Some(self.primary.clone()),
            },
        ))
    }
}
impl RequiredTaskCustody for ResidentOwner {
    fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        original: &'a RequiredTaskExit,
    ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(self.cleanup_original(original))
    }
}
impl LocalRequiredTaskCustody for ResidentOwner {
    fn execute(&mut self) -> LocalBoxFuture<'_, Result<(), AuraError>> {
        Box::pin(self.execute_original())
    }
    fn cleanup<'a>(
        &'a mut self,
        original: &'a RequiredTaskExit,
    ) -> LocalBoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        Box::pin(async move {
            let local = std::rc::Rc::new(());
            let outcome = self.cleanup_original(original).await;
            drop(local);
            outcome
        })
    }
}
fn cause<'a, T: std::error::Error + 'static>(
    mut error: &'a (dyn std::error::Error + 'static),
) -> Option<&'a T> {
    loop {
        if let Some(actual) = error.downcast_ref::<T>() {
            return Some(actual);
        }
        error = error.source()?;
    }
}
async fn original_executor_retry(local: bool, panic_first: bool) {
    let supervisor = TaskSupervisor::new();
    let group = supervisor.group("original-resident-cleanup");
    let primary = Arc::new(std::io::Error::other("original native operation"));
    let cleanup_causes: Vec<_> = (0..3)
        .map(|n| {
            Arc::new(std::io::Error::other(format!(
                "actual cleanup occurrence {n}"
            )))
        })
        .collect();
    let resource = Arc::new(());
    let weak = Arc::downgrade(&resource);
    let (entered, mut entries) = tokio::sync::mpsc::channel(4);
    let mut releases = Vec::new();
    let mut gates = std::collections::VecDeque::new();
    for _ in 0..4 {
        let (release, gate) = tokio::sync::oneshot::channel();
        releases.push(release);
        gates.push_back(gate);
    }
    let owner = ResidentOwner {
        primary: primary.clone(),
        cleanup_causes: cleanup_causes.clone(),
        gates,
        entered,
        attempt: 0,
        identity: None,
        #[cfg(not(target_arch = "wasm32"))]
        executor_task: None,
        resource: Some(resource),
        panic_first,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let admitted = if local {
        group.spawn_original_local_required_custody_with_resident_cleanup(
            "actual-original".into(),
            Box::new(owner),
            None,
        )
    } else {
        group.spawn_original_required_custody_with_resident_cleanup(
            "actual-original".into(),
            Box::new(owner),
            None,
        )
    };
    #[cfg(target_arch = "wasm32")]
    let admitted = {
        assert!(local);
        group.spawn_original_local_required_custody_with_resident_cleanup(
            "actual-original".into(),
            Box::new(owner),
            None,
        )
    };
    let mut original = admitted.unwrap_or_else(|_| panic!("actual original admission"));
    original
        .request_resident_cleanup_retry()
        .expect("pre-observation request retains initial original attempt");
    let observer = original.disposal_observer();
    let sealed = group.seal_admission();
    let mut first = None;
    for (attempt, release) in releases.into_iter().enumerate() {
        assert_eq!(
            entries.recv().await,
            Some(attempt),
            "actual original cleanup is pending"
        );
        assert!(weak.upgrade().is_some());
        assert!(observer.await_disposal().now_or_never().is_none());
        assert!(sealed.await_disposal().now_or_never().is_none());
        // Observation abandonment cannot own or destroy the resident future.
        if local {
            let mut abandoned = Box::pin(original.reconcile_local());
            assert!(futures::poll!(abandoned.as_mut()).is_pending());
        } else {
            let mut abandoned = Box::pin(original.reconcile());
            assert!(futures::poll!(abandoned.as_mut()).is_pending());
        }
        original.request_cancel();
        assert!(matches!(
            group.force_abort_remaining(),
            Err(TaskSupervisionError::ForcedAbort { .. })
        ));
        release
            .send(())
            .expect("same original cleanup future survives cancellation");
        if attempt < 3 {
            // Observe the actual original supervisor completion, including a
            // target whose previous bounded observer was already abandoned.
            loop {
                let completed = original.custody.retry_changed.notified();
                tokio::pin!(completed);
                completed.as_mut().enable();
                if original
                    .custody
                    .retry
                    .with(|retry| retry.attempt == (attempt + 1) as u64 && !retry.running)
                {
                    break;
                }
                completed.await;
            }
            let failure = original
                .await_original_cleanup_decision()
                .await
                .expect_err("actual producer refusal retained");
            let retained = first.get_or_insert_with(|| failure.clone());
            let diagnostic = cause::<OriginalRequiredCleanupDiagnostic>(retained)
                .expect("canonical first diagnostic");
            assert!(std::ptr::eq(
                diagnostic,
                cause::<OriginalRequiredCleanupDiagnostic>(&failure).expect("same diagnostic")
            ));
            assert_eq!(diagnostic.cleanup_failures().count(), attempt + 1);
            let RequiredTaskExit::Failed(execution) = original
                .original_exit_after_cleanup_refusal()
                .expect("original disposition retained")
            else {
                panic!("original native failure cannot become cancellation");
            };
            assert!(std::ptr::eq(
                cause::<std::io::Error>(&execution).expect("actual original native cause"),
                primary.as_ref()
            ));
            if attempt == 1 {
                // The explicit request made after the first abandoned failure
                // completed before this async observer is polled. Observation
                // must consume that SAME target, never enqueue a third attempt.
                if local {
                    original
                        .reconcile_local()
                        .await
                        .expect_err("same completed requested refusal");
                } else {
                    original
                        .reconcile()
                        .await
                        .expect_err("same completed requested refusal");
                }
                assert!(
                    original
                        .custody
                        .retry
                        .with(|retry| { retry.attempt == 2 && !retry.running && !retry.requested }),
                    "fast completion observation cannot request another attempt"
                );
                assert!(original.requested_resident_attempt.is_none());
            } else {
                assert_eq!(
                    original.requested_resident_attempt,
                    Some((attempt + 1) as u64)
                );
            }
            // This is a NEW public retry request after actual failed completion,
            // not a replay of the abandoned observer. It advances even while
            // the prior target remains in the handle, and still uses the same
            // original executor and genuine retained failure inventory.
            original
                .request_resident_cleanup_retry()
                .expect("explicit retry advances completed failed target");
            assert_eq!(
                original.requested_resident_attempt,
                Some((attempt + 2) as u64)
            );
        }
    }
    let disposed = observer.await_disposal().await;
    disposed
        .require_original(&original)
        .expect("same original resource ACK");
    sealed
        .await_disposal()
        .await
        .require_original_group(&group)
        .expect("genuine original group disposal");
    assert!(weak.upgrade().is_none());
    let retained = first.expect("actual first refusal retained");
    let diagnostic = cause::<OriginalRequiredCleanupDiagnostic>(&retained)
        .expect("same retained actual cause owner");
    let failures: Vec<_> = diagnostic.cleanup_failures().collect();
    assert_eq!(failures.len(), 3);
    for (index, failure) in failures.iter().enumerate() {
        assert!(std::ptr::eq(
            cause::<std::io::Error>(
                failure
                    .execution_failure()
                    .expect("original execution cause")
            )
            .expect("actual original native source"),
            primary.as_ref()
        ));
        if panic_first && index == 0 {
            let actual =
                cause::<aura_core::effects::task::RequiredTaskPanic>(failure.cleanup_failure())
                    .expect("actual cleanup unwind payload");
            assert_eq!(
                actual.phase(),
                aura_core::effects::task::RequiredTaskPanicPhase::Cleanup
            );
            assert_eq!(
                actual.with_payload::<Arc<std::io::Error>, _>(|payload| Arc::ptr_eq(
                    payload,
                    &cleanup_causes[index]
                )),
                Some(true)
            );
        } else {
            assert!(std::ptr::eq(
                cause::<std::io::Error>(failure.cleanup_failure())
                    .expect("independent actual cleanup native cause"),
                cleanup_causes[index].as_ref()
            ));
        }
    }
    let repeated = observer.await_disposal().await;
    repeated
        .require_original(&original)
        .expect("same immutable receipt");
    assert_eq!(
        diagnostic.cleanup_failures().count(),
        3,
        "observation creates no producer occurrence"
    );
}
#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn native_resident_cleanup_keeps_pending_future_and_all_actual_causes_after_observer_drop() {
    original_executor_retry(false, false).await;
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test)]
async fn local_resident_cleanup_keeps_rc_future_and_all_actual_causes_after_observer_drop() {
    #[cfg(not(target_arch = "wasm32"))]
    tokio::task::LocalSet::new()
        .run_until(original_executor_retry(true, false))
        .await;
    #[cfg(target_arch = "wasm32")]
    original_executor_retry(true, false).await;
}
#[cfg(not(target_arch = "wasm32"))]
#[tokio::test]
async fn local_resident_cleanup_retains_actual_panic_payload_and_later_native_causes() {
    tokio::task::LocalSet::new()
        .run_until(original_executor_retry(true, true))
        .await;
}
