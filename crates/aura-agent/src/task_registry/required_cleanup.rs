// Private registered task custody. This field belongs
// INSIDE the EXISTING TaskMetadata, not in another registry/map/queue. The
// completion guard retains the same opaque slot. No accessor extracts the owner.
enum OriginalRequiredDomainOwner {
    Native(Box<dyn RequiredTaskCustody>),
    Local(Box<dyn LocalRequiredTaskCustody>),
}

/// The same first refusal remains in every propagated failure observation.
/// Only actual retirement of its original custody may publish the settled
/// disposition. This diagnostic never authorizes disposal or removes a slot.
#[derive(Debug)]
pub(crate) struct OriginalRequiredCleanupDiagnostic {
    execution: RequiredTaskExit,
    refusal: Arc<aura_core::effects::task::RequiredTaskCleanupFailure>,
    later: std::sync::OnceLock<Arc<OriginalRequiredCleanupCause>>,
    settled: std::sync::OnceLock<aura_core::effects::task::RequiredTaskTerminalFailure>,
}
#[derive(Debug)]
struct OriginalRequiredCleanupCause {
    cause: Arc<aura_core::effects::task::RequiredTaskCleanupFailure>,
    next: std::sync::OnceLock<Arc<OriginalRequiredCleanupCause>>,
}
impl OriginalRequiredCleanupDiagnostic {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn original_execution(&self) -> &RequiredTaskExit {
        &self.execution
    }

    /// Actual producer-attempt causes in chronological order. Reobserving a
    /// receipt or requesting a retry does not append an occurrence.
    pub(crate) fn cleanup_failures(
        &self,
    ) -> impl Iterator<Item = &aura_core::effects::task::RequiredTaskCleanupFailure> {
        std::iter::once(self.refusal.as_ref()).chain(
            std::iter::successors(self.later.get().map(Arc::as_ref), |node| {
                node.next.get().map(Arc::as_ref)
            })
            .map(|node| node.cause.as_ref()),
        )
    }
    fn append_actual(&self, cause: Arc<aura_core::effects::task::RequiredTaskCleanupFailure>) {
        let actual = Arc::new(OriginalRequiredCleanupCause {
            cause,
            next: std::sync::OnceLock::new(),
        });
        let mut tail = &self.later;
        loop {
            match tail.set(actual.clone()) {
                Ok(()) => return,
                Err(_) => {
                    tail = &tail
                        .get()
                        .expect("actual cleanup occurrence was deposited")
                        .next;
                }
            }
        }
    }
}
impl std::fmt::Display for OriginalRequiredCleanupDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} ({} actual cleanup failures)",
            self.refusal,
            self.cleanup_failures().count()
        )
    }
}
impl std::error::Error for OriginalRequiredCleanupDiagnostic {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.settled.get() {
            Some(settled) => Some(settled),
            None => Some(self.refusal.as_ref()),
        }
    }
}
impl TaskGroup {
    fn settled_original_domain_outcome(
        &self,
        task_id: u64,
        custody: &Arc<RetainedRequiredTaskCustody>,
        original: TaskOutcome,
        settlement: RequiredTaskSettlement,
    ) -> Result<(String, TaskOutcome), AuraError> {
        {
            let _tree = self.shared.tree.state.lock();
            let mut tasks = self.shared.tasks.lock();
            let entry = tasks.get_mut(&task_id).ok_or_else(|| {
                AuraError::invalid("original domain settlement registration is missing")
            })?;
            if !entry
                .required_custody
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, custody))
            {
                return Err(AuraError::invalid(
                    "original domain settlement custody differs",
                ));
            }
            custody
                .retiring
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Acquire,
                )
                .map_err(|_| AuraError::invalid("original domain retirement already claimed"))?;
            if let RequiredTaskSettlement::ResolvedWithDomainFailure(source) = &settlement {
                entry.domain_terminal_failure.get_or_insert(source.clone());
            }
        }
        let retired = match custody.original.try_lock() {
            Ok(mut owner) => owner.take(),
            Err(_) => {
                custody
                    .retiring
                    .store(false, std::sync::atomic::Ordering::Release);
                return Err(AuraError::invalid(
                    "original domain owner is still borrowed",
                ));
            }
        }
        .ok_or_else(|| AuraError::invalid("original domain owner was already retired"))?;
        // Run the real domain owner's destructor outside the registry and
        // original-owner locks. Its operation/profile leases must be gone
        // before a disposal observer can see the resource ACK.
        drop(retired);
        let _tree = self.shared.tree.state.lock();
        let mut tasks = self.shared.tasks.lock();
        let entry = tasks.get_mut(&task_id).ok_or_else(|| {
            AuraError::invalid("original domain settlement registration was lost")
        })?;
        if !entry
            .required_custody
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, custody))
        {
            return Err(AuraError::invalid(
                "original domain settlement custody changed during retirement",
            ));
        }
        let name = entry.task_name.clone();
        let outcome = match entry.domain_terminal_failure.clone() {
            None => original,
            Some(terminal) => {
                let execution = match original {
                    TaskOutcome::Completed => RequiredTaskExit::Completed,
                    TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source),
                    TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
                    TaskOutcome::Panicked => RequiredTaskExit::Panicked,
                };
                TaskOutcome::Failed(AuraError::Internal {
                    message: "original domain terminal validation failed".into(),
                    source: Some(Arc::new(
                        aura_core::effects::task::RequiredTaskTerminalFailure::new(
                            execution, terminal,
                        ),
                    )),
                })
            }
        };
        *custody.disposed_outcome.lock() = Some(outcome.clone());
        if let Some(diagnostic) = custody.cleanup_diagnostic.get() {
            diagnostic.settled.get_or_init(|| {
                aura_core::effects::task::RequiredTaskTerminalFailure::new(
                    diagnostic.execution.clone(),
                    AuraError::Internal {
                        message:
                            "original required cleanup refusal retained after acknowledged retry"
                                .into(),
                        source: Some(diagnostic.refusal.clone()),
                    },
                )
            });
        }
        // Deposit the refined observation before publishing the resource ACK.
        custody
            .acknowledged
            .store(true, std::sync::atomic::Ordering::Release);
        Ok((name, outcome))
    }
}
/// One coalescing retry request in the existing original required owner. No
/// observer owns the cleanup future or creates another executor/registration.
#[derive(Default)]
struct OriginalRequiredCleanupRetry {
    resident: bool,
    running: bool,
    requested: bool,
    attempt: u64,
    failure: Option<AuraError>,
}
struct OriginalRequiredCleanupResidence<'a> {
    custody: &'a RetainedRequiredTaskCustody,
}
impl Drop for OriginalRequiredCleanupResidence<'_> {
    fn drop(&mut self) {
        self.custody.retry.with(|retry| retry.resident = false);
        self.custody.retry_changed.notify_waiters();
    }
}
struct RetainedRequiredTaskCustody {
    resident_cleanup: bool,
    retry: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell<
        OriginalRequiredCleanupRetry,
    >,
    retry_changed: tokio::sync::Notify,
    cleanup_diagnostic: std::sync::OnceLock<Arc<OriginalRequiredCleanupDiagnostic>>,
    disposed_outcome: parking_lot::Mutex<Option<TaskOutcome>>,
    original: std::sync::Arc<tokio::sync::Mutex<Option<OriginalRequiredDomainOwner>>>,
    retiring: std::sync::atomic::AtomicBool,
    acknowledged: std::sync::atomic::AtomicBool,
}
impl RetainedRequiredTaskCustody {
    fn new(original: Box<dyn RequiredTaskCustody>) -> Self {
        Self {
            resident_cleanup: false,
            retry: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell::new(
                OriginalRequiredCleanupRetry::default(),
            ),
            retry_changed: tokio::sync::Notify::new(),
            cleanup_diagnostic: std::sync::OnceLock::new(),
            original: std::sync::Arc::new(tokio::sync::Mutex::new(Some(
                OriginalRequiredDomainOwner::Native(original),
            ))),
            disposed_outcome: parking_lot::Mutex::new(None),
            retiring: std::sync::atomic::AtomicBool::new(false),
            acknowledged: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn new_local(original: Box<dyn LocalRequiredTaskCustody>) -> Self {
        Self {
            resident_cleanup: false,
            retry: crate::runtime::services::issuer_error_custody::SynchronousCustodyCell::new(
                OriginalRequiredCleanupRetry::default(),
            ),
            retry_changed: tokio::sync::Notify::new(),
            cleanup_diagnostic: std::sync::OnceLock::new(),
            original: Arc::new(tokio::sync::Mutex::new(Some(
                OriginalRequiredDomainOwner::Local(original),
            ))),
            disposed_outcome: parking_lot::Mutex::new(None),
            retiring: std::sync::atomic::AtomicBool::new(false),
            acknowledged: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn execute_local(&self) -> LocalBoxFuture<'static, Result<(), aura_core::AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut obligation = original.lock_owned().await;
            match obligation
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was retired"))?
            {
                OriginalRequiredDomainOwner::Native(owner) => owner.execute().await,
                OriginalRequiredDomainOwner::Local(owner) => owner.execute().await,
            }
        })
    }
    fn cleanup_local(
        &self,
        original_exit: RequiredTaskExit,
    ) -> LocalBoxFuture<'static, Result<RequiredTaskSettlement, aura_core::AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut obligation = original.lock_owned().await;
            match obligation
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was retired"))?
            {
                OriginalRequiredDomainOwner::Native(owner) => owner.cleanup(&original_exit).await,
                OriginalRequiredDomainOwner::Local(owner) => owner.cleanup(&original_exit).await,
            }
        })
    }
    fn execute(&self) -> futures::future::BoxFuture<'static, Result<(), aura_core::AuraError>> {
        let original = std::sync::Arc::clone(&self.original);
        Box::pin(async move {
            // Owned guard and domain object remain separate allocations; the
            // ordinary Rust borrow lasts only as long as this execution future.
            let mut obligation = original.lock_owned().await;
            match obligation
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was retired"))?
            {
                OriginalRequiredDomainOwner::Native(owner) => owner.execute().await,
                OriginalRequiredDomainOwner::Local(_) => Err(aura_core::AuraError::invalid(
                    "local obligation requires its original local executor",
                )),
            }
        })
    }
    fn cleanup(
        &self,
        original_exit: RequiredTaskExit,
    ) -> futures::future::BoxFuture<'static, Result<RequiredTaskSettlement, aura_core::AuraError>>
    {
        let original = std::sync::Arc::clone(&self.original);
        Box::pin(async move {
            let mut obligation = original.lock_owned().await;
            match obligation
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was retired"))?
            {
                OriginalRequiredDomainOwner::Native(owner) => owner.cleanup(&original_exit).await,
                OriginalRequiredDomainOwner::Local(_) => Err(aura_core::AuraError::invalid(
                    "local obligation requires its original local executor",
                )),
            }
        })
    }
}
// Admission must put RetainedRequiredTaskCustody into the original TaskMetadata
// BEFORE polling execute(). Required cleanup Err/outer Drop keeps that exact
// metadata active with concrete failure, so sealed disposal cannot ACK. Only
// cleanup Ok removes the original record under the same existing tree lock.
// A nonClone original registered handle may reconcile cleanup via this field;
// no task-name/raw-id lookup or public Arc/root extraction is available.

