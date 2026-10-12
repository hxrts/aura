//! Runtime-agnostic task spawning traits.

use async_trait::async_trait;
use futures::future::{BoxFuture, LocalBoxFuture};
use std::sync::Arc;

/// Cooperative cancellation token.
#[async_trait]
pub trait CancellationToken: Send + Sync {
    /// Resolves when cancellation is requested.
    async fn cancelled(&self);

    /// Non-blocking cancellation check.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A spawner cannot silently convert required work into a detached unit task.
#[derive(Debug, thiserror::Error)]
pub enum TaskSpawnError {
    /// This adapter has no retained required-task outcome owner.
    #[error("spawner does not support supervised required task {name}")]
    UnsupportedFallible {
        /// Stable name of the rejected required task.
        name: &'static str,
    },
}

/// Original supervision outcome observation, not a domain approval token.
#[derive(Clone, Debug)]
pub enum RequiredTaskExit {
    /// Original execution completed successfully before resource settlement.
    Completed,
    /// Original execution failed with this concrete native cause.
    Failed(crate::AuraError),
    /// Original execution was cancelled before completion.
    Cancelled,
    /// Original execution unwound with a panic.
    Panicked,
}
/// Completion and cleanup results do not construct protocol or durable receipts.
/// Resource settlement is independent from the operation's domain outcome.
/// This observation does not construct a domain receipt or authorize mutation.
pub enum RequiredTaskSettlement {
    /// Actual required resource cleanup completed.
    Resolved,
    /// Cleanup completed while the original operation retains this failure.
    ResolvedWithDomainFailure(crate::AuraError),
}
/// A settled resource obligation retains the independent failed domain outcome.
#[derive(Debug, thiserror::Error)]
#[error("required task domain terminal validation failed after resource settlement")]
pub struct RequiredTaskTerminalFailure {
    execution: RequiredTaskExit,
    #[source]
    terminal: crate::AuraError,
}
impl RequiredTaskTerminalFailure {
    /// Preserve the actual execution disposition and original domain failure.
    pub fn new(execution: RequiredTaskExit, terminal: crate::AuraError) -> Self {
        Self {
            execution,
            terminal,
        }
    }
    /// Original execution disposition, independent from domain validation.
    pub fn execution(&self) -> &RequiredTaskExit {
        &self.execution
    }
    /// Concrete original domain terminal validation failure.
    pub fn terminal(&self) -> &crate::AuraError {
        &self.terminal
    }
}
/// Borrow the actual retained domain obligation without moving it into a future.
pub trait RequiredTaskCustody: Send {
    /// Execute while borrowing the retained original obligation.
    fn execute(&mut self) -> futures::future::BoxFuture<'_, Result<(), crate::AuraError>>;
    /// Settle required resources while retaining the original execution disposition.
    fn cleanup<'a>(
        &'a mut self,
        original_exit: &'a RequiredTaskExit,
    ) -> futures::future::BoxFuture<'a, Result<RequiredTaskSettlement, crate::AuraError>>;
}
/// A retained Send owner whose borrowed operations stay on the local executor.
pub trait LocalRequiredTaskCustody: Send {
    /// Execute on the local executor while borrowing the retained original owner.
    fn execute(&mut self) -> futures::future::LocalBoxFuture<'_, Result<(), crate::AuraError>>;
    /// Settle resources on the local executor under the original execution disposition.
    fn cleanup<'a>(
        &'a mut self,
        original_exit: &'a RequiredTaskExit,
    ) -> futures::future::LocalBoxFuture<'a, Result<RequiredTaskSettlement, crate::AuraError>>;
}

