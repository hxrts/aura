//! Structured task supervision for agent background work.
//!
//! This module provides a root supervisor plus named task groups. Tasks are
//! owned by a group, inherit cancellation from their ancestors, and must exit
//! before the group is considered drained.

#![allow(clippy::disallowed_types)]

use std::collections::BTreeMap;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::runtime::{
    RuntimeDiagnostic, RuntimeDiagnosticKind, RuntimeDiagnosticSeverity, RuntimeDiagnosticSink,
};
use aura_core::effects::task::{CancellationToken, TaskSpawner};
use aura_core::effects::PhysicalTimeEffects;
use aura_core::{
    execute_with_timeout_budget, OwnedShutdownToken, OwnedTaskHandle, TimeoutBudget,
    TimeoutBudgetError, TimeoutRunError,
};
use aura_effects::time::PhysicalTimeHandler;
use futures::future::{Abortable, BoxFuture, LocalBoxFuture};
use futures::FutureExt;
#[cfg(not(target_arch = "wasm32"))]
use parking_lot::Mutex;
#[cfg(target_arch = "wasm32")]
use parking_lot::Mutex;
use tokio::sync::watch;
use tokio::sync::Notify;
#[cfg(not(target_arch = "wasm32"))]
use tokio::task::JoinHandle;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_futures::spawn_local;

const DEFAULT_TASK_NAME: &str = "task.default";

fn checked_interval_ms(interval: Duration) -> Result<u64, aura_core::AuraError> {
    let milliseconds = u64::try_from(interval.as_millis()).map_err(|_| {
        TimeoutBudgetError::invalid_policy("supervised interval exceeds millisecond range")
    })?;
    if milliseconds == 0 {
        return Err(TimeoutBudgetError::invalid_policy(
            "supervised interval must be at least one millisecond",
        )
        .into());
    }
    Ok(milliseconds)
}

fn required_interval_sleep_error(error: aura_core::effects::TimeError) -> aura_core::AuraError {
    aura_core::AuraError::Internal {
        message: "supervised interval sleep failed".into(),
        source: Some(Arc::new(error)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAdmissionKind {
    Groups,
    Tasks,
    Depth,
}

#[derive(Debug, Clone)]
pub enum TaskSupervisionError {
    AdmissionClosed {
        group: String,
        task: String,
    },
    AdmissionLimit {
        group: String,
        kind: TaskAdmissionKind,
        limit: usize,
    },
    TaskFailed {
        group: String,
        task: String,
        source: aura_core::AuraError,
    },
    Budget {
        group: String,
        source: Box<TimeoutBudgetError>,
    },
    Timeout {
        group: String,
        active_tasks: Vec<String>,
        source: Box<TimeoutBudgetError>,
    },
    ForcedAbort {
        group: String,
        aborted_tasks: Vec<String>,
        cause: Option<Box<TaskSupervisionError>>,
    },
    Cancelled {
        group: String,
        task: String,
    },
    Panicked {
        group: String,
        task: String,
    },
}

impl std::fmt::Display for TaskSupervisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AdmissionClosed { group, task } => {
                write!(f, "task admission closed for '{task}' in group '{group}'")
            }
            Self::AdmissionLimit { group, kind, limit } => write!(
                f,
                "task admission {kind:?} limit {limit} exceeded in '{group}'"
            ),
            Self::TaskFailed {
                group,
                task,
                source,
            } => {
                write!(f, "task '{task}' in group '{group}' failed: {source}")
            }
            Self::Budget { group, source } => {
                write!(f, "task group '{group}' budget failed: {source}")
            }
            Self::Timeout {
                group,
                active_tasks,
                ..
            } => write!(
                f,
                "task group '{group}' timed out waiting for tasks: {}",
                active_tasks.join(", ")
            ),
            Self::ForcedAbort {
                group,
                aborted_tasks,
                ..
            } => write!(
                f,
                "task group '{group}' force-aborted tasks: {}",
                aborted_tasks.join(", ")
            ),
            Self::Cancelled { group, task } => {
                write!(f, "task '{task}' in group '{group}' was cancelled")
            }
            Self::Panicked { group, task } => {
                write!(f, "task '{task}' in group '{group}' panicked")
            }
        }
    }
}

impl std::error::Error for TaskSupervisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Budget { source, .. } | Self::Timeout { source, .. } => Some(source.as_ref()),
            Self::ForcedAbort { cause, .. } => cause
                .as_deref()
                .map(|cause| cause as &(dyn std::error::Error + 'static)),
            Self::TaskFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
enum TaskOutcome {
    Completed,
    Failed(aura_core::AuraError),
    Cancelled,
    Panicked,
}

#[derive(Debug)]
struct TaskMetadata {
    abort: futures::future::AbortHandle,
    task_name: String,
    #[cfg(not(target_arch = "wasm32"))]
    handle: Option<JoinHandle<()>>,
}

const MAX_SUPERVISED_GROUPS: usize = 1024;
const MAX_SUPERVISED_TASKS: usize = 4096;
const MAX_GROUP_DEPTH: usize = 64;

struct TaskTreeState {
    next_group_id: u64,
    groups: BTreeMap<u64, std::sync::Weak<TaskGroupShared>>,
    active_tasks: usize,
}
struct TaskTreeShared {
    state: Mutex<TaskTreeState>,
}

/// Observed identity of an actual registered task. Private fields and registry-
/// private poll scopes prevent ids, wire values or caller snapshots from
/// establishing this execution context. It is independent of executor task ids.
#[derive(Clone)]
pub(crate) struct OwnedRuntimeTaskIdentity {
    tree: Arc<TaskTreeShared>,
    group_id: u64,
    task_id: u64,
}
impl std::fmt::Debug for OwnedRuntimeTaskIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedRuntimeTaskIdentity")
            .field("group_id", &self.group_id)
            .field("task_id", &self.task_id)
            .finish_non_exhaustive()
    }
}
impl PartialEq for OwnedRuntimeTaskIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.tree, &other.tree)
            && self.group_id == other.group_id
            && self.task_id == other.task_id
    }
}
impl Eq for OwnedRuntimeTaskIdentity {}
impl std::hash::Hash for OwnedRuntimeTaskIdentity {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.tree).hash(state);
        self.group_id.hash(state);
        self.task_id.hash(state);
    }
}

std::thread_local! {
    static CURRENT_REGISTERED_TASK: std::cell::RefCell<Option<OwnedRuntimeTaskIdentity>> = const {
        std::cell::RefCell::new(None)
    };
}

/// Observation only: this cannot enter a scope or admit a task.
pub(crate) fn current_owned_runtime_task() -> Option<OwnedRuntimeTaskIdentity> {
    CURRENT_REGISTERED_TASK.with(|context| context.borrow().clone())
}

/// Retain a destructor failure in the actual registered task's existing health
/// owner. This neither admits a task nor changes its completion outcome.
pub(crate) fn retain_current_task_cleanup_failure(source: aura_core::AuraError) -> bool {
    let Some(identity) = current_owned_runtime_task() else {
        return false;
    };
    let shared = identity
        .tree
        .state
        .lock()
        .groups
        .get(&identity.group_id)
        .and_then(std::sync::Weak::upgrade);
    let Some(shared) = shared else {
        return false;
    };
    let task = shared
        .tasks
        .lock()
        .get(&identity.task_id)
        .map(|metadata| metadata.task_name.clone());
    let Some(task) = task else {
        return false;
    };
    let group = TaskGroup { shared };
    group.propagate_failure(TaskSupervisionError::TaskFailed {
        group: group.shared.name.clone(),
        task,
        source,
    });
    true
}

/// Lexical, synchronous scope for exactly one future poll or destructor. The
/// previous caller scope is restored before control returns to the executor,
/// including nested polling, Pending, panics and cancellation.
struct RegisteredTaskPollScope {
    previous: Option<OwnedRuntimeTaskIdentity>,
}
impl RegisteredTaskPollScope {
    fn enter(identity: &OwnedRuntimeTaskIdentity) -> Self {
        Self {
            previous: CURRENT_REGISTERED_TASK
                .with(|context| context.replace(Some(identity.clone()))),
        }
    }
}
impl Drop for RegisteredTaskPollScope {
    fn drop(&mut self) {
        CURRENT_REGISTERED_TASK.with(|context| context.replace(self.previous.take()));
    }
}

struct RegisteredTaskCapability {
    identity: OwnedRuntimeTaskIdentity,
    abort_registration: futures::future::AbortRegistration,
}
impl RegisteredTaskCapability {
    fn bind<F: Future + Unpin>(
        self,
        future: F,
    ) -> (RegisteredTaskFuture<F>, futures::future::AbortRegistration) {
        (
            RegisteredTaskFuture {
                identity: self.identity,
                future: Some(future),
            },
            self.abort_registration,
        )
    }
}
struct RegisteredTaskFuture<F> {
    identity: OwnedRuntimeTaskIdentity,
    future: Option<F>,
}
impl<F: Future + Unpin> Future for RegisteredTaskFuture<F> {
    type Output = F::Output;
    fn poll(
        self: std::pin::Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        let _scope = RegisteredTaskPollScope::enter(&this.identity);
        std::pin::Pin::new(
            this.future
                .as_mut()
                .expect("registered future remains owned until drop"),
        )
        .poll(context)
    }
}
impl<F> Drop for RegisteredTaskFuture<F> {
    fn drop(&mut self) {
        let _scope = RegisteredTaskPollScope::enter(&self.identity);
        drop(self.future.take());
    }
}

/// Held inside the spawned future before its first poll. Native abort or local
/// future drop cannot leave a false active entry, nor publish idle before drop.
struct TaskCompletionGuard<F> {
    future: Option<F>,
    group: TaskGroup,
    task_id: u64,
    task_name: String,
    completed: bool,
}
impl<F> TaskCompletionGuard<F> {
    fn finish(mut self, outcome: TaskOutcome) {
        let dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(self.future.take())));
        let outcome = if dropped.is_err() {
            TaskOutcome::Panicked
        } else {
            outcome
        };
        emit_task_completion(
            self.group.shared.diagnostics.as_ref(),
            &self.group.shared.name,
            &self.task_name,
            self.task_id,
            &outcome,
        );
        self.group
            .complete_task(self.task_id, &self.task_name, outcome);
        self.completed = true;
    }
}
impl<F> Drop for TaskCompletionGuard<F> {
    fn drop(&mut self) {
        if !self.completed {
            let dropped = std::panic::catch_unwind(AssertUnwindSafe(|| drop(self.future.take())));
            let outcome = if dropped.is_err() {
                TaskOutcome::Panicked
            } else {
                TaskOutcome::Cancelled
            };
            self.group
                .complete_task(self.task_id, &self.task_name, outcome);
        }
    }
}

