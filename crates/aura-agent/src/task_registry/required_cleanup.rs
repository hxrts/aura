// Private registered task custody. This field belongs
// INSIDE the EXISTING TaskMetadata, not in another registry/map/queue. The
// completion guard retains the same opaque slot. No accessor extracts the owner.
enum OriginalRequiredDomainOwner {
    Native(Box<dyn RequiredTaskCustody>),
    Local(Box<dyn LocalRequiredTaskCustody>),
}
impl TaskGroup {
    fn settled_original_domain_outcome(
        &self,
        task_id: u64,
        custody: &Arc<RetainedRequiredTaskCustody>,
        original: TaskOutcome,
        settlement: RequiredTaskSettlement,
    ) -> Result<(String, TaskOutcome), AuraError> {
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
        if let RequiredTaskSettlement::ResolvedWithDomainFailure(source) = settlement {
            entry.domain_terminal_failure.get_or_insert(source);
        }
        let name = entry.task_name.clone();
        let Some(terminal) = entry.domain_terminal_failure.clone() else {
            custody
                .acknowledged
                .store(true, std::sync::atomic::Ordering::Release);
            return Ok((name, original));
        };
        let execution = match original {
            TaskOutcome::Completed => RequiredTaskExit::Completed,
            TaskOutcome::Failed(source) => RequiredTaskExit::Failed(source),
            TaskOutcome::Cancelled => RequiredTaskExit::Cancelled,
            TaskOutcome::Panicked => RequiredTaskExit::Panicked,
        };
        let outcome = TaskOutcome::Failed(AuraError::Internal {
            message: "original domain terminal validation failed".into(),
            source: Some(Arc::new(
                aura_core::effects::task::RequiredTaskTerminalFailure::new(execution, terminal),
            )),
        });
        // Original failure/name custody is committed under the SAME registry
        // lock before resource ACK becomes visible. No await follows ACK.
        custody
            .acknowledged
            .store(true, std::sync::atomic::Ordering::Release);
        Ok((name, outcome))
    }
}
struct RetainedRequiredTaskCustody {
    original: std::sync::Arc<tokio::sync::Mutex<OriginalRequiredDomainOwner>>,
    acknowledged: std::sync::atomic::AtomicBool,
}
impl RetainedRequiredTaskCustody {
    fn new(original: Box<dyn RequiredTaskCustody>) -> Self {
        Self {
            original: std::sync::Arc::new(tokio::sync::Mutex::new(
                OriginalRequiredDomainOwner::Native(original),
            )),
            acknowledged: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn new_local(original: Box<dyn LocalRequiredTaskCustody>) -> Self {
        Self {
            original: Arc::new(tokio::sync::Mutex::new(OriginalRequiredDomainOwner::Local(
                original,
            ))),
            acknowledged: std::sync::atomic::AtomicBool::new(false),
        }
    }
    fn execute_local(&self) -> LocalBoxFuture<'static, Result<(), aura_core::AuraError>> {
        let original = self.original.clone();
        Box::pin(async move {
            let mut obligation = original.lock_owned().await;
            match &mut *obligation {
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
            match &mut *obligation {
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
            match &mut *obligation {
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
            match &mut *obligation {
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
    fn acknowledged(&self) -> bool {
        self.acknowledged.load(std::sync::atomic::Ordering::Acquire)
    }
}
async fn settle_retained_required_cleanup_with<F, C, CFut>(
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
    let cleanup = RegisteredTaskFuture {
        identity,
        future: Some(Box::pin(cleanup(custody.clone(), exit))),
    };
    match AssertUnwindSafe(cleanup).catch_unwind().await {
        Ok(Ok(settlement)) => {
            // Domain implementation returned only after consuming its ACTUAL
            // terminal receipts. This flag is supervision/resource state only.
            match completion.group.settled_original_domain_outcome(
                completion.task_id,
                custody,
                original,
                settlement,
            ) {
                Ok((_name, outcome)) => outcome,
                Err(source) => TaskOutcome::Failed(source),
            }
        }
        Ok(Err(cleanup)) => {
            completion.group.retain_original_cleanup_failure(
                completion.task_id,
                TaskOutcome::Failed(cleanup.clone()),
            );
            TaskOutcome::Failed(aura_core::AuraError::Internal {
                message: "required original cleanup remains incomplete".into(),
                source: Some(Arc::new(
                    aura_core::effects::task::RequiredTaskCleanupFailure::new(
                        match original {
                            TaskOutcome::Failed(source) => Some(source),
                            _ => None,
                        },
                        cleanup,
                    ),
                )),
            })
        }
        Err(_) => {
            completion
                .group
                .retain_original_cleanup_failure(completion.task_id, TaskOutcome::Panicked);
            TaskOutcome::Panicked
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
    group: TaskGroup,
    task: OwnedTaskHandle<u64>,
    custody: Arc<RetainedRequiredTaskCustody>,
}
pub(crate) struct RejectedRequiredTaskCustody {
    custody: Arc<RetainedRequiredTaskCustody>,
    source: TaskSupervisionError,
}
impl TaskGroup {
    pub(crate) fn spawn_original_local_required_custody(
        &self,
        name: String,
        obligation: Box<dyn LocalRequiredTaskCustody>,
        cancellation: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OriginalRequiredTaskHandle, RejectedRequiredTaskCustody> {
        let custody = Arc::new(RetainedRequiredTaskCustody::new_local(obligation));
        match self.spawn_checked_fallible_boxed_local_with_custody(
            name,
            custody.execute_local(),
            cancellation,
            Some(custody.clone()),
        ) {
            Ok(task) => Ok(OriginalRequiredTaskHandle {
                group: self.clone(),
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
        let custody = Arc::new(RetainedRequiredTaskCustody::new(obligation));
        match self.spawn_checked_fallible_boxed_with_custody(
            name,
            custody.execute(),
            cancellation,
            Some(custody.clone()),
        ) {
            Ok(task) => Ok(OriginalRequiredTaskHandle {
                group: self.clone(),
                task,
                custody,
            }),
            Err((_unadmitted_id, source)) => Err(RejectedRequiredTaskCustody { custody, source }),
        }
    }
}
impl OriginalRequiredTaskHandle {
    async fn reconcile_original_cleanup_with<C, CFut>(
        &mut self,
        cleanup: C,
    ) -> Result<(), aura_core::AuraError>
    where
        C: FnOnce(Arc<RetainedRequiredTaskCustody>, RequiredTaskExit) -> CFut,
        CFut: Future<Output = Result<RequiredTaskSettlement, aura_core::AuraError>>,
    {
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
        let (name, outcome) = self.group.settled_original_domain_outcome(
            *self.task.handle_id(),
            &self.custody,
            original,
            settlement,
        )?;
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
        let OriginalRequiredDomainOwner::Native(obligation) = owner.into_inner() else {
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
        let OriginalRequiredDomainOwner::Local(obligation) = owner.into_inner() else {
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
        loop {
            let notified = group.shared.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let disposed = group
                .shared
                .tasks
                .lock()
                .get(original.task.handle_id())
                .expect("fixture requires the original retained task or proof")
                .execution_disposed;
            if disposed {
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
        assert!(original.custody.acknowledged());
        drop(original);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(matches!(
            &*group.shared.execution_outcome.lock(),
            TaskGroupExecutionOutcome::Interrupted(_)
        ));
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