/// Original registered supervision control; it cannot issue a domain acknowledgment.
pub trait RequiredTaskControl: Send {
    /// Cancel this exact original registration, retaining required cleanup custody.
    fn request_cancel(&mut self);
    /// Reconcile the same native obligation after failed disposal.
    fn reconcile(&mut self) -> BoxFuture<'_, Result<(), crate::AuraError>>;
    /// Reconcile a local borrowed obligation on its original executor.
    fn reconcile_local(&mut self) -> LocalBoxFuture<'_, Result<(), crate::AuraError>>;
}
/// Rejected admission returns the actual unadmitted obligation and original cause.
pub struct RejectedRequiredTask<C: ?Sized> {
    /// Actual original owner, not a reconstructed identifier.
    pub obligation: Box<C>,
    /// Concrete configured-provider admission failure.
    pub source: crate::AuraError,
}
impl<C: ?Sized> std::fmt::Debug for RejectedRequiredTask<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RejectedRequiredTask")
            .field("source", &self.source)
            .finish_non_exhaustive()
    }
}
/// Execution failure and failed required disposal retain their independent native causes.
#[derive(Debug, thiserror::Error)]
#[error("required task cleanup failed after execution")]
pub struct RequiredTaskCleanupFailure {
    execution: Option<crate::AuraError>,
    #[source]
    cleanup: crate::AuraError,
}
impl RequiredTaskCleanupFailure {
    /// Retain the independent original execution and required cleanup causes.
    pub fn new(execution: Option<crate::AuraError>, cleanup: crate::AuraError) -> Self {
        Self { execution, cleanup }
    }
    /// Observe the original execution failure, if execution failed before disposal.
    pub fn execution_failure(&self) -> Option<&crate::AuraError> {
        self.execution.as_ref()
    }
    /// Observe the concrete failure that prevented required disposal acknowledgment.
    pub fn cleanup_failure(&self) -> &crate::AuraError {
        &self.cleanup
    }
}

/// Task spawning contract for runtime implementations.
pub trait TaskSpawner: Send + Sync {
    /// Spawn a background task.
    fn spawn(&self, fut: BoxFuture<'static, ()>);

    /// Spawn a background task tied to a cancellation token.
    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>);

    /// Spawn a background task that may remain thread-local.
    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>);

    /// Spawn a thread-local background task tied to a cancellation token.
    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    );

    /// Admit required work whose actual failure must remain supervised.
    /// Unsupported adapters drop the supplied future and return a typed error.
    fn spawn_fallible_cancellable(
        &self,
        name: &'static str,
        fut: BoxFuture<'static, Result<(), crate::AuraError>>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<(), crate::AuraError> {
        drop(fut);
        Err(crate::AuraError::Internal {
            message: "required task supervision is unavailable".into(),
            source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
        })
    }

    /// Admit thread-local required work with retained failure supervision.
    fn spawn_local_fallible_cancellable(
        &self,
        name: &'static str,
        fut: LocalBoxFuture<'static, Result<(), crate::AuraError>>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<(), crate::AuraError> {
        drop(fut);
        Err(crate::AuraError::Internal {
            message: "required local task supervision is unavailable".into(),
            source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
        })
    }

    /// Admit the actual retained native owner. Rejection must return that owner.
    fn admit_required_custody(
        &self,
        name: &'static str,
        obligation: Box<dyn RequiredTaskCustody>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<Box<dyn RequiredTaskControl>, RejectedRequiredTask<dyn RequiredTaskCustody>> {
        Err(RejectedRequiredTask {
            obligation,
            source: crate::AuraError::Internal {
                message: "required retained custody is unavailable".into(),
                source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
            },
        })
    }
    /// Admit the actual Send owner while its borrowed futures remain local.
    fn admit_local_required_custody(
        &self,
        name: &'static str,
        obligation: Box<dyn LocalRequiredTaskCustody>,
        _token: Arc<dyn CancellationToken>,
    ) -> Result<Box<dyn RequiredTaskControl>, RejectedRequiredTask<dyn LocalRequiredTaskCustody>>
    {
        Err(RejectedRequiredTask {
            obligation,
            source: crate::AuraError::Internal {
                message: "required retained local custody is unavailable".into(),
                source: Some(Arc::new(TaskSpawnError::UnsupportedFallible { name })),
            },
        })
    }

    /// Return a cancellation token associated with this spawner.
    fn cancellation_token(&self) -> Arc<dyn CancellationToken>;
}