struct TaskGroupShared {
    tree: Arc<TaskTreeShared>,
    group_id: u64,
    lineage: Vec<u64>,
    parent: Option<Arc<TaskGroupShared>>,
    closing: AtomicBool,
    name: String,
    next_task_id: AtomicU64,
    shutdown_tx: watch::Sender<bool>,
    inherited_cancellation: Option<Arc<dyn CancellationToken>>,
    diagnostics: Option<Arc<RuntimeDiagnosticSink>>,
    tasks: Mutex<BTreeMap<u64, TaskMetadata>>,
    first_failure: Mutex<Option<TaskSupervisionError>>,
    notify: Arc<Notify>,
}

#[derive(Clone)]
pub struct TaskGroup {
    shared: Arc<TaskGroupShared>,
}

#[derive(Clone)]
pub struct TaskSupervisor {
    _owner: Arc<TaskSupervisorOwner>,
    root: TaskGroup,
}

impl TaskSupervisor {
    fn with_root(root: TaskGroup) -> Self {
        Self {
            _owner: Arc::new(TaskSupervisorOwner { root: root.clone() }),
            root,
        }
    }
    pub fn new() -> Self {
        Self::with_root(TaskGroup::root("runtime", None))
    }
    pub fn with_diagnostics(diagnostics: Arc<RuntimeDiagnosticSink>) -> Self {
        Self::with_root(TaskGroup::root("runtime", Some(diagnostics)))
    }

    /// Retained first native failure, including required descendant tasks.
    pub fn terminal_failure(&self) -> Option<TaskSupervisionError> {
        self.root.terminal_failure()
    }

    pub fn group(&self, name: impl Into<String>) -> TaskGroup {
        self.root.group(name)
    }

    /// Required one-shot work retains its native failure in health and drain.
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_try_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = Result<(), aura_core::AuraError>> + Send + 'static,
    {
        self.root
            .spawn_fallible_boxed(name.into(), Box::pin(fut), None)
    }
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_try_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = Result<(), aura_core::AuraError>> + 'static,
    {
        self.root
            .spawn_fallible_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.root.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.root.spawn_cancellable_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.root.spawn_local_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.root.spawn_local_cancellable_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.root
            .spawn_interval_until_named(name, time_effects, interval, f)
    }

    /// Required callbacks retain their concrete failure in supervised health and drain.
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_try_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<bool, aura_core::AuraError>> + Send + 'static,
    {
        self.root
            .spawn_try_interval_until_named(name, time_effects, interval, f)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + 'static,
        Fut: Future<Output = bool> + 'static,
    {
        self.root
            .spawn_local_interval_until_named(name, time_effects, interval, f)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_child<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_periodic<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.spawn_interval_until_named(name, time_effects, interval, f)
    }

    /// Physical tree and original internal lineage establish this root scope.
    /// Names and equal-valued task identifiers do not prove task ownership.
    pub(crate) fn owns_group(&self, group: &TaskGroup) -> bool {
        Arc::ptr_eq(&self.root.shared.tree, &group.shared.tree)
            && group.shared.lineage.starts_with(&self.root.shared.lineage)
    }

    pub fn request_cancellation(&self) {
        self.root.request_cancellation();
    }

    pub async fn wait_for_idle(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.root.wait_for_idle(timeout).await
    }

    pub fn force_abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.root.force_abort_remaining()
    }

    pub fn abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.force_abort_remaining()
    }

    pub async fn shutdown_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), TaskSupervisionError> {
        self.root.shutdown_with_timeout(timeout).await
    }

    /// Internal continuation of an already-owned original resource window.
    /// No new timeout or clock origin is allocated at this handoff.
    pub(crate) async fn shutdown_with_original_budget<T: PhysicalTimeEffects>(
        &self,
        time: &T,
        original: &TimeoutBudget,
    ) -> Result<(), TaskSupervisionError> {
        self.root
            .shutdown_with_original_budget(time, original)
            .await
    }

    pub async fn shutdown_gracefully(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.shutdown_with_timeout(timeout).await
    }

    pub fn shutdown(&self) {
        self.root.shutdown();
    }

    pub fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        self.root.cancellation_token()
    }

    pub fn active_tasks(&self) -> Vec<String> {
        self.root.active_tasks()
    }
}

impl Default for TaskSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// Last owner drops exactly once through Arc, including simultaneous clone drops.
struct TaskSupervisorOwner {
    root: TaskGroup,
}
impl Drop for TaskSupervisorOwner {
    fn drop(&mut self) {
        self.root.shutdown();
    }
}

impl TaskGroup {
    fn root(name: impl Into<String>, diagnostics: Option<Arc<RuntimeDiagnosticSink>>) -> Self {
        let tree = Arc::new(TaskTreeShared {
            state: Mutex::new(TaskTreeState {
                next_group_id: 2,
                groups: BTreeMap::new(),
                active_tasks: 0,
            }),
        });
        let (shutdown_tx, _shutdown_rx) = watch::channel(false);
        let group = Self {
            shared: Arc::new(TaskGroupShared {
                name: name.into(),
                next_task_id: AtomicU64::new(1),
                shutdown_tx,
                inherited_cancellation: None,
                diagnostics,
                tasks: Mutex::new(BTreeMap::new()),
                first_failure: Mutex::new(None),
                notify: Arc::new(Notify::new()),
                tree: tree.clone(),
                group_id: 1,
                lineage: vec![1],
                parent: None,
                closing: AtomicBool::new(false),
            }),
        };
        tree.state
            .lock()
            .groups
            .insert(1, Arc::downgrade(&group.shared));
        group
    }

    pub fn name(&self) -> &str {
        &self.shared.name
    }

    /// Retained first typed failure for the owning service's health observation.
    pub fn terminal_failure(&self) -> Option<TaskSupervisionError> {
        self.shared.first_failure.lock().clone()
    }

    /// Retain a required subsidiary failure after the primary operation has
    /// already published its terminal result. This changes owned health and
    /// drain, never the primary result, and admits no additional task.
    pub(crate) fn record_subsidiary_failure(
        &self,
        operation: impl Into<String>,
        source: aura_core::AuraError,
    ) {
        self.propagate_failure(TaskSupervisionError::TaskFailed {
            group: self.shared.name.clone(),
            task: operation.into(),
            source,
        });
    }

    pub fn group(&self, name: impl Into<String>) -> TaskGroup {
        if self.shared.group_id == 0 {
            return self.clone();
        }
        let full_name = format!("{}.{}", self.shared.name, name.into());
        let mut tree = self.shared.tree.state.lock();
        tree.groups.retain(|_, group| group.strong_count() != 0);
        let rejection = if self.shared.closing.load(Ordering::Acquire)
            || self.cancellation_token().is_cancelled()
        {
            Some(TaskSupervisionError::AdmissionClosed {
                group: self.shared.name.clone(),
                task: full_name.clone(),
            })
        } else if tree.groups.len() >= MAX_SUPERVISED_GROUPS {
            Some(TaskSupervisionError::AdmissionLimit {
                group: self.shared.name.clone(),
                kind: TaskAdmissionKind::Groups,
                limit: MAX_SUPERVISED_GROUPS,
            })
        } else if self.shared.lineage.len() >= MAX_GROUP_DEPTH {
            Some(TaskSupervisionError::AdmissionLimit {
                group: self.shared.name.clone(),
                kind: TaskAdmissionKind::Depth,
                limit: MAX_GROUP_DEPTH,
            })
        } else {
            None
        };
        let group_id = if rejection.is_none() {
            let id = tree.next_group_id;
            tree.next_group_id += 1;
            id
        } else {
            0
        };
        let mut lineage = self.shared.lineage.clone();
        lineage.push(group_id);
        let (shutdown_tx, _shutdown_rx) = watch::channel(rejection.is_some());
        let child = TaskGroup {
            shared: Arc::new(TaskGroupShared {
                name: full_name,
                next_task_id: AtomicU64::new(1),
                shutdown_tx,
                inherited_cancellation: Some(self.cancellation_token()),
                diagnostics: self.shared.diagnostics.clone(),
                tasks: Mutex::new(BTreeMap::new()),
                first_failure: Mutex::new(rejection.clone()),
                notify: Arc::new(Notify::new()),
                tree: self.shared.tree.clone(),
                group_id,
                lineage,
                parent: Some(self.shared.clone()),
                closing: AtomicBool::new(rejection.is_some()),
            }),
        };
        if rejection.is_none() {
            tree.groups.insert(group_id, Arc::downgrade(&child.shared));
        }
        drop(tree);
        if let Some(error) = rejection {
            self.propagate_failure(error);
        }
        child
    }

    fn subtree_groups_locked(&self, tree: &mut TaskTreeState) -> Vec<Arc<TaskGroupShared>> {
        tree.groups.retain(|_, group| group.strong_count() != 0);
        let mut groups: Vec<_> = tree
            .groups
            .values()
            .filter_map(std::sync::Weak::upgrade)
            .filter(|group| group.lineage.starts_with(&self.shared.lineage))
            .collect();
        if self.shared.group_id == 0 {
            groups.push(self.shared.clone());
        }
        groups
    }

    fn propagate_failure(&self, failure: TaskSupervisionError) {
        let mut current = Some(self.shared.clone());
        while let Some(group) = current {
            group
                .first_failure
                .lock()
                .get_or_insert_with(|| failure.clone());
            group.notify.notify_waiters();
            current = group.parent.clone();
        }
    }

    fn notify_ancestors(&self) {
        let mut current = Some(self.shared.clone());
        while let Some(group) = current {
            group.notify.notify_waiters();
            current = group.parent.clone();
        }
    }