#[cfg(test)]
mod borrowed_obligation_retention_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    struct ActualOwnedValue {
        drops: Arc<AtomicUsize>,
    }
    impl Drop for ActualOwnedValue {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for ActualOwnedValue {
        fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), aura_core::AuraError>> {
            Box::pin(std::future::pending())
        }
        fn cleanup<'a>(
            &'a mut self,
            _original_exit: &'a RequiredTaskExit,
        ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>>
        {
            Box::pin(async { Err(aura_core::AuraError::internal("concrete cleanup failure")) })
        }
    }
    #[tokio::test]
    async fn dropped_execution_borrow_preserves_actual_obligation_in_original_slot() {
        let drops = Arc::new(AtomicUsize::new(0));
        let slot = RetainedRequiredTaskCustody::new(Box::new(ActualOwnedValue {
            drops: drops.clone(),
        }));
        let mut execution = slot.execute();
        assert!(futures::poll!(execution.as_mut()).is_pending());
        assert!(slot.original.try_lock().is_err());
        drop(execution);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(slot.original.try_lock().is_ok());
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn cleanup_error_retains_actual_obligation_in_original_slot() {
        let drops = Arc::new(AtomicUsize::new(0));
        let slot = RetainedRequiredTaskCustody::new(Box::new(ActualOwnedValue {
            drops: drops.clone(),
        }));
        assert!(slot.cleanup(RequiredTaskExit::Cancelled).await.is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(slot.original.try_lock().is_ok());
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}

impl std::fmt::Debug for RetainedRequiredTaskCustody {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RetainedRequiredTaskCustody")
            .finish_non_exhaustive()
    }
}
impl RetainedRequiredTaskCustody {
    fn retain_actual_cleanup_failure(
        &self,
        execution: RequiredTaskExit,
        cleanup: AuraError,
    ) -> Arc<OriginalRequiredCleanupDiagnostic> {
        let actual = Arc::new(aura_core::effects::task::RequiredTaskCleanupFailure::new(
            match &execution {
                RequiredTaskExit::Failed(source) => Some(source.clone()),
                _ => None,
            },
            cleanup,
        ));
        let diagnostic = self.cleanup_diagnostic.get_or_init(|| {
            Arc::new(OriginalRequiredCleanupDiagnostic {
                execution,
                refusal: actual.clone(),
                later: std::sync::OnceLock::new(),
                settled: std::sync::OnceLock::new(),
            })
        });
        if !Arc::ptr_eq(&actual, &diagnostic.refusal) {
            diagnostic.append_actual(actual);
        }
        diagnostic.clone()
    }
    fn acknowledged(&self) -> bool {
        self.acknowledged.load(std::sync::atomic::Ordering::Acquire)
    }
}
async fn settle_retained_required_cleanup_attempt<F, C, CFut>(
    completion: &mut TaskCompletionGuard<F>,
    custody: Option<&Arc<RetainedRequiredTaskCustody>>,
    original: TaskOutcome,
    cleanup: C,
) -> TaskOutcome
where
    C: FnOnce(Arc<RetainedRequiredTaskCustody>, RequiredTaskExit) -> CFut,
    CFut: Future<Output = Result<RequiredTaskSettlement, aura_core::AuraError>>,
{
    let Some(custody) = custody else {
        return original;
    };
    completion
        .group
        .retain_original_execution_exit(completion.task_id, &original);
    let identity = OwnedRuntimeTaskIdentity {
        tree: completion.group.shared.tree.clone(),
        group_id: completion.group.shared.group_id,
        task_id: completion.task_id,
    };
    // Destroy actual execution (and every borrowed native frame) before cleanup.
    // The original TaskMetadata already owns the real custody throughout Drop.
    if std::panic::catch_unwind(AssertUnwindSafe(|| drop(completion.future.take()))).is_err() {
        return TaskOutcome::Panicked;
    }
    let exit = match &original {
        TaskOutcome::Completed => RequiredTaskExit::Completed,
        TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source.clone()),
        TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
        TaskOutcome::Panicked => RequiredTaskExit::Panicked,
    };
    let diagnostic_exit = exit.clone();
    let cleanup = RegisteredTaskFuture {
        identity,
        future: Some(Box::pin(cleanup(custody.clone(), exit))),
    };
    match AssertUnwindSafe(cleanup).catch_unwind().await {
        Ok(Ok(settlement)) => {
            // Domain implementation returned only after consuming its ACTUAL
            // terminal receipts. This flag is supervision/resource state only.
            match std::panic::catch_unwind(AssertUnwindSafe(|| {
                completion.group.settled_original_domain_outcome(
                    completion.task_id,
                    custody,
                    original,
                    settlement,
                )
            })) {
                Ok(Ok((_name, outcome))) => outcome,
                Ok(Err(source)) => TaskOutcome::Failed(source),
                Err(_) => {
                    completion
                        .group
                        .retain_original_cleanup_failure(completion.task_id, TaskOutcome::Panicked);
                    TaskOutcome::Panicked
                }
            }
        }
        Ok(Err(cleanup)) => {
            completion.group.retain_original_cleanup_failure(
                completion.task_id,
                TaskOutcome::Failed(cleanup.clone()),
            );
            let diagnostic = custody.retain_actual_cleanup_failure(diagnostic_exit, cleanup);
            TaskOutcome::Failed(aura_core::AuraError::Internal {
                message: "required original cleanup attempt failed".into(),
                source: Some(diagnostic),
            })
        }
        Err(payload) if custody.resident_cleanup => {
            let actual = AuraError::Internal {
                message: "original required cleanup unwound".into(),
                source: Some(Arc::new(aura_core::effects::task::RequiredTaskPanic::new(
                    aura_core::effects::task::RequiredTaskPanicPhase::Cleanup,
                    payload,
                ))),
            };
            let diagnostic = custody.retain_actual_cleanup_failure(diagnostic_exit, actual);
            let source = AuraError::Internal {
                message: "actual original cleanup panic retained".into(),
                source: Some(diagnostic),
            };
            completion.group.retain_original_cleanup_failure(
                completion.task_id,
                TaskOutcome::Failed(source.clone()),
            );
            TaskOutcome::Failed(source)
        }
        Err(_) => {
            completion
                .group
                .retain_original_cleanup_failure(completion.task_id, TaskOutcome::Panicked);
            TaskOutcome::Panicked
        }
    }
}

async fn settle_retained_required_cleanup_with<F, C, CFut>(
    completion: &mut TaskCompletionGuard<F>,
    custody: Option<&Arc<RetainedRequiredTaskCustody>>,
    original: TaskOutcome,
    mut cleanup: C,
) -> TaskOutcome
where
    C: FnMut(Arc<RetainedRequiredTaskCustody>, RequiredTaskExit) -> CFut,
    CFut: Future<Output = Result<RequiredTaskSettlement, AuraError>>,
{
    let Some(owner) = custody.filter(|owner| owner.resident_cleanup) else {
        return settle_retained_required_cleanup_attempt(completion, custody, original, cleanup)
            .await;
    };
    owner.retry.with(|retry| retry.resident = true);
    let _residence = OriginalRequiredCleanupResidence { custody: owner };
    loop {
        owner.retry.with(|retry| {
            retry.running = true;
            retry.requested = false;
        });
        let outcome = settle_retained_required_cleanup_attempt(
            completion,
            custody,
            original.clone(),
            &mut cleanup,
        )
        .await;
        let exhausted = owner.retry.with(|retry| {
            retry.running = false;
            let Some(attempt) = retry.attempt.checked_add(1) else {
                retry.failure = Some(AuraError::internal(
                    "original cleanup attempt generation exhausted",
                ));
                return true;
            };
            retry.attempt = attempt;
            retry.failure = if owner.acknowledged() {
                None
            } else {
                Some(match &outcome {
                    TaskOutcome::Failed(source) => source.clone(),
                    TaskOutcome::Panicked => AuraError::Internal {
                        message: "original resident cleanup panicked".into(),
                        source: Some(Arc::new(TaskSupervisionError::Panicked {
                            group: completion.group.shared.name.clone(),
                            task: completion.task_name.clone(),
                        })),
                    },
                    _ => AuraError::internal("original resident cleanup remains unacknowledged"),
                })
            };
            false
        });
        owner.retry_changed.notify_waiters();
        completion.group.notify_ancestors();
        if exhausted && !owner.acknowledged() {
            // No custody lock crosses this pending/no-ACK disposition.
            futures::future::pending::<()>().await;
        }
        if owner.acknowledged() {
            return outcome;
        }
        // Publish the actual failure, but keep execution_disposed false: the
        // same original supervisor still owns its retained cleanup lifecycle.
        match &outcome {
            TaskOutcome::Failed(source) => {
                completion
                    .group
                    .propagate_failure(TaskSupervisionError::TaskFailed {
                        group: completion.group.shared.name.clone(),
                        task: completion.task_name.clone(),
                        source: source.clone(),
                    });
            }
            TaskOutcome::Panicked => {
                completion
                    .group
                    .propagate_failure(TaskSupervisionError::Panicked {
                        group: completion.group.shared.name.clone(),
                        task: completion.task_name.clone(),
                    });
            }
            _ => {}
        }
        loop {
            let requested = owner.retry_changed.notified();
            tokio::pin!(requested);
            requested.as_mut().enable();
            if owner.retry.with(|retry| retry.requested) {
                break;
            }
            requested.await;
        }
    }
}

async fn settle_retained_required_cleanup<F>(
    completion: &mut TaskCompletionGuard<F>,
    custody: Option<&Arc<RetainedRequiredTaskCustody>>,
    original: TaskOutcome,
) -> TaskOutcome {
    settle_retained_required_cleanup_with(completion, custody, original, |owner, exit| {
        owner.cleanup(exit)
    })
    .await
}
async fn settle_retained_required_cleanup_local<F>(
    completion: &mut TaskCompletionGuard<F>,
    custody: Option<&Arc<RetainedRequiredTaskCustody>>,
    original: TaskOutcome,
) -> TaskOutcome {
    settle_retained_required_cleanup_with(completion, custody, original, |owner, exit| {
        owner.cleanup_local(exit)
    })
    .await
}
// NonClone handle minted only by accepted admission of the actual slot. It
// retains the original task/tree and may reconcile that SAME obligation.
pub(crate) struct OriginalRequiredTaskHandle {
    requested_resident_attempt: Option<u64>,
    group: TaskGroup,
    task_name: String,
    task: OwnedTaskHandle<u64>,
    custody: Arc<RetainedRequiredTaskCustody>,
}
pub(crate) struct RejectedRequiredTaskCustody {
    custody: Arc<RetainedRequiredTaskCustody>,
    source: TaskSupervisionError,
}