/// Cancellation token that never triggers.
pub struct NeverCancel;

#[async_trait]
impl CancellationToken for NeverCancel {
    async fn cancelled(&self) {
        futures::future::pending::<()>().await;
    }
}

#[cfg(test)]
mod required_task_contract_tests {
    use super::*;
    use std::error::Error;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct UnitOnlySpawner;
    impl TaskSpawner for UnitOnlySpawner {
        fn spawn(&self, future: BoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_cancellable(&self, future: BoxFuture<'static, ()>, _: Arc<dyn CancellationToken>) {
            drop(future);
        }
        fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_local_cancellable(
            &self,
            future: LocalBoxFuture<'static, ()>,
            _: Arc<dyn CancellationToken>,
        ) {
            drop(future);
        }
        fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
            Arc::new(NeverCancel)
        }
    }
    struct DropEvidence(Arc<AtomicBool>);
    impl Drop for DropEvidence {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    #[test]
    fn unsupported_required_adapter_drops_future_and_retains_structural_source() {
        let dropped = Arc::new(AtomicBool::new(false));
        let evidence = DropEvidence(dropped.clone());
        let future = Box::pin(async move {
            drop(evidence);
            Ok(())
        });
        let source = UnitOnlySpawner
            .spawn_fallible_cancellable("required", future, Arc::new(NeverCancel))
            .expect_err("unit-only adapter cannot claim required supervision");
        assert!(dropped.load(Ordering::Acquire));
        assert!(matches!(
            source
                .source()
                .and_then(|source| source.downcast_ref::<TaskSpawnError>()),
            Some(TaskSpawnError::UnsupportedFallible { name: "required" })
        ));
    }
}

#[cfg(test)]
mod rejected_custody_forwarding_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct UnitAdapter;
    impl TaskSpawner for UnitAdapter {
        fn spawn(&self, future: BoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_cancellable(&self, future: BoxFuture<'static, ()>, _: Arc<dyn CancellationToken>) {
            drop(future);
        }
        fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
            drop(future);
        }
        fn spawn_local_cancellable(
            &self,
            future: LocalBoxFuture<'static, ()>,
            _: Arc<dyn CancellationToken>,
        ) {
            drop(future);
        }
        fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
            Arc::new(NeverCancel)
        }
    }
    struct OriginalOwner(Arc<AtomicUsize>);
    impl Drop for OriginalOwner {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl RequiredTaskCustody for OriginalOwner {
        fn execute(&mut self) -> BoxFuture<'_, Result<(), crate::AuraError>> {
            panic!("unsupported provider cannot poll execution");
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> BoxFuture<'a, Result<RequiredTaskSettlement, crate::AuraError>> {
            panic!("unsupported provider cannot poll cleanup");
        }
    }
    #[test]
    fn configured_spawner_rejection_forwards_actual_owner_without_drop() {
        let drops = Arc::new(AtomicUsize::new(0));
        let spawner = crate::OwnedTaskSpawner::new(
            Arc::new(UnitAdapter),
            crate::OwnedShutdownToken::detached(),
        );
        let rejected = spawner
            .spawn_required_custody("original", Box::new(OriginalOwner(drops.clone())))
            .unwrap_err();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        use std::error::Error;
        assert!(matches!(
            rejected
                .source
                .source()
                .and_then(|source| source.downcast_ref::<TaskSpawnError>()),
            Some(TaskSpawnError::UnsupportedFallible { name: "original" })
        ));
        drop(rejected.obligation);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    impl LocalRequiredTaskCustody for OriginalOwner {
        fn execute(&mut self) -> LocalBoxFuture<'_, Result<(), crate::AuraError>> {
            panic!("unsupported local provider cannot poll execution");
        }
        fn cleanup<'a>(
            &'a mut self,
            _: &'a RequiredTaskExit,
        ) -> LocalBoxFuture<'a, Result<RequiredTaskSettlement, crate::AuraError>> {
            panic!("unsupported local provider cannot poll cleanup");
        }
    }
    #[test]
    fn configured_local_spawner_rejection_preserves_exact_owner_and_structural_source() {
        let drops = Arc::new(AtomicUsize::new(0));
        let owner = Box::new(OriginalOwner(drops.clone()));
        let original = (&*owner as *const OriginalOwner).cast::<()>();
        let spawner = crate::OwnedTaskSpawner::new(
            Arc::new(UnitAdapter),
            crate::OwnedShutdownToken::detached(),
        );
        let rejected = spawner
            .spawn_local_required_custody("original-local", owner)
            .unwrap_err();
        assert_eq!(
            (&*rejected.obligation as *const dyn LocalRequiredTaskCustody).cast::<()>(),
            original
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        use std::error::Error;
        assert!(matches!(
            rejected
                .source
                .source()
                .and_then(|source| source.downcast_ref::<TaskSpawnError>()),
            Some(TaskSpawnError::UnsupportedFallible {
                name: "original-local"
            })
        ));
        drop(rejected.obligation);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[derive(Debug, thiserror::Error)]
    #[error("original configured provider refused admission")]
    struct OriginalProviderFailure;
    struct RefusingAdapter(Arc<OriginalProviderFailure>);
    impl TaskSpawner for RefusingAdapter {
        fn spawn(&self, future: BoxFuture<'static, ()>) {
            UnitAdapter.spawn(future);
        }
        fn spawn_cancellable(
            &self,
            future: BoxFuture<'static, ()>,
            token: Arc<dyn CancellationToken>,
        ) {
            UnitAdapter.spawn_cancellable(future, token);
        }
        fn spawn_local(&self, future: LocalBoxFuture<'static, ()>) {
            UnitAdapter.spawn_local(future);
        }
        fn spawn_local_cancellable(
            &self,
            future: LocalBoxFuture<'static, ()>,
            token: Arc<dyn CancellationToken>,
        ) {
            UnitAdapter.spawn_local_cancellable(future, token);
        }
        fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
            Arc::new(NeverCancel)
        }
        fn admit_required_custody(
            &self,
            _: &'static str,
            obligation: Box<dyn RequiredTaskCustody>,
            _: Arc<dyn CancellationToken>,
        ) -> Result<Box<dyn RequiredTaskControl>, RejectedRequiredTask<dyn RequiredTaskCustody>>
        {
            Err(RejectedRequiredTask {
                obligation,
                source: crate::AuraError::Internal {
                    message: "configured original admission refusal".into(),
                    source: Some(self.0.clone()),
                },
            })
        }
    }
    #[test]
    fn configured_native_refusal_forwards_exact_owner_and_original_provider_source() {
        let drops = Arc::new(AtomicUsize::new(0));
        let owner = Box::new(OriginalOwner(drops.clone()));
        let original = (&*owner as *const OriginalOwner).cast::<()>();
        let source = Arc::new(OriginalProviderFailure);
        let spawner = crate::OwnedTaskSpawner::new(
            Arc::new(RefusingAdapter(source.clone())),
            crate::OwnedShutdownToken::detached(),
        );
        let rejected = spawner
            .spawn_required_custody("original-native", owner)
            .unwrap_err();
        assert_eq!(
            (&*rejected.obligation as *const dyn RequiredTaskCustody).cast::<()>(),
            original
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        use std::error::Error;
        let actual = rejected
            .source
            .source()
            .unwrap()
            .downcast_ref::<OriginalProviderFailure>()
            .unwrap();
        assert!(std::ptr::eq(actual, source.as_ref()));
        drop(rejected.obligation);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