    fn rejected_task_handle(&self, task_id: u64) -> OwnedTaskHandle<u64> {
        let (_sender, shutdown_rx) = watch::channel(true);
        let token: Arc<dyn CancellationToken> = Arc::new(TaskGroupCancellationToken {
            shutdown_rx,
            inherited: None,
        });
        OwnedTaskHandle::new(task_id, OwnedShutdownToken::attached(token))
    }

    /// Required one-shot work retains its native failure in health and drain.
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_try_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = Result<(), aura_core::AuraError>> + Send + 'static,
    {
        self.spawn_fallible_boxed(name.into(), Box::pin(fut), None)
    }
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_try_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = Result<(), aura_core::AuraError>> + 'static,
    {
        self.spawn_fallible_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_named<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_cancellable_named<F>(
        &self,
        name: impl Into<String>,
        fut: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + 'static,
    {
        self.spawn_boxed_local(name.into(), Box::pin(fut), None)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_with_token<F>(
        &self,
        name: impl Into<String>,
        fut: F,
        token: Arc<dyn CancellationToken>,
    ) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_boxed(name.into(), Box::pin(fut), Some(token))
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_child<F>(&self, name: impl Into<String>, fut: F) -> OwnedTaskHandle<u64>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.spawn_named(name, fut)
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.spawn_try_interval_until_named(name, time_effects, interval, move || {
            let step = f();
            async move { Ok(step.await) }
        })
    }

    /// `Ok(false)` is intentional completion; `Err` is a retained service failure.
    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_try_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = Result<bool, aura_core::AuraError>> + Send + 'static,
    {
        self.spawn_fallible_boxed(
            name.into(),
            Box::pin(async move {
                let interval_ms = checked_interval_ms(interval)?;
                loop {
                    if !f().await? {
                        return Ok(());
                    }

                    time_effects
                        .sleep_ms(interval_ms)
                        .await
                        .map_err(required_interval_sleep_error)?;
                }
            }),
            None,
        )
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + 'static,
        Fut: Future<Output = bool> + 'static,
    {
        self.spawn_local_try_interval_until_named(name, time_effects, interval, move || {
            let step = f();
            async move { Ok(step.await) }
        })
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_local_try_interval_until_named<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        mut f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + 'static,
        Fut: Future<Output = Result<bool, aura_core::AuraError>> + 'static,
    {
        self.spawn_fallible_boxed_local(
            name.into(),
            Box::pin(async move {
                let interval_ms = checked_interval_ms(interval)?;
                loop {
                    if !f().await? {
                        return Ok(());
                    }

                    time_effects
                        .sleep_ms(interval_ms)
                        .await
                        .map_err(required_interval_sleep_error)?;
                }
            }),
            None,
        )
    }

    #[must_use = "retain or explicitly discard the owned task handle"]
    pub fn spawn_periodic<F, Fut>(
        &self,
        name: impl Into<String>,
        time_effects: Arc<dyn PhysicalTimeEffects + Send + Sync>,
        interval: Duration,
        f: F,
    ) -> OwnedTaskHandle<u64>
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.spawn_interval_until_named(name, time_effects, interval, f)
    }

    pub fn request_cancellation(&self) {
        let mut tree = self.shared.tree.state.lock();
        for group in self.subtree_groups_locked(&mut tree) {
            group.closing.store(true, Ordering::Release);
            group.shutdown_tx.send_replace(true);
            group.notify.notify_waiters();
        }
        drop(tree);
        self.notify_ancestors();
    }

    pub async fn wait_for_idle(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        let time = PhysicalTimeHandler::new();
        self.wait_for_idle_with_time(timeout, &time).await
    }

    /// Observe actual owned completion without allocating another time window.
    /// Runtime protocol owners bound this readiness future with their original
    /// window. Cancellation completion follows destruction of the owned callback.
    pub(crate) async fn await_owned_task_completion(&self) -> Result<(), TaskSupervisionError> {
        loop {
            let changed = self.shared.notify.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if self.active_tasks().is_empty() {
                return self.terminal_failure().map_or(Ok(()), Err);
            }
            changed.await;
        }
    }

    async fn wait_for_idle_with_time<T: PhysicalTimeEffects>(
        &self,
        timeout: Duration,
        time: &T,
    ) -> Result<(), TaskSupervisionError> {
        let group_name = self.shared.name.clone();
        let started_at =
            time.physical_time()
                .await
                .map_err(|error| TaskSupervisionError::Budget {
                    group: group_name.clone(),
                    source: Box::new(TimeoutBudgetError::time_source_failure(error)),
                })?;
        let budget =
            TimeoutBudget::from_start_and_timeout(&started_at, timeout).map_err(|source| {
                TaskSupervisionError::Budget {
                    group: group_name.clone(),
                    source: Box::new(source),
                }
            })?;
        let result = execute_with_timeout_budget(time, &budget, || async {
            loop {
                let changed = self.shared.notify.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if self.active_tasks().is_empty() {
                    return self.terminal_failure().map_or(Ok(()), Err);
                }
                changed.await;
            }
        })
        .await;

        match result {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Timeout(source @ TimeoutBudgetError::DeadlineExceeded { .. })) => {
                Err(TaskSupervisionError::Timeout {
                    group: group_name,
                    active_tasks: self.active_tasks(),
                    source: Box::new(source),
                })
            }
            Err(TimeoutRunError::Timeout(source)) => Err(TaskSupervisionError::Budget {
                group: group_name,
                source: Box::new(source),
            }),
            Err(TimeoutRunError::Operation(error)) => Err(error),
        }
    }

    /// Request descendant abort without publishing false drain completion.
    /// Registered entries remain until the owned future has actually dropped.
    pub fn force_abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.request_cancellation();
        let mut tree = self.shared.tree.state.lock();
        let mut aborted_tasks = Vec::new();
        for group in self.subtree_groups_locked(&mut tree) {
            for entry in group.tasks.lock().values() {
                entry.abort.abort();
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(handle) = &entry.handle {
                    handle.abort();
                }
                aborted_tasks.push(format!("{}::{}", group.name, entry.task_name));
                emit_task_diagnostic(
                    group.diagnostics.as_ref(),
                    RuntimeDiagnosticSeverity::Warn,
                    "task_supervisor",
                    format!(
                        "force-aborted supervised task '{}' in group '{}'",
                        entry.task_name, group.name
                    ),
                );
            }
        }
        drop(tree);
        self.notify_ancestors();
        if aborted_tasks.is_empty() {
            Ok(())
        } else {
            Err(TaskSupervisionError::ForcedAbort {
                group: self.shared.name.clone(),
                aborted_tasks,
                cause: None,
            })
        }
    }

    pub fn abort_remaining(&self) -> Result<(), TaskSupervisionError> {
        self.force_abort_remaining()
    }

    /// Called only by the runtime that retains the original bounded operation.
    /// Completion observes actual callback destruction, including descendants.
    pub(crate) async fn wait_with_original_budget<T: PhysicalTimeEffects>(
        &self,
        time: &T,
        original: &TimeoutBudget,
    ) -> Result<(), TaskSupervisionError> {
        let group = self.shared.name.clone();
        match execute_with_timeout_budget(time, original, || self.await_owned_task_completion())
            .await
        {
            Ok(()) => Ok(()),
            Err(TimeoutRunError::Operation(source)) => Err(source),
            Err(TimeoutRunError::Timeout(source)) => {
                let failure = match source {
                    source @ TimeoutBudgetError::DeadlineExceeded { .. } => {
                        TaskSupervisionError::Timeout {
                            group,
                            active_tasks: self.active_tasks(),
                            source: Box::new(source),
                        }
                    }
                    source => TaskSupervisionError::Budget {
                        group,
                        source: Box::new(source),
                    },
                };
                // Abort requests never certify destruction or make handoff succeed.
                match self.force_abort_remaining() {
                    Ok(()) => Err(failure),
                    Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        ..
                    }) => Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        cause: Some(Box::new(failure)),
                    }),
                    Err(source) => Err(source),
                }
            }
        }
    }

    pub(crate) async fn shutdown_with_original_budget<T: PhysicalTimeEffects>(
        &self,
        time: &T,
        original: &TimeoutBudget,
    ) -> Result<(), TaskSupervisionError> {
        self.request_cancellation();
        self.wait_with_original_budget(time, original).await
    }

    pub async fn shutdown_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(), TaskSupervisionError> {
        self.request_cancellation();
        match self.wait_for_idle(timeout).await {
            Ok(()) => Ok(()),
            Err(timeout @ TaskSupervisionError::Timeout { .. }) => {
                match self.force_abort_remaining() {
                    Ok(()) => Err(timeout),
                    Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        ..
                    }) => Err(TaskSupervisionError::ForcedAbort {
                        group,
                        aborted_tasks,
                        cause: Some(Box::new(timeout)),
                    }),
                    Err(abort) => Err(abort),
                }
            }
            Err(other) => Err(other),
        }
    }

    pub async fn shutdown_gracefully(&self, timeout: Duration) -> Result<(), TaskSupervisionError> {
        self.shutdown_with_timeout(timeout).await
    }

    pub fn shutdown(&self) {
        self.request_cancellation();
        let _ = self.force_abort_remaining();
    }

    pub fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        Arc::new(TaskGroupCancellationToken {
            shutdown_rx: self.shared.shutdown_tx.subscribe(),
            inherited: self.shared.inherited_cancellation.clone(),
        })
    }

    pub fn active_tasks(&self) -> Vec<String> {
        let mut tree = self.shared.tree.state.lock();
        let mut result = Vec::new();
        for group in self.subtree_groups_locked(&mut tree) {
            for task in group.tasks.lock().values() {
                result.push(if group.group_id == self.shared.group_id {
                    task.task_name.clone()
                } else {
                    format!("{}::{}", group.name, task.task_name)
                });
            }
        }
        result
    }

    #[aura_macros::capability_boundary(
        category = "capability_gated",
        capability = "registered_runtime_task",
        capability_type = RegisteredTaskCapability,
        family = "runtime_helper"
    )]
    fn register_task(
        &self,
        task_id: u64,
        task_name: String,
    ) -> Result<RegisteredTaskCapability, TaskSupervisionError> {
        let mut tree = self.shared.tree.state.lock();
        let rejection = if self.shared.closing.load(Ordering::Acquire)
            || self.cancellation_token().is_cancelled()
        {
            Some(TaskSupervisionError::AdmissionClosed {
                group: self.shared.name.clone(),
                task: task_name.clone(),
            })
        } else if tree.active_tasks >= MAX_SUPERVISED_TASKS {
            Some(TaskSupervisionError::AdmissionLimit {
                group: self.shared.name.clone(),
                kind: TaskAdmissionKind::Tasks,
                limit: MAX_SUPERVISED_TASKS,
            })
        } else {
            None
        };
        if let Some(error) = rejection {
            drop(tree);
            self.propagate_failure(error.clone());
            return Err(error);
        }
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        self.shared.tasks.lock().insert(
            task_id,
            TaskMetadata {
                task_name,
                abort,
                #[cfg(not(target_arch = "wasm32"))]
                handle: None,
            },
        );
        tree.active_tasks += 1;
        Ok(RegisteredTaskCapability {
            identity: OwnedRuntimeTaskIdentity {
                tree: self.shared.tree.clone(),
                group_id: self.shared.group_id,
                task_id,
            },
            abort_registration: registration,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn attach_native_handle(&self, task_id: u64, handle: JoinHandle<()>) {
        if let Some(metadata) = self.shared.tasks.lock().get_mut(&task_id) {
            metadata.handle = Some(handle);
        }
    }

    fn complete_task(&self, task_id: u64, task_name: &str, outcome: TaskOutcome) {
        let mut tree = self.shared.tree.state.lock();
        if !self.shared.tasks.lock().contains_key(&task_id) {
            return;
        }
        let failure = match &outcome {
            TaskOutcome::Failed(source) => Some(TaskSupervisionError::TaskFailed {
                group: self.shared.name.clone(),
                task: task_name.to_owned(),
                source: source.clone(),
            }),
            TaskOutcome::Panicked => Some(TaskSupervisionError::Panicked {
                group: self.shared.name.clone(),
                task: task_name.to_owned(),
            }),
            TaskOutcome::Completed | TaskOutcome::Cancelled => None,
        };
        if let Some(error) = failure {
            self.propagate_failure(error);
        }
        self.shared.tasks.lock().remove(&task_id);
        tree.active_tasks -= 1;
        drop(tree);
        self.notify_ancestors();
    }

    fn spawn_boxed(
        &self,
        task_name: String,
        fut: BoxFuture<'static, ()>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        self.spawn_fallible_boxed(
            task_name,
            Box::pin(async move {
                fut.await;
                Ok(())
            }),
            external_token,
        )
    }

    fn spawn_fallible_boxed(
        &self,
        task_name: String,
        fut: BoxFuture<'static, Result<(), aura_core::AuraError>>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        self.spawn_checked_fallible_boxed(task_name, fut, external_token)
            .unwrap_or_else(|(task_id, _source)| self.rejected_task_handle(task_id))
    }

    fn spawn_checked_fallible_boxed(
        &self,
        task_name: String,
        fut: BoxFuture<'static, Result<(), aura_core::AuraError>>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OwnedTaskHandle<u64>, (u64, TaskSupervisionError)> {
        let task_id = self.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        let registered = self
            .register_task(task_id, task_name.clone())
            .map_err(|source| (task_id, source))?;
        let (scoped, abort_registration) = registered.bind(fut);
        let fut: BoxFuture<'static, Result<(), aura_core::AuraError>> = Box::pin(scoped);
        let mut completion = TaskCompletionGuard {
            future: Some(fut),
            group: self.clone(),
            task_id,
            task_name: task_name.clone(),
            completed: false,
        };
        let group_name = self.shared.name.clone();
        let mut shutdown_rx = self.shared.shutdown_tx.subscribe();
        let inherited = self.shared.inherited_cancellation.clone();

        tracing::debug!(
            event = "runtime.task.spawned",
            task_group = %group_name,
            task_name = %task_name,
            task_id,
            "Spawned supervised task"
        );

        #[cfg(not(target_arch = "wasm32"))]
        let handle = tokio::spawn(async move {
            let outcome = AssertUnwindSafe(Abortable::new(async {
                tokio::select! {
                    biased;
                    _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                    _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                    _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                    result = completion.future.as_mut().expect("registered task owns its future") => match result {
                        Ok(()) => TaskOutcome::Completed,
                        Err(error) => TaskOutcome::Failed(error),
                    },
                }
            },abort_registration))
            .catch_unwind()
            .await
            .map(|result|result.unwrap_or(TaskOutcome::Cancelled))
            .unwrap_or(TaskOutcome::Panicked);
            completion.finish(outcome);
        });

        #[cfg(not(target_arch = "wasm32"))]
        self.attach_native_handle(task_id, handle);

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                let outcome = AssertUnwindSafe(Abortable::new(async {
                    tokio::select! {
                    biased;
                        _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                        _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                        _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                        result = completion.future.as_mut().expect("registered task owns its future") => match result {
                            Ok(()) => TaskOutcome::Completed,
                            Err(error) => TaskOutcome::Failed(error),
                        },
                    }
                },abort_registration))
                .catch_unwind()
                .await
                .map(|result|result.unwrap_or(TaskOutcome::Cancelled))
                .unwrap_or(TaskOutcome::Panicked);
                completion.finish(outcome);
            });
        }

        Ok(OwnedTaskHandle::new(
            task_id,
            OwnedShutdownToken::attached(self.cancellation_token()),
        ))
    }

    fn spawn_boxed_local(
        &self,
        task_name: String,
        fut: LocalBoxFuture<'static, ()>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        self.spawn_fallible_boxed_local(
            task_name,
            Box::pin(async move {
                fut.await;
                Ok(())
            }),
            external_token,
        )
    }

    fn spawn_fallible_boxed_local(
        &self,
        task_name: String,
        fut: LocalBoxFuture<'static, Result<(), aura_core::AuraError>>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> OwnedTaskHandle<u64> {
        self.spawn_checked_fallible_boxed_local(task_name, fut, external_token)
            .unwrap_or_else(|(task_id, _source)| self.rejected_task_handle(task_id))
    }

    fn spawn_checked_fallible_boxed_local(
        &self,
        task_name: String,
        fut: LocalBoxFuture<'static, Result<(), aura_core::AuraError>>,
        external_token: Option<Arc<dyn CancellationToken>>,
    ) -> Result<OwnedTaskHandle<u64>, (u64, TaskSupervisionError)> {
        let task_id = self.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        let registered = self
            .register_task(task_id, task_name.clone())
            .map_err(|source| (task_id, source))?;
        let (scoped, abort_registration) = registered.bind(fut);
        let fut: LocalBoxFuture<'static, Result<(), aura_core::AuraError>> = Box::pin(scoped);
        let mut completion = TaskCompletionGuard {
            future: Some(fut),
            group: self.clone(),
            task_id,
            task_name: task_name.clone(),
            completed: false,
        };
        let mut shutdown_rx = self.shared.shutdown_tx.subscribe();
        let inherited = self.shared.inherited_cancellation.clone();

        #[cfg(not(target_arch = "wasm32"))]
        let handle = tokio::task::spawn_local(async move {
            let outcome = AssertUnwindSafe(Abortable::new(async {
                tokio::select! {
                    biased;
                    _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                    _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                    _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                    result = completion.future.as_mut().expect("registered task owns its future") => match result {
                        Ok(()) => TaskOutcome::Completed,
                        Err(error) => TaskOutcome::Failed(error),
                    },
                }
            },abort_registration))
            .catch_unwind()
            .await
            .map(|result|result.unwrap_or(TaskOutcome::Cancelled))
            .unwrap_or(TaskOutcome::Panicked);
            completion.finish(outcome);
        });

        #[cfg(not(target_arch = "wasm32"))]
        self.attach_native_handle(task_id, handle);

        #[cfg(target_arch = "wasm32")]
        {
            spawn_local(async move {
                let outcome = AssertUnwindSafe(Abortable::new(async {
                    tokio::select! {
                    biased;
                        _ = shutdown_cancelled(&mut shutdown_rx) => TaskOutcome::Cancelled,
                        _ = inherited_cancelled(inherited.as_ref()) => TaskOutcome::Cancelled,
                        _ = external_cancelled(external_token.as_deref()) => TaskOutcome::Cancelled,
                        result = completion.future.as_mut().expect("registered task owns its future") => match result {
                            Ok(()) => TaskOutcome::Completed,
                            Err(error) => TaskOutcome::Failed(error),
                        },
                    }
                },abort_registration))
                .catch_unwind()
                .await
                .map(|result|result.unwrap_or(TaskOutcome::Cancelled))
                .unwrap_or(TaskOutcome::Panicked);
                completion.finish(outcome);
            });
        }

        Ok(OwnedTaskHandle::new(
            task_id,
            OwnedShutdownToken::attached(self.cancellation_token()),
        ))
    }
}