/// Preserve the concrete move-only obligation until synchronous admission has
/// either installed the original metadata slot or returned the exact box.
/// The extra Arc is dropped on success; it is never an authority handle.
#[cfg(any(test, not(target_arch = "wasm32")))]
struct TypedAdmissionAdapter<T: RequiredTaskCustody + 'static> {
    original: Arc<tokio::sync::Mutex<Option<Box<T>>>>,
}
#[cfg(target_arch = "wasm32")]
struct TypedLocalAdmissionAdapter<T: LocalRequiredTaskCustody + 'static> {
    original: Arc<tokio::sync::Mutex<Option<Box<T>>>>,
}
#[cfg(target_arch = "wasm32")]
impl<T: LocalRequiredTaskCustody + 'static> LocalRequiredTaskCustody
    for TypedLocalAdmissionAdapter<T>
{
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut owner = original.lock_owned().await;
            owner
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original local required owner was withdrawn"))?
                .execute()
                .await
        })
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut owner = original.lock_owned().await;
            owner
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original local required owner was withdrawn"))?
                .cleanup(exit)
                .await
        })
    }
}
#[cfg(any(test, not(target_arch = "wasm32")))]
impl<T: RequiredTaskCustody + 'static> RequiredTaskCustody for TypedAdmissionAdapter<T> {
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut owner = original.lock_owned().await;
            owner
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was withdrawn"))?
                .execute()
                .await
        })
    }
    fn cleanup<'a>(
        &'a mut self,
        exit: &'a RequiredTaskExit,
    ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut owner = original.lock_owned().await;
            owner
                .as_mut()
                .ok_or_else(|| AuraError::invalid("original required owner was withdrawn"))?
                .cleanup(exit)
                .await
        })
    }
}
impl TaskGroup {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn spawn_original_local_required_custody_typed<
        T: LocalRequiredTaskCustody + 'static,
    >(
        &self,
        name: String,
        obligation: Box<T>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, aura_core::effects::task::RejectedRequiredTask<T>> {
        let original = Arc::new(tokio::sync::Mutex::new(Some(obligation)));
        let adapter = Box::new(TypedLocalAdmissionAdapter {
            original: original.clone(),
        });
        match self.spawn_original_local_required_custody(name, adapter, cancellation) {
            Ok(handle) => Ok(handle),
            Err(rejected) => {
                let rejected = rejected.into_local();
                drop(rejected.obligation);
                let slot = Arc::try_unwrap(original)
                    .unwrap_or_else(|_| panic!("rejected local owner has no other reference"));
                let obligation = slot
                    .into_inner()
                    .expect("rejected local owner remains unpolled");
                Err(aura_core::effects::task::RejectedRequiredTask {
                    obligation,
                    source: rejected.source,
                })
            }
        }
    }
    #[cfg(any(test, not(target_arch = "wasm32")))]
    pub(crate) fn spawn_original_required_custody_typed<T: RequiredTaskCustody + 'static>(
        &self,
        name: String,
        obligation: Box<T>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, aura_core::effects::task::RejectedRequiredTask<T>> {
        let original = Arc::new(tokio::sync::Mutex::new(Some(obligation)));
        let adapter = Box::new(TypedAdmissionAdapter {
            original: original.clone(),
        });
        match self.spawn_original_required_custody(name, adapter, cancellation) {
            Ok(handle) => Ok(handle),
            Err(rejected) => {
                let rejected = rejected.into_native();
                drop(rejected.obligation);
                let slot = Arc::try_unwrap(original)
                    .unwrap_or_else(|_| panic!("rejected required owner has no other reference"));
                let obligation = slot
                    .into_inner()
                    .expect("rejected required owner remains unpolled");
                Err(aura_core::effects::task::RejectedRequiredTask {
                    obligation,
                    source: rejected.source,
                })
            }
        }
    }
    pub(crate) fn spawn_original_local_required_custody(
        &self,
        name: String,
        obligation: Box<dyn LocalRequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        self.spawn_original_local_required_custody_with_cleanup_policy(
            name,
            obligation,
            cancellation,
            false,
        )
    }
    /// Select original-executor cleanup residence before actual admission.
    #[cfg(any(test, target_arch = "wasm32"))]
    pub(crate) fn spawn_original_local_required_custody_with_resident_cleanup(
        &self,
        name: String,
        obligation: Box<dyn LocalRequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        self.spawn_original_local_required_custody_with_cleanup_policy(
            name,
            obligation,
            cancellation,
            true,
        )
    }
    fn spawn_original_local_required_custody_with_cleanup_policy(
        &self,
        name: String,
        obligation: Box<dyn LocalRequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
        resident_cleanup: bool,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        let mut original_custody = RetainedRequiredTaskCustody::new_local(obligation);
        original_custody.resident_cleanup = resident_cleanup;
        let custody = Arc::new(original_custody);
        let original_task_name = name.clone();
        match self.spawn_checked_fallible_boxed_local_with_custody(
            name,
            custody.execute_local(),
            cancellation,
            Some(custody.clone()),
        ) {
            Ok(task) => Ok(OriginalRequiredTaskHandle {
                requested_resident_attempt: None,
                group: self.clone(),
                task_name: original_task_name,
                task,
                custody,
            }),
            Err((_unadmitted_id, source)) => Err(RejectedRequiredTaskCustody { custody, source }),
        }
    }
    pub(crate) fn spawn_original_required_custody(
        &self,
        name: String,
        obligation: Box<dyn RequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        self.spawn_original_required_custody_with_cleanup_policy(
            name,
            obligation,
            cancellation,
            false,
        )
    }
    /// Select original-executor cleanup residence before actual admission.
    #[cfg(any(test, not(target_arch = "wasm32")))]
    pub(crate) fn spawn_original_required_custody_with_resident_cleanup(
        &self,
        name: String,
        obligation: Box<dyn RequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        self.spawn_original_required_custody_with_cleanup_policy(
            name,
            obligation,
            cancellation,
            true,
        )
    }
    fn spawn_original_required_custody_with_cleanup_policy(
        &self,
        name: String,
        obligation: Box<dyn RequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
        resident_cleanup: bool,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        let mut original_custody = RetainedRequiredTaskCustody::new(obligation);
        original_custody.resident_cleanup = resident_cleanup;
        let custody = Arc::new(original_custody);
        let original_task_name = name.clone();
        match self.spawn_checked_fallible_boxed_with_custody(
            name,
            custody.execute(),
            cancellation,
            Some(custody.clone()),
        ) {
            Ok(task) => Ok(OriginalRequiredTaskHandle {
                requested_resident_attempt: None,
                group: self.clone(),
                task_name: original_task_name,
                task,
                custody,
            }),
            Err((_unadmitted_id, source)) => Err(RejectedRequiredTaskCustody { custody, source }),
        }
    }
}
impl OriginalRequiredTaskHandle {
    /// Observe the actual disposed execution when required cleanup refused.
    /// This is supervision evidence only; it cannot authorize a domain ACK.
    pub(crate) fn original_exit_after_cleanup_refusal(
        &self,
    ) -> Result<aura_core::effects::task::RequiredTaskExit, AuraError> {
        let _tree = self.group.shared.tree.state.lock();
        let tasks = self.group.shared.tasks.lock();
        let retained = tasks
            .get(self.task.handle_id())
            .ok_or_else(|| AuraError::invalid("original required task no longer retained"))?;
        if !retained
            .required_custody
            .as_ref()
            .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
            || !(retained.execution_disposed
                || self.custody.retry.with(|retry| {
                    retry.resident && retry.attempt > 0 && !retry.running && retry.failure.is_some()
                }))
            || retained.cleanup_failure.is_none()
        {
            return Err(AuraError::invalid(
                "original required cleanup refusal is not retained",
            ));
        }
        match retained.original_exit.as_ref().ok_or_else(|| {
            AuraError::invalid("original required execution exit was not retained")
        })? {
            TaskOutcome::Completed => Ok(aura_core::effects::task::RequiredTaskExit::Completed),
            TaskOutcome::Failed(source) => Ok(aura_core::effects::task::RequiredTaskExit::Failed(
                source.clone(),
            )),
            TaskOutcome::Cancelled => Ok(aura_core::effects::task::RequiredTaskExit::Cancelled),
            TaskOutcome::Panicked => Ok(aura_core::effects::task::RequiredTaskExit::Panicked),
        }
    }
    /// Await the original slot's actual cleanup ACK or its retained concrete
    /// cleanup failure. Execution disposal alone is not a resource settlement.
    pub(crate) async fn await_original_cleanup_decision(&self) -> Result<(), AuraError> {
        loop {
            let changed = self.group.shared.notify.notified();
            futures::pin_mut!(changed);
            changed.as_mut().enable();
            {
                let _tree = self.group.shared.tree.state.lock();
                let tasks = self.group.shared.tasks.lock();
                if self.custody.acknowledged() {
                    return Ok(());
                }
                let retained = tasks.get(self.task.handle_id()).ok_or_else(|| {
                    AuraError::invalid("original required task no longer retained")
                })?;
                if !retained
                    .required_custody
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
                {
                    return Err(AuraError::invalid("original required task custody differs"));
                }
                if retained.execution_disposed {
                    if let Some(TaskOutcome::Failed(source)) = retained.cleanup_failure.as_ref() {
                        return Err(source.clone());
                    }
                    if matches!(
                        retained.cleanup_failure.as_ref(),
                        Some(TaskOutcome::Cancelled | TaskOutcome::Panicked)
                    ) {
                        return Err(AuraError::invalid(
                            "original required cleanup was interrupted",
                        ));
                    }
                }
            }
            if self.custody.resident_cleanup {
                if let Some(source) = self.custody.retry.with(|retry| retry.failure.clone()) {
                    return Err(source);
                }
            }
            changed.await;
        }
    }
    pub(crate) fn acknowledged(&self) -> bool {
        self.custody.acknowledged()
    }
    /// Observe the actual original attempt's disposal before retrying its same
    /// retained cleanup. The caller supplies its original bounded observation.
    pub(crate) async fn await_original_attempt_disposition(&self) -> Result<(), AuraError> {
        loop {
            let changed = self.group.shared.notify.notified();
            futures::pin_mut!(changed);
            changed.as_mut().enable();
            {
                let _tree = self.group.shared.tree.state.lock();
                let tasks = self.group.shared.tasks.lock();
                if self.custody.acknowledged() {
                    return Ok(());
                }
                let retained = tasks.get(self.task.handle_id()).ok_or_else(|| {
                    AuraError::invalid("original required task no longer retained")
                })?;
                if !retained
                    .required_custody
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
                {
                    return Err(AuraError::invalid("original required task custody differs"));
                }
                if retained.execution_disposed {
                    return Ok(());
                }
            }
            if self.custody.resident_cleanup
                && self
                    .custody
                    .retry
                    .with(|retry| retry.attempt > 0 && !retry.running)
            {
                return Ok(());
            }
            changed.await;
        }
    }