struct TaskGroupCancellationToken {
    shutdown_rx: watch::Receiver<bool>,
    inherited: Option<Arc<dyn CancellationToken>>,
}

#[async_trait::async_trait]
impl CancellationToken for TaskGroupCancellationToken {
    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }

        let mut shutdown_rx = self.shutdown_rx.clone();
        match self.inherited.clone() {
            Some(inherited) => {
                tokio::select! {
                    biased;
                    _ = shutdown_cancelled(&mut shutdown_rx) => {}
                    _ = inherited.cancelled() => {}
                }
            }
            None => {
                shutdown_cancelled(&mut shutdown_rx).await;
            }
        }
    }

    fn is_cancelled(&self) -> bool {
        *self.shutdown_rx.borrow()
            || self
                .inherited
                .as_ref()
                .map(|token| token.is_cancelled())
                .unwrap_or(false)
    }
}

impl TaskSpawner for TaskSupervisor {
    fn spawn_fallible_cancellable(
        &self,
        name: &'static str,
        fut: BoxFuture<'static, Result<(), aura_core::AuraError>>,
        token: Arc<dyn CancellationToken>,
    ) -> Result<(), aura_core::AuraError> {
        self.root
            .spawn_checked_fallible_boxed(name.to_owned(), fut, Some(token))
            .map(|_owned_handle| ())
            .map_err(|(_task_id, source)| aura_core::AuraError::Internal {
                message: "required task admission rejected".into(),
                source: Some(Arc::new(source)),
            })
    }
    fn spawn_local_fallible_cancellable(
        &self,
        name: &'static str,
        fut: LocalBoxFuture<'static, Result<(), aura_core::AuraError>>,
        token: Arc<dyn CancellationToken>,
    ) -> Result<(), aura_core::AuraError> {
        self.root
            .spawn_checked_fallible_boxed_local(name.to_owned(), fut, Some(token))
            .map(|_owned_handle| ())
            .map_err(|(_task_id, source)| aura_core::AuraError::Internal {
                message: "required local task admission rejected".into(),
                source: Some(Arc::new(source)),
            })
    }
    fn spawn(&self, fut: BoxFuture<'static, ()>) {
        let _ = self.spawn_named(DEFAULT_TASK_NAME, fut);
    }

    fn spawn_cancellable(&self, fut: BoxFuture<'static, ()>, token: Arc<dyn CancellationToken>) {
        let _ = self
            .root
            .spawn_boxed(DEFAULT_TASK_NAME.to_string(), fut, Some(token));
    }

    fn spawn_local(&self, fut: LocalBoxFuture<'static, ()>) {
        let _ = self
            .root
            .spawn_boxed_local(DEFAULT_TASK_NAME.to_string(), fut, None);
    }

    fn spawn_local_cancellable(
        &self,
        fut: LocalBoxFuture<'static, ()>,
        token: Arc<dyn CancellationToken>,
    ) {
        let _ = self
            .root
            .spawn_boxed_local(DEFAULT_TASK_NAME.to_string(), fut, Some(token));
    }

    fn cancellation_token(&self) -> Arc<dyn CancellationToken> {
        self.cancellation_token()
    }
}

fn emit_task_completion(
    diagnostics: Option<&Arc<RuntimeDiagnosticSink>>,
    group: &str,
    task_name: &str,
    task_id: u64,
    outcome: &TaskOutcome,
) {
    match outcome {
        TaskOutcome::Failed(source) => tracing::error!(
            event = "runtime.task.failed",
            task_group = %group,
            task_name = %task_name,
            task_id,
            %source,
            "Supervised task failed"
        ),
        TaskOutcome::Completed => tracing::debug!(
            event = "runtime.task.completed",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task completed"
        ),
        TaskOutcome::Cancelled => tracing::info!(
            event = "runtime.task.cancelled",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task cancelled"
        ),
        TaskOutcome::Panicked => tracing::error!(
            event = "runtime.task.panicked",
            task_group = %group,
            task_name = %task_name,
            task_id,
            "Supervised task panicked"
        ),
    }

    if matches!(outcome, TaskOutcome::Panicked | TaskOutcome::Failed(_)) {
        emit_task_diagnostic(
            diagnostics,
            RuntimeDiagnosticSeverity::Error,
            "task_supervisor",
            format!("supervised task '{task_name}' in group '{group}' failed: {outcome:?}"),
        );
    }
}

fn emit_task_diagnostic(
    diagnostics: Option<&Arc<RuntimeDiagnosticSink>>,
    severity: RuntimeDiagnosticSeverity,
    component: &'static str,
    message: String,
) {
    if let Some(diagnostics) = diagnostics {
        diagnostics.emit(RuntimeDiagnostic {
            severity,
            kind: RuntimeDiagnosticKind::SupervisedTaskFailed,
            component,
            message,
        });
    }
}

async fn shutdown_cancelled(shutdown_rx: &mut watch::Receiver<bool>) {
    loop {
        if *shutdown_rx.borrow() {
            return;
        }
        if shutdown_rx.changed().await.is_err() {
            return;
        }
    }
}

async fn inherited_cancelled(token: Option<&Arc<dyn CancellationToken>>) {
    match token {
        Some(token) => token.cancelled().await,
        None => futures::future::pending::<()>().await,
    }
}

async fn external_cancelled(token: Option<&dyn CancellationToken>) {
    match token {
        Some(token) => token.cancelled().await,
        None => futures::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn required_callback_failure_stops_before_timer_and_retains_original_cause() {
        let group = TaskGroup::root("callback-failure", None);
        let _owner = group.spawn_try_interval_until_named(
            "required-callback",
            Arc::new(FailedIntervalClock),
            Duration::from_millis(1),
            || async {
                Err(aura_core::AuraError::from(
                    aura_core::effects::TimeError::OperationFailed {
                        reason: "callback clock failed".into(),
                    },
                ))
            },
        );
        let error = group
            .wait_for_idle(Duration::from_secs(1))
            .await
            .unwrap_err();
        let cause = std::error::Error::source(&error)
            .and_then(std::error::Error::source)
            .and_then(|cause| cause.downcast_ref::<aura_core::effects::TimeError>());
        assert!(
            matches!(cause, Some(aura_core::effects::TimeError::OperationFailed { reason }) if reason == "callback clock failed")
        );
        assert!(matches!(
            group.terminal_failure(),
            Some(TaskSupervisionError::TaskFailed { .. })
        ));
    }

    struct FailedIntervalClock;

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl PhysicalTimeEffects for FailedIntervalClock {
        async fn physical_time(
            &self,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
            panic!("interval needs only its required timer")
        }
        async fn sleep_ms(&self, _: u64) -> Result<(), aura_core::effects::TimeError> {
            Err(aura_core::effects::TimeError::ServiceUnavailable)
        }
    }

    #[tokio::test]
    async fn interval_sleep_failure_is_retained_in_health_and_drain() {
        use std::error::Error;
        let group = TaskGroup::root("failed-interval", None);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let _owner = group.spawn_interval_until_named(
            "required-service",
            Arc::new(FailedIntervalClock),
            Duration::from_millis(1),
            move || {
                observed.fetch_add(1, Ordering::SeqCst);
                async { true }
            },
        );
        let error = group
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect_err("failed interval cannot drain successfully");
        assert!(
            matches!(&error, TaskSupervisionError::TaskFailed { task, .. } if task=="required-service")
        );
        assert!(matches!(
            error
                .source()
                .and_then(Error::source)
                .and_then(|source| source.downcast_ref::<aura_core::effects::TimeError>()),
            Some(aura_core::effects::TimeError::ServiceUnavailable)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(
            group.terminal_failure(),
            Some(TaskSupervisionError::TaskFailed { .. })
        ));
    }

    #[tokio::test]
    async fn invalid_interval_fails_before_callback_and_local_sleep_failure_is_observable() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let invalid = TaskGroup::root("invalid-interval", None);
                let _owner = invalid.spawn_interval_until_named(
                    "invalid-policy",
                    Arc::new(FailedIntervalClock),
                    Duration::MAX,
                    || async { panic!("invalid policy must not invoke callback") },
                );
                let error = invalid
                    .wait_for_idle(Duration::from_secs(1))
                    .await
                    .expect_err("invalid interval must fail");
                assert!(matches!(
                    error,
                    TaskSupervisionError::TaskFailed {
                        source: aura_core::AuraError::Invalid { .. },
                        ..
                    }
                ));
                let group = TaskGroup::root("local-failed-interval", None);
                let _owner = group.spawn_local_interval_until_named(
                    "local-required-service",
                    Arc::new(FailedIntervalClock),
                    Duration::from_millis(1),
                    || async { true },
                );
                assert!(matches!(
                    group.wait_for_idle(Duration::from_secs(1)).await,
                    Err(TaskSupervisionError::TaskFailed { .. })
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn original_shutdown_window_preserves_required_clock_fault_without_claiming_drain(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let supervisor = TaskSupervisor::new();
        let started = PhysicalTimeHandler::new().physical_time().await?;
        let original = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))?;
        let result = supervisor
            .shutdown_with_original_budget(&UnavailableSupervisorClock, &original)
            .await;
        let Err(error) = result else {
            panic!("required clock outage cannot certify original-window drain")
        };
        let mut cause: &(dyn std::error::Error + 'static) = &error;
        let mut found = false;
        loop {
            if matches!(
                cause.downcast_ref::<aura_core::effects::TimeError>(),
                Some(aura_core::effects::TimeError::ServiceUnavailable)
            ) {
                found = true;
            }
            match cause.source() {
                Some(next) => cause = next,
                None => break,
            }
        }
        assert!(
            found,
            "original required provider failure remains observable"
        );
        assert!(!matches!(error, TaskSupervisionError::Timeout { .. }));
        assert!(supervisor.active_tasks().is_empty());
        // No record or handoff success is minted even if there were no tasks.
        Ok(())
    }

    struct UnavailableSupervisorClock;

    #[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
    #[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
    impl PhysicalTimeEffects for UnavailableSupervisorClock {
        async fn physical_time(
            &self,
        ) -> Result<aura_core::time::PhysicalTime, aura_core::effects::TimeError> {
            Err(aura_core::effects::TimeError::ServiceUnavailable)
        }

        async fn sleep_ms(&self, _: u64) -> Result<(), aura_core::effects::TimeError> {
            panic!("failed clock must not start the wait timer")
        }
    }

    #[tokio::test]
    async fn supervisor_clock_and_policy_faults_are_not_timeouts() {
        use std::error::Error;
        let group = TaskGroup::root("typed-supervisor-faults", None);
        let clock = group
            .wait_for_idle_with_time(Duration::from_secs(1), &UnavailableSupervisorClock)
            .await
            .expect_err("required clock failure");
        assert!(matches!(
            &clock,
            TaskSupervisionError::Budget {
                source,
                ..
            } if matches!(source.as_ref(), TimeoutBudgetError::TimeSourceUnavailable { .. })
        ));
        assert!(matches!(
            clock
                .source()
                .and_then(Error::source)
                .and_then(Error::source)
                .and_then(|cause| cause.downcast_ref::<aura_core::effects::TimeError>()),
            Some(aura_core::effects::TimeError::ServiceUnavailable)
        ));
        let policy = group
            .wait_for_idle(Duration::MAX)
            .await
            .expect_err("unrepresentable duration");
        assert!(matches!(
            policy,
            TaskSupervisionError::Budget {
                source,
                ..
            } if matches!(source.as_ref(), TimeoutBudgetError::InvalidPolicy { .. })
        ));
    }

    use super::*;
    use crate::runtime::{RuntimeDiagnosticKind, RuntimeDiagnosticSeverity};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn shutdown_with_timeout_cancels_supervised_tasks() {
        let supervisor = TaskSupervisor::new();
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        supervisor
            .shutdown_with_timeout(Duration::from_millis(50))
            .await
            .expect("shutdown should cancel pending task");
        assert!(supervisor.active_tasks().is_empty());
    }

    #[tokio::test]
    async fn child_groups_inherit_parent_cancellation() {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("child");
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = child.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        supervisor.request_cancellation();
        child
            .wait_for_idle(Duration::from_millis(50))
            .await
            .expect("child tasks should stop when parent is cancelled");
    }

    #[tokio::test]
    async fn wait_for_idle_times_out_and_force_abort_reports_tasks() {
        let supervisor = TaskSupervisor::new();
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        let timeout = supervisor.wait_for_idle(Duration::from_millis(10)).await;
        assert!(matches!(timeout, Err(TaskSupervisionError::Timeout { .. })));
        let cause = std::error::Error::source(timeout.as_ref().unwrap_err())
            .and_then(|cause| cause.downcast_ref::<TimeoutBudgetError>());
        assert!(
            matches!(cause, Some(TimeoutBudgetError::DeadlineExceeded { deadline_at_ms, observed_at_ms }) if observed_at_ms >= deadline_at_ms)
        );

        let abort = supervisor.force_abort_remaining();
        assert!(matches!(
            abort,
            Err(TaskSupervisionError::ForcedAbort { .. })
        ));
        supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect("aborted futures actually drop before idle");
        assert!(supervisor.active_tasks().is_empty());
    }

    #[tokio::test]
    async fn force_abort_emits_runtime_diagnostic() {
        let diagnostics = Arc::new(RuntimeDiagnosticSink::new());
        let supervisor = TaskSupervisor::with_diagnostics(diagnostics.clone());
        let (started_tx, started_rx) = oneshot::channel();

        let _task_handle = supervisor.spawn_named("test.pending", async move {
            let _ = started_tx.send(());
            futures::future::pending::<()>().await;
        });

        started_rx.await.expect("task should start");
        let mut rx = diagnostics.subscribe();
        let abort = supervisor.force_abort_remaining();
        assert!(matches!(
            abort,
            Err(TaskSupervisionError::ForcedAbort { .. })
        ));

        let diagnostic = rx.try_recv().expect("diagnostic emitted");
        assert_eq!(diagnostic.kind, RuntimeDiagnosticKind::SupervisedTaskFailed);
        assert_eq!(diagnostic.severity, RuntimeDiagnosticSeverity::Warn);
    }

    #[test]
    fn loom_shutdown_race_does_not_leave_task_registered() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::{Arc as LoomArc, Mutex as LoomMutex};
            use loom::thread;

            let active = LoomArc::new(LoomMutex::new(Vec::<u8>::new()));
            let cancelled = LoomArc::new(AtomicBool::new(false));

            let register_active = LoomArc::clone(&active);
            let register_cancelled = LoomArc::clone(&cancelled);
            let register = thread::spawn(move || {
                {
                    let mut tasks = register_active.lock().unwrap();
                    tasks.push(1);
                }
                if register_cancelled.load(Ordering::Acquire) {
                    let mut tasks = register_active.lock().unwrap();
                    tasks.retain(|task| *task != 1);
                }
            });

            let shutdown_active = LoomArc::clone(&active);
            let shutdown_cancelled = LoomArc::clone(&cancelled);
            let shutdown = thread::spawn(move || {
                shutdown_cancelled.store(true, Ordering::Release);
                let mut tasks = shutdown_active.lock().unwrap();
                tasks.retain(|task| *task != 1);
            });

            register.join().expect("register thread");
            shutdown.join().expect("shutdown thread");
            assert!(
                active.lock().unwrap().is_empty(),
                "task bookkeeping should not leak active entries across shutdown races"
            );
        });
    }

    #[test]
    fn loom_shutdown_token_propagation_reaches_child() {
        loom::model(|| {
            use loom::sync::atomic::{AtomicBool, Ordering};
            use loom::sync::Arc as LoomArc;
            use loom::thread;

            let cancelled = LoomArc::new(AtomicBool::new(false));
            let child_observed = LoomArc::new(AtomicBool::new(false));

            let child = {
                let cancelled = cancelled.clone();
                let child_observed = child_observed.clone();
                thread::spawn(move || {
                    while !cancelled.load(Ordering::Acquire) {
                        thread::yield_now();
                    }
                    child_observed.store(true, Ordering::Release);
                })
            };

            let parent = {
                let cancelled = cancelled.clone();
                thread::spawn(move || {
                    cancelled.store(true, Ordering::Release);
                })
            };

            parent.join().expect("parent joins");
            child.join().expect("child joins");

            assert!(
                child_observed.load(Ordering::Acquire),
                "child cancellation observer must see parent-driven shutdown"
            );
        });
    }
}