    /// Queue mandatory cleanup on this exact original executor BEFORE clock
    /// observation. One request target remains in the same handle across a
    /// dropped observation; ids and new deadlines cannot recreate it.
    pub(crate) fn request_resident_cleanup_retry(&mut self) -> Result<(), AuraError> {
        if self.custody.acknowledged() {
            return Ok(());
        }
        let prior_request = self.requested_resident_attempt;
        let target = {
            let _tree = self.group.shared.tree.state.lock();
            let tasks = self.group.shared.tasks.lock();
            let entry = tasks.get(self.task.handle_id()).ok_or_else(|| {
                AuraError::invalid("original required cleanup registration is unavailable")
            })?;
            if !self.custody.resident_cleanup
                || !entry
                    .required_custody
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
                || entry.execution_disposed
            {
                return Err(AuraError::permission_denied(
                    "original resident cleanup custody is unavailable",
                ));
            }
            self.custody.retry.with(|retry| {
                if let Some(prior) = prior_request {
                    // Explicit new public requests may advance an actually
                    // completed failed request. A merely abandoned observer
                    // cannot replace a still-pending target.
                    if retry.attempt < prior || retry.failure.is_none() {
                        return Ok(prior);
                    }
                }
                if !retry.resident && retry.attempt > 0 {
                    return Err(AuraError::invalid(
                        "original required cleanup executor is unavailable",
                    ));
                }
                let target = retry.attempt.checked_add(1).ok_or_else(|| {
                    AuraError::invalid("original cleanup attempt generation exhausted")
                })?;
                if !retry.running {
                    retry.requested = true;
                }
                Ok(target)
            })?
        };
        self.requested_resident_attempt = Some(target);
        self.custody.retry_changed.notify_waiters();
        Ok(())
    }
    async fn request_original_resident_cleanup(&mut self) -> Result<(), AuraError> {
        if self.custody.acknowledged() {
            self.requested_resident_attempt = None;
            return Ok(());
        }
        // Reconciliation observes the request already issued by the caller.
        // It must not resubmit after fast completion between those two polls.
        if self.requested_resident_attempt.is_none() {
            self.request_resident_cleanup_retry()?;
        }
        let target = self
            .requested_resident_attempt
            .ok_or_else(|| AuraError::invalid("actual original cleanup request is absent"))?;
        loop {
            let changed = self.custody.retry_changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.custody.acknowledged() {
                self.requested_resident_attempt = None;
                return Ok(());
            }
            let registration_changed = self.group.shared.notify.notified();
            tokio::pin!(registration_changed);
            registration_changed.as_mut().enable();
            let before_initial_attempt = {
                let _tree = self.group.shared.tree.state.lock();
                let tasks = self.group.shared.tasks.lock();
                tasks.get(self.task.handle_id()).is_some_and(|entry| {
                    !entry.execution_disposed
                        && entry
                            .required_custody
                            .as_ref()
                            .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
                })
            };
            if let Some(source) = self.custody.retry.with(|retry| {
                if retry.attempt >= target {
                    return Some(retry.failure.clone().unwrap_or_else(|| {
                        AuraError::invalid("original cleanup lacks actual settlement")
                    }));
                }
                (!(retry.resident || before_initial_attempt && retry.attempt == 0)).then(|| {
                    AuraError::invalid("original required cleanup executor was interrupted")
                })
            }) {
                self.requested_resident_attempt = None;
                return Err(source);
            }
            tokio::select! { _ = changed => {}, _ = registration_changed => {} };
        }
    }

    async fn reconcile_original_cleanup_with<C, CFut>(
        &mut self,
        cleanup: C,
    ) -> Result<(), aura_core::AuraError>
    where
        C: FnOnce(Arc<RetainedRequiredTaskCustody>, RequiredTaskExit) -> CFut,
        CFut: Future<Output = Result<RequiredTaskSettlement, aura_core::AuraError>>,
    {
        if self.custody.resident_cleanup {
            return self.request_original_resident_cleanup().await;
        }
        let original = {
            let _tree = self.group.shared.tree.state.lock();
            let tasks = self.group.shared.tasks.lock();
            let retained = tasks.get(self.task.handle_id()).ok_or_else(|| {
                aura_core::AuraError::invalid("original required task no longer retained")
            })?;
            if !retained
                .required_custody
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
            {
                return Err(aura_core::AuraError::invalid(
                    "original required task custody differs",
                ));
            }
            // Reconciliation is unavailable while actual execution/cleanup is
            // live. Outcome is published only by original completion/Drop.
            if !retained.execution_disposed {
                return Err(aura_core::AuraError::invalid(
                    "original execution remains active",
                ));
            }
            retained.original_exit.clone().ok_or_else(|| {
                aura_core::AuraError::invalid("original required task is still executing")
            })?
        };
        let exit = match &original {
            TaskOutcome::Completed => RequiredTaskExit::Completed,
            TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source.clone()),
            TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
            TaskOutcome::Panicked => RequiredTaskExit::Panicked,
        };
        let identity = OwnedRuntimeTaskIdentity {
            tree: self.group.shared.tree.clone(),
            group_id: self.group.shared.group_id,
            task_id: *self.task.handle_id(),
        };
        let cleanup_result = AssertUnwindSafe(RegisteredTaskFuture {
            identity,
            future: Some(Box::pin(cleanup(self.custody.clone(), exit))),
        })
        .catch_unwind()
        .await;
        let settlement = match cleanup_result {
            Ok(Ok(settlement)) => settlement,
            Ok(Err(source)) => {
                self.group.retain_original_cleanup_failure(
                    *self.task.handle_id(),
                    TaskOutcome::Failed(source.clone()),
                );
                return Err(source);
            }
            Err(_) => {
                self.group
                    .retain_original_cleanup_failure(*self.task.handle_id(), TaskOutcome::Panicked);
                return Err(aura_core::AuraError::Internal {
                    message: "original required cleanup panicked".into(),
                    source: Some(Arc::new(TaskSupervisionError::Panicked {
                        group: self.group.shared.name.clone(),
                        task: "original-required-cleanup".into(),
                    })),
                });
            }
        };
        // No await follows actual cleanup ACK. Original execution disposition
        // remains independent: cancellation cannot become positive completion.
        let (name, outcome) = match std::panic::catch_unwind(AssertUnwindSafe(|| {
            self.group.settled_original_domain_outcome(
                *self.task.handle_id(),
                &self.custody,
                original,
                settlement,
            )
        })) {
            Ok(result) => result?,
            Err(_) => {
                self.group
                    .retain_original_cleanup_failure(*self.task.handle_id(), TaskOutcome::Panicked);
                return Err(aura_core::AuraError::Internal {
                    message: "original required owner retirement panicked".into(),
                    source: Some(Arc::new(TaskSupervisionError::Panicked {
                        group: self.group.shared.name.clone(),
                        task: "original-required-retirement".into(),
                    })),
                });
            }
        };
        self.group
            .complete_task(*self.task.handle_id(), &name, outcome);
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod original_cleanup_disposition_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Debug, thiserror::Error)]
    #[error("original native cleanup refusal")]
    struct NativeCleanupRefusal;

    struct RefusedThenRetiredOwner {
        entered: Option<tokio::sync::oneshot::Sender<()>>,
        attempts: Arc<AtomicUsize>,
        dropped: Arc<AtomicUsize>,
        cause: Arc<NativeCleanupRefusal>,
    }
    impl Drop for RefusedThenRetiredOwner {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for RefusedThenRetiredOwner {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async {
                self.entered
                    .take()
                    .expect("original entry sender")
                    .send(())
                    .expect("original entry observer remains live");
                futures::future::pending().await
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            exit: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                assert!(matches!(exit, RequiredTaskExit::Cancelled));
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(AuraError::Internal {
                        message: "original required cleanup refusal".into(),
                        source: Some(self.cause.clone()),
                    });
                }
                Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                    AuraError::invalid("original cancelled domain"),
                ))
            })
        }
    }

    #[tokio::test]
    async fn acknowledged_retry_retains_first_cleanup_cause_and_original_cancelled_disposition() {
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
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-refused-cleanup");
        let (entered, observed) = tokio::sync::oneshot::channel();
        let attempts = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let native = Arc::new(NativeCleanupRefusal);
        let mut original = group
            .spawn_original_required_custody_typed(
                "original".into(),
                Box::new(RefusedThenRetiredOwner {
                    entered: Some(entered),
                    attempts: Arc::clone(&attempts),
                    dropped: Arc::clone(&dropped),
                    cause: Arc::clone(&native),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual original admission"));
        observed.await.expect("actual original entered execution");
        aura_core::effects::task::RequiredTaskControl::request_cancel(&mut original);
        original
            .await_original_attempt_disposition()
            .await
            .expect("original refusal observed");
        let first = group
            .terminal_failure()
            .expect("first native cleanup failure retained");
        assert!(!original.acknowledged());
        assert_eq!(dropped.load(Ordering::SeqCst), 0);
        assert!(
            cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(&first).is_none(),
            "a failed attempt cannot report resource settlement"
        );
        assert!(std::ptr::eq(
            cause::<NativeCleanupRefusal>(&first).unwrap(),
            native.as_ref()
        ));
        let observed_disposal = original.disposal_observer();
        let observed_refusal = match observed_disposal
            .await_disposal_or_cleanup_failure_for_test()
            .await
        {
            Err(source) => source,
            Ok(_) => panic!("unacknowledged original cleanup cannot report disposal"),
        };
        assert!(std::ptr::eq(
            cause::<NativeCleanupRefusal>(&observed_refusal).unwrap(),
            native.as_ref()
        ));
        let sealed = group.seal_admission();
        let mut disposal = Box::pin(sealed.await_disposal());
        assert!(futures::poll!(disposal.as_mut()).is_pending());
        drop(disposal);
        aura_core::effects::task::RequiredTaskControl::reconcile(&mut original)
            .await
            .expect("same admitted owner acknowledges retry");
        assert!(original.acknowledged());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        observed_disposal
            .await_disposal_or_cleanup_failure_for_test()
            .await
            .expect("actual resource ACK supersedes the retained attempt diagnostic")
            .require_original(&original)
            .unwrap();
        sealed
            .await_disposal()
            .await
            .require_original_group(&group)
            .unwrap();
        // Query the same previously held error, without replacing its first cause.
        let terminal = cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(&first)
            .expect("actual ACK retains original disposition in existing observation");
        assert!(matches!(terminal.execution(), RequiredTaskExit::Cancelled));
        assert!(std::ptr::eq(
            cause::<NativeCleanupRefusal>(&first).unwrap(),
            native.as_ref()
        ));
        let shutdown = supervisor
            .shutdown_with_timeout(std::time::Duration::from_secs(1))
            .await
            .expect_err("strict shutdown retains the original failed domain");
        assert!(matches!(
            cause::<aura_core::effects::task::RequiredTaskTerminalFailure>(&shutdown)
                .unwrap()
                .execution(),
            RequiredTaskExit::Cancelled
        ));
        assert!(std::ptr::eq(
            cause::<NativeCleanupRefusal>(&shutdown).unwrap(),
            native.as_ref()
        ));
    }

    struct OriginalCleanupDropIdentity(OwnedRuntimeTaskIdentity);
    impl Drop for OriginalCleanupDropIdentity {
        fn drop(&mut self) {
            assert_eq!(current_owned_runtime_task(), Some(self.0.clone()));
        }
    }
    struct OriginalObligation {
        attempts: Arc<AtomicUsize>,
        expected_identity: OwnedRuntimeTaskIdentity,
        original_failure: bool,
        panic_cleanup: bool,
    }
    impl RequiredTaskCustody for OriginalObligation {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), aura_core::AuraError>> {
            Box::pin(std::future::pending())
        }
        fn cleanup<'a>(
            &'a mut self,
            exit: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>> {
            Box::pin(async move {
                let _original_drop_scope =
                    OriginalCleanupDropIdentity(self.expected_identity.clone());
                assert_eq!(
                    current_owned_runtime_task(),
                    Some(self.expected_identity.clone())
                );
                if self.original_failure {
                    assert!(
                        matches!(exit, RequiredTaskExit::Failed(aura_core::AuraError::Internal { message, .. }) if message == "original execution fault")
                    );
                } else {
                    assert!(matches!(exit, RequiredTaskExit::Cancelled));
                }
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    if self.panic_cleanup {
                        panic!("original cleanup panic");
                    }
                    return Err(aura_core::AuraError::internal("original cleanup fault"));
                }
                Ok(RequiredTaskSettlement::Resolved)
            })
        }
    }

    async fn original_exit_survives_cleanup_failure(original_failure: bool, panic_cleanup: bool) {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-required-owner");
        let task_id = group.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        let identity = OwnedRuntimeTaskIdentity {
            tree: group.shared.tree.clone(),
            group_id: group.shared.group_id,
            task_id,
        };
        let attempts = Arc::new(AtomicUsize::new(0));
        let custody = Arc::new(RetainedRequiredTaskCustody::new(Box::new(
            OriginalObligation {
                attempts: attempts.clone(),
                expected_identity: identity.clone(),
                original_failure,
                panic_cleanup,
            },
        )));
        let registered = group
            .register_task(task_id, "original-task".into(), Some(custody.clone()))
            .expect("fixture requires the original retained task or proof");
        let (execution, _) = registered.bind(custody.execute());
        let mut completion = TaskCompletionGuard {
            future: Some(Box::pin(execution)),
            group: group.clone(),
            task_id,
            task_name: "original-task".into(),
            completed: false,
        };
        let original = if original_failure {
            TaskOutcome::Failed(aura_core::AuraError::internal("original execution fault"))
        } else {
            TaskOutcome::Cancelled
        };
        let aggregate =
            settle_retained_required_cleanup(&mut completion, Some(&custody), original).await;
        completion.finish(aggregate);
        {
            let tasks = group.shared.tasks.lock();
            let retained = tasks
                .get(&task_id)
                .expect("actual original obligation retained");
            assert!(retained.execution_disposed);
            assert!(retained.cleanup_failure.is_some());
            if original_failure {
                assert!(matches!(
                    retained.original_exit,
                    Some(TaskOutcome::Failed(_))
                ));
            } else {
                assert!(matches!(
                    retained.original_exit,
                    Some(TaskOutcome::Cancelled)
                ));
            }
        }
        let original_exit = group
            .shared
            .tasks
            .lock()
            .get(&task_id)
            .expect("fixture requires the original retained task or proof")
            .original_exit
            .clone()
            .expect("fixture requires the original retained task or proof");
        let exit = match original_exit {
            TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source),
            TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
            _ => panic!("original disposition was replaced"),
        };
        RegisteredTaskFuture {
            identity,
            future: Some(Box::pin(custody.cleanup(exit))),
        }
        .await
        .expect("fixture requires the original retained task or proof");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn cancelled_original_survives_failed_cleanup_and_retry() {
        original_exit_survives_cleanup_failure(false, false).await;
    }
    #[tokio::test]
    async fn original_execution_failure_survives_cleanup_panic_and_retry() {
        original_exit_survives_cleanup_failure(true, true).await;
    }
}