#[cfg(test)]
mod descendant_supervision_tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[tokio::test]
    async fn original_shutdown_window_acknowledges_actual_descendant_destruction(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("original-window-descendant");
        let dropped = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let owner = child.spawn_named(
            "retained-original-callback",
            PendingDropProbe {
                dropped: dropped.clone(),
                started: Some(started_tx),
            },
        );
        started_rx.await?;
        assert!(!dropped.load(Ordering::Acquire));
        let time = PhysicalTimeHandler::new();
        let started = time.physical_time().await?;
        let original = TimeoutBudget::from_start_and_timeout(&started, Duration::from_secs(30))?;
        supervisor
            .shutdown_with_original_budget(&time, &original)
            .await?;
        assert!(dropped.load(Ordering::Acquire));
        assert!(supervisor.active_tasks().is_empty());
        assert!(child.active_tasks().is_empty());
        drop(owner);
        Ok(())
    }

    struct PendingDropProbe {
        dropped: Arc<AtomicBool>,
        started: Option<tokio::sync::oneshot::Sender<()>>,
    }
    impl Future for PendingDropProbe {
        type Output = ();
        fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            if let Some(started) = self.started.take() {
                let _ = started.send(());
            }
            Poll::Pending
        }
    }
    impl Drop for PendingDropProbe {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn root_covers_dropped_child_handles_and_abort_waits_for_future_drop() {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("child");
        let grandchild = child.group("grandchild");
        let dropped = Arc::new(AtomicBool::new(false));
        let (started, ready) = tokio::sync::oneshot::channel();
        let _owner = grandchild.spawn_named(
            "owned",
            PendingDropProbe {
                dropped: dropped.clone(),
                started: Some(started),
            },
        );
        ready.await.unwrap();
        drop(child);
        drop(grandchild);
        assert_eq!(
            supervisor.active_tasks(),
            vec!["runtime.child.grandchild::owned".to_owned()]
        );
        let timeout = supervisor
            .wait_for_idle(Duration::from_millis(1))
            .await
            .unwrap_err();
        assert!(
            matches!(timeout,TaskSupervisionError::Timeout {active_tasks,..} if active_tasks==vec!["runtime.child.grandchild::owned".to_owned()])
        );
        assert!(
            matches!(supervisor.force_abort_remaining(),Err(TaskSupervisionError::ForcedAbort {aborted_tasks,..}) if aborted_tasks==vec!["runtime.child.grandchild::owned".to_owned()])
        );
        // An abort request alone does not clear registration or release resources.
        assert!(!dropped.load(Ordering::Acquire));
        assert_eq!(supervisor.active_tasks().len(), 1);
        supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .unwrap();
        assert!(dropped.load(Ordering::Acquire));
        assert!(supervisor.active_tasks().is_empty());
    }

    #[tokio::test]
    async fn descendant_failure_reaches_root_and_survives_child_registry_pruning() {
        use std::error::Error;
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("failed-child");
        let _owner = child.spawn_try_interval_until_named(
            "required",
            Arc::new(PhysicalTimeHandler::new()),
            Duration::from_millis(1),
            || async {
                Err(aura_core::AuraError::Storage {
                    message: "required IO".into(),
                    source: Some(Arc::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "denied",
                    ))),
                })
            },
        );
        drop(child);
        let error = supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .unwrap_err();
        assert!(
            matches!(&error,TaskSupervisionError::TaskFailed {group,task,..} if group=="runtime.failed-child" && task=="required")
        );
        assert!(
            matches!(error.source().and_then(Error::source).and_then(|e|e.downcast_ref::<std::io::Error>()),Some(e) if e.kind()==std::io::ErrorKind::PermissionDenied)
        );
        assert!(supervisor.active_tasks().is_empty());
        assert!(matches!(
            supervisor.root.terminal_failure(),
            Some(TaskSupervisionError::TaskFailed { .. })
        ));
        assert_eq!(supervisor.root.shared.tree.state.lock().groups.len(), 1);
    }

    #[tokio::test]
    async fn cancellation_closes_descendant_admission_and_never_polls_rejected_work() {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("child");
        supervisor.request_cancellation();
        let dropped = Arc::new(AtomicBool::new(false));
        let (started, ready) = tokio::sync::oneshot::channel();
        let _owner = child.spawn_named(
            "late",
            PendingDropProbe {
                dropped: dropped.clone(),
                started: Some(started),
            },
        );
        assert!(dropped.load(Ordering::Acquire));
        assert!(
            ready.await.is_err(),
            "rejected future is dropped without polling"
        );
        let late = child.group("late-child");
        assert!(late.cancellation_token().is_cancelled());
        assert!(supervisor.active_tasks().is_empty());
        assert!(matches!(
            supervisor.wait_for_idle(Duration::from_secs(1)).await,
            Err(TaskSupervisionError::AdmissionClosed { .. })
        ));
    }

    #[tokio::test]
    async fn local_descendants_propagate_failure_and_drop_before_root_drain() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let supervisor = TaskSupervisor::new();
                let child = supervisor.group("local");
                let marker = std::rc::Rc::new(std::cell::Cell::new(false));
                let moved = marker.clone();
                struct LocalDrop(std::rc::Rc<std::cell::Cell<bool>>);
                impl Drop for LocalDrop {
                    fn drop(&mut self) {
                        self.0.set(true);
                    }
                }
                let owned = LocalDrop(moved);
                let _owner = child.spawn_local_try_interval_until_named(
                    "required-local",
                    Arc::new(PhysicalTimeHandler::new()),
                    Duration::from_millis(1),
                    move || {
                        let _keep = &owned;
                        async { Err(aura_core::AuraError::invalid("local policy failed")) }
                    },
                );
                drop(child);
                assert!(matches!(
                    supervisor.wait_for_idle(Duration::from_secs(1)).await,
                    Err(TaskSupervisionError::TaskFailed { .. })
                ));
                assert!(
                    marker.get(),
                    "local owned resources must drop before root reports completion"
                );
                assert!(supervisor.active_tasks().is_empty());
            })
            .await;
    }

    #[test]
    fn group_registry_is_bounded_prunes_dead_children_and_rejects_deep_trees() {
        let supervisor = TaskSupervisor::new();
        let groups: Vec<_> = (0..MAX_SUPERVISED_GROUPS - 1)
            .map(|index| supervisor.group(format!("child-{index}")))
            .collect();
        let rejected = supervisor.group("overflow");
        assert!(matches!(
            rejected.terminal_failure(),
            Some(TaskSupervisionError::AdmissionLimit { .. })
        ));
        assert!(rejected.cancellation_token().is_cancelled());
        assert_eq!(
            supervisor.root.shared.tree.state.lock().groups.len(),
            MAX_SUPERVISED_GROUPS
        );
        drop(rejected);
        drop(groups);
        assert!(supervisor.active_tasks().is_empty());
        assert_eq!(supervisor.root.shared.tree.state.lock().groups.len(), 1);
        let independent = TaskSupervisor::new();
        let mut deepest = independent.group("child");
        for _ in 2..MAX_GROUP_DEPTH {
            deepest = deepest.group("child");
        }
        let rejected = deepest.group("too-deep");
        assert!(matches!(
            rejected.terminal_failure(),
            Some(TaskSupervisionError::AdmissionLimit {
                kind: TaskAdmissionKind::Depth,
                limit: MAX_GROUP_DEPTH,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn dropping_supervisor_clone_does_not_cancel_the_live_owner() {
        let supervisor = TaskSupervisor::new();
        drop(supervisor.clone());
        assert!(!supervisor.cancellation_token().is_cancelled());
        let _owner = supervisor.spawn_named("complete", async {});
        supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .unwrap();
    }
}

#[cfg(test)]
mod bounded_descendant_task_tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    struct DropCount(Arc<std::sync::atomic::AtomicUsize>);
    impl Future for DropCount {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            Poll::Pending
        }
    }
    impl Drop for DropCount {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[tokio::test]
    async fn tree_task_limit_counts_descendants_and_rejected_work_is_not_scheduled() {
        let supervisor = TaskSupervisor::new();
        let children = [supervisor.group("first"), supervisor.group("second")];
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for index in 0..MAX_SUPERVISED_TASKS {
            let _owner = children[index % 2]
                .spawn_named(format!("pending-{index}"), DropCount(drops.clone()));
        }
        let rejected = children[0].spawn_named("overflow", DropCount(drops.clone()));
        drop(rejected);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert_eq!(supervisor.active_tasks().len(), MAX_SUPERVISED_TASKS);
        assert!(matches!(
            supervisor.root.terminal_failure(),
            Some(TaskSupervisionError::AdmissionLimit {
                kind: TaskAdmissionKind::Tasks,
                limit: MAX_SUPERVISED_TASKS,
                ..
            })
        ));
        let _abort = supervisor.force_abort_remaining();
        assert!(matches!(
            supervisor.wait_for_idle(Duration::from_secs(5)).await,
            Err(TaskSupervisionError::AdmissionLimit { .. })
        ));
        assert_eq!(drops.load(Ordering::SeqCst), MAX_SUPERVISED_TASKS + 1);
        assert!(supervisor.active_tasks().is_empty());
    }
    #[tokio::test]
    async fn descendant_panic_is_retained_by_root_health() {
        let supervisor = TaskSupervisor::new();
        let child = supervisor.group("panicked");
        let _owner = child.spawn_named("panic", async { panic!("real task panic") });
        drop(child);
        assert!(
            matches!(supervisor.wait_for_idle(Duration::from_secs(1)).await,Err(TaskSupervisionError::Panicked {group,task}) if group=="runtime.panicked" && task=="panic")
        );
        assert!(supervisor.active_tasks().is_empty());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod actual_shutdown_admission_race_tests {
    use super::*;
    #[tokio::test]
    async fn real_group_registration_races_shutdown_without_escaping_root_drain() {
        let supervisor = TaskSupervisor::new();
        let worker_owner = supervisor.clone();
        let runtime = tokio::runtime::Handle::current();
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let drops = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let worker_drops = drops.clone();
        struct Probe(Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let worker = std::thread::spawn(move || {
            let _runtime = runtime.enter();
            worker_barrier.wait();
            for index in 0..32 {
                let child = worker_owner.group(format!("racing-{index}"));
                let resource = Probe(worker_drops.clone());
                let _owner = child.spawn_named("pending", async move {
                    let _resource = resource;
                    futures::future::pending::<()>().await;
                });
            }
        });
        barrier.wait();
        supervisor.request_cancellation();
        worker.join().unwrap();
        let result = supervisor.wait_for_idle(Duration::from_secs(1)).await;
        assert!(matches!(
            result,
            Ok(()) | Err(TaskSupervisionError::AdmissionClosed { .. })
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 32);
        assert!(supervisor.active_tasks().is_empty());
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod required_task_admission_tests {
    use super::*;
    use std::error::Error;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll};

    #[derive(Debug, thiserror::Error)]
    #[error("required refresh provider failed")]
    struct RequiredProviderFault;

    fn provider_fault() -> aura_core::AuraError {
        aura_core::AuraError::Storage {
            message: "required refresh read failed".into(),
            source: Some(Arc::new(RequiredProviderFault)),
        }
    }

    async fn assert_required_failure(supervisor: &TaskSupervisor) {
        let error = supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect_err("required task failure must reach root supervision");
        let TaskSupervisionError::TaskFailed { source, .. } = error else {
            panic!("required task must retain its structural failure");
        };
        assert!(matches!(&source, aura_core::AuraError::Storage { .. }));
        assert!(source
            .source()
            .expect("original provider source")
            .is::<RequiredProviderFault>());
    }

    #[tokio::test]
    async fn native_required_task_retains_original_provider_failure() {
        let supervisor = TaskSupervisor::new();
        TaskSpawner::spawn_fallible_cancellable(
            &supervisor,
            "required-refresh",
            Box::pin(async { Err(provider_fault()) }),
            supervisor.cancellation_token(),
        )
        .expect("admit required task");
        assert_required_failure(&supervisor).await;
    }

    #[tokio::test]
    async fn local_required_task_retains_original_provider_failure() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let supervisor = TaskSupervisor::new();
                TaskSpawner::spawn_local_fallible_cancellable(
                    &supervisor,
                    "required-local-refresh",
                    Box::pin(async { Err(provider_fault()) }),
                    supervisor.cancellation_token(),
                )
                .expect("admit required local task");
                assert_required_failure(&supervisor).await;
            })
            .await;
    }

    struct RejectedFuture(Arc<AtomicBool>);
    impl Future for RejectedFuture {
        type Output = Result<(), aura_core::AuraError>;
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
            panic!("closed admission must not poll the supplied future")
        }
    }
    impl Drop for RejectedFuture {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn closed_required_admission_returns_source_after_dropping_future() {
        let supervisor = TaskSupervisor::new();
        supervisor.request_cancellation();
        let dropped = Arc::new(AtomicBool::new(false));
        let error = TaskSpawner::spawn_fallible_cancellable(
            &supervisor,
            "rejected-refresh",
            Box::pin(RejectedFuture(dropped.clone())),
            supervisor.cancellation_token(),
        )
        .expect_err("closed owner must reject required work immediately");
        assert!(dropped.load(Ordering::Acquire));
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<TaskSupervisionError>()),
            Some(TaskSupervisionError::AdmissionClosed { .. })
        ));
        assert!(supervisor.active_tasks().is_empty());
    }
}
#[cfg(test)]
mod registered_task_context_tests {
    use super::*;
    use crate::runtime::subsystems::choreography::{
        ChoreographyState, RuntimeChoreographySessionId,
    };
    use aura_core::{AuthorityId, ContextId, DeviceId};
    use aura_protocol::effects::{ChoreographicRole, RoleIndex};
    use futures::channel::oneshot;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// This helper mints only through actual registry admission. Manual polling
    /// tests do not invent an identity or bypass the bounded registry state.
    fn registered_fixture<F: Future + 'static>(
        group: &TaskGroup,
        name: &str,
        future: F,
    ) -> (RegisteredTaskFuture<Pin<Box<F>>>, u64) {
        let id = group.shared.next_task_id.fetch_add(1, Ordering::Relaxed);
        let admitted = group
            .register_task(id, name.to_string())
            .expect("actual bounded registration");
        let (future, _abort) = admitted.bind(Box::pin(future));
        (future, id)
    }

    #[test]
    fn registered_context_restores_parent_after_nested_pending_and_panic_polls() {
        let supervisor = TaskSupervisor::new();
        let outer = supervisor.group("context.outer");
        let inner = supervisor.group("context.inner");
        let (mut nested, nested_id) = registered_fixture(
            &inner,
            "nested",
            futures::future::poll_fn(|_| {
                assert!(
                    current_owned_runtime_task().is_some(),
                    "actual nested registration is in scope"
                );
                Poll::<()>::Pending
            }),
        );
        let inner_identity = nested.identity.clone();
        let (mut parent, parent_id) = registered_fixture(
            &outer,
            "parent",
            futures::future::poll_fn(move |cx| {
                let before = current_owned_runtime_task().expect("registered parent scope");
                assert_ne!(before, inner_identity);
                assert!(Pin::new(&mut nested).poll(cx).is_pending());
                assert_eq!(current_owned_runtime_task(), Some(before));
                Poll::Ready(())
            }),
        );
        let mut cx = Context::from_waker(futures::task::noop_waker_ref());
        assert!(Pin::new(&mut parent).poll(&mut cx).is_ready());
        assert!(
            current_owned_runtime_task().is_none(),
            "scope ends before executor receives Ready"
        );
        drop(parent);
        outer.complete_task(parent_id, "parent", TaskOutcome::Completed);
        inner.complete_task(nested_id, "nested", TaskOutcome::Cancelled);
        let (mut panicking, id) = registered_fixture(
            &outer,
            "panic",
            futures::future::poll_fn(|_| -> Poll<()> {
                assert!(current_owned_runtime_task().is_some());
                panic!("deliberate actual poll panic");
            }),
        );
        assert!(std::panic::catch_unwind(AssertUnwindSafe(
            || Pin::new(&mut panicking).poll(&mut cx)
        ))
        .is_err());
        assert!(
            current_owned_runtime_task().is_none(),
            "panic restores executor context lexically"
        );
        drop(panicking);
        outer.complete_task(id, "panic", TaskOutcome::Panicked);
    }

    struct DropWithContext {
        seen: Arc<Mutex<Option<OwnedRuntimeTaskIdentity>>>,
    }
    impl Future for DropWithContext {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
            assert!(current_owned_runtime_task().is_some());
            Poll::Pending
        }
    }
    impl Drop for DropWithContext {
        fn drop(&mut self) {
            *self.seen.lock() = current_owned_runtime_task();
        }
    }

    #[test]
    fn registered_future_cancellation_and_unpolled_drop_keep_actual_identity_only_during_drop() {
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("context.drop");
        for poll_first in [false, true] {
            let seen = Arc::new(Mutex::new(None));
            let (mut future, id) =
                registered_fixture(&group, "drop", DropWithContext { seen: seen.clone() });
            let expected = future.identity.clone();
            if poll_first {
                assert!(Pin::new(&mut future)
                    .poll(&mut Context::from_waker(futures::task::noop_waker_ref()))
                    .is_pending());
                assert!(
                    current_owned_runtime_task().is_none(),
                    "Pending restores executor context"
                );
            }
            drop(future);
            assert_eq!(
                *seen.lock(),
                Some(expected),
                "owned destructor sees original registered identity"
            );
            assert!(
                current_owned_runtime_task().is_none(),
                "cancelled destructor cannot leak task scope"
            );
            group.complete_task(id, "drop", TaskOutcome::Cancelled);
        }
    }

    async fn local_registered_sibling_session_contract() {
        let supervisor = TaskSupervisor::new();
        let first_group = supervisor.group("context.local.first");
        let second_group = supervisor.group("context.local.second");
        let state = Arc::new(Mutex::new(ChoreographyState::new()));
        let (first_ready_tx, first_ready) = oneshot::channel();
        let (second_ready_tx, second_ready) = oneshot::channel();
        let (release_first, released_first) = oneshot::channel();
        let (release_second, released_second) = oneshot::channel();
        let (first_done_tx, first_done) = oneshot::channel();
        let (second_done_tx, second_done) = oneshot::channel();
        let role = ChoreographicRole::new(
            DeviceId::new_from_entropy([0xa1; 32]),
            AuthorityId::new_from_entropy([0xa2; 32]),
            RoleIndex::new(0).expect("fixed role"),
        );
        let first_session =
            RuntimeChoreographySessionId::from_uuid(uuid::Uuid::from_bytes([0xa3; 16]));
        let second_session =
            RuntimeChoreographySessionId::from_uuid(uuid::Uuid::from_bytes([0xa4; 16]));
        let first_state = state.clone();
        let _first_handle = first_group.spawn_local_try_named("first", async move {
            let identity = current_owned_runtime_task().expect("real local task registration");
            first_state
                .lock()
                .start_session(
                    first_session,
                    Some("context.first".into()),
                    ContextId::new_from_entropy([0xa5; 32]),
                    vec![role],
                    role,
                    None,
                    1,
                )
                .expect("first registered local session");
            first_ready_tx
                .send(identity.clone())
                .expect("caller observes first owner");
            released_first.await.expect("caller releases first owner");
            assert_eq!(current_owned_runtime_task(), Some(identity));
            assert_eq!(first_state.lock().current_session_id(), Some(first_session));
            first_state
                .lock()
                .end_session_observed(Some(2))
                .expect("first owner retires only own session");
            first_done_tx
                .send(())
                .expect("caller observes first completion");
            Ok(())
        });
        let first_identity = first_ready
            .await
            .expect("first registered future reaches await");
        assert!(
            current_owned_runtime_task().is_none(),
            "awaiting caller cannot borrow child's ambient owner"
        );
        let second_state = state.clone();
        let _second_handle = second_group.spawn_local_try_named("second", async move {
            let identity = current_owned_runtime_task().expect("real second local registration");
            second_state
                .lock()
                .start_session(
                    second_session,
                    Some("context.second".into()),
                    ContextId::new_from_entropy([0xa6; 32]),
                    vec![role],
                    role,
                    None,
                    1,
                )
                .expect("sibling on same executor thread has an independent session owner");
            second_ready_tx
                .send(identity.clone())
                .expect("caller observes second owner");
            released_second.await.expect("caller releases second owner");
            assert_eq!(current_owned_runtime_task(), Some(identity));
            assert_eq!(
                second_state.lock().current_session_id(),
                Some(second_session)
            );
            second_state
                .lock()
                .end_session_observed(Some(2))
                .expect("second owner retires only own session");
            second_done_tx
                .send(())
                .expect("caller observes second completion");
            Ok(())
        });
        let second_identity = second_ready
            .await
            .expect("second actual owner reaches await");
        assert_eq!(
            first_identity.task_id, second_identity.task_id,
            "group-local ids intentionally collide"
        );
        assert_ne!(
            first_identity, second_identity,
            "actual group custody disambiguates identical ids"
        );
        release_second.send(()).expect("release second");
        second_done.await.expect("second retires independently");
        release_first.send(()).expect("release first");
        first_done
            .await
            .expect("first retains original binding across sibling completion");
        assert!(current_owned_runtime_task().is_none());
        assert!(
            supervisor.active_tasks().is_empty(),
            "real registered callbacks finish before caller wakes"
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn native_local_registered_siblings_keep_independent_session_bindings_across_await() {
        tokio::task::LocalSet::new()
            .run_until(local_registered_sibling_session_contract())
            .await;
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn wasm_local_registered_siblings_keep_independent_session_bindings_across_await() {
        local_registered_sibling_session_contract().await;
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[tokio::test]
    async fn native_send_registered_identity_is_stable_and_executor_observation_stays_unowned() {
        let supervisor = TaskSupervisor::new();
        let (completed_tx, completed) = oneshot::channel();
        let _handle = supervisor.spawn_try_named("context.send", async move {
            let before = current_owned_runtime_task().expect("actual native registration");
            tokio::task::yield_now().await;
            assert_eq!(current_owned_runtime_task(), Some(before));
            completed_tx.send(()).expect("caller observes completion");
            Ok(())
        });
        completed.await.expect("real registered task completes");
        supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect("owned native task drains");
        assert!(current_owned_runtime_task().is_none());
    }
    #[test]
    fn destructor_failure_retains_actual_source_in_registered_service_health() {
        struct RequiredCleanup;
        impl Future for RequiredCleanup {
            type Output = ();
            fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
                Poll::Pending
            }
        }
        impl Drop for RequiredCleanup {
            fn drop(&mut self) {
                let original = std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "required destructor fixture",
                );
                assert!(retain_current_task_cleanup_failure(
                    aura_core::AuraError::Internal {
                        message: "required cleanup".to_owned(),
                        source: Some(Arc::new(original)),
                    }
                ));
            }
        }
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("required-cleanup");
        let (future, id) = registered_fixture(&group, "actual-destructor", RequiredCleanup);
        drop(future);
        group.complete_task(id, "actual-destructor", TaskOutcome::Cancelled);
        for failure in [group.terminal_failure(), supervisor.root.terminal_failure()] {
            let Some(TaskSupervisionError::TaskFailed { source, .. }) = failure else {
                panic!("registered destructor failure must reach owning service health");
            };
            let original = std::error::Error::source(&source)
                .and_then(|cause| cause.downcast_ref::<std::io::Error>())
                .expect("actual concrete cause survives cancellation publication");
            assert_eq!(original.kind(), std::io::ErrorKind::PermissionDenied);
        }
        assert!(current_owned_runtime_task().is_none());
    }
    #[tokio::test]
    async fn completed_primary_subsidiary_failure_retains_source_in_health_and_drain() {
        use std::error::Error;
        let supervisor = TaskSupervisor::new();
        let group = supervisor.group("completed-primary");
        let (primary_tx, primary_rx) = tokio::sync::oneshot::channel();
        let _primary_task = group.spawn_try_named("primary-publication", async move {
            primary_tx
                .send("published-cancelled")
                .expect("primary observer remains live");
            Ok(())
        });
        let primary = primary_rx.await.expect("actual primary task publishes");
        group
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect("primary task completes before subsidiary failure");
        group.record_subsidiary_failure(
            "negative-notice-preparation",
            aura_core::AuraError::Storage {
                message: "required retained checkpoint".into(),
                source: Some(Arc::new(aura_core::effects::StorageError::WriteFailed(
                    "actual controlled write failure".into(),
                ))),
            },
        );
        assert_eq!(primary, "published-cancelled");
        let health = group
            .terminal_failure()
            .expect("subsidiary retained in owner health");
        let drain = supervisor
            .wait_for_idle(Duration::from_secs(1))
            .await
            .expect_err("required subsidiary fault remains observable during drain");
        for retained in [&health, &drain] {
            let mut source = retained.source();
            let mut storage = false;
            while let Some(error) = source {
                storage |= error.is::<aura_core::effects::StorageError>();
                source = error.source();
            }
            assert!(
                storage,
                "actual concrete storage cause survives standard source chain"
            );
        }
        assert!(
            supervisor.active_tasks().is_empty(),
            "recording admits no diagnostic task"
        );
    }
}

#[cfg(test)]
mod shutdown_scope_tests {
    use super::*;
    #[test]
    fn actual_shutdown_task_root_rejects_equal_named_foreign_and_sibling_scopes() {
        let first = TaskSupervisor::default();
        let second = TaskSupervisor::default();
        let child = first.group("same_service");
        let foreign = second.group("same_service");
        let sibling = first.group("other_service");
        assert!(first.owns_group(&child));
        assert!(!first.owns_group(&foreign));
        let narrow = TaskSupervisor::with_root(child.clone());
        assert!(narrow.owns_group(&child));
        assert!(narrow.owns_group(&child.group("worker")));
        assert!(!narrow.owns_group(&sibling));
        assert!(!narrow.owns_group(&foreign));
    }
}