impl aura_core::effects::task::RequiredTaskControl for OriginalRequiredTaskHandle {
    fn request_cancel(&mut self) {
        let _tree = self.group.shared.tree.state.lock();
        let tasks = self.group.shared.tasks.lock();
        if let Some(entry) = tasks.get(self.task.handle_id()) {
            if entry
                .required_custody
                .as_ref()
                .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
            {
                entry.abort.abort();
            }
        }
    }
    fn reconcile(&mut self) -> BoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.reconcile_original_cleanup_with(|owner, exit| owner.cleanup(exit)))
    }
    fn reconcile_local(&mut self) -> LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
        Box::pin(self.reconcile_original_cleanup_with(|owner, exit| owner.cleanup_local(exit)))
    }
}
impl RejectedRequiredTaskCustody {
    pub(crate) fn into_native(
        self,
    ) -> aura_core::effects::task::RejectedRequiredTask<dyn RequiredTaskCustody> {
        let custody = Arc::try_unwrap(self.custody)
            .expect("unadmitted owner has no registered custody clone");
        let owner = Arc::try_unwrap(custody.original)
            .unwrap_or_else(|_| panic!("unadmitted owner has no polled borrow"));
        let Some(OriginalRequiredDomainOwner::Native(obligation)) = owner.into_inner() else {
            unreachable!("native admission retains native owner");
        };
        aura_core::effects::task::RejectedRequiredTask {
            obligation,
            source: aura_core::AuraError::Internal {
                message: "original required task admission refused".into(),
                source: Some(Arc::new(self.source)),
            },
        }
    }
    pub(crate) fn into_local(
        self,
    ) -> aura_core::effects::task::RejectedRequiredTask<dyn LocalRequiredTaskCustody> {
        let custody = Arc::try_unwrap(self.custody)
            .expect("unadmitted owner has no registered custody clone");
        let owner = Arc::try_unwrap(custody.original)
            .unwrap_or_else(|_| panic!("unadmitted owner has no polled borrow"));
        let Some(OriginalRequiredDomainOwner::Local(obligation)) = owner.into_inner() else {
            unreachable!("local admission retains local owner");
        };
        aura_core::effects::task::RejectedRequiredTask {
            obligation,
            source: aura_core::AuraError::Internal {
                message: "original required task admission refused".into(),
                source: Some(Arc::new(self.source)),
            },
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod registered_required_custody_tests {
    use super::*;
    use aura_core::effects::task::RequiredTaskControl;
    use futures::FutureExt;
    use std::sync::atomic::AtomicUsize;

    struct ActualRegisteredObligation {
        started: Option<tokio::sync::oneshot::Sender<()>>,
        identity: Option<OwnedRuntimeTaskIdentity>,
        attempts: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for ActualRegisteredObligation {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for ActualRegisteredObligation {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), aura_core::AuraError>> {
            Box::pin(async move {
                self.identity = current_owned_runtime_task();
                assert!(self.identity.is_some());
                self.started
                    .take()
                    .expect("fixture requires the original retained task or proof")
                    .send(())
                    .expect("fixture requires the original retained task or proof");
                futures::future::pending().await
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            original: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>> {
            Box::pin(async move {
                assert_eq!(current_owned_runtime_task(), self.identity);
                assert!(matches!(original, RequiredTaskExit::Cancelled));
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(aura_core::AuraError::internal(
                        "actual registered cleanup fault",
                    ))
                } else {
                    Ok(RequiredTaskSettlement::Resolved)
                }
            })
        }
    }
    #[tokio::test]
    async fn actual_registered_cancellation_retains_owner_until_original_retry_ack() {
        registered_cancellation(0).await;
    }
    #[tokio::test]
    async fn actual_registered_force_abort_retains_owner_until_original_retry_ack() {
        registered_cancellation(1).await;
    }
    #[tokio::test]
    async fn actual_registered_outer_abort_retains_owner_through_failed_retry() {
        registered_cancellation(2).await;
    }
    #[tokio::test]
    async fn unacknowledged_original_slot_retains_group_after_external_owner_drop() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("retained-original-slot");
        let weak = Arc::downgrade(&group.shared);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let drops = Arc::new(AtomicUsize::new(0));
        let mut original = group
            .spawn_original_required_custody(
                "retained-obligation".into(),
                Box::new(ActualRegisteredObligation {
                    started: Some(started_tx),
                    identity: None,
                    attempts: Arc::new(AtomicUsize::new(0)),
                    drops: drops.clone(),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("original admission"));
        started_rx.await.expect("original execution started");
        original.request_cancel();
        original
            .await_original_attempt_disposition()
            .await
            .expect("failed cleanup retains the original slot");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(original);
        drop(group);
        drop(supervisor);
        let retained = weak
            .upgrade()
            .expect("unacknowledged slot retains its group");
        assert_eq!(retained.tasks.lock().len(), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
    }
    async fn registered_cancellation(mode: u8) {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("actual-required-owner");
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let attempts = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let mut original = group
            .spawn_original_required_custody(
                "original-obligation".into(),
                Box::new(ActualRegisteredObligation {
                    started: Some(started_tx),
                    identity: None,
                    attempts: attempts.clone(),
                    drops: drops.clone(),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("original registration admission"));
        started_rx
            .await
            .expect("fixture requires the original retained task or proof");
        assert!(
            original
                .await_original_attempt_disposition()
                .now_or_never()
                .is_none(),
            "live original execution has no first-attempt disposition"
        );
        if mode == 2 {
            group
                .shared
                .tasks
                .lock()
                .get(original.task.handle_id())
                .expect("fixture requires the original retained task or proof")
                .handle
                .as_ref()
                .expect("fixture requires the original retained task or proof")
                .abort();
        } else if mode == 1 {
            assert!(matches!(
                group.force_abort_remaining(),
                Err(TaskSupervisionError::ForcedAbort { .. })
            ));
        } else {
            original.request_cancel();
        }
        original
            .await_original_attempt_disposition()
            .await
            .expect("actual original execution and first cleanup attempt yielded retained custody");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            if mode == 2 { 0 } else { 1 }
        );
        assert_eq!(group.shared.tasks.lock().len(), 1);
        let seal = group.seal_admission();
        let mut disposal = Box::pin(seal.await_disposal());
        assert!(futures::poll!(disposal.as_mut()).is_pending());
        assert!(!original.custody.acknowledged());
        if mode == 2 {
            assert!(original.reconcile().await.is_err());
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            assert!(futures::poll!(disposal.as_mut()).is_pending());
            assert!(!original.custody.acknowledged());
        }
        original
            .reconcile()
            .await
            .expect("fixture requires the original retained task or proof");
        assert!(group.shared.tasks.lock().is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        let acknowledged = disposal.await;
        acknowledged
            .require_original_group(&group)
            .expect("disposal acknowledges the original registered group");
        drop(acknowledged);
        drop(seal);
        assert!(original.custody.acknowledged());
        drop(original);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &*group.shared.execution_outcome.lock(),
            TaskGroupExecutionOutcome::Interrupted(_)
        ));
        let weak = Arc::downgrade(&group.shared);
        drop(group);
        drop(supervisor);
        assert!(
            weak.upgrade().is_none(),
            "acknowledged slot breaks retention"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod rejected_required_custody_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    struct UnadmittedOwner(Arc<AtomicUsize>);
    impl Drop for UnadmittedOwner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for UnadmittedOwner {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), aura_core::AuraError>> {
            panic!("rejected execution cannot be constructed");
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>> {
            panic!("rejected cleanup cannot be polled");
        }
    }
    #[tokio::test]
    async fn rejected_registration_returns_actual_owner_without_poll_or_drop() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("closed-admission");
        let weak = Arc::downgrade(&group.shared);
        let _sealed = group.seal_admission();
        let drops = Arc::new(AtomicUsize::new(0));
        let rejected = match group.spawn_original_required_custody(
            "unadmitted-original".into(),
            Box::new(UnadmittedOwner(drops.clone())),
            None,
        ) {
            Err(rejected) => rejected.into_native(),
            Ok(_) => panic!("sealed original group must refuse"),
        };
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(group.shared.tasks.lock().is_empty());
        use std::error::Error;
        assert!(matches!(
            rejected
                .source
                .source()
                .and_then(|source| source.downcast_ref::<TaskSupervisionError>()),
            Some(TaskSupervisionError::AdmissionClosed { .. })
        ));
        drop(rejected.obligation);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(_sealed);
        drop(group);
        drop(supervisor);
        assert!(
            weak.upgrade().is_none(),
            "rejection installs no retained slot"
        );
    }

    #[tokio::test]
    async fn typed_rejection_returns_exact_unpolled_original_box() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("typed-closed-admission");
        let seal = group.seal_admission();
        let drops = Arc::new(AtomicUsize::new(0));
        let owner = Box::new(UnadmittedOwner(drops.clone()));
        let identity = (&*owner as *const UnadmittedOwner) as usize;
        let rejected = group
            .spawn_original_required_custody_typed("typed-original".into(), owner, None)
            .err()
            .expect("sealed original group refuses typed registration");
        assert_eq!(
            (&*rejected.obligation as *const UnadmittedOwner) as usize,
            identity
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(group.shared.tasks.lock().is_empty());
        drop(rejected);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(seal);
    }

    #[tokio::test]
    async fn ordinary_completed_task_does_not_retain_its_group() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("ordinary-release");
        let weak = Arc::downgrade(&group.shared);
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        let handle = group.spawn_named("ordinary", async move {
            let _ = finished_tx.send(());
        });
        finished_rx.await.expect("ordinary task ran");
        group
            .wait_for_idle(std::time::Duration::from_secs(1))
            .await
            .expect("ordinary task finished");
        drop(handle);
        drop(group);
        drop(supervisor);
        assert!(
            weak.upgrade().is_none(),
            "ordinary completion leaves no cycle"
        );
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod local_borrowed_required_custody_tests {
    use super::*;
    use aura_core::effects::task::RequiredTaskControl;
    use std::sync::atomic::AtomicUsize;
    struct LocalBorrowedOwner {
        started: Option<tokio::sync::oneshot::Sender<()>>,
        identity: Option<OwnedRuntimeTaskIdentity>,
        attempts: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }
    impl Drop for LocalBorrowedOwner {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl LocalRequiredTaskCustody for LocalBorrowedOwner {
        fn execute(&mut self) -> LocalBoxFuture<'_, Result<(), aura_core::AuraError>> {
            Box::pin(async move {
                let local = std::rc::Rc::new(());
                self.identity = current_owned_runtime_task();
                assert!(self.identity.is_some());
                self.started
                    .take()
                    .expect("original local owner starts only once")
                    .send(())
                    .expect("fixture observes original local execution start");
                let result = futures::future::pending().await;
                drop(local);
                result
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            original: &'a RequiredTaskExit,
        ) -> LocalBoxFuture<'a, Result<RequiredTaskSettlement, aura_core::AuraError>> {
            Box::pin(async move {
                assert_eq!(current_owned_runtime_task(), self.identity);
                assert!(matches!(original, RequiredTaskExit::Cancelled));
                if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(aura_core::AuraError::internal(
                        "original local cleanup fault",
                    ))
                } else {
                    Ok(RequiredTaskSettlement::Resolved)
                }
            })
        }
    }
    #[tokio::test]
    async fn send_retained_owner_supports_local_borrowed_execution_and_original_retry() {
        registered_local_cancellation(0).await;
    }
    #[tokio::test]
    async fn local_force_abort_retains_exact_owner_until_original_retry_ack() {
        registered_local_cancellation(1).await;
    }
    #[tokio::test]
    async fn local_outer_abort_retains_exact_owner_through_failed_retry_and_disposal() {
        registered_local_cancellation(2).await;
    }
    async fn registered_local_cancellation(mode: u8) {
        tokio::task::LocalSet::new()
            .run_until(async {
                let supervisor = TaskSupervisor::new();
                let group = supervisor.group("original-local-owner");
                let (started_tx, started_rx) = tokio::sync::oneshot::channel();
                let attempts = Arc::new(AtomicUsize::new(0));
                let drops = Arc::new(AtomicUsize::new(0));
                let mut original = group
                    .spawn_original_local_required_custody(
                        "local-obligation".into(),
                        Box::new(LocalBorrowedOwner {
                            started: Some(started_tx),
                            identity: None,
                            attempts: attempts.clone(),
                            drops: drops.clone(),
                        }),
                        None,
                    )
                    .unwrap_or_else(|_| panic!("original local admission"));
                started_rx
                    .await
                    .expect("original local execution publishes readiness");
                if mode == 2 {
                    group
                        .shared
                        .tasks
                        .lock()
                        .get(original.task.handle_id())
                        .expect("original local registration remains retained")
                        .handle
                        .as_ref()
                        .expect("original local registration retains its native handle")
                        .abort();
                } else if mode == 1 {
                    assert!(matches!(
                        group.force_abort_remaining(),
                        Err(TaskSupervisionError::ForcedAbort { .. })
                    ));
                } else {
                    original.request_cancel();
                }
                loop {
                    let notified = group.shared.notify.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    if group
                        .shared
                        .tasks
                        .lock()
                        .get(original.task.handle_id())
                        .expect("original local registration remains retained until disposal")
                        .execution_disposed
                    {
                        break;
                    }
                    notified.await;
                }
                assert_eq!(drops.load(Ordering::SeqCst), 0);
                assert_eq!(
                    attempts.load(Ordering::SeqCst),
                    if mode == 2 { 0 } else { 1 }
                );
                assert_eq!(group.shared.tasks.lock().len(), 1);
                let seal = group.seal_admission();
                let mut disposal = Box::pin(seal.await_disposal());
                assert!(futures::poll!(disposal.as_mut()).is_pending());
                assert!(!original.custody.acknowledged());
                if mode == 2 {
                    assert!(original.reconcile_local().await.is_err());
                    assert_eq!(drops.load(Ordering::SeqCst), 0);
                    assert!(futures::poll!(disposal.as_mut()).is_pending());
                    assert!(!original.custody.acknowledged());
                }
                original
                    .reconcile_local()
                    .await
                    .expect("original local retry settles retained cleanup");
                assert_eq!(attempts.load(Ordering::SeqCst), 2);
                assert!(group.shared.tasks.lock().is_empty());
                let acknowledged = disposal.await;
                acknowledged
                    .require_original_group(&group)
                    .expect("local disposal acknowledges the original registered group");
                assert!(original.custody.acknowledged());
                assert_eq!(
                    drops.load(Ordering::SeqCst),
                    1,
                    "local owner is retired before ACK while the handle is still held"
                );
                drop(original);
                assert_eq!(drops.load(Ordering::SeqCst), 1);
                assert!(matches!(
                    &*group.shared.execution_outcome.lock(),
                    TaskGroupExecutionOutcome::Interrupted(_)
                ));
            })
            .await;
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod settled_domain_failure_tests {
    use super::*;
    use aura_core::effects::task::RequiredTaskControl;
    #[derive(Debug, thiserror::Error)]
    #[error("original terminal validation provider fault")]
    struct OriginalValidationFault;
    struct CompletedExecution {
        source: Arc<OriginalValidationFault>,
        execution_source: Arc<OriginalValidationFault>,
        mode: u8,
        started: Option<tokio::sync::oneshot::Sender<()>>,
    }
    impl RequiredTaskCustody for CompletedExecution {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async move {
                self.started
                    .take()
                    .expect("fixture requires the original retained task or proof")
                    .send(())
                    .expect("fixture requires the original retained task or proof");
                match self.mode {
                    0 => Ok(()),
                    1 => Err(AuraError::Internal {
                        message: "original execution failed".into(),
                        source: Some(self.execution_source.clone()),
                    }),
                    2 => futures::future::pending().await,
                    _ => panic!("invalid fixture mode"),
                }
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            original: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                assert!(matches!(
                    (self.mode, original),
                    (0, RequiredTaskExit::Completed)
                        | (1, RequiredTaskExit::Failed(_))
                        | (2, RequiredTaskExit::Cancelled)
                ));
                assert!(current_owned_runtime_task().is_some());
                // This test owner represents an actual resolved negative
                // resource obligation with a separate failed domain outcome.
                Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                    AuraError::Internal {
                        message: "original validation failed".into(),
                        source: Some(self.source.clone()),
                    },
                ))
            })
        }
    }
    #[tokio::test]
    async fn completed_execution_negative_cleanup_resolves_custody_and_retains_domain_failure() {
        settled_domain_failure(0).await;
    }
    #[tokio::test]
    async fn failed_execution_and_terminal_failure_keep_both_actual_sources_after_cleanup() {
        settled_domain_failure(1).await;
    }
    #[tokio::test]
    async fn cancelled_execution_remains_cancelled_after_settled_terminal_failure() {
        settled_domain_failure(2).await;
    }
    async fn settled_domain_failure(mode: u8) {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("resolved-domain-failure");
        let native = Arc::new(OriginalValidationFault);
        let execution_native = Arc::new(OriginalValidationFault);
        let (started, ready) = tokio::sync::oneshot::channel();
        let mut original = group
            .spawn_original_required_custody(
                "completed-execution".into(),
                Box::new(CompletedExecution {
                    source: native.clone(),
                    execution_source: execution_native.clone(),
                    mode,
                    started: Some(started),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("original required admission"));
        ready
            .await
            .expect("fixture requires the original retained task or proof");
        if mode == 2 {
            original.request_cancel();
        }
        let sealed = group.seal_admission();
        let disposal = sealed.await_disposal().await;
        disposal
            .require_original_group(&group)
            .expect("fixture requires the original retained task or proof");
        assert!(original
            .custody
            .acknowledged
            .load(std::sync::atomic::Ordering::Acquire));
        assert!(group.shared.tasks.lock().is_empty());
        let TaskSupervisionError::TaskFailed { source, .. } = disposal
            .failure()
            .expect("fixture requires the original retained task or proof")
        else {
            panic!("resolved resource disposal must preserve failed domain outcome");
        };
        let terminal = std::error::Error::source(source)
            .expect("fixture requires the original retained task or proof")
            .downcast_ref::<aura_core::effects::task::RequiredTaskTerminalFailure>()
            .expect("fixture requires the original retained task or proof");
        match (mode, terminal.execution()) {
            (0, RequiredTaskExit::Completed) | (2, RequiredTaskExit::Cancelled) => {}
            (1, RequiredTaskExit::Failed(source)) => {
                let actual = std::error::Error::source(source)
                    .expect("fixture requires the original retained task or proof")
                    .downcast_ref::<OriginalValidationFault>()
                    .expect("fixture requires the original retained task or proof");
                assert!(std::ptr::eq(actual, execution_native.as_ref()));
            }
            _ => panic!("original execution disposition changed"),
        }
        let actual = std::error::Error::source(terminal.terminal())
            .expect("fixture requires the original retained task or proof")
            .downcast_ref::<OriginalValidationFault>()
            .expect("fixture requires the original retained task or proof");
        assert!(std::ptr::eq(actual, native.as_ref()));
        assert!(matches!(
            disposal.execution_outcome(),
            TaskGroupExecutionOutcome::Interrupted(_)
        ));
    }
}
/// Observation of the exact issued task after execution and required cleanup
/// disposal. This does not authorize any domain terminal decision.
pub(crate) struct OriginalRequiredTaskDisposalObservation {
    original_group: std::sync::Weak<TaskGroupShared>,
    group_name: String,
    task_name: String,
    custody: Arc<RetainedRequiredTaskCustody>,
    outcome: TaskOutcome,
}
impl OriginalRequiredTaskDisposalObservation {
    /// Establish the exact original required task as the only accepted task in
    /// this sealed, actually disposed subtree. This is resource evidence only;
    /// it cannot authorize a signed terminal or change the retained task exit.
    pub(crate) fn require_only_original_task(
        &self,
        disposal: &TaskGroupDisposalAcknowledgment,
    ) -> Result<(), AuraError> {
        let original = self.original_group.upgrade().ok_or_else(|| {
            AuraError::permission_denied("original required task group is no longer retained")
        })?;
        let subtree = &disposal.group.shared;
        let _tree = subtree.tree.state.lock();
        if !Arc::ptr_eq(&original.tree, &subtree.tree)
            || !original.lineage.starts_with(&subtree.lineage)
            || !subtree.closing.load(Ordering::Acquire)
            || subtree.accepted_subtree_tasks.load(Ordering::Relaxed) != 1
            || !self.custody.acknowledged()
        {
            return Err(AuraError::permission_denied(
                "disposal does not contain only the original acknowledged required task",
            ));
        }
        if let Some(source) = disposal.failure() {
            if !self.matches_supervision_failure(source) {
                return Err(AuraError::permission_denied(
                    "foreign failure in original task disposal",
                ));
            }
        }
        if let TaskGroupExecutionOutcome::Interrupted(source) = disposal.execution_outcome() {
            let original_cancel = matches!(
                (source, &self.outcome),
                (TaskSupervisionError::Cancelled {group, task}, TaskOutcome::Cancelled)
                    if group == &self.group_name && task == &self.task_name
            );
            if !original_cancel && !self.matches_supervision_failure(source) {
                return Err(AuraError::permission_denied(
                    "foreign interruption in original task disposal",
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn require_original(
        &self,
        original: &OriginalRequiredTaskHandle,
    ) -> Result<(), AuraError> {
        if Arc::ptr_eq(&self.custody, &original.custody) {
            Ok(())
        } else {
            Err(AuraError::permission_denied(
                "foreign required task disposal observation",
            ))
        }
    }
    /// Observes only the exact acknowledged task and its original native failure.
    #[aura_macros::capability_boundary(category = "capability_gated", capability = "OriginalRequiredTaskDisposalObservation", receiver_type = OriginalRequiredTaskDisposalObservation, family = "runtime_helper")]
    pub(crate) fn matches_supervision_failure(&self, failure: &TaskSupervisionError) -> bool {
        let TaskSupervisionError::TaskFailed {
            group,
            task,
            source,
        } = failure
        else {
            return false;
        };
        if group != &self.group_name
            || task != &self.task_name
            || !self
                .custody
                .acknowledged
                .load(std::sync::atomic::Ordering::Acquire)
        {
            return false;
        }
        let terminal: Option<&(dyn std::error::Error + 'static)> = match &self.outcome {
            TaskOutcome::Failed(actual) => Some(actual),
            _ => None,
        };
        // A resolved same-slot retry retains its original cleanup refusal even
        // when execution completed. Only this custody-issued diagnostic can
        // supply the independent first failure; neither names nor a new owner
        // can substitute an unrelated source.
        let cleanup = self
            .custody
            .cleanup_diagnostic
            .get()
            .map(|actual| actual.as_ref() as &(dyn std::error::Error + 'static));
        for original in [terminal, cleanup].into_iter().flatten() {
            let mut left: Option<&(dyn std::error::Error + 'static)> = Some(original);
            while let Some(actual) = left {
                let mut right: Option<&(dyn std::error::Error + 'static)> = Some(source);
                while let Some(candidate) = right {
                    if std::ptr::eq(actual, candidate) {
                        return true;
                    }
                    right = candidate.source();
                }
                left = actual.source();
            }
        }
        false
    }
    pub(crate) fn outcome(&self) -> RequiredTaskExit {
        match &self.outcome {
            TaskOutcome::Completed => RequiredTaskExit::Completed,
            TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source.clone()),
            TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
            TaskOutcome::Panicked => RequiredTaskExit::Panicked,
        }
    }
}
pub(crate) struct OriginalRequiredTaskDisposalObserver {
    task_name: String,
    group: TaskGroup,
    custody: Arc<RetainedRequiredTaskCustody>,
}
impl OriginalRequiredTaskDisposalObserver {
    #[cfg(test)]
    pub(crate) async fn await_disposal_or_cleanup_failure_for_test(
        &self,
    ) -> Result<OriginalRequiredTaskDisposalObservation, AuraError> {
        tokio::select! {
            biased;
            actual = self.await_disposal() => Ok(actual),
            source = async {
                loop {
                    let changed = self.group.shared.notify.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    if let Some(original) = self.custody.cleanup_diagnostic.get() {
                        return AuraError::Internal {
                            message: "original required cleanup refusal observed".into(),
                            source: Some(original.clone()),
                        };
                    }
                    changed.await;
                }
            } => Err(source),
        }
    }

    pub(crate) async fn await_disposal(&self) -> OriginalRequiredTaskDisposalObservation {
        loop {
            let changed = self.group.shared.notify.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.custody.acknowledged() {
                if let Some(outcome) = self.custody.disposed_outcome.lock().clone() {
                    return OriginalRequiredTaskDisposalObservation {
                        original_group: Arc::downgrade(&self.group.shared),
                        group_name: self.group.shared.name.clone(),
                        task_name: self.task_name.clone(),
                        custody: self.custody.clone(),
                        outcome,
                    };
                }
            }
            changed.await;
        }
    }
}
impl OriginalRequiredTaskHandle {
    pub(crate) fn disposal_observer(&self) -> OriginalRequiredTaskDisposalObserver {
        OriginalRequiredTaskDisposalObserver {
            task_name: self.task_name.clone(),
            group: self.group.clone(),
            custody: self.custody.clone(),
        }
    }
    pub(crate) fn original_registration_observation(
        &self,
    ) -> Result<RegisteredRuntimeTaskObservation, AuraError> {
        let _tree = self.group.shared.tree.state.lock();
        let tasks = self.group.shared.tasks.lock();
        let exact = tasks.get(self.task.handle_id()).is_some_and(|entry| {
            !entry.execution_disposed
                && entry
                    .required_custody
                    .as_ref()
                    .is_some_and(|actual| Arc::ptr_eq(actual, &self.custody))
        });
        if !exact {
            return Err(AuraError::permission_denied(
                "original required registration is not live",
            ));
        }
        Ok(RegisteredRuntimeTaskObservation {
            group: self.group.clone(),
            original: self.task.clone(),
        })
    }
}

#[cfg(test)]
mod issued_required_task_observation_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct RetiredOwnedValue {
        drops: Arc<AtomicUsize>,
        cleanup_attempts: Arc<AtomicUsize>,
    }
    impl Drop for RetiredOwnedValue {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for RetiredOwnedValue {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async { Ok(()) })
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                if self.cleanup_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(AuraError::invalid("original cleanup remains incomplete"))
                } else {
                    Ok(RequiredTaskSettlement::Resolved)
                }
            })
        }
    }
    #[tokio::test]
    async fn original_owner_retires_before_ack_even_with_handle_and_observer_held() {
        use aura_core::effects::task::RequiredTaskControl;
        let drops = Arc::new(AtomicUsize::new(0));
        let cleanup_attempts = Arc::new(AtomicUsize::new(0));
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-owner-retirement");
        let mut original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(RetiredOwnedValue {
                    drops: Arc::clone(&drops),
                    cleanup_attempts: Arc::clone(&cleanup_attempts),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual required admission"));
        let observer = original.disposal_observer();
        original
            .await_original_attempt_disposition()
            .await
            .expect("original failed cleanup attempt became observable");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(!original.acknowledged());
        assert!(observer.await_disposal().now_or_never().is_none());
        original
            .reconcile()
            .await
            .expect("same original cleanup retry ACK");
        assert_eq!(cleanup_attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "real owner retired before ACK"
        );
        let disposed = observer.await_disposal().await;
        disposed
            .require_original(&original)
            .expect("same original slot");
        assert!(matches!(disposed.outcome(), RequiredTaskExit::Completed));
        let disposal = group.seal_admission().await_disposal().await;
        disposed
            .require_only_original_task(&disposal)
            .expect("retry does not admit a second task");
        let TaskSupervisionError::TaskFailed {
            group,
            task,
            source,
        } = disposal
            .failure()
            .expect("first original cleanup refusal stays retained")
        else {
            panic!("original cleanup refusal must remain source-bearing");
        };
        assert!(disposed.matches_supervision_failure(
            disposal.failure().expect("same original cleanup refusal")
        ));
        for (group, task, source) in [
            (format!("foreign-{group}"), task.clone(), source.clone()),
            (group.clone(), format!("foreign-{task}"), source.clone()),
            (
                group.clone(),
                task.clone(),
                AuraError::Internal {
                    message: "original cleanup remains incomplete".into(),
                    source: Some(Arc::new(std::io::Error::other("foreign native refusal"))),
                },
            ),
        ] {
            assert!(
                !disposed.matches_supervision_failure(&TaskSupervisionError::TaskFailed {
                    group,
                    task,
                    source
                }),
                "same-slot retry cannot authorize foreign names or native sources"
            );
        }
        assert!(
            matches!(disposed.outcome(), RequiredTaskExit::Completed),
            "observing retained cleanup refusal cannot rewrite original execution"
        );

        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "held observer cannot retain operation resources"
        );
    }
    #[tokio::test]
    async fn exclusive_original_disposal_rejects_foreign_and_sibling_subtrees() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("exclusive-original");
        let original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(RetiredOwnedValue {
                    drops: Arc::new(AtomicUsize::new(0)),
                    cleanup_attempts: Arc::new(AtomicUsize::new(1)),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual required admission"));
        let observed = original.disposal_observer().await_disposal().await;
        let sibling = supervisor.group("sibling");
        let sibling_disposal = sibling.seal_admission().await_disposal().await;
        assert!(observed
            .require_only_original_task(&sibling_disposal)
            .is_err());
        let foreign = TaskSupervisor::new();
        let foreign_group = foreign.group("exclusive-original");
        let foreign_disposal = foreign_group.seal_admission().await_disposal().await;
        assert!(observed
            .require_only_original_task(&foreign_disposal)
            .is_err());
        let disposal = group.seal_admission().await_disposal().await;
        observed
            .require_only_original_task(&disposal)
            .expect("exact original subtree");
        group.record_subsidiary_failure("unrelated", AuraError::invalid("independent failure"));
        let contaminated = group.seal_admission().await_disposal().await;
        assert!(
            observed.require_only_original_task(&contaminated).is_err(),
            "a foreign failure cannot be waived by sole-task custody"
        );
    }

    #[tokio::test]
    async fn exclusive_original_disposal_remembers_completed_dropped_descendant() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("exclusive-history");
        let earlier = group.group("earlier");
        let weak = Arc::downgrade(&earlier.shared);
        let handle = earlier.spawn_named("completed", async {});
        let earlier_disposal = earlier.seal_admission().await_disposal().await;
        drop(earlier_disposal);
        drop(handle);
        drop(earlier);
        assert!(
            weak.upgrade().is_none(),
            "completed group no longer enumerates"
        );
        let notice = group.group("original-notice");
        let original = notice
            .spawn_original_required_custody(
                "original".into(),
                Box::new(RetiredOwnedValue {
                    drops: Arc::new(AtomicUsize::new(0)),
                    cleanup_attempts: Arc::new(AtomicUsize::new(1)),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual required admission"));
        let observed = original.disposal_observer().await_disposal().await;
        let notice_disposal = notice.seal_admission().await_disposal().await;
        observed
            .require_only_original_task(&notice_disposal)
            .expect("notice subtree alone contains exact original");
        let disposal = group.seal_admission().await_disposal().await;
        assert!(
            observed.require_only_original_task(&disposal).is_err(),
            "dropping a completed descendant cannot erase accepted admission history"
        );
    }

    struct PanickingRetiredOwner;
    impl Drop for PanickingRetiredOwner {
        fn drop(&mut self) {
            panic!("injected original owner destructor failure");
        }
    }
    impl RequiredTaskCustody for PanickingRetiredOwner {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async { Ok(()) })
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async {
                Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                    AuraError::invalid("original domain failure before retirement"),
                ))
            })
        }
    }
    #[tokio::test]
    async fn destructor_panic_retains_original_domain_failure_without_resource_ack() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-owner-panic-retirement");
        let original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(PanickingRetiredOwner),
                None,
            )
            .unwrap_or_else(|_| panic!("actual required admission"));
        original
            .await_original_attempt_disposition()
            .await
            .expect("panicked original retirement remains inspectable");
        let _tree = group.shared.tree.state.lock();
        let tasks = group.shared.tasks.lock();
        let entry = tasks
            .get(original.task.handle_id())
            .expect("unacknowledged original slot remains retained");
        assert!(matches!(
            entry.domain_terminal_failure.as_ref(),
            Some(AuraError::Invalid { message, .. }) if message == "original domain failure before retirement"
        ));
        assert!(matches!(
            entry.cleanup_failure.as_ref(),
            Some(TaskOutcome::Panicked)
        ));
        assert!(matches!(
            group.terminal_failure(),
            Some(TaskSupervisionError::Panicked { task, .. }) if task == "original"
        ));
        assert!(!original.acknowledged());
        assert!(original.custody.disposed_outcome.lock().is_none());
    }
    struct ObservedCustody {
        execution: Option<tokio::sync::oneshot::Receiver<()>>,
        cleanup: Option<tokio::sync::oneshot::Receiver<()>>,
    }
    impl RequiredTaskCustody for ObservedCustody {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
            Box::pin(async move {
                self.execution
                    .as_mut()
                    .expect("original execution gate")
                    .await
                    .expect("release execution");
                Ok(())
            })
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
            Box::pin(async move {
                self.cleanup
                    .as_mut()
                    .expect("original cleanup gate")
                    .await
                    .expect("release cleanup");
                Ok(RequiredTaskSettlement::Resolved)
            })
        }
    }
    #[cfg(feature = "simulation")]
    #[tokio::test]
    async fn failed_disposal_matches_only_original_supervised_task_and_native_source() {
        struct NativeFailure {
            source: AuraError,
        }
        impl RequiredTaskCustody for NativeFailure {
            fn execute(&mut self) -> BoxFuture<'_, Result<(), AuraError>> {
                Box::pin(async { Err(self.source.clone()) })
            }
            fn cleanup<'a>(
                &'a mut self,
                _: &'a RequiredTaskExit,
            ) -> BoxFuture<'a, Result<RequiredTaskSettlement, AuraError>> {
                Box::pin(async {
                    Ok(RequiredTaskSettlement::ResolvedWithDomainFailure(
                        self.source.clone(),
                    ))
                })
            }
        }
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-failed-disposal");
        let native = AuraError::Internal {
            message: "original native fault".into(),
            source: Some(Arc::new(std::io::Error::other("original fault"))),
        };
        let original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(NativeFailure { source: native }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual original admission refused"));
        let disposal = original.disposal_observer().await_disposal().await;
        disposal
            .require_original(&original)
            .expect("actual original disposal");
        let subtree = group.seal_admission().await_disposal().await;
        subtree
            .require_original_group(&group)
            .expect("actual original sealed subtree");
        let failure = supervisor
            .shutdown_gracefully(Duration::from_secs(5))
            .await
            .expect_err("actual settled domain failure");
        assert!(disposal.matches_supervision_failure(&failure));
        assert!(subtree.matches_supervision_failure(&failure));
        let TaskSupervisionError::TaskFailed {
            group,
            task,
            source,
        } = failure
        else {
            panic!("unexpected supervision outcome");
        };
        assert!(
            !subtree.matches_supervision_failure(&TaskSupervisionError::TaskFailed {
                group: format!("foreign-{group}"),
                task: task.clone(),
                source: source.clone()
            })
        );
        assert!(
            !subtree.matches_supervision_failure(&TaskSupervisionError::TaskFailed {
                group: group.clone(),
                task: format!("foreign-{task}"),
                source: source.clone()
            })
        );
        assert!(
            !subtree.matches_supervision_failure(&TaskSupervisionError::TaskFailed {
                group,
                task,
                source: AuraError::Internal {
                    message: "foreign native fault".into(),
                    source: Some(Arc::new(std::io::Error::other("foreign fault")))
                }
            })
        );
        assert!(supervisor.active_tasks().is_empty());
    }
    #[tokio::test]
    async fn staged_disposal_outcome_without_actual_ack_remains_unobservable() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("staged-original-outcome");
        let (run, execution) = tokio::sync::oneshot::channel();
        let (settle, cleanup) = tokio::sync::oneshot::channel();
        let original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(ObservedCustody {
                    execution: Some(execution),
                    cleanup: Some(cleanup),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("actual original custody admission"));
        let observer = original.disposal_observer();
        // This is deliberately incomplete negative evidence. Only the actual
        // registered settlement below may publish the resource acknowledgment.
        *original.custody.disposed_outcome.lock() = Some(TaskOutcome::Completed);
        assert!(!original.custody.acknowledged());
        assert!(
            observer.await_disposal().now_or_never().is_none(),
            "staged outcome cannot precede its actual resource ACK"
        );
        *original.custody.disposed_outcome.lock() = None;
        run.send(()).expect("release actual original execution");
        settle.send(()).expect("release actual original cleanup");
        let disposed = observer.await_disposal().await;
        disposed
            .require_original(&original)
            .expect("actual registered cleanup ACK belongs to original control");
        assert!(matches!(disposed.outcome(), RequiredTaskExit::Completed));
    }

    #[tokio::test]
    async fn issued_observation_waits_for_actual_cleanup_and_survives_dropped_wait() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("original-observed");
        let (run, execution) = tokio::sync::oneshot::channel();
        let (settle, cleanup) = tokio::sync::oneshot::channel();
        let original = group
            .spawn_original_required_custody(
                "original".into(),
                Box::new(ObservedCustody {
                    execution: Some(execution),
                    cleanup: Some(cleanup),
                }),
                None,
            )
            .unwrap_or_else(|_| panic!("original custody admission refused"));
        original
            .original_registration_observation()
            .expect("issued live observation")
            .require_running()
            .expect("same registration live");
        let observer = original.disposal_observer();
        assert!(observer.await_disposal().now_or_never().is_none());
        run.send(()).expect("release original execution");
        assert!(observer.await_disposal().now_or_never().is_none());
        settle.send(()).expect("release original cleanup");
        let disposed = observer.await_disposal().await;
        disposed
            .require_original(&original)
            .expect("exact issued control");
        assert!(matches!(disposed.outcome(), RequiredTaskExit::Completed));
        assert!(original.original_registration_observation().is_err());
        let repeated = observer.await_disposal().await;
        repeated
            .require_original(&original)
            .expect("same immutable observation");
    }
    #[tokio::test]
    async fn issued_disposal_observation_rejects_foreign_same_named_control() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("same-name");
        let mut issued = Vec::new();
        for _ in 0..2 {
            let (run, execution) = tokio::sync::oneshot::channel();
            let (settle, cleanup) = tokio::sync::oneshot::channel();
            let original = group
                .spawn_original_required_custody(
                    "same-name".into(),
                    Box::new(ObservedCustody {
                        execution: Some(execution),
                        cleanup: Some(cleanup),
                    }),
                    None,
                )
                .unwrap_or_else(|_| panic!("exact custody admission refused"));
            run.send(()).expect("release execution");
            settle.send(()).expect("release cleanup");
            issued.push(original);
        }
        let disposed = issued[0].disposal_observer().await_disposal().await;
        assert!(disposed.require_original(&issued[1]).is_err());
        issued[1].disposal_observer().await_disposal().await;
    }
}
